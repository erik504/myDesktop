#![windows_subsystem = "windows"]

mod core;
mod dialogs;
mod error_indicator;
mod explorer;
mod ipc;
mod protected;
mod spinner;
mod startup;
mod success;
mod service;
#[cfg(test)]
mod version;

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;

type Handle = *mut c_void;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;
const WM_DESTROY: u32 = 2;
const WM_COMMAND: u32 = 0x111;
const WM_CONTEXTMENU: u32 = 0x7b;
const WM_RBUTTONUP: u32 = 0x205;
const WM_TIMER: u32 = 0x113;
const WM_APP: u32 = 0x8000;
const TRAY_CALLBACK: u32 = WM_APP + 1;
const JOB_FINISHED: u32 = WM_APP + 2;
const NIM_ADD: u32 = 0;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 1;
const NIF_ICON: u32 = 2;
const NIF_TIP: u32 = 4;
const MF_STRING: u32 = 0;
const MF_SEPARATOR: u32 = 0x800;
const MF_CHECKED: u32 = 0x8;
const TPM_RIGHTBUTTON: u32 = 2;
const TPM_RETURNCMD: u32 = 0x100;
const ERROR_ALREADY_EXISTS: u32 = 183;
static BUSY: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static TRAY_WINDOW: AtomicIsize = AtomicIsize::new(0);
enum JobCompletion { Standard(Result<String, String>), Service { install: bool, result: Result<(), String> } }
static JOB_RESULT: Mutex<Option<JobCompletion>> = Mutex::new(None);

#[repr(C)]
struct WndClassW {
    style: u32, wnd_proc: Option<unsafe extern "system" fn(Handle, u32, Wparam, Lparam) -> Lresult>,
    cls_extra: i32, wnd_extra: i32, instance: Handle, icon: Handle, cursor: Handle,
    background: Handle, menu_name: *const u16, class_name: *const u16,
}
#[repr(C)]
struct Point { x: i32, y: i32 }
#[repr(C)]
struct Msg { hwnd: Handle, message: u32, wparam: Wparam, lparam: Lparam, time: u32, point: Point, private: u32 }
#[repr(C)]
struct Guid { d1: u32, d2: u16, d3: u16, d4: [u8; 8] }
#[repr(C)]
struct NotifyIconDataW {
    size: u32, hwnd: Handle, id: u32, flags: u32, callback: u32, icon: Handle,
    tip: [u16; 128], state: u32, state_mask: u32, info: [u16; 256], timeout_or_version: u32,
    info_title: [u16; 64], info_flags: u32, guid: Guid, balloon_icon: Handle,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(ex_style: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: Handle, menu: Handle, instance: Handle, param: *mut c_void) -> Handle;
    fn DefWindowProcW(hwnd: Handle, msg: u32, w: Wparam, l: Lparam) -> Lresult;
    fn DestroyWindow(hwnd: Handle) -> i32;
    fn PostQuitMessage(code: i32);
    fn GetMessageW(msg: *mut Msg, hwnd: Handle, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
    fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    fn MessageBoxW(hwnd: Handle, text: *const u16, title: *const u16, flags: u32) -> i32;
    fn CreatePopupMenu() -> Handle;
    fn AppendMenuW(menu: Handle, flags: u32, id: usize, text: *const u16) -> i32;
    fn TrackPopupMenu(menu: Handle, flags: u32, x: i32, y: i32, reserved: i32, hwnd: Handle, rect: *const c_void) -> u32;
    fn DestroyMenu(menu: Handle) -> i32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn SetForegroundWindow(hwnd: Handle) -> i32;
    fn PostMessageW(hwnd: Handle, msg: u32, w: Wparam, l: Lparam) -> i32;
    fn RegisterWindowMessageW(name: *const u16) -> u32;
    fn SetProcessDpiAwarenessContext(context: isize) -> i32;
    fn SetProcessDPIAware() -> i32;
}
#[link(name = "shell32")]
extern "system" { fn Shell_NotifyIconW(message: u32, data: *mut NotifyIconDataW) -> i32; fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32; }
#[repr(C)]
struct ShellExecuteInfo {
    size: u32, mask: u32, owner: Handle, verb: *const u16, file: *const u16,
    parameters: *const u16, directory: *const u16, show: i32, instance: Handle,
    id_list: *mut c_void, class: *const u16, class_key: Handle, hot_key: u32,
    icon_or_monitor: Handle, process: Handle,
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn CreateMutexW(attrs: *mut c_void, owner: i32, name: *const u16) -> Handle;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, timeout: u32) -> u32;
    fn GetExitCodeProcess(handle: Handle, exit: *mut u32) -> i32;
}
#[link(name = "ole32")]
extern "system" { fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32; fn CoUninitialize(); }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn tell(s: &str, error: bool) { unsafe { if error { let hwnd=TRAY_WINDOW.load(Ordering::Acquire) as Handle; if !hwnd.is_null(){error_indicator::report(hwnd,s);} } else { MessageBoxW(null_mut(), wide(s).as_ptr(), wide("myDesktop").as_ptr(), 0x40); } } }
unsafe fn tray_data(hwnd: Handle) -> NotifyIconDataW {
    let mut data: NotifyIconDataW = std::mem::zeroed();
    data.size = std::mem::size_of::<NotifyIconDataW>() as u32;
    data.hwnd = hwnd; data.id = 1; data.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.callback = TRAY_CALLBACK; data.icon = LoadIconW(GetModuleHandleW(null()), 1usize as *const u16);
    for (out, ch) in data.tip.iter_mut().zip(wide("myDesktop - configurazione finestre")) { *out = ch; }
    data
}
fn start_job(hwnd: Handle, save: bool, path: Option<std::path::PathBuf>) {
    if BUSY.swap(true, Ordering::AcqRel) { tell("Un'operazione è già in corso. Attendi il completamento.", true); return; }
    unsafe { success::stop(hwnd); spinner::start(hwnd); }
    let target = hwnd as usize;
    let started = std::thread::Builder::new().spawn(move || {
        let outcome = std::panic::catch_unwind(|| match (save, path.as_deref()) { (true, Some(path)) => core::save_to(path), (false, Some(path)) => core::restore_from(path), (true, None) => core::save(), (false, None) => core::restore() });
        let result = JobCompletion::Standard(match outcome { Ok(result) => result, Err(_) => Err("Errore inatteso durante l'operazione.".into()) });
        *JOB_RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        if unsafe { PostMessageW(target as Handle, JOB_FINISHED, 0, 0) } == 0 {
            BUSY.store(false, Ordering::Release);
        }
    });
    if let Err(e) = started {
        BUSY.store(false, Ordering::Release);
        unsafe { spinner::stop(hwnd); }
        tell(&format!("Impossibile avviare l'operazione: {e}"), true);
    }
}
fn start_service_change(hwnd: Handle, install: bool) {
    if BUSY.swap(true, Ordering::AcqRel) { tell("Un'operazione è già in corso. Attendi il completamento.", true); return; }
    unsafe { success::stop(hwnd); spinner::start(hwnd); }
    let target = hwnd as usize;
    let started = std::thread::Builder::new().spawn(move || {
        let operation = std::panic::catch_unwind(|| -> Result<(),String> { unsafe {
            let exe = std::env::current_exe().map_err(|e|format!("Impossibile trovare l'eseguibile: {e}"))?;
            use std::os::windows::ffi::OsStrExt;
            let file: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
            let verb=wide("runas");
            let argument=if install { format!("--install-service {}", service::current_user_sid()?) } else { "--uninstall-service".into() };
            let parameters=wide(&argument);
            let mut info: ShellExecuteInfo = std::mem::zeroed();
            info.size=std::mem::size_of::<ShellExecuteInfo>() as u32; info.mask=0x40 | 0x100;
            info.owner=target as Handle; info.verb=verb.as_ptr(); info.file=file.as_ptr(); info.parameters=parameters.as_ptr(); info.show=1;
            let initialized=CoInitializeEx(null_mut(),2)>=0;
            let ok=ShellExecuteExW(&mut info);
            if ok==0 { let e=std::io::Error::last_os_error(); if initialized {CoUninitialize();} return Err(if e.raw_os_error()==Some(1223) { "Operazione annullata: autorizzazione amministrativa non concessa.".into() } else { format!("Impossibile avviare l'operazione amministrativa: {e}") }); }
            if info.process.is_null() { if initialized {CoUninitialize();} return Err("Impossibile attendere l'operazione amministrativa.".into()); }
            let waited=WaitForSingleObject(info.process, 0xffff_ffff); let mut exit=1u32;
            let read=GetExitCodeProcess(info.process, &mut exit); CloseHandle(info.process); if initialized {CoUninitialize();}
            if waited!=0 || read==0 { return Err("Impossibile verificare l'esito dell'operazione amministrativa.".into()); }
            if exit!=0 { return Err("L'operazione sul servizio non è riuscita.".into()); }
            Ok(())
        }});
        let result=operation.unwrap_or_else(|_|Err("Errore inatteso durante la gestione del servizio.".into()));
        *JOB_RESULT.lock().unwrap_or_else(|e|e.into_inner())=Some(JobCompletion::Service {install, result});
        if unsafe { PostMessageW(target as Handle, JOB_FINISHED, 0, 0) } == 0 { BUSY.store(false, Ordering::Release); }
    });
    if let Err(e)=started { BUSY.store(false, Ordering::Release); unsafe { spinner::stop(hwnd); } tell(&format!("Impossibile avviare l'operazione: {e}"), true); }
}
unsafe extern "system" fn wnd_proc(hwnd: Handle, msg: u32, w: Wparam, l: Lparam) -> Lresult {
    if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) {
        Shell_NotifyIconW(NIM_ADD, &mut tray_data(hwnd));
        if BUSY.load(Ordering::Acquire) { spinner::tick(hwnd); }
        else if error_indicator::active() { error_indicator::draw(hwnd); }
        else if success::active() { success::draw(hwnd); }
        return 0;
    }
    match msg {
        TRAY_CALLBACK if l as u32 == WM_RBUTTONUP || l as u32 == WM_CONTEXTMENU => {
            let menu = CreatePopupMenu();
            if !menu.is_null() {
                AppendMenuW(menu, MF_STRING, 1, wide("Memorizza finestre").as_ptr());
                AppendMenuW(menu, MF_STRING, 2, wide("Ripristina finestre").as_ptr());
                AppendMenuW(menu, MF_STRING, 6, wide("Memorizza finestre come...").as_ptr());
                AppendMenuW(menu, MF_STRING, 7, wide("Ripristina finestre da...").as_ptr());
                let service_status=service::is_installed();
                let startup_status = startup::enabled();
                let incomplete=matches!((&startup_status,&service_status),(Ok(a),Ok(b)) if a!=b);
                let startup_label=if incomplete {"Avvia con Windows (configurazione incompleta)"} else {"Avvia con Windows"};
                AppendMenuW(menu, MF_STRING | if matches!(startup_status, Ok(true)) || matches!(service_status, Ok(true)) { MF_CHECKED } else { 0 }, 5, wide(startup_label).as_ptr());
                AppendMenuW(menu, MF_STRING, 4, wide("Informazioni sulla versione").as_ptr());
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
                AppendMenuW(menu, MF_STRING, 3, wide("Esci").as_ptr());
                let mut p = Point { x: 0, y: 0 }; GetCursorPos(&mut p); SetForegroundWindow(hwnd);
                let command = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, p.x, p.y, 0, hwnd, null());
                DestroyMenu(menu); PostMessageW(hwnd, 0, 0, 0);
                match command {
                    1 => start_job(hwnd, true, None),
                    2 => start_job(hwnd, false, None),
                    6 | 7 => {
                        if BUSY.load(Ordering::Acquire) { tell("Un'operazione è già in corso. Attendi il completamento.", true); }
                        else { match dialogs::choose(hwnd, command==6) { Ok(Some(path)) => start_job(hwnd, command==6, Some(path)), Ok(None) => (), Err(e) => tell(&e,true) } }
                    },
                    4 => tell(concat!("myDesktop ", env!("MYDESKTOP_VERSION"), "\nData di compilazione: ", env!("MYDESKTOP_BUILD_DATE")), false),
                    5 => match (startup_status,service_status) {
                        (Err(e),_) | (_,Err(e)) => tell(&e,true),
                        (Ok(startup),Ok(installed)) => {
                            if BUSY.load(Ordering::Acquire) {tell("Un'operazione è già in corso. Attendi il completamento.",true);}
                            else if !installed && startup { match startup::set(false) {Ok(())=>success::start(hwnd),Err(e)=>tell(&e,true)} }
                            else { start_service_change(hwnd, !installed); }
                        }
                    },
                    3 if !BUSY.load(Ordering::Acquire) => { DestroyWindow(hwnd); },
                    3 => tell("Attendi il completamento dell'operazione prima di uscire.", true),
                    _ => ()
                }
            }
            0
        }
        WM_COMMAND => 0,
        WM_TIMER if w == spinner::TIMER_ID => {
            if BUSY.load(Ordering::Acquire) { spinner::tick(hwnd); } else { spinner::stop(hwnd); if error_indicator::active(){error_indicator::draw(hwnd);} }
            0
        }
        WM_TIMER if w == error_indicator::TIMER => { if !BUSY.load(Ordering::Acquire) { error_indicator::tick(hwnd); } 0 }
        WM_TIMER if w == success::TIMER_ID => { if !BUSY.load(Ordering::Acquire) && success::active() && success::expired() { success::stop(hwnd); } 0 }
        JOB_FINISHED => {
            let result = JOB_RESULT.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(result) = result {
                BUSY.store(false, Ordering::Release);
                spinner::stop(hwnd);
                match result {
                    JobCompletion::Standard(Ok(_)) => if error_indicator::active(){error_indicator::draw(hwnd)}else{success::start(hwnd)},
                    JobCompletion::Standard(Err(message)) => tell(&message, true),
                    JobCompletion::Service { install, result: Err(message) } => { let _=install; tell(&message, true); },
                    JobCompletion::Service { install, result: Ok(()) } => {
                        if let Err(e)=startup::set(install) { tell(&e,true); }
                        else if error_indicator::active(){error_indicator::draw(hwnd)}else{success::start(hwnd)}
                    }
                }
            }
            0
        }
        WM_DESTROY => { success::stop(hwnd); spinner::stop(hwnd); Shell_NotifyIconW(NIM_DELETE, &mut tray_data(hwnd)); PostQuitMessage(0); 0 }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
fn main() { 
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--install-service") | Some("--uninstall-service") => {
            let result=if args[1]=="--install-service" { if args.len()!=3 {Err("Identità dell'utente non specificata.".into())} else {service::install_for(&args[2])} } else if args.len()!=2 {Err("Argomenti non validi.".into())} else {service::uninstall()};
            if let Err(error)=result {tell(&error,true);std::process::exit(1);} return;
        },
        Some("--service") => {if args.len()!=3 || service::run(&args[2]).is_err(){std::process::exit(1)} return;},
        Some("--elevated-helper") => {unsafe { if SetProcessDpiAwarenessContext(-4)==0 {SetProcessDPIAware();} } if args.len()!=2 || ipc::serve().is_err(){std::process::exit(1)} return;},
        Some(_) => {tell("Argomento non riconosciuto.", true); std::process::exit(1);},
        None => ()
    }
    unsafe {
    if SetProcessDpiAwarenessContext(-4) == 0 { SetProcessDPIAware(); }
    let mutex = CreateMutexW(null_mut(), 0, wide("Local\\myDesktop.Singleton").as_ptr());
    if mutex.is_null() { tell("Impossibile inizializzare myDesktop.", true); return; }
    if GetLastError() == ERROR_ALREADY_EXISTS { tell("myDesktop è già in esecuzione nell'area di notifica.", false); CloseHandle(mutex); return; }
    let instance = GetModuleHandleW(null());
    TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Ordering::Relaxed);
    let class_name = wide("myDesktopTrayWindow");
    let class = WndClassW { style: 0, wnd_proc: Some(wnd_proc), cls_extra: 0, wnd_extra: 0, instance, icon: null_mut(), cursor: null_mut(), background: null_mut(), menu_name: null(), class_name: class_name.as_ptr() };
    if RegisterClassW(&class) == 0 { tell("Impossibile inizializzare la finestra di myDesktop.", true); CloseHandle(mutex); return; }
    let hwnd = CreateWindowExW(0, class_name.as_ptr(), wide("myDesktop").as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), instance, null_mut());
    if hwnd.is_null() { tell("Impossibile avviare myDesktop.", true); CloseHandle(mutex); return; }
    TRAY_WINDOW.store(hwnd as isize,Ordering::Release);
    if Shell_NotifyIconW(NIM_ADD, &mut tray_data(hwnd)) == 0 { tell("Impossibile mostrare l'icona nell'area di notifica.", true); DestroyWindow(hwnd); CloseHandle(mutex); return; }
    let mut msg: Msg = std::mem::zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 { TranslateMessage(&msg); DispatchMessageW(&msg); }
    CloseHandle(mutex);
} }

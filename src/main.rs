#![windows_subsystem = "windows"]

mod core;
mod explorer;

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

type Handle = *mut c_void;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;
const WM_DESTROY: u32 = 2;
const WM_COMMAND: u32 = 0x111;
const WM_CONTEXTMENU: u32 = 0x7b;
const WM_RBUTTONUP: u32 = 0x205;
const WM_APP: u32 = 0x8000;
const TRAY_CALLBACK: u32 = WM_APP + 1;
const NIM_ADD: u32 = 0;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 1;
const NIF_ICON: u32 = 2;
const NIF_TIP: u32 = 4;
const MF_STRING: u32 = 0;
const MF_SEPARATOR: u32 = 0x800;
const TPM_RIGHTBUTTON: u32 = 2;
const TPM_RETURNCMD: u32 = 0x100;
const ERROR_ALREADY_EXISTS: u32 = 183;
static BUSY: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

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
extern "system" { fn Shell_NotifyIconW(message: u32, data: *mut NotifyIconDataW) -> i32; }
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn CreateMutexW(attrs: *mut c_void, owner: i32, name: *const u16) -> Handle;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: Handle) -> i32;
}
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn tell(s: &str, error: bool) { unsafe { MessageBoxW(null_mut(), wide(s).as_ptr(), wide("myDesktop").as_ptr(), if error { 0x10 } else { 0x40 }); } }
unsafe fn tray_data(hwnd: Handle) -> NotifyIconDataW {
    let mut data: NotifyIconDataW = std::mem::zeroed();
    data.size = std::mem::size_of::<NotifyIconDataW>() as u32;
    data.hwnd = hwnd; data.id = 1; data.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.callback = TRAY_CALLBACK; data.icon = LoadIconW(GetModuleHandleW(null()), 1usize as *const u16);
    for (out, ch) in data.tip.iter_mut().zip(wide("myDesktop - configurazione finestre")) { *out = ch; }
    data
}
fn start_job(save: bool) {
    if BUSY.swap(true, Ordering::AcqRel) { tell("Un'operazione è già in corso. Attendi il completamento.", true); return; }
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| if save { core::save() } else { core::restore() });
        match outcome { Ok(Ok(message)) => tell(&message, false), Ok(Err(message)) => tell(&message, true), Err(_) => tell("Errore inatteso durante l'operazione.", true) }
        BUSY.store(false, Ordering::Release);
    });
}
unsafe extern "system" fn wnd_proc(hwnd: Handle, msg: u32, w: Wparam, l: Lparam) -> Lresult {
    if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) {
        Shell_NotifyIconW(NIM_ADD, &mut tray_data(hwnd));
        return 0;
    }
    match msg {
        TRAY_CALLBACK if l as u32 == WM_RBUTTONUP || l as u32 == WM_CONTEXTMENU => {
            let menu = CreatePopupMenu();
            if !menu.is_null() {
                AppendMenuW(menu, MF_STRING, 1, wide("Memorizza finestre").as_ptr());
                AppendMenuW(menu, MF_STRING, 2, wide("Ripristina finestre").as_ptr());
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
                AppendMenuW(menu, MF_STRING, 3, wide("Esci").as_ptr());
                let mut p = Point { x: 0, y: 0 }; GetCursorPos(&mut p); SetForegroundWindow(hwnd);
                let command = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, p.x, p.y, 0, hwnd, null());
                DestroyMenu(menu); PostMessageW(hwnd, 0, 0, 0);
                match command { 1 => start_job(true), 2 => start_job(false), 3 if !BUSY.load(Ordering::Acquire) => { DestroyWindow(hwnd); }, 3 => tell("Attendi il completamento dell'operazione prima di uscire.", true), _ => () }
            }
            0
        }
        WM_COMMAND => 0,
        WM_DESTROY => { Shell_NotifyIconW(NIM_DELETE, &mut tray_data(hwnd)); PostQuitMessage(0); 0 }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
fn main() { unsafe {
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
    if Shell_NotifyIconW(NIM_ADD, &mut tray_data(hwnd)) == 0 { tell("Impossibile mostrare l'icona nell'area di notifica.", true); DestroyWindow(hwnd); CloseHandle(mutex); return; }
    let mut msg: Msg = std::mem::zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 { TranslateMessage(&msg); DispatchMessageW(&msg); }
    CloseHandle(mutex);
} }

//! Windows service lifecycle. The service launches only its own protected executable
//! in the installing administrator's interactive session, as a narrowly scoped helper.
use std::{ffi::{c_void, OsStr}, fs, io::Write, mem::{size_of, zeroed}, os::windows::{ffi::OsStrExt,io::FromRawHandle}, path::PathBuf, ptr::{null, null_mut}, sync::{atomic::{AtomicIsize, Ordering}, Mutex}, time::Duration};

type Handle = *mut c_void;
type ScHandle = *mut c_void;
const SERVICE_NAME: &str = "myDesktopWindowHelper";
const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
const SERVICE_START_PENDING: u32 = 2;
const SERVICE_STOP_PENDING: u32 = 3;
const SERVICE_RUNNING: u32 = 4;
const SERVICE_STOPPED: u32 = 1;
const SERVICE_ACCEPT_STOP: u32 = 1;
const SERVICE_ACCEPT_SHUTDOWN: u32 = 4;
const SERVICE_CONTROL_STOP: u32 = 1;
const SERVICE_CONTROL_SHUTDOWN: u32 = 5;
const WAIT_OBJECT_0: u32 = 0;
const TOKEN_QUERY: u32 = 8;

#[repr(C)] struct Guid { d1: u32, d2: u16, d3: u16, d4: [u8; 8] }
const FOLDERID_PROGRAMFILES: Guid = Guid { d1: 0x905e63b6, d2: 0xc1bf, d3: 0x494e, d4: [0xb2, 0x9c, 0x65, 0xb7, 0x32, 0xd3, 0xd2, 0x1a] };
#[repr(C)] struct SecurityAttributes { length: u32, descriptor: *mut c_void, inherit: i32 }
#[repr(C)] #[derive(Clone, Copy)] struct ServiceStatus { service_type: u32, state: u32, accepted: u32, win32_exit: u32, service_exit: u32, checkpoint: u32, wait_hint: u32 }
#[repr(C)] struct ServiceTableEntry { name: *mut u16, function: Option<unsafe extern "system" fn(u32, *mut *mut u16)> }
#[repr(C)] struct SessionInfo { id: u32, station: *mut u16, state: u32 }
#[repr(C)] struct StartupInfo { size: u32, reserved: *mut u16, desktop: *mut u16, title: *mut u16, x: u32, y: u32, x_size: u32, y_size: u32, x_chars: u32, y_chars: u32, fill: u32, flags: u32, show: u16, reserved2_size: u16, reserved2: *mut u8, stdin: Handle, stdout: Handle, stderr: Handle }
#[repr(C)] struct ProcessInfo { process: Handle, thread: Handle, process_id: u32, thread_id: u32 }
#[repr(C)] struct QueryServiceConfig { service_type: u32, start_type: u32, error_control: u32, binary_path: *mut u16, load_order: *mut u16, tag_id: u32, dependencies: *mut u16, service_start_name: *mut u16, display_name: *mut u16 }

#[link(name="advapi32")] unsafe extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> ScHandle;
    fn OpenServiceW(manager: ScHandle, name: *const u16, access: u32) -> ScHandle;
    fn CreateServiceW(manager: ScHandle, name: *const u16, display: *const u16, access: u32, typ: u32, start: u32, error: u32, binary: *const u16, group: *const u16, tag: *mut u32, deps: *const u16, account: *const u16, password: *const u16) -> ScHandle;
    fn StartServiceW(service: ScHandle, count: u32, args: *const *const u16) -> i32;
    fn ControlService(service: ScHandle, code: u32, status: *mut ServiceStatus) -> i32;
    fn QueryServiceStatus(service: ScHandle, status: *mut ServiceStatus) -> i32;
    fn QueryServiceConfigW(service: ScHandle, config: *mut QueryServiceConfig, bytes: u32, needed: *mut u32) -> i32;
    fn DeleteService(service: ScHandle) -> i32;
    fn CloseServiceHandle(handle: ScHandle) -> i32;
    fn StartServiceCtrlDispatcherW(table: *const ServiceTableEntry) -> i32;
    fn RegisterServiceCtrlHandlerExW(name: *const u16, handler: Option<unsafe extern "system" fn(u32,u32,*mut c_void,*mut c_void)->u32>, context: *mut c_void) -> ScHandle;
    fn SetServiceStatus(handle: ScHandle, status: *const ServiceStatus) -> i32;
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn GetTokenInformation(token: Handle, class: u32, info: *mut c_void, length: u32, returned: *mut u32) -> i32;
    fn DuplicateTokenEx(existing: Handle, access: u32, attrs: *const c_void, level: u32, typ: u32, token: *mut Handle) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text: *const u16, revision: u32, descriptor: *mut *mut c_void, size: *mut u32) -> i32;
    fn CreateProcessAsUserW(token: Handle, app: *const u16, cmd: *mut u16, process_attrs: *const c_void, thread_attrs: *const c_void, inherit: i32, flags: u32, environment: *mut c_void, directory: *const u16, startup: *mut StartupInfo, process: *mut ProcessInfo) -> i32;
    fn RegisterEventSourceW(server:*const u16,source:*const u16)->Handle;
    fn ReportEventW(event:Handle,typ:u16,category:u16,id:u32,user:*const c_void,count:u16,data_size:u32,strings:*const *const u16,data:*const c_void)->i32;
    fn DeregisterEventSource(event:Handle)->i32;
}
#[link(name="kernel32")] unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: Handle) -> i32;
    fn CreateDirectoryW(path: *const u16, attrs: *const SecurityAttributes) -> i32;
    fn CreateFileW(path: *const u16, access: u32, share: u32, attrs: *const c_void, creation: u32, flags: u32, template: Handle) -> Handle;
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    fn LocalFree(ptr: *mut c_void) -> *mut c_void;
    fn CreateEventW(attrs: *const c_void, manual: i32, initial: i32, name: *const u16) -> Handle;
    fn SetEvent(event: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, ms: u32) -> u32;
    fn TerminateProcess(process: Handle, code: u32) -> i32;
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
}
#[link(name="shell32")] unsafe extern "system" { fn SHGetKnownFolderPath(id: *const Guid, flags: u32, token: Handle, path: *mut *mut u16) -> i32; }
#[link(name="ole32")] unsafe extern "system" { fn CoTaskMemFree(ptr: *mut c_void); }
#[link(name="wtsapi32")] unsafe extern "system" {
    fn WTSEnumerateSessionsW(server: Handle, reserved: u32, version: u32, sessions: *mut *mut SessionInfo, count: *mut u32) -> i32;
    fn WTSQueryUserToken(session: u32, token: *mut Handle) -> i32;
    fn WTSFreeMemory(ptr: *mut c_void);
}

struct OwnedHandle(Handle);
impl Drop for OwnedHandle { fn drop(&mut self) { if !self.0.is_null() { unsafe { CloseHandle(self.0); } } } }
struct OwnedService(ScHandle);
impl Drop for OwnedService { fn drop(&mut self) { if !self.0.is_null() { unsafe { CloseServiceHandle(self.0); } } } }
fn wide(s: &OsStr) -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() }
fn ws(s: &str) -> Vec<u16> { wide(OsStr::new(s)) }
unsafe fn ptr_text(p: *const u16) -> String { unsafe { let mut n=0; while *p.add(n)!=0 {n+=1}; String::from_utf16_lossy(std::slice::from_raw_parts(p,n)) } }
fn last(context: &str) -> String { format!("{context}: {}", std::io::Error::last_os_error()) }

/// Only a directory created with a protected DACL in the machine's actual Program Files
/// is accepted; a pre-existing directory (including a junction) is never followed.
pub fn installed_path() -> Result<PathBuf,String> { unsafe {
    let mut p=null_mut(); if SHGetKnownFolderPath(&FOLDERID_PROGRAMFILES,0,null_mut(),&mut p)<0 || p.is_null() {return Err("Impossibile trovare Program Files".into())}
    let base=PathBuf::from(ptr_text(p)); CoTaskMemFree(p.cast()); Ok(base.join("myDesktop").join("myDesktop.exe"))
} }
fn open_manager(access:u32)->Result<OwnedService,String>{ let h=unsafe{OpenSCManagerW(null(),null(),access)}; if h.is_null(){Err(last("Impossibile aprire il gestore servizi"))}else{Ok(OwnedService(h))} }
fn open_service(manager:&OwnedService, access:u32)->Result<OwnedService,String>{ let h=unsafe{OpenServiceW(manager.0,ws(SERVICE_NAME).as_ptr(),access)}; if h.is_null(){Err(last("Impossibile aprire il servizio"))}else{Ok(OwnedService(h))} }
fn token_info(token:Handle, class:u32)->Result<Vec<usize>,String>{ unsafe {
    let mut n=0; GetTokenInformation(token,class,null_mut(),0,&mut n); if n==0{return Err(last("Impossibile leggere il token"))}
    let mut buf=vec![0usize;(n as usize+size_of::<usize>()-1)/size_of::<usize>()];
    if GetTokenInformation(token,class,buf.as_mut_ptr().cast(),n,&mut n)==0{return Err(last("Impossibile leggere il token"))} Ok(buf)
} }
fn sid_string(token:Handle)->Result<String,String>{ unsafe {
    let data=token_info(token,1)?; let sid=*(data.as_ptr() as *const *mut c_void); let mut p=null_mut();
    if ConvertSidToStringSidW(sid,&mut p)==0{return Err(last("Impossibile leggere l'identità utente"))} let s=ptr_text(p); LocalFree(p.cast()); Ok(s)
} }
fn is_elevated(token:Handle)->bool { token_info(token,20).ok().is_some_and(|b| unsafe{*(b.as_ptr() as *const u32)!=0}) }
fn current_token()->Result<OwnedHandle,String>{ let mut t=null_mut(); if unsafe{OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut t)}==0 {Err(last("Impossibile aprire il token"))} else {Ok(OwnedHandle(t))} }
pub fn current_user_sid()->Result<String,String>{let token=current_token()?;sid_string(token.0)}
struct SecurityDescriptor(*mut c_void);
impl Drop for SecurityDescriptor { fn drop(&mut self) {unsafe{LocalFree(self.0);}} }
fn secure_attrs()->Result<SecurityDescriptor,String> {let mut sd=null_mut(); if unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(ws("O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)").as_ptr(),1,&mut sd,null_mut())}==0{Err(last("Impossibile creare i permessi protetti"))}else{Ok(SecurityDescriptor(sd))}}
fn service_identity(service:&OwnedService)->Result<String,String>{
    let mut needed=0; unsafe{QueryServiceConfigW(service.0,null_mut(),0,&mut needed)};
    if needed==0||needed>65536{return Err(last("Impossibile verificare il servizio"))}
    let mut buf=vec![0usize;(needed as usize+size_of::<usize>()-1)/size_of::<usize>()];
    if unsafe{QueryServiceConfigW(service.0,buf.as_mut_ptr().cast(),needed,&mut needed)}==0{return Err(last("Impossibile verificare il servizio"))}
    let cfg=unsafe{&*(buf.as_ptr() as *const QueryServiceConfig)}; if cfg.binary_path.is_null() || cfg.service_start_name.is_null(){return Err("Configurazione servizio non valida".into())}
    let command=unsafe{ptr_text(cfg.binary_path)}; let prefix=format!("\"{}\" --service ",installed_path()?.display());
    if !command.starts_with(&prefix) || cfg.service_type!=SERVICE_WIN32_OWN_PROCESS || !unsafe{ptr_text(cfg.service_start_name)}.eq_ignore_ascii_case("LocalSystem") {return Err("Il servizio esistente non corrisponde all'installazione myDesktop; nessuna modifica eseguita.".into())}
    let sid=&command[prefix.len()..];
    if !sid.starts_with("S-1-")||!sid.bytes().all(|c|c.is_ascii_digit()||c==b'S'||c==b'-'){return Err("Identità del servizio non valida".into())} Ok(sid.to_owned())
}
/// Read-only service state for the tray. An unrelated name collision is an error.
pub fn is_installed()->Result<bool,String>{
    let manager=open_manager(1)?;let h=unsafe{OpenServiceW(manager.0,ws(SERVICE_NAME).as_ptr(),1)};
    if h.is_null(){let e=unsafe{GetLastError()};return if e==1060{Ok(false)}else{Err(format!("Impossibile verificare il servizio: {}",std::io::Error::from_raw_os_error(e as i32)))}}
    let service=OwnedService(h);let expected=service_identity(&service)?;let token=current_token()?;if !sid_string(token.0)?.eq_ignore_ascii_case(&expected){return Err("Il servizio myDesktop è configurato per un altro utente Windows.".into())}Ok(true)
}

/// Must be explicitly called in an elevated installer process. Existing files are
/// reused only after handle-based checks of the entire path and ACL; no caller-supplied
/// destination or SID is accepted.
pub fn install_for(expected_sid:&str) -> Result<String,String> {
    let token=current_token()?; if !is_elevated(token.0) {return Err("Per installare il servizio occorre autorizzare UAC come amministratore.".into())}
    let sid=sid_string(token.0)?; if !sid.eq_ignore_ascii_case(expected_sid){return Err("Il servizio può essere abilitato solo con i privilegi amministrativi dell'utente Windows corrente, non con le credenziali di un altro account.".into())}
    let target=installed_path()?; let dir=target.parent().ok_or("Directory non valida")?;
    let base=dir.parent().ok_or("Directory Program Files non valida")?;
    let _parents=crate::protected::parent_chain(base)?;
    let manager=open_manager(0x0001|0x0002)?;
    // Refuse both unrelated service collisions and pre-existing directories.
    let existing=unsafe{OpenServiceW(manager.0,ws(SERVICE_NAME).as_ptr(),0x0004)};
    if !existing.is_null(){unsafe{CloseServiceHandle(existing)};return Err("Il servizio myDesktop è già presente; non verrà sovrascritto.".into())}
    if unsafe{GetLastError()}!=1060{return Err(last("Impossibile verificare eventuali servizi già presenti"))}
    let sd=secure_attrs()?;
    let attrs=SecurityAttributes{length:size_of::<SecurityAttributes>() as u32,descriptor:sd.0,inherit:0};
    let made=unsafe{CreateDirectoryW(wide(dir.as_os_str()).as_ptr(),&attrs)};
    if made==0 && unsafe{GetLastError()}!=183 {return Err(last("Impossibile creare la cartella protetta"))}
    let _directory=crate::protected::installation_dir(dir)?;
    match fs::symlink_metadata(&target){Ok(_)=>crate::protected::installed_file(&target)?,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(format!("Impossibile verificare il file esistente: {e}"))}
    let result=(|| -> Result<(),String>{
        let source=std::env::current_exe().map_err(|e|e.to_string())?;
        let mut input=fs::File::open(source).map_err(|e|e.to_string())?;
        let temp=dir.join("myDesktop.exe.installing");
        let raw=unsafe{CreateFileW(wide(temp.as_os_str()).as_ptr(),0x40000000,1,(&attrs as *const SecurityAttributes).cast(),1,0x00200080,null_mut())};
        if raw as isize == -1{return Err(last("Impossibile creare il file temporaneo protetto"))}
        let mut out=unsafe{fs::File::from_raw_handle(raw)};
        if let Err(e)=std::io::copy(&mut input,&mut out).and_then(|_|out.flush()).and_then(|_|out.sync_all()) {drop(out);let _=fs::remove_file(&temp);return Err(e.to_string())}
        drop(out);
        if let Err(e)=crate::protected::installed_file(&temp){let _=fs::remove_file(&temp);return Err(e)}
        if unsafe{MoveFileExW(wide(temp.as_os_str()).as_ptr(),wide(target.as_os_str()).as_ptr(),0x1|0x8)}==0{let e=last("Impossibile sostituire il file protetto; chiudi eventuali istanze e riprova");let _=fs::remove_file(&temp);return Err(e)}
        let binary=format!("\"{}\" --service {}",target.display(),sid);
        let service=unsafe{CreateServiceW(manager.0,ws(SERVICE_NAME).as_ptr(),ws("myDesktop Window Helper").as_ptr(),0x00010000|0x0010|0x0004|0x0020, SERVICE_WIN32_OWN_PROCESS,2,1,ws(&binary).as_ptr(),null(),null_mut(),null(),null(),null())};
        if service.is_null(){return Err(last("Impossibile creare il servizio"))} let service=OwnedService(service);
        if unsafe{StartServiceW(service.0,0,null())}==0{let error=last("Impossibile avviare il servizio");unsafe{DeleteService(service.0)};return Err(error)}
        let mut status:ServiceStatus=unsafe{zeroed()};
        for _ in 0..100 {if unsafe{QueryServiceStatus(service.0,&mut status)}==0{break} if status.state==SERVICE_RUNNING{return Ok(())} if status.state==SERVICE_STOPPED{break} std::thread::sleep(Duration::from_millis(100));}
        unsafe{ControlService(service.0,SERVICE_CONTROL_STOP,&mut status);DeleteService(service.0)};
        Err("Il servizio non è entrato nello stato in esecuzione ed è stato rimosso.".into())
    })();
    match result {Ok(())=>Ok(format!("Servizio installato per l'utente {sid}.")),Err(e)=>Err(format!("{e}. Se la creazione è stata interrotta, verificare il servizio e la cartella protetta prima di riprovare."))}
}

/// Stop/delete only the named service when its registered executable matches ours.
/// Deliberately leaves the protected executable in place: it can still be mapped by a
/// running process and is never deleted or overwritten as a side effect.
pub fn uninstall()->Result<String,String>{
    let token=current_token()?; if !is_elevated(token.0){return Err("Per rimuovere il servizio occorre autorizzare UAC come amministratore.".into())}
    let manager=open_manager(1)?; let service=open_service(&manager,0x0001|0x0004|0x0020|0x10000)?;
    let expected=service_identity(&service)?; if !sid_string(token.0)?.eq_ignore_ascii_case(&expected){return Err("Il servizio myDesktop appartiene a un altro utente Windows.".into())}
    let mut status:ServiceStatus=unsafe{zeroed()}; unsafe{ControlService(service.0,SERVICE_CONTROL_STOP,&mut status)};
    for _ in 0..100 { if unsafe{QueryServiceStatus(service.0,&mut status)}==0{return Err(last("Impossibile verificare l'arresto del servizio"))} if status.state==SERVICE_STOPPED{break} std::thread::sleep(Duration::from_millis(100)); }
    if status.state!=SERVICE_STOPPED{return Err("Il servizio non si è arrestato; non è stato eliminato.".into())}
    if unsafe{DeleteService(service.0)}==0{return Err(last("Impossibile rimuovere il servizio"))}
    Ok("Servizio rimosso. Il file protetto in Program Files è conservato e può essere rimosso da un amministratore.".into())
}

static STATUS_HANDLE: AtomicIsize=AtomicIsize::new(0);
static STOP_EVENT: AtomicIsize=AtomicIsize::new(0);
static SERVICE_SID: Mutex<Option<String>>=Mutex::new(None);
fn set_status(state:u32, accepted:u32, exit:u32, hint:u32){ let h=STATUS_HANDLE.load(Ordering::Acquire) as ScHandle; if !h.is_null(){ let s=ServiceStatus{service_type:SERVICE_WIN32_OWN_PROCESS,state,accepted,win32_exit:exit,service_exit:0,checkpoint:0,wait_hint:hint}; unsafe{SetServiceStatus(h,&s)}; } }
unsafe extern "system" fn handler(code:u32,_:u32,_:*mut c_void,_:*mut c_void)->u32 { if code==SERVICE_CONTROL_STOP || code==SERVICE_CONTROL_SHUTDOWN {set_status(SERVICE_STOP_PENDING,0,0,10000);let event=STOP_EVENT.load(Ordering::Acquire) as Handle;if !event.is_null(){unsafe{SetEvent(event)};}} 0 }
struct Helper { session:u32, process:OwnedHandle }
fn active_sessions()->Vec<u32>{unsafe{let mut p=null_mut();let mut count=0; if WTSEnumerateSessionsW(null_mut(),0,1,&mut p,&mut count)==0{return Vec::new()} let mut v=Vec::new();for s in std::slice::from_raw_parts(p,count as usize){if s.state==0 {v.push(s.id)}} WTSFreeMemory(p.cast());v}}
fn report_error(message:&str){unsafe{let source=RegisterEventSourceW(null(),ws("myDesktopWindowHelper").as_ptr());if source.is_null(){return}let text=ws(message);let strings=[text.as_ptr()];ReportEventW(source,1,0,0,null(),1,0,strings.as_ptr(),null());DeregisterEventSource(source);}}
fn launch_helper(session:u32, allowed_sid:&str)->Result<OwnedHandle,String>{ unsafe {
    let mut raw=null_mut(); if WTSQueryUserToken(session,&mut raw)==0{return Err(last("Token della sessione non disponibile"))} let token=OwnedHandle(raw);
    if !sid_string(token.0)?.eq_ignore_ascii_case(allowed_sid){return Err("Sessione di un altro utente".into())}
    let high=if is_elevated(token.0){token.0}else{let data=token_info(token.0,19)?;let h=*(data.as_ptr() as *const Handle);if h.is_null(){return Err("L'utente non possiede un token amministrativo".into())}h};
    let linked=if high==token.0 {None}else{Some(OwnedHandle(high))};
    let linked_session=token_info(high,12)?;
    if !is_elevated(high) || !sid_string(high)?.eq_ignore_ascii_case(allowed_sid) || *(linked_session.as_ptr() as *const u32)!=session{return Err("Token elevato non autorizzato".into())}
    let mut primary=null_mut(); if DuplicateTokenEx(high,0x02000000,null(),2,1,&mut primary)==0{return Err(last("Impossibile duplicare il token"))} let primary=OwnedHandle(primary); drop(linked);
    let executable=installed_path()?; let actual=std::env::current_exe().map_err(|e|e.to_string())?;
    if !actual.to_string_lossy().eq_ignore_ascii_case(&executable.to_string_lossy()){return Err("Il servizio non è eseguito dalla posizione protetta".into())}
    let mut command=ws(&format!("\"{}\" --elevated-helper",executable.display())); let mut desktop=ws("winsta0\\default");
    let mut startup:StartupInfo=zeroed(); startup.size=size_of::<StartupInfo>() as u32; startup.desktop=desktop.as_mut_ptr();
    let mut pi:ProcessInfo=zeroed(); let mut system_dir=[0u16;32768]; if GetSystemDirectoryW(system_dir.as_mut_ptr(),system_dir.len() as u32)==0{return Err(last("Directory di sistema non disponibile"))}
    if CreateProcessAsUserW(primary.0,wide(executable.as_os_str()).as_ptr(),command.as_mut_ptr(),null(),null(),0,0,null_mut(),system_dir.as_ptr(),&mut startup,&mut pi)==0{return Err(last("Impossibile avviare il componente della sessione"))}
    CloseHandle(pi.thread); Ok(OwnedHandle(pi.process))
} }
unsafe extern "system" fn service_main(_count:u32,_args:*mut *mut u16){unsafe{
    let h=RegisterServiceCtrlHandlerExW(ws(SERVICE_NAME).as_ptr(),Some(handler),null_mut()); if h.is_null(){return} STATUS_HANDLE.store(h as isize,Ordering::Release);
    set_status(SERVICE_START_PENDING,0,0,10000); let event=CreateEventW(null(),1,0,null()); if event.is_null(){set_status(SERVICE_STOPPED,0,1,0);return} let event=OwnedHandle(event); STOP_EVENT.store(event.0 as isize,Ordering::Release);
    let sid=SERVICE_SID.lock().unwrap_or_else(|e|e.into_inner()).clone().unwrap_or_default(); if sid.is_empty(){set_status(SERVICE_STOPPED,0,87,0);return}
    set_status(SERVICE_RUNNING,SERVICE_ACCEPT_STOP|SERVICE_ACCEPT_SHUTDOWN,0,0); let mut helpers:Vec<Helper>=Vec::new();let mut failures:Vec<(u32,String)>=Vec::new();
    loop { helpers.retain(|h|WaitForSingleObject(h.process.0,0)!=WAIT_OBJECT_0); for session in active_sessions(){if !helpers.iter().any(|h|h.session==session){match launch_helper(session,&sid){Ok(process)=>{helpers.push(Helper{session,process});failures.retain(|(id,_)|*id!=session);},Err(e)=>{if e!="Sessione di un altro utente" && !failures.iter().any(|(id,old)|*id==session&&old==&e){report_error(&format!("Sessione {session}: {e}"));failures.retain(|(id,_)|*id!=session);failures.push((session,e));}}}}} if WaitForSingleObject(event.0,5000)==WAIT_OBJECT_0{break} }
    // Only terminate helper processes created and retained by this service.
    for h in helpers {TerminateProcess(h.process.0,0);WaitForSingleObject(h.process.0,2000);}
    STOP_EVENT.store(0,Ordering::Release);set_status(SERVICE_STOPPED,0,0,0);
} }
/// Entry point when invoked by the SCM as `--service <installer-SID>`.
pub fn run(sid:&str)->Result<(),String>{
    if !sid.starts_with("S-1-") || !sid.bytes().all(|b|b.is_ascii_digit()||b==b'S'||b==b'-') {return Err("Identità del servizio non valida".into())}
    *SERVICE_SID.lock().unwrap_or_else(|e|e.into_inner())=Some(sid.to_owned());
    let mut name=ws(SERVICE_NAME); let table=[ServiceTableEntry{name:name.as_mut_ptr(),function:Some(service_main)},ServiceTableEntry{name:null_mut(),function:None}];
    if unsafe{StartServiceCtrlDispatcherW(table.as_ptr())}==0{Err(last("Avviare il servizio tramite Windows"))}else{Ok(())}
}

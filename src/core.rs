use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, mem::{size_of, zeroed}, os::windows::ffi::OsStrExt, path::{Path, PathBuf}, process::Command, sync::atomic::{AtomicU64, Ordering}, thread, time::Duration};

type Hwnd = isize;
#[repr(C)] #[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
#[repr(C)] #[derive(Clone, Copy, Default)] struct Point { x: i32, y: i32 }
#[repr(C)] #[derive(Clone, Copy)] struct Placement { length: u32, flags: u32, show: u32, min: Point, max: Point, normal: Rect }
#[repr(C)] struct MonitorInfo { size: u32, monitor: Rect, work: Rect, flags: u32, device: [u16; 32] }
#[repr(C)] struct DisplayDevice { size: u32, name: [u16; 32], string: [u16; 128], flags: u32, id: [u16; 128], key: [u16; 128] }

#[link(name="user32")] unsafe extern "system" {
    fn EnumWindows(cb: Option<unsafe extern "system" fn(Hwnd, isize) -> i32>, arg: isize) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn GetWindow(hwnd: Hwnd, cmd: u32) -> Hwnd;
    fn GetWindowLongW(hwnd: Hwnd, idx: i32) -> i32;
    fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max: i32) -> i32;
    fn GetClassNameW(hwnd: Hwnd, name: *mut u16, max: i32) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn GetWindowPlacement(hwnd: Hwnd, p: *mut std::ffi::c_void) -> i32;
    fn GetWindowRect(hwnd: Hwnd, r: *mut std::ffi::c_void) -> i32;
    fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> isize;
    fn EnumDisplayMonitors(dc: isize, clip: *const Rect, cb: Option<unsafe extern "system" fn(isize,isize,*mut Rect,isize)->i32>, arg: isize) -> i32;
    fn GetMonitorInfoW(m: isize, info: *mut MonitorInfo) -> i32;
    fn EnumDisplayDevicesW(device: *const u16, index: u32, display: *mut DisplayDevice, flags: u32) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
}
#[link(name="kernel32")] unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn QueryFullProcessImageNameW(process: *mut std::ffi::c_void, flags: u32, path: *mut u16, size: *mut u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
}
#[link(name="dwmapi")] unsafe extern "system" { fn DwmGetWindowAttribute(hwnd: Hwnd, attr: u32, value: *mut u32, size: u32) -> i32; }
#[link(name="shell32")] unsafe extern "system" { fn ShellExecuteW(hwnd: Hwnd, operation: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> isize; }

fn text(buf: &[u16]) -> String { String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]) }
fn db_path() -> Result<PathBuf,String> { let p=std::env::var_os("APPDATA").ok_or("Cartella APPDATA non disponibile")?; Ok(PathBuf::from(p).join("myDesktop").join("layout.json")) }

#[derive(Clone, Serialize, Deserialize)] struct Monitor { id: String, device: String, rect: Rect, work: Rect, primary: bool }
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)] struct WindowIdentity { hwnd:u64, pid:u32, created:u64 }
#[derive(Clone, Serialize, Deserialize)] struct Window { exe: String, class: String, title: String, folder: Option<String>, monitor_id: String, monitor_device: String, rect: Rect, show: u32, #[serde(default)] identity:Option<WindowIdentity> }
#[derive(Serialize, Deserialize)] struct Layout { version: u32, windows: Vec<Window> }
#[derive(Clone)] struct Live { hwnd: Hwnd, exe: String, class: String, title: String, folder: Option<String>, monitor: isize, rect: Rect, show: u32, identity:Option<WindowIdentity> }

unsafe fn monitor_data(h: isize) -> Option<Monitor> { unsafe {
    let mut mi: MonitorInfo=zeroed(); mi.size=size_of::<MonitorInfo>() as u32;
    if GetMonitorInfoW(h, &mut mi)==0 { return None; }
    let device=text(&mi.device);
    let mut dd: DisplayDevice=zeroed(); dd.size=size_of::<DisplayDevice>() as u32;
    let id=if EnumDisplayDevicesW(mi.device.as_ptr(),0,&mut dd,0)!=0 { let key=text(&dd.key); if key.is_empty(){text(&dd.id)} else {key} } else { String::new() };
    Some(Monitor { id, device, rect:mi.monitor, work:mi.work, primary:mi.flags & 1 != 0 })
} }
unsafe extern "system" fn monitor_cb(h:isize,_:isize,_:*mut Rect,arg:isize)->i32 { unsafe { if let Some(m)=monitor_data(h) { (*(arg as *mut Vec<Monitor>)).push(m); } } 1 }
fn monitors()->Vec<Monitor>{ let mut m=Vec::new(); unsafe { EnumDisplayMonitors(0,std::ptr::null(),Some(monitor_cb),&mut m as *mut _ as isize); } m }
unsafe fn process_path(pid:u32)->Option<String>{ unsafe { let p=OpenProcess(0x1000,0,pid); if p.is_null(){return None} let mut b=vec![0u16;32768]; let mut n=b.len() as u32; let ok=QueryFullProcessImageNameW(p,0,b.as_mut_ptr(),&mut n); CloseHandle(p); if ok==0 {None} else {Some(String::from_utf16_lossy(&b[..n as usize]))} } }
unsafe extern "system" fn window_cb(h:Hwnd,arg:isize)->i32 { unsafe {
    if IsWindowVisible(h)==0 || GetWindow(h,4)!=0 || GetWindowLongW(h,-20)&0x80!=0 { return 1; }
    let mut cloaked=0u32; if DwmGetWindowAttribute(h,14,&mut cloaked,4)==0 && cloaked!=0 {return 1}
    let mut pid=0; GetWindowThreadProcessId(h,&mut pid); if pid==std::process::id(){return 1}
    let Some(exe)=process_path(pid) else{return 1};
    let mut cls=[0u16;256]; GetClassNameW(h,cls.as_mut_ptr(),cls.len() as i32); let class=text(&cls);
    if ["Progman","WorkerW","Shell_TrayWnd","Windows.UI.Core.CoreWindow"].contains(&class.as_str()) {return 1}
    let len=GetWindowTextLengthW(h).clamp(0,32767); let mut tb=vec![0u16;len as usize+1]; GetWindowTextW(h,tb.as_mut_ptr(),tb.len() as i32); let title=text(&tb);
    if title.is_empty() {return 1}
    let mut wp:Placement=zeroed(); wp.length=size_of::<Placement>() as u32; if GetWindowPlacement(h,(&mut wp as *mut Placement).cast())==0{return 1}
    let monitor=MonitorFromWindow(h,2); let Some(mi)=monitor_data(monitor) else{return 1};
    let mut r=Rect::default(); if wp.show==1 { GetWindowRect(h,(&mut r as *mut Rect).cast()); } else { r=wp.normal; let dx=mi.work.left-mi.rect.left; let dy=mi.work.top-mi.rect.top; r.left+=dx;r.right+=dx;r.top+=dy;r.bottom+=dy; }
    let identity=crate::ipc::process_created(pid).map(|created|WindowIdentity{hwnd:h as u64,pid,created});
    (*(arg as *mut Vec<Live>)).push(Live{ hwnd:h,exe,class,title,folder:None,monitor,rect:r,show:wp.show,identity }); 1
} }
fn windows()->Result<Vec<Live>,String>{ let mut v:Vec<Live>=Vec::new(); unsafe{EnumWindows(Some(window_cb),&mut v as *mut _ as isize);} let folders=crate::explorer::capture()?; for w in &mut v { w.folder=folders.get(&w.hwnd).cloned(); if w.class=="CabinetWClass" && w.folder.is_none(){return Err(format!("Impossibile leggere la cartella di Esplora file: {}",w.title));} } Ok(v) }

pub fn save()->Result<String,String>{
    save_to(&db_path()?)
}
pub fn save_to(path: &Path)->Result<String,String>{
    let mut out=Vec::new(); for w in windows()?{ if let Some(m)=unsafe{monitor_data(w.monitor)} {out.push(Window{exe:w.exe,class:w.class,title:w.title,folder:w.folder,monitor_id:m.id,monitor_device:m.device,rect:Rect{left:w.rect.left-m.rect.left,top:w.rect.top-m.rect.top,right:w.rect.right-m.rect.left,bottom:w.rect.bottom-m.rect.top},show:w.show,identity:w.identity});} }
    if out.is_empty(){return Err("Nessuna finestra gestibile trovata: configurazione precedente conservata.".into())}
    fs::create_dir_all(path.parent().ok_or("Cartella del file non valida")?).map_err(|e|e.to_string())?;
    let data=serde_json::to_vec_pretty(&Layout{version:1,windows:out}).map_err(|e|e.to_string())?;
    write_layout(path, &data)?;
    Ok(format!("Configurazione salvata in {}",path.display()))
}
fn write_layout(path: &Path, data: &[u8]) -> Result<(), String> {
    static NEXT: AtomicU64=AtomicU64::new(0);
    let mut name=path.as_os_str().to_os_string(); name.push(format!(".{}.{}.tmp",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    let tmp=PathBuf::from(name);
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e|format!("Impossibile creare il file temporaneo: {e}"))?;
    if let Err(e)=file.write_all(&data){drop(file);let _=fs::remove_file(&tmp);return Err(format!("Impossibile scrivere il file temporaneo: {e}"));} drop(file);
    let from:Vec<u16>=tmp.as_os_str().encode_wide().chain(Some(0)).collect(); let to:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe{MoveFileExW(from.as_ptr(),to.as_ptr(),0x9)}==0{let e=std::io::Error::last_os_error();let _=fs::remove_file(&tmp);return Err(format!("Impossibile sostituire il salvataggio: {e}"))}
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    fn monitor(id:&str,device:&str,primary:bool)->Monitor{Monitor{id:id.into(),device:device.into(),rect:Rect::default(),work:Rect::default(),primary}}
    fn saved()->Window{Window{exe:"C:\\App.exe".into(),class:"Class".into(),title:"first".into(),folder:None,monitor_id:"physical-A".into(),monitor_device:"display1".into(),rect:Rect::default(),show:1,identity:None}}
    fn live(hwnd:isize,title:&str,folder:Option<&str>)->Live{Live{hwnd,exe:"c:\\app.exe".into(),class:"Class".into(),title:title.into(),folder:folder.map(str::to_owned),monitor:0,rect:Rect::default(),show:1,identity:None}}
    #[test] fn selects_saved_physical_monitor_and_falls_back_to_primary(){let ms=vec![monitor("physical-B","display1",true),monitor("physical-A","display2",false)];let mut s=saved();assert_eq!(target(&s,&ms).unwrap().device,"display2");s.monitor_id="missing".into();assert_eq!(target(&s,&ms).unwrap().device,"display1");}
    #[test] fn matches_title_and_never_reuses_window(){let s=saved();let a=vec![live(1,"other",None),live(2,"first",None)];let mut used=HashSet::new();assert_eq!(best(&s,&a,&used),Some(2));used.insert(2);assert_eq!(best(&s,&a,&used),Some(1));}
    #[test] fn matches_explorer_folder(){let mut s=saved();s.folder=Some("C:\\folder".into());let a=vec![live(1,"first",Some("C:\\other")),live(2,"other",Some("c:\\FOLDER"))];assert_eq!(best(&s,&a,&HashSet::new()),Some(2));}
    #[test] fn matches_mfc_changed_handles_only_for_same_style_and_title(){let mut s=saved();s.class="Afx:00400000:28:00010007:00000000:00000000".into();let mut w=live(1,"first",None);w.class="Afx:00700000:28:00020007:00000000:00000000".into();assert!(matches(&s,&w));w.title="another".into();assert!(!matches(&s,&w));w.title="first".into();w.class="Afx:00700000:29:00020007:00000000:00000000".into();assert!(!matches(&s,&w));}
    #[test] fn exact_class_match_outranks_mfc_fallback(){let mut s=saved();s.class="Afx:00400000:28:00010007:00000000:00000000".into();let mut a=live(1,"first",None);a.class="Afx:00700000:28:00020007:00000000:00000000".into();let mut b=live(2,"first",None);b.class=s.class.clone();assert_eq!(best(&s,&[a,b],&HashSet::new()),Some(2));}
    #[test] fn mfc_fallback_requires_same_executable_and_valid_class(){let mut s=saved();s.class="Afx:00400000:28:00010007:00000000:00000000".into();let mut w=live(1,"first",None);w.class="Afx:00700000:28:00020007:00000000:00000000".into();w.exe="C:\\Another.exe".into();assert!(!matches(&s,&w));w.exe=s.exe.clone();w.class="Afx:00700000:28:broken".into();assert!(!matches(&s,&w));}
    fn chrome_saved(title:&str,id:Option<WindowIdentity>)->Window{let mut w=saved();w.exe="C:\\Chrome\\chrome.exe".into();w.class="Chrome_WidgetWin_1".into();w.title=title.into();w.identity=id;w}
    fn chrome_live(hwnd:isize,title:&str,id:Option<WindowIdentity>)->Live{let mut w=live(hwnd,title,None);w.exe="C:\\Chrome\\chrome.exe".into();w.class="Chrome_WidgetWin_1".into();w.identity=id;w}
    fn id(hwnd:u64,created:u64)->WindowIdentity{WindowIdentity{hwnd,pid:42,created}}
    #[test] fn chrome_reordered_and_changed_titles_keep_their_own_handles(){
        let saved=vec![chrome_saved("Prima",Some(id(10,123))),chrome_saved("Seconda",Some(id(20,123)))];
        let live=vec![chrome_live(20,"Seconda",Some(id(20,123))),chrome_live(10,"Nuova scheda",Some(id(10,123)))];
        assert_eq!(plan_matches(&saved,&live,&HashSet::new()),vec![Some(10),Some(20)]);
    }
    #[test] fn chrome_missing_title_does_not_steal_other_saved_window(){
        let saved=vec![chrome_saved("Prima",None),chrome_saved("Seconda",None)];
        let live=vec![chrome_live(20,"Seconda",None),chrome_live(30,"Altra",None)];
        assert_eq!(plan_matches(&saved,&live,&HashSet::new()),vec![None,Some(20)]);
    }
    #[test] fn chrome_duplicate_titles_are_not_guessed(){
        let saved=vec![chrome_saved("Scheda",None),chrome_saved("Scheda",None)];
        let live=vec![chrome_live(10,"Scheda",None),chrome_live(20,"Scheda",None)];
        assert_eq!(plan_matches(&saved,&live,&HashSet::new()),vec![None,None]);
    }
    #[test] fn old_layouts_read_without_window_identity(){
        let mut json=serde_json::to_value(saved()).unwrap();json.as_object_mut().unwrap().remove("identity");
        assert!(serde_json::from_value::<Window>(json).unwrap().identity.is_none());
    }
    #[test] fn recycled_process_identity_is_not_trusted(){
        let saved=vec![chrome_saved("Vecchia",Some(id(10,123)))];
        let live=vec![chrome_live(10,"Diversa",Some(id(10,456)))];
        assert_eq!(plan_matches(&saved,&live,&HashSet::new()),vec![None]);
    }
    #[test] #[ignore = "read-only check against currently open Chrome windows"]
    fn real_chrome_windows_keep_their_identity_when_order_and_title_change(){
        let mut observed:Vec<Live>=Vec::new();unsafe{EnumWindows(Some(window_cb),&mut observed as *mut _ as isize);}
        let mut chrome:Vec<Live>=observed.into_iter().filter(|w|is_chrome(&w.exe)&&w.identity.is_some()).collect();
        assert!(chrome.len()>=2,"Servono almeno due finestre Chrome identificabili");
        let saved:Vec<Window>=chrome.iter().map(|w|Window{exe:w.exe.clone(),class:w.class.clone(),title:w.title.clone(),folder:None,monitor_id:String::new(),monitor_device:String::new(),rect:w.rect,show:w.show,identity:w.identity.clone()}).collect();
        chrome.reverse();chrome[0].title.push_str(" - titolo modificato nella prova");
        let actual=plan_matches(&saved,&chrome,&HashSet::new());
        let expected:Vec<_>=saved.iter().map(|w|w.identity.as_ref().map(|id|id.hwnd as isize)).collect();
        assert_eq!(actual,expected);
    }
    #[test] fn named_layout_writes_selected_file_and_preserves_other_file(){let dir=std::env::temp_dir().join(format!("mydesktop-layout-test-{}-{:?}",std::process::id(),std::thread::current().id()));fs::create_dir(&dir).unwrap();let a=dir.join("first.json");let b=dir.join("second.json");write_layout(&a,b"first").unwrap();write_layout(&b,b"second").unwrap();write_layout(&a,b"updated").unwrap();assert_eq!(fs::read(&a).unwrap(),b"updated");assert_eq!(fs::read(&b).unwrap(),b"second");fs::remove_file(a).unwrap();fs::remove_file(b).unwrap();fs::remove_dir(dir).unwrap();}
}
fn equivalent(a:&str,b:&str)->bool{a.eq_ignore_ascii_case(b)}
fn mfc_style(class:&str)->Option<&str>{ let mut fields=class.split(':'); if fields.next()? != "Afx" {return None} let instance=fields.next()?; let style=fields.next()?; let cursor=fields.next()?; let background=fields.next()?; let icon=fields.next()?; if fields.next().is_some() || [instance,style,cursor,background,icon].iter().any(|s|s.is_empty() || !s.bytes().all(|c|c.is_ascii_hexdigit())) {return None} Some(style) }
fn matches(saved:&Window,live:&Live)->bool { equivalent(&saved.exe,&live.exe) && (saved.class==live.class || (saved.title==live.title && mfc_style(&saved.class).is_some_and(|s|mfc_style(&live.class).is_some_and(|v|s.eq_ignore_ascii_case(v))))) && match &saved.folder {Some(folder)=>live.folder.as_ref().is_some_and(|f|equivalent(folder,f)),None=>true} }
fn best(saved:&Window, live:&[Live],used:&HashSet<Hwnd>)->Option<Hwnd>{ live.iter().filter(|w|!used.contains(&w.hwnd)&&matches(saved,w)).max_by_key(|w|(w.title==saved.title,w.class==saved.class)).map(|w|w.hwnd) }
fn is_chrome(exe:&str)->bool { Path::new(exe).file_name().is_some_and(|name|name.to_string_lossy().eq_ignore_ascii_case("chrome.exe")) }
/// Reserve all confident matches before allowing weaker matches to consume windows.
fn plan_matches(saved:&[Window],live:&[Live],used:&HashSet<Hwnd>)->Vec<Option<Hwnd>> {
    let mut result=vec![None;saved.len()];let mut reserved=used.clone();
    // Handles only count when the owning process and its creation time also match.
    for (i,s) in saved.iter().enumerate() {
        let Some(id)=s.identity.as_ref() else{continue};
        if let Some(w)=live.iter().find(|w|!reserved.contains(&w.hwnd)&&w.identity.as_ref()==Some(id)&&matches(s,w)) {result[i]=Some(w.hwnd);reserved.insert(w.hwnd);}
    }
    // Exact titles are assigned globally before any fallback, so a changed title
    // cannot steal another saved window. Ambiguous Chrome titles are not guessed.
    for (i,s) in saved.iter().enumerate() {
        if result[i].is_some(){continue}
        let candidates:Vec<_>=live.iter().filter(|w|!reserved.contains(&w.hwnd)&&matches(s,w)&&w.title==s.title).collect();
        if is_chrome(&s.exe) {
            let competing=saved.iter().enumerate().filter(|(j,other)|result[*j].is_none()&&is_chrome(&other.exe)&&equivalent(&s.exe,&other.exe)&&s.class==other.class&&s.title==other.title).count();
            if candidates.len()!=1 || competing!=1 {continue}
        }
        if let Some(w)=candidates.first(){result[i]=Some(w.hwnd);reserved.insert(w.hwnd);}
    }
    for (i,s) in saved.iter().enumerate() {
        if result[i].is_none() && !is_chrome(&s.exe) {if let Some(h)=best(s,live,&reserved){result[i]=Some(h);reserved.insert(h);}}
    }
    result
}
fn launch_application(exe:&str)->Result<(),String>{
    let mut command=Command::new(exe);
    if let Some(parent)=Path::new(exe).parent(){command.current_dir(parent);}
    match command.spawn(){
        Ok(_)=>Ok(()),
        Err(e) if e.raw_os_error()==Some(740)=>{
            // The Windows shell performs the normal UAC consent flow when needed.
            let file:Vec<u16>=std::ffi::OsStr::new(exe).encode_wide().chain(Some(0)).collect();
            let directory:Option<Vec<u16>>=Path::new(exe).parent().map(|p|p.as_os_str().encode_wide().chain(Some(0)).collect());
            let result=unsafe{ShellExecuteW(0,std::ptr::null(),file.as_ptr(),std::ptr::null(),directory.as_ref().map_or(std::ptr::null(),|p|p.as_ptr()),1)};
            if result>32 {Ok(())} else {Err(format!("Avvio con richiesta di autorizzazione non riuscito (codice {result}): {exe}"))}
        },
        Err(e)=>Err(format!("Impossibile avviare {exe}: {e}")),
    }
}
fn target<'a>(w:&Window,monitors:&'a [Monitor])->Option<&'a Monitor>{
    // Device registry key identifies a physical display across changes to its position.
    let by_id=if w.monitor_id.is_empty(){None}else{monitors.iter().find(|m|m.id.eq_ignore_ascii_case(&w.monitor_id))};
    by_id.or_else(||if w.monitor_id.is_empty(){monitors.iter().find(|m|m.device.eq_ignore_ascii_case(&w.monitor_device))}else{None}).or_else(||monitors.iter().find(|m|m.primary)).or_else(||monitors.first())
}
fn state_kind(show:u32)->u8 { if show==3 {1} else if [2,6,7,11].contains(&show) {2} else {0} }
fn interpolate(a:i32,b:i32,step:i64,total:i64)->i32 { (i64::from(a)+(i64::from(b)-i64::from(a))*step/total) as i32 }
#[derive(Debug,PartialEq,Eq)] enum PlacementAction { None, StateOnly, Geometry }
fn placement_action(old:Rect,old_show:u32,new:Rect,new_show:u32)->PlacementAction {
    if old!=new {PlacementAction::Geometry} else if state_kind(old_show)!=state_kind(new_show) {PlacementAction::StateOnly} else {PlacementAction::None}
}
#[cfg(test)]
#[test]
fn skips_unchanged_window_and_distinguishes_state_and_geometry() {
    let r=Rect{left:10,top:20,right:110,bottom:120};
    assert_eq!(placement_action(r,1,r,1),PlacementAction::None);
    assert_eq!(placement_action(r,3,r,3),PlacementAction::None);
    assert_eq!(placement_action(r,1,r,3),PlacementAction::StateOnly);
    assert_eq!(placement_action(r,1,Rect{left:11,right:111,..r},1),PlacementAction::Geometry);
    assert_eq!(interpolate(-100,100,20,20),100);
}
#[cfg(test)]
#[test]
fn actual_window_is_untouched_then_moves_smoothly() {
    use std::{ffi::c_void, ptr::{null,null_mut}, sync::atomic::{AtomicUsize,Ordering}};
    static CHANGES:AtomicUsize=AtomicUsize::new(0);
    unsafe extern "system" fn proc(h:*mut c_void,m:u32,w:usize,l:isize)->isize {
        if m==0x46 {CHANGES.fetch_add(1,Ordering::SeqCst);} unsafe{crate::DefWindowProcW(h,m,w,l)}
    }
    let name:Vec<u16>="myDesktopPlacementTest".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let instance=crate::GetModuleHandleW(null());let class=crate::WndClassW{style:0,wnd_proc:Some(proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu_name:null(),class_name:name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let window=crate::CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,40,40,200,100,null_mut(),null_mut(),instance,null_mut());assert!(!window.is_null());let hwnd=window as isize;
        let mut rect=Rect::default();assert_ne!(GetWindowRect(hwnd,(&mut rect as *mut Rect).cast()),0);
        let live=Live{hwnd,exe:String::new(),class:String::new(),title:String::new(),folder:None,monitor:0,rect,show:1,identity:None};
        CHANGES.store(0,Ordering::SeqCst);place_window(hwnd,&live,rect,1).unwrap();assert_eq!(CHANGES.load(Ordering::SeqCst),0);
        let goal=Rect{left:rect.left+20,right:rect.right+20,..rect};let start=std::time::Instant::now();place_window(hwnd,&live,goal,1).unwrap();
        assert!(start.elapsed()>=Duration::from_millis(450));let mut actual=Rect::default();assert_ne!(GetWindowRect(hwnd,(&mut actual as *mut Rect).cast()),0);assert_eq!(actual,goal);assert!(CHANGES.load(Ordering::SeqCst)>1);
        crate::DestroyWindow(window);
    }
}
fn place_window(h:Hwnd, live:&Live, target:Rect, show:u32)->Result<(),String> {
    let old_state=state_kind(live.show); let new_state=state_kind(show);
    let action=placement_action(live.rect,live.show,target,show);
    if action==PlacementAction::None {return Ok(());}
    let width=(target.right-target.left).max(1); let height=(target.bottom-target.top).max(1);
    if action==PlacementAction::StateOnly {
        unsafe {ShowWindow(h, if new_state==1 {3} else if new_state==2 {6} else {9});}
        let mut p:Placement=unsafe{zeroed()};p.length=size_of::<Placement>() as u32;
        if unsafe{GetWindowPlacement(h,(&mut p as *mut Placement).cast())}!=0 && state_kind(p.show)==new_state {return Ok(());}
        return crate::ipc::move_window(h,target.left,target.top,width,height,show);
    }
    // Preserve normal windows' state and size whenever possible. Minimized or
    // maximized windows must first be restored to change their normal rectangle.
    if old_state!=0 {unsafe{ShowWindow(h,9);}}
    let size_same=live.rect.right-live.rect.left==width && live.rect.bottom-live.rect.top==height;
    let smooth=old_state==0 && new_state==0;
    let steps=if smooth {20} else {1};
    let started=std::time::Instant::now();
    for step in 1..=steps {
        if smooth {let due=Duration::from_millis(500*step as u64/steps as u64);if let Some(wait)=due.checked_sub(started.elapsed()){thread::sleep(wait);}}
        let x=interpolate(live.rect.left,target.left,step,steps);
        let y=interpolate(live.rect.top,target.top,step,steps);
        let w=interpolate(live.rect.right-live.rect.left,width,step,steps);
        let hgt=interpolate(live.rect.bottom-live.rect.top,height,step,steps);
        let flags=0x14 | if size_same {1} else {0};
        if unsafe{SetWindowPos(h,0,x,y,w,hgt,flags)}==0 {
            let error=std::io::Error::last_os_error();
            return if error.raw_os_error()==Some(5) {crate::ipc::move_window(h,target.left,target.top,width,height,show)} else {Err(error.to_string())};
        }
    }
    if new_state!=0 {unsafe{ShowWindow(h,if new_state==1 {3}else{6});}}
    Ok(())
}
pub fn restore()->Result<String,String>{
    restore_from(&db_path()?)
}
pub fn restore_from(path: &Path)->Result<String,String>{
    let data=fs::read(path).map_err(|e|format!("Impossibile leggere la configurazione salvata: {e}"))?; let layout:Layout=serde_json::from_slice(&data).map_err(|e|format!("Configurazione non valida: {e}"))?; if layout.version!=1{return Err("Versione della configurazione non supportata.".into())}
    let monitors=monitors(); if monitors.is_empty(){return Err("Nessun monitor disponibile.".into())}
    let mut used=HashSet::new(); let mut succeeded=0; let mut failed=Vec::new();let mut chrome_launches=HashSet::new();
    for (index,s) in layout.windows.iter().enumerate() {
        let mut current=windows()?; let mut found=plan_matches(&layout.windows[index..],&current,&used)[0];
        if found.is_none(){
            // Chrome cannot reconstruct a specific window's tabs from its title.
            // Never launch another instance when other Chrome windows are present.
            if is_chrome(&s.exe) && (current.iter().any(|w|equivalent(&s.exe,&w.exe)) || !chrome_launches.insert(s.exe.to_lowercase())) {
                failed.push(format!("{} (finestra Chrome non identificabile con certezza)",s.title));continue;
            }
            let launched=if let Some(folder)=&s.folder{crate::explorer::launch(folder).map_err(|e|e.to_string())}else{launch_application(&s.exe)};
            match launched { Ok(())=>{for _ in 0..30{thread::sleep(Duration::from_millis(300));current=windows()?;found=plan_matches(&layout.windows[index..],&current,&used)[0];if found.is_some(){break}}}, Err(error)=>{failed.push(format!("{} ({error})",s.title));continue;} }
        }
        if let Some(h)=found { used.insert(h); let m=target(s,&monitors).unwrap(); let r=s.rect; let mut x=m.rect.left+r.left; let mut y=m.rect.top+r.top; let mut width=(r.right-r.left).max(1); let mut height=(r.bottom-r.top).max(1); let same=if s.monitor_id.is_empty(){m.device.eq_ignore_ascii_case(&s.monitor_device)}else{m.id.eq_ignore_ascii_case(&s.monitor_id)}; if !same {width=width.min(m.work.right-m.work.left);height=height.min(m.work.bottom-m.work.top);x=x.clamp(m.work.left,m.work.right-width);y=y.clamp(m.work.top,m.work.bottom-height);} let desired=Rect{left:x,top:y,right:x+width,bottom:y+height}; if let Some(live)=current.iter().find(|w|w.hwnd==h) {match place_window(h,live,desired,s.show){Ok(())=>succeeded+=1,Err(error)=>failed.push(format!("{} ({error})",s.title))}} else {failed.push(s.title.clone())} } else {failed.push(s.title.clone())}
    }
    if failed.is_empty(){Ok(format!("Ripristinate {succeeded} finestre."))} else {Err(format!("Ripristinate {succeeded} finestre; {} non ripristinate: {}",failed.len(),failed.join(", ")))}
}

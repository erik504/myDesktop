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
    fn GetMonitorInfoW(m: isize, info: *mut std::ffi::c_void) -> i32;
    fn EnumDisplayDevicesW(device: *const u16, index: u32, display: *mut DisplayDevice, flags: u32) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
    fn IsWindow(hwnd: Hwnd) -> i32;
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
    if GetMonitorInfoW(h, (&mut mi as *mut MonitorInfo).cast())==0 { return None; }
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

/// The picker uses the same eligibility rules as the normal save operation.
pub fn selectable(hwnd:isize)->bool {
    let mut found:Vec<Live>=Vec::new();
    unsafe {window_cb(hwnd,&mut found as *mut _ as isize);}
    !found.is_empty()
}

fn saved_window(w:&Live)->Result<Window,String>{
    let m=unsafe{monitor_data(w.monitor)}.ok_or("Impossibile identificare il monitor della finestra selezionata")?;
    Ok(Window{exe:w.exe.clone(),class:w.class.clone(),title:w.title.clone(),folder:w.folder.clone(),monitor_id:m.id,monitor_device:m.device,rect:Rect{left:w.rect.left-m.rect.left,top:w.rect.top-m.rect.top,right:w.rect.right-m.rect.left,bottom:w.rect.bottom-m.rect.top},show:w.show,identity:w.identity.clone()})
}

fn update_one(layout:&mut Layout,open:&[Live],selected:&Live,saved:Window)->Result<(),String>{
    // A navigated Explorer window retains its identity even when its folder changes.
    let exact:Vec<_>=layout.windows.iter().enumerate().filter(|(_,old)|
        selected.identity.is_some() && old.identity==selected.identity && equivalent(&old.exe,&selected.exe) && old.class==selected.class
    ).map(|(i,_)|i).collect();
    if exact.len()>1{return Err("La finestra compare più volte nel salvataggio: impossibile aggiornarla con certezza.".into())}
    if let Some(&i)=exact.first(){layout.windows[i]=saved;return Ok(())}
    let plan=plan_matches(&layout.windows,open,&HashSet::new());
    if let Some(i)=plan.iter().position(|h|*h==Some(selected.hwnd)){layout.windows[i]=saved;return Ok(())}
    // Do not append a duplicate or overwrite another window if existing
    // entries cannot be assigned uniquely to the current set of windows.
    if layout.windows.iter().enumerate().any(|(i,old)|plan[i].is_none() && potentially_same_window(old,selected)) {
        return Err("Impossibile distinguere con certezza questa finestra da altre nel salvataggio; configurazione conservata.".into());
    }
    layout.windows.push(saved);Ok(())
}
fn potentially_same_window(old:&Window,current:&Live)->bool {
    if !equivalent(&old.exe,&current.exe){return false}
    if old.class!=current.class && !mfc_style(&old.class).is_some_and(|s|mfc_style(&current.class).is_some_and(|v|s.eq_ignore_ascii_case(v))){return false}
    if let (Some(a),Some(b))=(&old.identity,&current.identity) {
        if a.pid==b.pid && a.created==b.created && a.hwnd!=b.hwnd{return false}
    }
    true
}

pub fn selection_token(hwnd:isize)->Result<(u32,Option<u64>),String>{
    let mut pid=0;unsafe{GetWindowThreadProcessId(hwnd,&mut pid);}
    if pid==0 || !selectable(hwnd){return Err("La finestra selezionata non è più disponibile o non è gestibile.".into())}
    Ok((pid,crate::ipc::process_created(pid)))
}
pub fn save_one(hwnd:isize,token:(u32,Option<u64>))->Result<String,String>{save_one_to(hwnd,token,&db_path()?)}
fn save_one_to(hwnd:isize,token:(u32,Option<u64>),path:&Path)->Result<String,String>{
    let mut open:Vec<Live>=Vec::new();unsafe{EnumWindows(Some(window_cb),&mut open as *mut _ as isize);}
    let selected_index=open.iter().position(|w|w.hwnd==hwnd).ok_or("La finestra selezionata non è più disponibile o non è gestibile.")?;
    let mut pid=0;unsafe{GetWindowThreadProcessId(hwnd,&mut pid);}
    if pid!=token.0 || token.1.is_some_and(|created|open[selected_index].identity.as_ref().map(|i|i.created)!=Some(created)) {
        return Err("La finestra selezionata è cambiata durante la memorizzazione; configurazione conservata.".into());
    }
    if open[selected_index].class=="CabinetWClass" {
        let folders=crate::explorer::capture()?;
        for w in &mut open {if w.class=="CabinetWClass" {w.folder=folders.get(&w.hwnd).cloned();}}
        if open[selected_index].folder.is_none(){return Err("Impossibile leggere la cartella della finestra di Esplora file selezionata; configurazione conservata.".into())}
    }
    let selected=&open[selected_index];let saved=saved_window(selected)?;
    write_single_update(path,&open,selected,saved)?;
    Ok("Finestra memorizzata nella configurazione predefinita.".into())
}
fn write_single_update(path:&Path,open:&[Live],selected:&Live,saved:Window)->Result<(),String>{
    let mut layout=match fs::read(path){
        Ok(data)=>serde_json::from_slice::<Layout>(&data).map_err(|e|format!("Configurazione non valida: {e}"))?,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Layout{version:1,windows:Vec::new()},
        Err(e)=>return Err(format!("Impossibile leggere la configurazione: {e}")),
    };
    if layout.version!=1{return Err("Versione della configurazione non supportata; configurazione conservata.".into())}
    update_one(&mut layout,&open,selected,saved)?;
    let data=serde_json::to_vec_pretty(&layout).map_err(|e|e.to_string())?;
    fs::create_dir_all(path.parent().ok_or("Cartella del file non valida")?).map_err(|e|e.to_string())?;
    write_layout(path,&data)?;
    Ok(())
}

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
    #[test] fn matches_title_and_never_reuses_window(){let s=saved();let a=vec![live(1,"other",None),live(2,"first",None)];let mut used=HashSet::new();assert_eq!(plan_matches(&[s.clone()],&a,&used),vec![Some(2)]);used.insert(2);assert_eq!(plan_matches(&[s],&a,&used),vec![None]);}
    #[test] fn matches_explorer_folder(){let mut s=saved();s.folder=Some("C:\\folder".into());let a=vec![live(1,"first",Some("C:\\other")),live(2,"first",Some("c:\\FOLDER"))];assert_eq!(plan_matches(&[s],&a,&HashSet::new()),vec![Some(2)]);}
    #[test] fn matches_mfc_changed_handles_only_for_same_style_and_title(){let mut s=saved();s.class="Afx:00400000:28:00010007:00000000:00000000".into();let mut w=live(1,"first",None);w.class="Afx:00700000:28:00020007:00000000:00000000".into();assert!(matches(&s,&w));w.title="another".into();assert!(!matches(&s,&w));w.title="first".into();w.class="Afx:00700000:29:00020007:00000000:00000000".into();assert!(!matches(&s,&w));}
    #[test] fn identical_titles_without_identity_are_not_guessed_even_for_mfc(){let mut s=saved();s.class="Afx:00400000:28:00010007:00000000:00000000".into();let mut a=live(1,"first",None);a.class="Afx:00700000:28:00020007:00000000:00000000".into();let mut b=live(2,"first",None);b.class=s.class.clone();assert_eq!(plan_matches(&[s],&[a,b],&HashSet::new()),vec![None]);}
    #[test] fn mfc_fallback_requires_same_executable_and_valid_class(){let mut s=saved();s.class="Afx:00400000:28:00010007:00000000:00000000".into();let mut w=live(1,"first",None);w.class="Afx:00700000:28:00020007:00000000:00000000".into();w.exe="C:\\Another.exe".into();assert!(!matches(&s,&w));w.exe=s.exe.clone();w.class="Afx:00700000:28:broken".into();assert!(!matches(&s,&w));}
    fn chrome_saved(title:&str,id:Option<WindowIdentity>)->Window{let mut w=saved();w.exe="C:\\Chrome\\chrome.exe".into();w.class="Chrome_WidgetWin_1".into();w.title=title.into();w.identity=id;w}
    fn chrome_live(hwnd:isize,title:&str,id:Option<WindowIdentity>)->Live{let mut w=live(hwnd,title,None);w.exe="C:\\Chrome\\chrome.exe".into();w.class="Chrome_WidgetWin_1".into();w.identity=id;w}
    fn id(hwnd:u64,created:u64)->WindowIdentity{WindowIdentity{hwnd,pid:42,created}}
    #[test] fn single_update_replaces_only_the_selected_identity(){
        let a=chrome_saved("Prima",Some(id(10,123)));let b=chrome_saved("Seconda",Some(id(20,123)));
        let current=vec![chrome_live(10,"Titolo nuovo",Some(id(10,123))),chrome_live(20,"Seconda",Some(id(20,123)))];
        let mut replacement=a.clone();replacement.title="Titolo nuovo".into();replacement.rect.left=250;
        let mut layout=Layout{version:1,windows:vec![a,b]};update_one(&mut layout,&current,&current[0],replacement).unwrap();
        assert_eq!(layout.windows.len(),2);assert_eq!(layout.windows[0].rect.left,250);assert_eq!(layout.windows[1].title,"Seconda");
    }
    #[test] fn single_update_tracks_explorer_even_after_navigation(){
        let mut old=saved();old.class="CabinetWClass".into();old.folder=Some("C:\\old".into());old.identity=Some(id(10,123));
        let mut current=live(10,"new",Some("C:\\new"));current.class=old.class.clone();current.identity=old.identity.clone();
        let mut replacement=old.clone();replacement.folder=current.folder.clone();
        let mut layout=Layout{version:1,windows:vec![old]};update_one(&mut layout,&[current.clone()],&current,replacement).unwrap();
        assert_eq!(layout.windows.len(),1);assert_eq!(layout.windows[0].folder.as_deref(),Some("C:\\new"));
    }
    #[test] fn single_update_preserves_ambiguous_existing_entries(){
        let first=chrome_saved("Scheda",None);let current=vec![chrome_live(10,"Scheda",None),chrome_live(20,"Scheda",None)];
        let mut layout=Layout{version:1,windows:vec![first.clone(),first.clone()]};
        assert!(update_one(&mut layout,&current,&current[0],first).is_err());assert_eq!(layout.windows.len(),2);
    }
    #[test] fn single_update_does_not_duplicate_an_unidentifiable_old_window(){
        let old=chrome_saved("Titolo precedente",None);let current=chrome_live(20,"Titolo cambiato",None);
        let mut layout=Layout{version:1,windows:vec![old.clone()]};let replacement=chrome_saved("Titolo cambiato",None);
        assert!(update_one(&mut layout,&[current.clone()],&current,replacement).is_err());
        assert_eq!(layout.windows.len(),1);assert_eq!(layout.windows[0].title,old.title);
    }
    #[test] fn single_update_appends_a_distinct_new_window(){
        let first=chrome_saved("Prima",Some(id(10,123)));let current=vec![chrome_live(10,"Prima",Some(id(10,123))),chrome_live(20,"Nuova",Some(id(20,123)))];
        let mut layout=Layout{version:1,windows:vec![first]};let replacement=chrome_saved("Nuova",Some(id(20,123)));
        update_one(&mut layout,&current,&current[1],replacement).unwrap();assert_eq!(layout.windows.len(),2);assert_eq!(layout.windows[0].title,"Prima");
    }
    #[test] fn single_update_creates_file_and_preserves_invalid_file(){
        let dir=std::env::temp_dir().join(format!("mydesktop-single-test-{}-{:?}",std::process::id(),std::thread::current().id()));
        fs::create_dir(&dir).unwrap();let path=dir.join("layout.json");
        let selected=chrome_live(10,"Prima",Some(id(10,123)));let entry=chrome_saved("Prima",selected.identity.clone());
        write_single_update(&path,&[selected.clone()],&selected,entry.clone()).unwrap();
        let layout:Layout=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(layout.windows.len(),1);
        let invalid=b"{ invalid";fs::write(&path,invalid).unwrap();
        assert!(write_single_update(&path,&[selected.clone()],&selected,entry).is_err());assert_eq!(fs::read(&path).unwrap(),invalid);
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }
    #[test] fn single_update_leaves_ambiguous_file_unchanged(){
        let dir=std::env::temp_dir().join(format!("mydesktop-ambiguous-test-{}-{:?}",std::process::id(),std::thread::current().id()));
        fs::create_dir(&dir).unwrap();let path=dir.join("layout.json");
        let old=chrome_saved("Scheda",None);let current=vec![chrome_live(10,"Scheda",None),chrome_live(20,"Scheda",None)];
        let original=serde_json::to_vec(&Layout{version:1,windows:vec![old.clone(),old.clone()]}).unwrap();write_layout(&path,&original).unwrap();
        assert!(write_single_update(&path,&current,&current[0],old).is_err());assert_eq!(fs::read(&path).unwrap(),original);
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }
    #[test] #[ignore] fn single_window_test_child(){unsafe{
        let Ok(dir)=std::env::var("MYDESKTOP_SINGLE_TEST_DIR") else{return};let dir=PathBuf::from(dir);
        let name=crate::wide("myDesktopSingleSaveTestTarget");let instance=crate::GetModuleHandleW(std::ptr::null());
        let class=crate::WndClassW{style:0,wnd_proc:Some(crate::DefWindowProcW),cls_extra:0,wnd_extra:0,instance,icon:std::ptr::null_mut(),cursor:std::ptr::null_mut(),background:std::ptr::null_mut(),menu_name:std::ptr::null(),class_name:name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let target=crate::CreateWindowExW(0x08000000,name.as_ptr(),crate::wide("myDesktop selection test").as_ptr(),0x80000000,60,60,20,20,std::ptr::null_mut(),std::ptr::null_mut(),instance,std::ptr::null_mut());
        assert!(!target.is_null());assert_ne!(SetWindowPos(target as isize,-1,60,60,20,20,0x10|0x40),0);
        fs::write(dir.join("ready"),(target as isize).to_string()).unwrap();
        for _ in 0..200 {if dir.join("stop").exists(){break}std::thread::sleep(Duration::from_millis(25));}
        crate::DestroyWindow(target);
    }}
    #[test] #[ignore] fn single_update_captures_real_external_window(){
        use std::process::Stdio;
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard{fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
        let dir=std::env::temp_dir().join(format!("mydesktop-real-single-{}",std::process::id()));fs::create_dir(&dir).unwrap();
        let child=Command::new(std::env::current_exe().unwrap()).args(["--ignored","--exact","core::tests::single_window_test_child"])
            .env("MYDESKTOP_SINGLE_TEST_DIR",&dir).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut child=ChildGuard(child);
        for _ in 0..100 {if dir.join("ready").exists(){break}std::thread::sleep(Duration::from_millis(25));}
        let hwnd:usize=fs::read_to_string(dir.join("ready")).unwrap().parse().unwrap();let hwnd=hwnd as isize;
        let token=selection_token(hwnd).unwrap();let path=dir.join("layout.json");
        let untouched=saved();let initial=Layout{version:1,windows:vec![untouched.clone()]};
        write_layout(&path,&serde_json::to_vec(&initial).unwrap()).unwrap();
        save_one_to(hwnd,token,&path).unwrap();
        let mut layout:Layout=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(layout.windows.len(),2);
        assert_eq!(layout.windows[0].title,untouched.title);assert_eq!(layout.windows[1].identity.as_ref().unwrap().hwnd,hwnd as u64);
        layout.windows[1].rect.left+=1000;write_layout(&path,&serde_json::to_vec(&layout).unwrap()).unwrap();
        save_one_to(hwnd,token,&path).unwrap();
        let updated:Layout=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(updated.windows.len(),2);
        assert_eq!(updated.windows[0].title,untouched.title);assert_ne!(updated.windows[1].rect.left,layout.windows[1].rect.left);
        fs::write(dir.join("stop"),b"").unwrap();child.0.wait().unwrap();
        fs::remove_file(dir.join("stop")).unwrap();fs::remove_file(dir.join("ready")).unwrap();fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }
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
    #[test] fn another_window_in_same_running_process_is_never_mistaken_for_saved(){
        let saved=vec![chrome_saved("Titolo",Some(id(10,123)))];
        let current=vec![chrome_live(20,"Titolo",Some(id(20,123)))];
        assert_eq!(plan_matches(&saved,&current,&HashSet::new()),vec![None]);
    }
    #[test] fn hard_closes_only_apps_absent_from_layout(){
        let s=saved();let mut extra=live(30,"Altra app",None);extra.exe=r"C:\Other\other.exe".into();
        let initial=vec![live(1,"first",None),live(2,"extra della stessa app",None),extra];
        assert!(extras_to_close(RestoreMode::Normal,&[s.clone()],&initial).is_empty());
        assert!(extras_to_close(RestoreMode::Soft,&[s.clone()],&initial).is_empty());
        let targets=extras_to_close(RestoreMode::Hard,&[s],&initial);
        assert_eq!(targets.len(),1);assert_eq!(targets[0].hwnd,30);
    }
    #[test] fn ambiguous_extra_windows_remain_protected_on_later_passes(){
        let saved=vec![chrome_saved("Scheda",None),chrome_saved("Scheda",None)];
        let live=vec![chrome_live(10,"Scheda",None)];
        let protected=protect_unmatched_initial(&saved,&live);
        assert!(protected.contains(&10));
        assert_eq!(plan_matches(&saved[1..],&live,&protected),vec![None]);
    }
    #[test] fn newly_opened_and_recycled_windows_are_not_touched(){
        let initial=vec![chrome_live(10,"Prima",Some(id(10,123)))];
        let mut current=vec![chrome_live(10,"Riciclata",Some(id(10,456))),chrome_live(20,"Nuova",Some(id(20,123)))];
        let mut protected=HashSet::new();guard_live_windows(&initial,&mut current,&HashSet::new(),&mut protected);
        assert!(protected.contains(&10));assert!(protected.contains(&20));
    }
    #[test] fn different_applications_reserve_titles_and_never_steal_windows(){
        for exe in [r"C:\Firefox\firefox.exe",r"C:\Sublime Text\sublime_text.exe",r"C:\New App\future.exe"] {
            let mut a=saved();a.exe=exe.into();a.title="Documento A".into();
            let mut b=a.clone();b.title="Documento B".into();
            let mut wa=live(10,"Titolo cambiato",None);wa.exe=exe.into();
            let mut wb=live(20,"Documento B",None);wb.exe=exe.into();
            assert_eq!(plan_matches(&[a,b],&[wa,wb],&HashSet::new()),vec![None,Some(20)]);
        }
    }
    #[test] fn generic_duplicate_titles_are_not_guessed(){
        let saved=vec![saved(),saved()];let open=vec![live(1,"first",None),live(2,"first",None)];
        assert_eq!(plan_matches(&saved,&open,&HashSet::new()),vec![None,None]);
    }
    #[test] fn explorer_unique_folder_matches_even_if_title_changes(){
        let mut s=saved();s.folder=Some(r"C:\Folder".into());
        let open=vec![live(10,"Titolo diverso",Some(r"c:\folder")),live(20,"first",Some(r"C:\Other"))];
        assert_eq!(plan_matches(&[s],&open,&HashSet::new()),vec![Some(10)]);
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
fn matches(saved:&Window,live:&Live)->bool {
    // A different window created by the same still-running process is not the
    // saved one, even if it happens to have the same title.
    if let (Some(a),Some(b))=(&saved.identity,&live.identity) {
        if a.pid==b.pid && a.created==b.created && a.hwnd!=b.hwnd {return false}
    }
    equivalent(&saved.exe,&live.exe) && (saved.class==live.class || (saved.title==live.title && mfc_style(&saved.class).is_some_and(|s|mfc_style(&live.class).is_some_and(|v|s.eq_ignore_ascii_case(v))))) && match &saved.folder {Some(folder)=>live.folder.as_ref().is_some_and(|f|equivalent(folder,f)),None=>true}
}
#[cfg(test)] fn is_chrome(exe:&str)->bool { Path::new(exe).file_name().is_some_and(|name|name.to_string_lossy().eq_ignore_ascii_case("chrome.exe")) }
/// Reserve all confident matches before allowing weaker matches to consume windows.
fn plan_matches(saved:&[Window],live:&[Live],used:&HashSet<Hwnd>)->Vec<Option<Hwnd>> {
    let mut result=vec![None;saved.len()];let mut reserved=used.clone();
    // Handles only count when the owning process and its creation time also match.
    for (i,s) in saved.iter().enumerate() {
        let Some(id)=s.identity.as_ref() else{continue};
        if let Some(w)=live.iter().find(|w|!reserved.contains(&w.hwnd)&&w.identity.as_ref()==Some(id)&&matches(s,w)) {result[i]=Some(w.hwnd);reserved.insert(w.hwnd);}
    }
    // Explorer can be recognized by its saved folder even when its title changes.
    for (i,s) in saved.iter().enumerate() {
        if result[i].is_some()||s.folder.is_none(){continue}
        let candidates:Vec<_>=live.iter().filter(|w|!reserved.contains(&w.hwnd)&&matches(s,w)).collect();
        if candidates.len()!=1 {continue} let w=candidates[0];
        let competing=saved.iter().enumerate().filter(|(j,other)|result[*j].is_none()&&other.folder.is_some()&&matches(other,w)).count();
        if competing==1 {result[i]=Some(w.hwnd);reserved.insert(w.hwnd);}
    }
    // Exact titles are assigned only where the pairing is unambiguous. In
    // Explorer, matches() also compares the saved folder.
    for (i,s) in saved.iter().enumerate() {
        if result[i].is_some(){continue}
        let candidates:Vec<_>=live.iter().filter(|w|!reserved.contains(&w.hwnd)&&matches(s,w)&&w.title==s.title).collect();
        if candidates.len()!=1 {continue}
        let w=candidates[0];
        let competing=saved.iter().enumerate().filter(|(j,other)|result[*j].is_none()&&other.title==w.title&&matches(other,w)).count();
        if competing==1 {result[i]=Some(w.hwnd);reserved.insert(w.hwnd);}
    }
    result
}
fn protect_unmatched_initial(saved:&[Window],initial:&[Live])->HashSet<Hwnd>{
    let matched:HashSet<Hwnd>=plan_matches(saved,initial,&HashSet::new()).into_iter().flatten().collect();
    initial.iter().filter(|w|!matched.contains(&w.hwnd)).map(|w|w.hwnd).collect()
}
fn guard_live_windows(initial:&[Live],current:&mut [Live],launched:&HashSet<Hwnd>,used:&mut HashSet<Hwnd>){
    for w in current {
        match initial.iter().find(|original|original.hwnd==w.hwnd) {
            Some(original) if original.identity!=w.identity => {used.insert(w.hwnd);},
            Some(_) => {},
            None if launched.contains(&w.hwnd) => {w.identity=None;},
            None => {used.insert(w.hwnd);}
        }
    }
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
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum RestoreMode { Normal, Soft, Hard }
fn extras_to_close<'a>(mode:RestoreMode,saved:&[Window],initial:&'a [Live])->Vec<&'a Live>{
    if mode!=RestoreMode::Hard{return Vec::new()}
    initial.iter().filter(|w|!saved.iter().any(|s|equivalent(&s.exe,&w.exe))).collect()
}
#[derive(PartialEq,Eq)] enum WindowStatus { Same, GoneOrReplaced, Unknown }
fn window_status(w:&Live)->WindowStatus {
    let Some(id)=w.identity.as_ref() else{return WindowStatus::Unknown};
    let mut pid=0;unsafe{if IsWindow(w.hwnd)==0{return WindowStatus::GoneOrReplaced} if GetWindowThreadProcessId(w.hwnd,&mut pid)==0{return WindowStatus::Unknown}}
    if pid!=id.pid{return WindowStatus::GoneOrReplaced}
    match crate::ipc::process_created(pid) {Some(created) if created==id.created=>WindowStatus::Same,Some(_)=>WindowStatus::GoneOrReplaced,None=>WindowStatus::Unknown}
}
fn close_extra_apps(mode:RestoreMode,saved:&[Window],initial:&[Live])->Vec<String>{
    let mut failed=Vec::new();let mut requested=Vec::new();
    for w in extras_to_close(mode,saved,initial){
        match window_status(w){WindowStatus::Unknown=>{failed.push(format!("{} (identità non verificabile)",w.title));continue},WindowStatus::GoneOrReplaced=>continue,WindowStatus::Same=>{}}
        // WM_CLOSE allows each application to ask about unsaved work. Never kill
        // a process or bypass a save prompt; inaccessible windows are reported.
        if unsafe{crate::PostMessageW(w.hwnd as *mut std::ffi::c_void,0x10,0,0)}==0 {
            failed.push(format!("{} (chiusura non consentita: {})",w.title,std::io::Error::last_os_error()));
        } else {requested.push(w);}
    }
    let until=std::time::Instant::now()+Duration::from_secs(3);
    while std::time::Instant::now()<until && requested.iter().any(|w|window_status(w)==WindowStatus::Same){thread::sleep(Duration::from_millis(100));}
    for w in requested {match window_status(w){WindowStatus::Same=>failed.push(format!("{} (ancora aperta; verifica eventuali richieste di salvataggio)",w.title)),WindowStatus::Unknown=>failed.push(format!("{} (impossibile verificare la chiusura)",w.title)),WindowStatus::GoneOrReplaced=>{}}}
    failed
}
#[cfg(test)]
#[test]
fn hard_close_targets_only_extra_test_window_and_keeps_saved_app_open(){
    use std::{ffi::c_void,ptr::{null,null_mut},sync::{atomic::{AtomicUsize,Ordering},mpsc}};
    static DESTROYED:AtomicUsize=AtomicUsize::new(0);
    unsafe extern "system" fn proc(h:*mut c_void,msg:u32,w:usize,l:isize)->isize {
        if msg==2 && DESTROYED.fetch_add(1,Ordering::SeqCst)+1==2 {unsafe{crate::PostQuitMessage(0)}}
        unsafe{crate::DefWindowProcW(h,msg,w,l)}
    }
    let (tx,rx)=mpsc::channel();
    let thread=std::thread::spawn(move||unsafe{
        let name:Vec<u16>="myDesktopCloseTest".encode_utf16().chain(Some(0)).collect();let instance=crate::GetModuleHandleW(null());
        let class=crate::WndClassW{style:0,wnd_proc:Some(proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu_name:null(),class_name:name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let a=crate::CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,100,100,null_mut(),null_mut(),instance,null_mut());
        let b=crate::CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,100,100,null_mut(),null_mut(),instance,null_mut());
        assert!(!a.is_null()&&!b.is_null());tx.send((a as isize,b as isize)).unwrap();
        let mut msg:crate::Msg=zeroed();while crate::GetMessageW(&mut msg,null_mut(),0,0)>0 {crate::TranslateMessage(&msg);crate::DispatchMessageW(&msg);}
    });
    let (keep,extra)=rx.recv_timeout(Duration::from_secs(5)).unwrap();let pid=std::process::id();let created=crate::ipc::process_created(pid).unwrap();
    let make=|hwnd:Hwnd,exe:&str|Live{hwnd,exe:exe.into(),class:"Test".into(),title:"Test".into(),folder:None,monitor:0,rect:Rect::default(),show:1,identity:Some(WindowIdentity{hwnd:hwnd as u64,pid,created})};
    let saved=Window{exe:"C:\\saved.exe".into(),class:"Test".into(),title:"Test".into(),folder:None,monitor_id:String::new(),monitor_device:String::new(),rect:Rect::default(),show:1,identity:None};
    let initial=vec![make(keep,"C:\\saved.exe"),make(extra,"C:\\extra.exe")];
    assert!(close_extra_apps(RestoreMode::Normal,&[saved.clone()],&initial).is_empty());
    assert!(close_extra_apps(RestoreMode::Soft,&[saved.clone()],&initial).is_empty());
    assert!(window_status(&initial[0])==WindowStatus::Same&&window_status(&initial[1])==WindowStatus::Same);
    assert!(close_extra_apps(RestoreMode::Hard,&[saved],&initial).is_empty());
    assert!(window_status(&initial[0])==WindowStatus::Same);assert!(window_status(&initial[1])==WindowStatus::GoneOrReplaced);
    unsafe{crate::PostMessageW(keep as *mut c_void,0x10,0,0)};thread.join().unwrap();
}
pub fn restore()->Result<String,String>{
    restore_mode(RestoreMode::Normal)
}
pub fn restore_mode(mode:RestoreMode)->Result<String,String>{restore_from_mode(&db_path()?,mode)}
pub fn restore_from(path: &Path)->Result<String,String>{
    restore_from_mode(path,RestoreMode::Normal)
}
pub fn restore_from_mode(path:&Path,mode:RestoreMode)->Result<String,String>{
    let data=fs::read(path).map_err(|e|format!("Impossibile leggere la configurazione salvata: {e}"))?; let layout:Layout=serde_json::from_slice(&data).map_err(|e|format!("Configurazione non valida: {e}"))?; if layout.version!=1{return Err("Versione della configurazione non supportata.".into())} if layout.windows.is_empty(){return Err("La configurazione è vuota: nessuna finestra è stata modificata.".into())}
    let monitors=monitors(); if monitors.is_empty(){return Err("Nessun monitor disponibile.".into())}
    let initial=windows()?;
    let mut used=protect_unmatched_initial(&layout.windows,&initial);
    let mut launched_windows=HashSet::new();
    let mut succeeded=0; let mut skipped=0; let mut failed=Vec::new();let mut launched_exes=HashSet::new();
    for (index,s) in layout.windows.iter().enumerate() {
        let mut current=windows()?;
        guard_live_windows(&initial,&mut current,&launched_windows,&mut used);
        let mut found=plan_matches(&layout.windows[index..],&current,&used)[0];
        if found.is_none(){
            if mode==RestoreMode::Soft {skipped+=1;continue}
            // A generic executable cannot identify which document or window to
            // reopen. Explorer folders are an explicit exception: they have a
            // saved path and can be opened independently.
            if s.folder.is_none() && (current.iter().any(|w|equivalent(&s.exe,&w.exe)) || !launched_exes.insert(s.exe.to_lowercase())) {
                failed.push(format!("{} (finestra non identificabile con certezza)",s.title));continue;
            }
            let before:HashSet<Hwnd>=current.iter().map(|w|w.hwnd).collect();
            let launched=if let Some(folder)=&s.folder{crate::explorer::launch(folder).map_err(|e|e.to_string())}else{launch_application(&s.exe)};
            match launched { Ok(())=>{for _ in 0..30{thread::sleep(Duration::from_millis(300));current=windows()?;for w in &current {if !before.contains(&w.hwnd)&&equivalent(&s.exe,&w.exe){launched_windows.insert(w.hwnd);}}guard_live_windows(&initial,&mut current,&launched_windows,&mut used);found=plan_matches(&layout.windows[index..],&current,&used)[0];if found.is_some(){break}}}, Err(error)=>{failed.push(format!("{} ({error})",s.title));continue;} }
        }
        if let Some(h)=found { used.insert(h); let m=target(s,&monitors).unwrap(); let r=s.rect; let mut x=m.rect.left+r.left; let mut y=m.rect.top+r.top; let mut width=(r.right-r.left).max(1); let mut height=(r.bottom-r.top).max(1); let same=if s.monitor_id.is_empty(){m.device.eq_ignore_ascii_case(&s.monitor_device)}else{m.id.eq_ignore_ascii_case(&s.monitor_id)}; if !same {width=width.min(m.work.right-m.work.left);height=height.min(m.work.bottom-m.work.top);x=x.clamp(m.work.left,m.work.right-width);y=y.clamp(m.work.top,m.work.bottom-height);} let desired=Rect{left:x,top:y,right:x+width,bottom:y+height}; if let Some(live)=current.iter().find(|w|w.hwnd==h) {match place_window(h,live,desired,s.show){Ok(())=>succeeded+=1,Err(error)=>failed.push(format!("{} ({error})",s.title))}} else {failed.push(s.title.clone())} } else {failed.push(s.title.clone())}
    }
    if mode==RestoreMode::Hard {
        if failed.is_empty() {failed.extend(close_extra_apps(mode,&layout.windows,&initial));}
        else {failed.push("Nessuna app aggiuntiva è stata chiusa perché il ripristino è incompleto".into());}
    }
    if failed.is_empty(){Ok(format!("Ripristinate {succeeded} finestre; {skipped} non aperte o non identificabili."))} else {Err(format!("Ripristinate {succeeded} finestre; {} problemi: {}",failed.len(),failed.join(", ")))}
}

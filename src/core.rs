use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, mem::{size_of, zeroed}, os::windows::ffi::OsStrExt, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}, thread, time::Duration};
#[cfg(test)] use std::process::Command;

type Hwnd = isize;
#[repr(C)] #[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
#[repr(C)] #[derive(Clone, Copy, Default)] struct Point { x: i32, y: i32 }
#[repr(C)] #[derive(Clone, Copy)] struct Placement { length: u32, flags: u32, show: u32, min: Point, max: Point, normal: Rect }
#[repr(C)] struct MonitorInfo { size: u32, monitor: Rect, work: Rect, flags: u32, device: [u16; 32] }
#[repr(C)] struct DisplayDevice { size: u32, name: [u16; 32], string: [u16; 128], flags: u32, id: [u16; 128], key: [u16; 128] }
#[repr(C)] struct StartupInfo {size:u32,reserved:*mut u16,desktop:*mut u16,title:*mut u16,x:u32,y:u32,width:u32,height:u32,x_chars:u32,y_chars:u32,fill:u32,flags:u32,show:u16,reserved2_len:u16,reserved2:*mut u8,stdin:*mut std::ffi::c_void,stdout:*mut std::ffi::c_void,stderr:*mut std::ffi::c_void}
#[repr(C)] struct ProcessInfo {process:*mut std::ffi::c_void,thread:*mut std::ffi::c_void,pid:u32,tid:u32}

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
    fn CreateProcessW(application:*const u16,command:*mut u16,process_attrs:*mut std::ffi::c_void,thread_attrs:*mut std::ffi::c_void,inherit:i32,flags:u32,environment:*mut std::ffi::c_void,directory:*const u16,startup:*mut StartupInfo,info:*mut ProcessInfo)->i32;
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
unsafe fn read_window(h:Hwnd)->Option<Live> { unsafe {
    if IsWindowVisible(h)==0 || GetWindow(h,4)!=0 || GetWindowLongW(h,-20)&0x80!=0 { return None; }
    let mut cloaked=0u32; if DwmGetWindowAttribute(h,14,&mut cloaked,4)==0 && cloaked!=0 {return None}
    let mut pid=0; GetWindowThreadProcessId(h,&mut pid); if pid==std::process::id(){return None}
    let exe=process_path(pid)?;
    let mut cls=[0u16;256]; GetClassNameW(h,cls.as_mut_ptr(),cls.len() as i32); let class=text(&cls);
    if ["Progman","WorkerW","Shell_TrayWnd","Windows.UI.Core.CoreWindow"].contains(&class.as_str()) {return None}
    let len=GetWindowTextLengthW(h).clamp(0,32767); let mut tb=vec![0u16;len as usize+1]; GetWindowTextW(h,tb.as_mut_ptr(),tb.len() as i32); let title=text(&tb);
    if title.is_empty() {return None}
    let mut wp:Placement=zeroed(); wp.length=size_of::<Placement>() as u32; if GetWindowPlacement(h,(&mut wp as *mut Placement).cast())==0{return None}
    let monitor=MonitorFromWindow(h,2); let mi=monitor_data(monitor)?;
    let mut r=Rect::default(); if wp.show==1 { GetWindowRect(h,(&mut r as *mut Rect).cast()); } else { r=wp.normal; let dx=mi.work.left-mi.rect.left; let dy=mi.work.top-mi.rect.top; r.left+=dx;r.right+=dx;r.top+=dy;r.bottom+=dy; }
    let identity=crate::ipc::process_created(pid).map(|created|WindowIdentity{hwnd:h as u64,pid,created});
    Some(Live{ hwnd:h,exe,class,title,folder:None,monitor,rect:r,show:wp.show,identity })
} }
fn scan_windows(highlight_owner:Option<isize>)->Vec<Live> {
    struct Scan { windows:Vec<Live>, highlight_owner:Option<isize> }
    unsafe extern "system" fn window_cb(hwnd:Hwnd,arg:isize)->i32 {unsafe{
        if let Some(window)=read_window(hwnd) {
            let scan=&mut *(arg as *mut Scan);
            scan.windows.push(window);
            if let Some(owner)=scan.highlight_owner {
                crate::save_highlight::enqueue(owner,hwnd);
            }
        }
        1
    }}
    let mut scan=Scan{windows:Vec::new(),highlight_owner};
    unsafe {EnumWindows(Some(window_cb),&mut scan as *mut Scan as isize);}
    scan.windows
}
fn raw_windows()->Vec<Live>{scan_windows(None)}
fn windows(highlight_owner:Option<isize>)->Result<Vec<Live>,String>{ let mut v=scan_windows(highlight_owner); if v.iter().any(|w|w.class=="CabinetWClass"){let folders=crate::explorer::capture()?;for w in &mut v {w.folder=folders.get(&w.hwnd).cloned();if w.class=="CabinetWClass" && w.folder.is_none(){return Err(format!("Impossibile leggere la cartella di Esplora file: {}",w.title));}}} Ok(v) }

/// The picker uses the same eligibility rules as the normal save operation.
pub fn selectable(hwnd:isize)->bool {
    unsafe {read_window(hwnd).is_some()}
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
    let mut open=raw_windows();
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

pub fn save(owner:isize)->Result<String,String>{
    save_impl(&db_path()?,Some(owner))
}
pub fn save_to(path: &Path)->Result<String,String>{
    save_impl(path,None)
}
fn save_impl(path:&Path,highlight_owner:Option<isize>)->Result<String,String>{
    let mut out=Vec::new();
    for w in windows(highlight_owner)? {if let Ok(saved)=saved_window(&w){out.push(saved);}}
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
    #[test] fn missing_apps_are_grouped_and_soft_mode_never_launches(){
        let mut a=saved();a.title="A".into();let mut b=a.clone();b.title="B".into();
        let mut explorer=a.clone();explorer.exe="C:\\Windows\\explorer.exe".into();explorer.folder=Some("C:\\uno".into());
        let mut explorer2=explorer.clone();explorer2.folder=Some("C:\\due".into());
        let ms=vec![monitor("physical-A","display1",true)];let all=vec![a,b,explorer,explorer2];
        let (plans,pending,skipped,failed)=launch_plans(&all,&[],&[None;4],RestoreMode::Normal,&ms);
        assert_eq!(plans.len(),3);assert_eq!(plans[0].indices,vec![0,1]);assert_eq!(plans[1].indices,vec![2]);assert_eq!(plans[2].indices,vec![3]);
        assert!(pending.iter().all(|p|*p));assert_eq!(skipped,0);assert!(failed.is_empty());
        let (plans,pending,skipped,failed)=launch_plans(&all,&[],&[None;4],RestoreMode::Soft,&ms);
        assert!(plans.is_empty());assert!(pending.iter().all(|p|!*p));assert_eq!(skipped,4);assert!(failed.is_empty());
    }
    #[test] fn existing_app_is_not_relaunched_for_an_unidentified_window(){
        let s=saved();let open=vec![live(10,"unrelated",None)];let ms=vec![monitor("physical-A","display1",true)];
        let (plans,pending,_,failed)=launch_plans(&[s],&open,&[None],RestoreMode::Normal,&ms);
        assert!(plans.is_empty());assert_eq!(pending,vec![false]);assert_eq!(failed.len(),1);
    }
    #[test] fn startup_hints_include_size_and_negative_monitor_coordinates(){
        let r=Rect{left:-1500,top:35,right:-900,bottom:435};let startup=startup_for(r);
        assert_eq!(startup.flags&6,6);assert_eq!(startup.x as i32,-1500);assert_eq!(startup.y,35);assert_eq!((startup.width,startup.height),(600,400));
    }
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
    #[test] #[ignore] fn parallel_restore_starts_real_apps_and_preserves_extra(){
        #[link(name="kernel32")] unsafe extern "system" {fn SetErrorMode(mode:u32)->u32;}
        unsafe{SetErrorMode(2);}
        struct Cleanup{dir:PathBuf,exes:Vec<String>}
        impl Drop for Cleanup{fn drop(&mut self){
            for _ in 0..100 {let windows:Vec<_>=raw_windows().into_iter().filter(|w|self.exes.iter().any(|e|equivalent(e,&w.exe))).collect();
                if windows.is_empty(){break}for w in windows{unsafe{crate::PostMessageW(w.hwnd as *mut std::ffi::c_void,0x10,0,0);}}thread::sleep(Duration::from_millis(30));}
            for name in ["fixture_base.exe","md_restore_a.exe","md_restore_b.exe","md_restore_extra.exe","layout.json"] {let _=fs::remove_file(self.dir.join(name));}
            for name in ["md_restore_a","md_restore_b","md_restore_extra"]{for suffix in ["started","initial","shown","changes"]{let _=fs::remove_file(self.dir.join(format!("{name}.{suffix}")));}}
            let _=fs::remove_dir(&self.dir);
        }}
        let dir=std::env::temp_dir().join(format!("mydesktop-parallel-test-{}",std::process::id()));fs::create_dir(&dir).unwrap();
        let names=["md_restore_a","md_restore_b","md_restore_extra"];
        let exes:Vec<_>=names.iter().map(|n|dir.join(format!("{n}.exe"))).collect();
        let cleanup=Cleanup{dir:dir.clone(),exes:exes.iter().map(|p|p.to_string_lossy().into_owned()).collect()};
        let fixture=Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/restore_app.rs");let base=dir.join("fixture_base.exe");
        let status=Command::new("rustc").args(["--edition=2021","-C","opt-level=1"]).arg(fixture).arg("-o").arg(&base).status().unwrap();assert!(status.success());
        for exe in &exes {fs::copy(&base,exe).unwrap();}
        let _extra=Command::new(&exes[2]).spawn().unwrap();
        for _ in 0..120{if dir.join("md_restore_extra.shown").exists(){break}thread::sleep(Duration::from_millis(25));}
        let extra=raw_windows().into_iter().find(|w|equivalent(&w.exe,&cleanup.exes[2])).expect("Finestra di test aggiuntiva non disponibile");
        let ms=monitors();let m=ms.iter().find(|m|m.primary).unwrap();
        let make=|index:usize,left:i32|Window{exe:cleanup.exes[index].clone(),class:"myDesktopRestoreFixture".into(),title:names[index].into(),folder:None,monitor_id:m.id.clone(),monitor_device:m.device.clone(),rect:Rect{left,top:80,right:left+300,bottom:240},show:1,identity:None};
        let saved=vec![make(0,80),make(1,420)];let expected_a=destination(&saved[0],&ms).unwrap();let expected_b=destination(&saved[1],&ms).unwrap();
        let path=dir.join("layout.json");fs::write(&path,serde_json::to_vec(&Layout{version:1,windows:saved}).unwrap()).unwrap();
        restore_from_mode(&path,RestoreMode::Normal).unwrap();
        let timestamp=|name:&str,suffix:&str|->u128{fs::read_to_string(dir.join(format!("{name}.{suffix}"))).unwrap().parse().unwrap()};
        assert!(timestamp(names[0],"started").max(timestamp(names[1],"started"))<timestamp(names[0],"shown").min(timestamp(names[1],"shown")),"Gli avvii sono stati seriali");
        let parse_rect=|name:&str|{let text=fs::read_to_string(dir.join(format!("{name}.initial"))).unwrap();let values:Vec<i32>=text.split_whitespace().map(|v|v.parse().unwrap()).collect();Rect{left:values[0],top:values[1],right:values[2],bottom:values[3]}};
        assert_eq!(parse_rect(names[0]),expected_a,"I suggerimenti per la posizione iniziale non sono stati applicati");
        let current=raw_windows();let a=current.iter().find(|w|equivalent(&w.exe,&cleanup.exes[0])).unwrap();let b=current.iter().find(|w|equivalent(&w.exe,&cleanup.exes[1])).unwrap();
        assert_eq!(a.rect,expected_a);assert_eq!(b.rect,expected_b);
        let moves=fs::read_to_string(dir.join("md_restore_b.changes")).unwrap().parse::<usize>().unwrap();assert_eq!(moves,1,"La finestra riaperta è stata animata");
        let kept=current.iter().find(|w|w.hwnd==extra.hwnd).expect("La finestra aggiuntiva è stata chiusa");assert_eq!(kept.rect,extra.rect);
        drop(cleanup);
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
        let observed=raw_windows();
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
fn startup_for(rect:Rect)->StartupInfo {
    let mut startup:StartupInfo=unsafe{zeroed()};startup.size=size_of::<StartupInfo>() as u32;
    startup.flags=0x4|0x2; // STARTF_USEPOSITION | STARTF_USESIZE
    startup.x=rect.left as u32;startup.y=rect.top as u32;
    startup.width=(rect.right-rect.left).max(1) as u32;startup.height=(rect.bottom-rect.top).max(1) as u32;
    startup
}
fn launch_application(exe:&str,rect:Rect)->Result<(),String>{
    if exe.contains('"'){return Err("Percorso eseguibile non valido".into())}
    let file:Vec<u16>=std::ffi::OsStr::new(exe).encode_wide().chain(Some(0)).collect();
    let mut command:Vec<u16>=format!("\"{exe}\"").encode_utf16().chain(Some(0)).collect();
    let directory:Option<Vec<u16>>=Path::new(exe).parent().map(|p|p.as_os_str().encode_wide().chain(Some(0)).collect());
    let mut startup=startup_for(rect);let mut info:ProcessInfo=unsafe{zeroed()};
    let ok=unsafe{CreateProcessW(file.as_ptr(),command.as_mut_ptr(),std::ptr::null_mut(),std::ptr::null_mut(),0,0,std::ptr::null_mut(),directory.as_ref().map_or(std::ptr::null(),|p|p.as_ptr()),&mut startup,&mut info)};
    if ok!=0 {unsafe{CloseHandle(info.thread);CloseHandle(info.process);}return Ok(())}
    let e=std::io::Error::last_os_error();
    match e.raw_os_error(){
        Some(740)=>{
            // The Windows shell performs the normal UAC consent flow when needed.
            let result=unsafe{ShellExecuteW(0,std::ptr::null(),file.as_ptr(),std::ptr::null(),directory.as_ref().map_or(std::ptr::null(),|p|p.as_ptr()),1)};
            if result>32 {Ok(())} else {Err(format!("Avvio con richiesta di autorizzazione non riuscito (codice {result}): {exe}"))}
        },
        _=>Err(format!("Impossibile avviare {exe}: {e}")),
    }
}
fn target<'a>(w:&Window,monitors:&'a [Monitor])->Option<&'a Monitor>{
    // Device registry key identifies a physical display across changes to its position.
    let by_id=if w.monitor_id.is_empty(){None}else{monitors.iter().find(|m|m.id.eq_ignore_ascii_case(&w.monitor_id))};
    by_id.or_else(||if w.monitor_id.is_empty(){monitors.iter().find(|m|m.device.eq_ignore_ascii_case(&w.monitor_device))}else{None}).or_else(||monitors.iter().find(|m|m.primary)).or_else(||monitors.first())
}
fn destination(w:&Window,monitors:&[Monitor])->Option<Rect>{
    let m=target(w,monitors)?;let r=w.rect;let mut x=m.rect.left+r.left;let mut y=m.rect.top+r.top;
    let mut width=(r.right-r.left).max(1);let mut height=(r.bottom-r.top).max(1);
    let same=if w.monitor_id.is_empty(){m.device.eq_ignore_ascii_case(&w.monitor_device)}else{m.id.eq_ignore_ascii_case(&w.monitor_id)};
    if !same {width=width.min(m.work.right-m.work.left);height=height.min(m.work.bottom-m.work.top);x=x.clamp(m.work.left,m.work.right-width);y=y.clamp(m.work.top,m.work.bottom-height);}
    Some(Rect{left:x,top:y,right:x+width,bottom:y+height})
}
fn state_kind(show:u32)->u8 { if show==3 {1} else if [2,6,7,11].contains(&show) {2} else {0} }
fn interpolate(a:i32,b:i32,step:i64,total:i64)->i32 { (i64::from(a)+(i64::from(b)-i64::from(a))*step/total) as i32 }
const ANIMATION_MS:u64=250;
const ANIMATION_FRAMES:u64=16;
fn eased_progress(elapsed:Duration)->i64 {
    const SCALE:i64=10_000;
    let t=(elapsed.as_micros().min(u128::from(ANIMATION_MS)*1000) as i64)*SCALE/(ANIMATION_MS as i64*1000);
    t*t*(3*SCALE-2*t)/(SCALE*SCALE)
}
#[link(name="winmm")] unsafe extern "system" {fn timeBeginPeriod(ms:u32)->u32;fn timeEndPeriod(ms:u32)->u32;}
struct TimerResolution(bool);
impl TimerResolution {fn acquire()->Self{Self(unsafe{timeBeginPeriod(1)==0})}}
impl Drop for TimerResolution {fn drop(&mut self){if self.0{unsafe{timeEndPeriod(1);}}}}
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
        CHANGES.store(0,Ordering::SeqCst);place_window(hwnd,&live,rect,1,true).unwrap();assert_eq!(CHANGES.load(Ordering::SeqCst),0);
        let goal=Rect{left:rect.left+20,right:rect.right+20,..rect};let start=std::time::Instant::now();place_window(hwnd,&live,goal,1,true).unwrap();
        assert!(start.elapsed()>=Duration::from_millis(225));let mut actual=Rect::default();assert_ne!(GetWindowRect(hwnd,(&mut actual as *mut Rect).cast()),0);assert_eq!(actual,goal);assert!(CHANGES.load(Ordering::SeqCst)>1);
        let moved=Live{rect:goal,..live};CHANGES.store(0,Ordering::SeqCst);
        place_window(hwnd,&moved,rect,1,false).unwrap();assert_eq!(CHANGES.load(Ordering::SeqCst),1);
        crate::DestroyWindow(window);
    }
}
#[cfg(test)]
#[test]
fn existing_windows_animate_concurrently() {
    use std::{ffi::c_void,ptr::{null,null_mut},sync::{mpsc,OnceLock,atomic::{AtomicIsize,AtomicU64,AtomicUsize,Ordering}}};
    static START:OnceLock<std::time::Instant>=OnceLock::new();
    static A:AtomicIsize=AtomicIsize::new(0);static B:AtomicIsize=AtomicIsize::new(0);
    static FIRST_A:AtomicU64=AtomicU64::new(0);static FIRST_B:AtomicU64=AtomicU64::new(0);
    static LAST_A:AtomicU64=AtomicU64::new(0);static LAST_B:AtomicU64=AtomicU64::new(0);
    static CLOSED:AtomicUsize=AtomicUsize::new(0);
    unsafe extern "system" fn proc(h:*mut c_void,msg:u32,w:usize,l:isize)->isize {
        if msg==0x46 {let t=START.get().unwrap().elapsed().as_micros() as u64+1;
            let pair=if h as isize==A.load(Ordering::SeqCst){Some((&FIRST_A,&LAST_A))}else if h as isize==B.load(Ordering::SeqCst){Some((&FIRST_B,&LAST_B))}else{None};
            if let Some((first,last))=pair{let _=first.compare_exchange(0,t,Ordering::SeqCst,Ordering::SeqCst);last.store(t,Ordering::SeqCst);}}
        if msg==2 && CLOSED.fetch_add(1,Ordering::SeqCst)+1==2{unsafe{crate::PostQuitMessage(0)}}
        unsafe{crate::DefWindowProcW(h,msg,w,l)}
    }
    START.get_or_init(std::time::Instant::now);
    let(tx,rx)=mpsc::channel();let ui=thread::spawn(move||unsafe{
        let name=crate::wide("myDesktopParallelPlacementTest");let instance=crate::GetModuleHandleW(null());
        let class=crate::WndClassW{style:0,wnd_proc:Some(proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu_name:null(),class_name:name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let a=crate::CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,40,40,200,100,null_mut(),null_mut(),instance,null_mut());
        let b=crate::CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,80,80,200,100,null_mut(),null_mut(),instance,null_mut());
        assert!(!a.is_null()&&!b.is_null());A.store(a as isize,Ordering::SeqCst);B.store(b as isize,Ordering::SeqCst);tx.send((a as isize,b as isize)).unwrap();
        let mut msg:crate::Msg=zeroed();while crate::GetMessageW(&mut msg,null_mut(),0,0)>0{crate::TranslateMessage(&msg);crate::DispatchMessageW(&msg);}
    });
    let(a,b)=rx.recv_timeout(Duration::from_secs(5)).unwrap();let mut work=Vec::new();
    for hwnd in [a,b]{let mut r=Rect::default();unsafe{assert_ne!(GetWindowRect(hwnd,(&mut r as *mut Rect).cast()),0);}
        let live=Live{hwnd,exe:String::new(),class:String::new(),title:"finestra test".into(),folder:None,monitor:0,rect:r,show:1,identity:None};
        let saved=Window{exe:String::new(),class:String::new(),title:"finestra test".into(),folder:None,monitor_id:String::new(),monitor_device:String::new(),rect:r,show:1,identity:None};
        let target=Rect{left:r.left+40,right:r.right+40,..r};queue_placement(&mut work,&saved,live,target,true).unwrap();}
    for (_,handle) in work{handle.join().unwrap().unwrap();}
    let(first_a,first_b,last_a,last_b)=(FIRST_A.load(Ordering::SeqCst),FIRST_B.load(Ordering::SeqCst),LAST_A.load(Ordering::SeqCst),LAST_B.load(Ordering::SeqCst));
    unsafe{crate::PostMessageW(a as *mut c_void,0x10,0,0);crate::PostMessageW(b as *mut c_void,0x10,0,0);}ui.join().unwrap();
    assert!(first_a>0&&first_b>0&&first_a<last_b&&first_b<last_a,"Le animazioni non si sono sovrapposte");
}
fn place_window(h:Hwnd, live:&Live, target:Rect, show:u32, animate:bool)->Result<(),String> {
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
    // Use the visible rectangle as the start for a maximized window, then put
    // its restored frame there before animating. Minimized windows are not visible.
    let mut from=live.rect;
    if animate && old_state==1 {let mut visible=Rect::default();if unsafe{GetWindowRect(h,(&mut visible as *mut Rect).cast())}!=0{from=visible;}}
    if old_state!=0 {unsafe{ShowWindow(h,9);}}
    if animate && old_state==1 {
        let ok=unsafe{SetWindowPos(h,0,from.left,from.top,(from.right-from.left).max(1),(from.bottom-from.top).max(1),0x14)};
        if ok==0 {let e=std::io::Error::last_os_error();return if e.raw_os_error()==Some(5){crate::ipc::move_window(h,target.left,target.top,width,height,show)}else{Err(e.to_string())};}
    }
    let size_same=from.right-from.left==width && from.bottom-from.top==height;
    let smooth=animate && old_state!=2;
    let _resolution=if smooth{Some(TimerResolution::acquire())}else{None};
    let steps=if smooth {ANIMATION_FRAMES} else {1};
    let started=std::time::Instant::now();
    for step in 1..=steps {
        if smooth {let due=Duration::from_micros(ANIMATION_MS*1000*step/steps);if let Some(wait)=due.checked_sub(started.elapsed()){thread::sleep(wait);}}
        let progress=if smooth{eased_progress(started.elapsed())}else{10_000};
        let x=interpolate(from.left,target.left,progress,10_000);
        let y=interpolate(from.top,target.top,progress,10_000);
        let w=interpolate(from.right-from.left,width,progress,10_000);
        let hgt=interpolate(from.bottom-from.top,height,progress,10_000);
        let flags=0x14 | if size_same {1} else {0};
        if unsafe{SetWindowPos(h,0,x,y,w,hgt,flags)}==0 {
            let error=std::io::Error::last_os_error();
            return if error.raw_os_error()==Some(5) {crate::ipc::move_window(h,target.left,target.top,width,height,show)} else {Err(error.to_string())};
        }
        if progress==10_000 {break}
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
#[derive(Clone)] struct LaunchPlan {exe:String,folder:Option<String>,indices:Vec<usize>,rect:Rect}
#[derive(Clone,Copy,PartialEq,Eq)] enum LaunchState {Waiting,Started,Failed}

fn queue_placement(work:&mut Vec<(String,thread::JoinHandle<Result<(),String>>)>,s:&Window,live:Live,rect:Rect,animate:bool)->Result<(),String>{
    let title=s.title.clone();let show=s.show;
    let handle=thread::Builder::new().name("myDesktop-placement".into()).spawn(move||{
        if window_status(&live)==WindowStatus::GoneOrReplaced{return Err("finestra non più disponibile".into())}
        place_window(live.hwnd,&live,rect,show,animate)
    }).map_err(|e|format!("Impossibile avviare il riposizionamento: {e}"))?;
    work.push((title,handle));Ok(())
}

/// Build launch groups without starting duplicate generic processes. Explorer
/// folders are independent requests and must each be opened separately.
fn launch_plans(saved:&[Window],initial:&[Live],matched:&[Option<Hwnd>],mode:RestoreMode,monitors:&[Monitor])->(Vec<LaunchPlan>,Vec<bool>,usize,Vec<String>){
    let mut plans:Vec<LaunchPlan>=Vec::new();let mut pending=vec![false;saved.len()];let mut skipped=0;let mut failed=Vec::new();
    for (i,s) in saved.iter().enumerate(){
        if matched[i].is_some(){continue}
        if mode==RestoreMode::Soft {skipped+=1;continue}
        if s.folder.is_none() && initial.iter().any(|w|equivalent(&s.exe,&w.exe)) {
            failed.push(format!("{} (finestra non identificabile con certezza)",s.title));continue
        }
        let Some(rect)=destination(s,monitors) else{failed.push(format!("{} (monitor non disponibile)",s.title));continue};
        pending[i]=true;
        if s.folder.is_none(){
            if let Some(plan)=plans.iter_mut().find(|p|p.folder.is_none()&&equivalent(&p.exe,&s.exe)){plan.indices.push(i);continue}
        }
        plans.push(LaunchPlan{exe:s.exe.clone(),folder:s.folder.clone(),indices:vec![i],rect});
    }
    (plans,pending,skipped,failed)
}
pub fn restore_from_mode(path:&Path,mode:RestoreMode)->Result<String,String>{
    let data=fs::read(path).map_err(|e|format!("Impossibile leggere la configurazione salvata: {e}"))?; let layout:Layout=serde_json::from_slice(&data).map_err(|e|format!("Configurazione non valida: {e}"))?; if layout.version!=1{return Err("Versione della configurazione non supportata.".into())} if layout.windows.is_empty(){return Err("La configurazione è vuota: nessuna finestra è stata modificata.".into())}
    let monitors=monitors(); if monitors.is_empty(){return Err("Nessun monitor disponibile.".into())}
    // Folder discovery uses the shell and is only needed to identify saved
    // Explorer locations; other restores can inspect windows immediately.
    let initial=if layout.windows.iter().any(|w|w.folder.is_some()){windows(None)?}else{raw_windows()};
    let mut used=protect_unmatched_initial(&layout.windows,&initial);
    let initial_handles:HashSet<_>=initial.iter().map(|w|w.hwnd).collect();
    let matched=plan_matches(&layout.windows,&initial,&used);
    let (plans,mut pending,skipped,mut failed)=launch_plans(&layout.windows,&initial,&matched,mode,&monitors);
    let mut work=Vec::new();
    for (i,found) in matched.iter().enumerate(){if let Some(h)=found{
        used.insert(*h);let s=&layout.windows[i];
        if let (Some(live),Some(rect))=(initial.iter().find(|w|w.hwnd==*h),destination(s,&monitors)){
            if let Err(e)=queue_placement(&mut work,s,live.clone(),rect,true){failed.push(format!("{} ({e})",s.title));}
        }else{failed.push(format!("{} (finestra o monitor non disponibile)",s.title));}
    }}
    let (tx,rx)=std::sync::mpsc::channel();let mut states=vec![LaunchState::Waiting;plans.len()];let mut attempted=HashSet::new();
    for (group,plan) in plans.iter().cloned().enumerate(){
        let sender=tx.clone();let exe=plan.exe.clone();
        match thread::Builder::new().name("myDesktop-launch".into()).spawn(move||{
            let result=std::panic::catch_unwind(||if let Some(folder)=&plan.folder {crate::explorer::launch(folder).map_err(|e|e.to_string())}else{launch_application(&plan.exe,plan.rect)})
                .unwrap_or_else(|_|Err("Avvio dell'applicazione interrotto".into()));
            let _=sender.send((group,result));
        }){
            Ok(_)=>{attempted.insert(exe.to_lowercase());},
            Err(e)=>{states[group]=LaunchState::Failed;for &i in &plans[group].indices{pending[i]=false;failed.push(format!("{} (impossibile avviare: {e})",layout.windows[i].title));}}
        }
    }
    drop(tx);
    let mut launched=HashSet::new();let mut folders=std::collections::HashMap::new();
    let mut folder_rx:Option<std::sync::mpsc::Receiver<Result<std::collections::HashMap<isize,String>,String>>>=None;
    let mut folder_error:Option<String>=None;let deadline=std::time::Instant::now()+Duration::from_secs(30);
    while pending.iter().any(|&p|p) && std::time::Instant::now()<deadline {
        while let Ok((group,result))=rx.try_recv(){match result{
            Ok(())=>states[group]=LaunchState::Started,
            Err(e)=>{states[group]=LaunchState::Failed;for &i in &plans[group].indices{if pending[i]{pending[i]=false;failed.push(format!("{} ({e})",layout.windows[i].title));}}}
        }}
        let active:Vec<usize>=plans.iter().enumerate().filter(|(g,_)|states[*g]==LaunchState::Started)
            .flat_map(|(_,p)|p.indices.iter().copied()).filter(|&i|pending[i]).collect();
        if active.iter().any(|&i|layout.windows[i].folder.is_some()) && folder_rx.is_none(){
            let (folder_tx,receiver)=std::sync::mpsc::channel();
            if thread::Builder::new().name("myDesktop-folders".into()).spawn(move||{let _=folder_tx.send(crate::explorer::capture());}).is_ok(){folder_rx=Some(receiver);}
        }
        if let Some(receiver)=&folder_rx {match receiver.try_recv(){
            Ok(Ok(found))=>{folders=found;folder_rx=None;folder_error=None;},
            Ok(Err(e))=>{folder_error=Some(e);folder_rx=None;},
            Err(std::sync::mpsc::TryRecvError::Disconnected)=>{folder_error=Some("Lettura cartelle di Esplora file interrotta".into());folder_rx=None;},
            Err(std::sync::mpsc::TryRecvError::Empty)=>()
        }}
        if !active.is_empty(){
            let original=raw_windows();let mut current=original.clone();
            for w in &mut current {w.folder=folders.get(&w.hwnd).cloned();if !initial_handles.contains(&w.hwnd)&&attempted.contains(&w.exe.to_lowercase()){launched.insert(w.hwnd);}}
            guard_live_windows(&initial,&mut current,&launched,&mut used);
            let candidates:Vec<_>=active.iter().map(|&i|layout.windows[i].clone()).collect();
            let assignments=plan_matches(&candidates,&current,&used);
            for (&i,found) in active.iter().zip(assignments){if let Some(h)=found{
                pending[i]=false;used.insert(h);let s=&layout.windows[i];
                if let (Some(live),Some(rect))=(original.iter().find(|w|w.hwnd==h),destination(s,&monitors)){
                    if let Err(e)=queue_placement(&mut work,s,live.clone(),rect,false){failed.push(format!("{} ({e})",s.title));}
                }else{failed.push(format!("{} (finestra o monitor non disponibile)",s.title));}
            }}
        }
        if pending.iter().any(|&p|p){thread::sleep(Duration::from_millis(50));}
    }
    for (i,&waiting) in pending.iter().enumerate(){if waiting{
        let detail=if layout.windows[i].folder.is_some(){folder_error.as_deref().unwrap_or("finestra non identificabile entro il tempo previsto")}else{"finestra non identificabile entro il tempo previsto"};
        failed.push(format!("{} ({detail})",layout.windows[i].title));
    }}
    let mut succeeded=0;
    for (title,handle) in work {match handle.join(){Ok(Ok(()))=>succeeded+=1,Ok(Err(e))=>failed.push(format!("{title} ({e})")),Err(_)=>failed.push(format!("{title} (riposizionamento interrotto)"))}}
    if mode==RestoreMode::Hard {
        if failed.is_empty() {failed.extend(close_extra_apps(mode,&layout.windows,&initial));}
        else {failed.push("Nessuna app aggiuntiva è stata chiusa perché il ripristino è incompleto".into());}
    }
    if failed.is_empty(){Ok(format!("Ripristinate {succeeded} finestre; {skipped} non aperte o non identificabili."))} else {Err(format!("Ripristinate {succeeded} finestre; {} problemi: {}",failed.len(),failed.join(", ")))}
}

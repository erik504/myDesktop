use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, mem::{size_of, zeroed}, os::windows::ffi::OsStrExt, path::PathBuf, process::Command, thread, time::Duration};

type Hwnd = isize;
#[repr(C)] #[derive(Clone, Copy, Default, Serialize, Deserialize)]
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
    fn GetWindowPlacement(hwnd: Hwnd, p: *mut Placement) -> i32;
    fn GetWindowRect(hwnd: Hwnd, r: *mut Rect) -> i32;
    fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> isize;
    fn EnumDisplayMonitors(dc: isize, clip: *const Rect, cb: Option<unsafe extern "system" fn(isize,isize,*mut Rect,isize)->i32>, arg: isize) -> i32;
    fn GetMonitorInfoW(m: isize, info: *mut MonitorInfo) -> i32;
    fn EnumDisplayDevicesW(device: *const u16, index: u32, display: *mut DisplayDevice, flags: u32) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
}
#[link(name="kernel32")] unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn QueryFullProcessImageNameW(process: isize, flags: u32, path: *mut u16, size: *mut u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
}
#[link(name="dwmapi")] unsafe extern "system" { fn DwmGetWindowAttribute(hwnd: Hwnd, attr: u32, value: *mut u32, size: u32) -> i32; }

fn text(buf: &[u16]) -> String { String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]) }
fn db_path() -> Result<PathBuf,String> { let p=std::env::var_os("APPDATA").ok_or("Cartella APPDATA non disponibile")?; Ok(PathBuf::from(p).join("myDesktop").join("layout.json")) }

#[derive(Clone, Serialize, Deserialize)] struct Monitor { id: String, device: String, rect: Rect, work: Rect, primary: bool }
#[derive(Clone, Serialize, Deserialize)] struct Window { exe: String, class: String, title: String, folder: Option<String>, monitor_id: String, monitor_device: String, rect: Rect, show: u32 }
#[derive(Serialize, Deserialize)] struct Layout { version: u32, windows: Vec<Window> }
struct Live { hwnd: Hwnd, exe: String, class: String, title: String, folder: Option<String>, monitor: isize, rect: Rect, show: u32 }

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
unsafe fn process_path(pid:u32)->Option<String>{ unsafe { let p=OpenProcess(0x1000,0,pid); if p==0{return None} let mut b=vec![0u16;32768]; let mut n=b.len() as u32; let ok=QueryFullProcessImageNameW(p,0,b.as_mut_ptr(),&mut n); CloseHandle(p as *mut std::ffi::c_void); if ok==0 {None} else {Some(String::from_utf16_lossy(&b[..n as usize]))} } }
unsafe extern "system" fn window_cb(h:Hwnd,arg:isize)->i32 { unsafe {
    if IsWindowVisible(h)==0 || GetWindow(h,4)!=0 || GetWindowLongW(h,-20)&0x80!=0 { return 1; }
    let mut cloaked=0u32; if DwmGetWindowAttribute(h,14,&mut cloaked,4)==0 && cloaked!=0 {return 1}
    let mut pid=0; GetWindowThreadProcessId(h,&mut pid); if pid==std::process::id(){return 1}
    let Some(exe)=process_path(pid) else{return 1};
    let mut cls=[0u16;256]; GetClassNameW(h,cls.as_mut_ptr(),cls.len() as i32); let class=text(&cls);
    if ["Progman","WorkerW","Shell_TrayWnd","Windows.UI.Core.CoreWindow"].contains(&class.as_str()) {return 1}
    let len=GetWindowTextLengthW(h).clamp(0,32767); let mut tb=vec![0u16;len as usize+1]; GetWindowTextW(h,tb.as_mut_ptr(),tb.len() as i32); let title=text(&tb);
    if title.is_empty() {return 1}
    let mut wp:Placement=zeroed(); wp.length=size_of::<Placement>() as u32; if GetWindowPlacement(h,&mut wp)==0{return 1}
    let monitor=MonitorFromWindow(h,2); let Some(mi)=monitor_data(monitor) else{return 1};
    let mut r=Rect::default(); if wp.show==1 { GetWindowRect(h,&mut r); } else { r=wp.normal; let dx=mi.work.left-mi.rect.left; let dy=mi.work.top-mi.rect.top; r.left+=dx;r.right+=dx;r.top+=dy;r.bottom+=dy; }
    (*(arg as *mut Vec<Live>)).push(Live{ hwnd:h,exe,class,title,folder:None,monitor,rect:r,show:wp.show }); 1
} }
fn windows()->Result<Vec<Live>,String>{ let mut v:Vec<Live>=Vec::new(); unsafe{EnumWindows(Some(window_cb),&mut v as *mut _ as isize);} let folders=crate::explorer::capture()?; for w in &mut v { w.folder=folders.get(&w.hwnd).cloned(); if w.class=="CabinetWClass" && w.folder.is_none(){return Err(format!("Impossibile leggere la cartella di Esplora file: {}",w.title));} } Ok(v) }

pub fn save()->Result<String,String>{
    let mut out=Vec::new(); for w in windows()?{ if let Some(m)=unsafe{monitor_data(w.monitor)} {out.push(Window{exe:w.exe,class:w.class,title:w.title,folder:w.folder,monitor_id:m.id,monitor_device:m.device,rect:Rect{left:w.rect.left-m.rect.left,top:w.rect.top-m.rect.top,right:w.rect.right-m.rect.left,bottom:w.rect.bottom-m.rect.top},show:w.show});} }
    if out.is_empty(){return Err("Nessuna finestra gestibile trovata: configurazione precedente conservata.".into())}
    let path=db_path()?; fs::create_dir_all(path.parent().unwrap()).map_err(|e|e.to_string())?;
    let data=serde_json::to_vec_pretty(&Layout{version:1,windows:out}).map_err(|e|e.to_string())?;
    let tmp=path.with_extension("json.tmp"); fs::write(&tmp,data).map_err(|e|e.to_string())?;
    let from:Vec<u16>=tmp.as_os_str().encode_wide().chain(Some(0)).collect(); let to:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe{MoveFileExW(from.as_ptr(),to.as_ptr(),0x9)}==0{return Err(format!("Impossibile sostituire il salvataggio: {}",std::io::Error::last_os_error()))}
    Ok(format!("Configurazione salvata in {}",path.display()))
}

#[cfg(test)] mod tests {
    use super::*;
    fn monitor(id:&str,device:&str,primary:bool)->Monitor{Monitor{id:id.into(),device:device.into(),rect:Rect::default(),work:Rect::default(),primary}}
    fn saved()->Window{Window{exe:"C:\\App.exe".into(),class:"Class".into(),title:"first".into(),folder:None,monitor_id:"physical-A".into(),monitor_device:"display1".into(),rect:Rect::default(),show:1}}
    fn live(hwnd:isize,title:&str,folder:Option<&str>)->Live{Live{hwnd,exe:"c:\\app.exe".into(),class:"Class".into(),title:title.into(),folder:folder.map(str::to_owned),monitor:0,rect:Rect::default(),show:1}}
    #[test] fn selects_saved_physical_monitor_and_falls_back_to_primary(){let ms=vec![monitor("physical-B","display1",true),monitor("physical-A","display2",false)];let mut s=saved();assert_eq!(target(&s,&ms).unwrap().device,"display2");s.monitor_id="missing".into();assert_eq!(target(&s,&ms).unwrap().device,"display1");}
    #[test] fn matches_title_and_never_reuses_window(){let s=saved();let a=vec![live(1,"other",None),live(2,"first",None)];let mut used=HashSet::new();assert_eq!(best(&s,&a,&used),Some(2));used.insert(2);assert_eq!(best(&s,&a,&used),Some(1));}
    #[test] fn matches_explorer_folder(){let mut s=saved();s.folder=Some("C:\\folder".into());let a=vec![live(1,"first",Some("C:\\other")),live(2,"other",Some("c:\\FOLDER"))];assert_eq!(best(&s,&a,&HashSet::new()),Some(2));}
}
fn equivalent(a:&str,b:&str)->bool{a.eq_ignore_ascii_case(b)}
fn matches(saved:&Window,live:&Live)->bool { equivalent(&saved.exe,&live.exe) && saved.class==live.class && match &saved.folder {Some(folder)=>live.folder.as_ref().is_some_and(|f|equivalent(folder,f)),None=>true} }
fn best(saved:&Window, live:&[Live],used:&HashSet<Hwnd>)->Option<Hwnd>{ live.iter().filter(|w|!used.contains(&w.hwnd)&&matches(saved,w)).max_by_key(|w|if w.title==saved.title{1}else{0}).map(|w|w.hwnd) }
fn target<'a>(w:&Window,monitors:&'a [Monitor])->Option<&'a Monitor>{
    // Device registry key identifies a physical display across changes to its position.
    let by_id=if w.monitor_id.is_empty(){None}else{monitors.iter().find(|m|m.id.eq_ignore_ascii_case(&w.monitor_id))};
    by_id.or_else(||if w.monitor_id.is_empty(){monitors.iter().find(|m|m.device.eq_ignore_ascii_case(&w.monitor_device))}else{None}).or_else(||monitors.iter().find(|m|m.primary)).or_else(||monitors.first())
}
pub fn restore()->Result<String,String>{
    let data=fs::read(db_path()?).map_err(|e|format!("Impossibile leggere la configurazione salvata: {e}"))?; let layout:Layout=serde_json::from_slice(&data).map_err(|e|format!("Configurazione non valida: {e}"))?; if layout.version!=1{return Err("Versione della configurazione non supportata.".into())}
    let monitors=monitors(); if monitors.is_empty(){return Err("Nessun monitor disponibile.".into())}
    let mut used=HashSet::new(); let mut succeeded=0; let mut failed=Vec::new();
    for s in &layout.windows {
        let mut current=windows()?; let mut found=best(s,&current,&used);
        if found.is_none(){
            let launched=if let Some(folder)=&s.folder{crate::explorer::launch(folder)}else{Command::new(&s.exe).spawn().map(|_|())};
            if launched.is_ok(){for _ in 0..30{thread::sleep(Duration::from_millis(300));current=windows()?;found=best(s,&current,&used);if found.is_some(){break}}}
        }
        if let Some(h)=found { used.insert(h); let m=target(s,&monitors).unwrap(); let r=s.rect; let mut x=m.rect.left+r.left; let mut y=m.rect.top+r.top; let mut width=(r.right-r.left).max(1); let mut height=(r.bottom-r.top).max(1); let same=if s.monitor_id.is_empty(){m.device.eq_ignore_ascii_case(&s.monitor_device)}else{m.id.eq_ignore_ascii_case(&s.monitor_id)}; if !same {width=width.min(m.work.right-m.work.left);height=height.min(m.work.bottom-m.work.top);x=x.clamp(m.work.left,m.work.right-width);y=y.clamp(m.work.top,m.work.bottom-height);} let ok=unsafe {ShowWindow(h,9); let ok=SetWindowPos(h,0,x,y,width,height,0x14); if s.show==3{ShowWindow(h,3);}else if [2,6,7,11].contains(&s.show){ShowWindow(h,6);} ok}; if ok!=0{succeeded+=1}else{failed.push(s.title.clone())} } else {failed.push(s.title.clone())}
    }
    if failed.is_empty(){Ok(format!("Ripristinate {succeeded} finestre."))} else {Ok(format!("Ripristinate {succeeded} finestre; {} non ripristinate: {}",failed.len(),failed.join(", ")))}
}

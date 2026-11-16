//! Tray error indicator. Keep the tooltip visible until the pointer leaves.
use std::{ffi::c_void, sync::Mutex, time::{Duration, Instant}};
type Handle = *mut c_void;
pub const TIMER: usize = 0x4d46;
#[repr(C)] struct Point { x:i32, y:i32 }
#[repr(C)] struct Rect { left:i32, top:i32, right:i32, bottom:i32 }
#[repr(C)] struct Identifier { size:u32, hwnd:Handle, id:u32, guid:crate::Guid }
struct State { pages:Vec<String>, page:usize, hover:Option<Instant>, seen:bool }
static ERROR: Mutex<Option<State>> = Mutex::new(None);
#[link(name="user32")] unsafe extern "system" {
    fn SetTimer(hwnd:Handle,id:usize,ms:u32,callback:*const c_void)->usize;
    fn KillTimer(hwnd:Handle,id:usize)->i32;
    fn GetCursorPos(p:*mut Point)->i32;
    fn CreateIcon(instance:Handle,width:i32,height:i32,planes:u8,bits:u8,and_mask:*const u8,xor_mask:*const u8)->Handle;
    fn DestroyIcon(icon:Handle)->i32;
}
#[link(name="shell32")] unsafe extern "system" { fn Shell_NotifyIconGetRect(id:*const Identifier,rect:*mut Rect)->i32; }
pub fn active()->bool { ERROR.lock().unwrap_or_else(|e|e.into_inner()).is_some() }
fn pages(text:&str)->Vec<String> {
    let mut result=Vec::new(); let mut part=String::new(); let mut units=0;
    for ch in text.replace(['\r','\n']," ").chars() {
        let n=ch.len_utf16(); if units+n>105 {result.push(std::mem::take(&mut part));units=0;}
        part.push(ch);units+=n;
    }
    if !part.is_empty() || result.is_empty(){result.push(part);} result
}
#[cfg(test)]
#[test] fn long_errors_fit_tooltip_and_keep_all_text() {
    let input="finestra 🪟 ".repeat(40);let p=pages(&input);
    assert!(p.len()>1);assert_eq!(p.concat(),input);assert!(p.iter().all(|s|s.encode_utf16().count()<=105));
}
pub unsafe fn report(hwnd:Handle, text:&str) {
    *ERROR.lock().unwrap_or_else(|e|e.into_inner())=Some(State{pages:pages(text),page:0,hover:None,seen:false});
    unsafe { draw(hwnd); SetTimer(hwnd,TIMER,150,std::ptr::null()); }
}
pub unsafe fn draw(hwnd:Handle) {
    let lock=ERROR.lock().unwrap_or_else(|e|e.into_inner());
    let Some(state)=lock.as_ref() else{return};
    let mut d=unsafe{crate::tray_data(hwnd)};
    d.flags=crate::NIF_ICON|crate::NIF_TIP;
    d.tip.fill(0);
    let message=if state.pages.len()>1 {format!("Errore ({}/{}): {}",state.page+1,state.pages.len(),state.pages[state.page])} else {format!("Errore: {}",state.pages[0])};
    let mut chars=message.encode_utf16();
    for dst in d.tip.iter_mut().take(127) { if let Some(ch)=chars.next(){*dst=ch}else{break} }
    if state.seen { unsafe{crate::Shell_NotifyIconW(1,&mut d);} return; }
    let mut mask=[0xffu8;128]; let mut pixels=[0u8;4096];
    for y in 0..32usize {for x in 0..32usize {
        let dx=x as i32-16; let dy=y as i32-16;
        if dx*dx+dy*dy>14*14 {continue}
        let white=(dx-dy).abs()<=2 || (dx+dy).abs()<=2;
        let color=if white {[255,255,255,255]}else{[35,35,210,255]};
        let i=(y*32+x)*4; pixels[i..i+4].copy_from_slice(&color);mask[y*4+x/8]&=!(0x80>>(x%8));
    }}
    unsafe {let icon=CreateIcon(std::ptr::null_mut(),32,32,1,32,mask.as_ptr(),pixels.as_ptr()); if !icon.is_null(){d.icon=icon;crate::Shell_NotifyIconW(1,&mut d);DestroyIcon(icon);}}
}
pub unsafe fn tick(hwnd:Handle) {
    let mut state=ERROR.lock().unwrap_or_else(|e|e.into_inner());
    let Some(e)=state.as_mut() else {unsafe{KillTimer(hwnd,TIMER);}return};
    let id=Identifier{size:std::mem::size_of::<Identifier>() as u32,hwnd,id:1,guid:crate::Guid{d1:0,d2:0,d3:0,d4:[0;8]}};
    let mut r=Rect{left:0,top:0,right:0,bottom:0}; let mut p=Point{x:0,y:0};
    let inside=unsafe{Shell_NotifyIconGetRect(&id,&mut r)==0 && GetCursorPos(&mut p)!=0} && p.x>=r.left && p.x<r.right && p.y>=r.top && p.y<r.bottom;
    if inside {
        let start=e.hover.get_or_insert_with(Instant::now);
        if !e.seen && start.elapsed()>=Duration::from_millis(2500) {
            if e.page+1<e.pages.len(){e.page+=1;e.hover=Some(Instant::now());}else{e.seen=true;}
            drop(state);unsafe{draw(hwnd)};
        }
    } else if e.seen {
        *state=None;drop(state);unsafe{KillTimer(hwnd,TIMER);let mut d=crate::tray_data(hwnd);d.flags=crate::NIF_ICON|crate::NIF_TIP;crate::Shell_NotifyIconW(1,&mut d);}
    } else {e.hover=None;}
}

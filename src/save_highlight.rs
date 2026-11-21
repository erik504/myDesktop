//! A click-through screen overlay; the target windows are only queried.
use std::{ffi::c_void,ptr::{null,null_mut},sync::atomic::{AtomicU64,Ordering},thread,time::Duration};
type Handle=*mut c_void;
const FLASH_DURATION:Duration=Duration::from_millis(200);
const YELLOW:u32=0x0000ffff; // Win32 COLORREF: red + green.
#[repr(C)] #[derive(Clone,Copy,Default,Debug,PartialEq,Eq)] struct Rect{left:i32,top:i32,right:i32,bottom:i32}
#[link(name="user32")] unsafe extern "system" {
    fn IsWindowVisible(hwnd:isize)->i32;
    fn IsIconic(hwnd:isize)->i32;
    fn GetWindowRect(hwnd:isize,rect:*mut c_void)->i32;
    fn SetLayeredWindowAttributes(hwnd:isize,key:u32,alpha:u8,flags:u32)->i32;
    fn SetWindowPos(hwnd:isize,after:isize,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
    fn ShowWindow(hwnd:isize,cmd:i32)->i32;
    fn UpdateWindow(hwnd:Handle)->i32;
    fn UnregisterClassW(name:*const u16,instance:Handle)->i32;
}
#[link(name="dwmapi")] unsafe extern "system" {fn DwmGetWindowAttribute(hwnd:isize,attr:u32,value:*mut u32,size:u32)->i32;}
#[link(name="gdi32")] unsafe extern "system" {fn CreateSolidBrush(color:u32)->Handle;fn DeleteObject(object:Handle)->i32;}

unsafe extern "system" fn overlay_proc(hwnd:Handle,msg:u32,w:usize,l:isize)->isize {unsafe{
    match msg {0x84=>-1,0x21=>3,_=>crate::DefWindowProcW(hwnd,msg,w,l)} // click-through, no activation
}}

/// The four strips are separate top-level windows owned by this worker thread.
pub struct Overlay {name:Vec<u16>,instance:Handle,brush:Handle,borders:[Handle;4],registered:bool}
impl Overlay {
    pub fn new()->Result<Self,String>{unsafe{
        static NEXT:AtomicU64=AtomicU64::new(0);
        let name=crate::wide(&format!("myDesktopSaveHighlight-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        let instance=crate::GetModuleHandleW(null());let brush=CreateSolidBrush(YELLOW);
        if brush.is_null(){return Err("Impossibile creare il colore per evidenziare le finestre.".into())}
        let mut overlay=Self{name,instance,brush,borders:[null_mut();4],registered:false};
        let class=crate::WndClassW{style:0,wnd_proc:Some(overlay_proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:brush,menu_name:null(),class_name:overlay.name.as_ptr()};
        if crate::RegisterClassW(&class)==0{return Err("Impossibile preparare l'evidenziazione delle finestre.".into())}
        overlay.registered=true;
        for slot in &mut overlay.borders {
            // TOPMOST | TRANSPARENT | TOOLWINDOW | LAYERED | NOACTIVATE.
            let hwnd=crate::CreateWindowExW(0x080800a8,overlay.name.as_ptr(),null(),0x80000000,0,0,0,0,null_mut(),null_mut(),instance,null_mut());
            if hwnd.is_null(){return Err("Impossibile creare l'evidenziazione delle finestre.".into())}
            *slot=hwnd;
            if SetLayeredWindowAttributes(hwnd as isize,0,255,2)==0{return Err("Impossibile mostrare l'evidenziazione delle finestre.".into())}
        }
        Ok(overlay)
    }}
    fn hide(&self){unsafe{for &h in &self.borders{if !h.is_null(){ShowWindow(h as isize,0);}}}}
    fn show(&self,hwnd:isize)->Result<bool,String>{unsafe{
        if IsWindowVisible(hwnd)==0 || IsIconic(hwnd)!=0{return Ok(false)}
        let mut r=Rect::default();
        // DWM's visible frame avoids drawing into invisible resize margins.
        if DwmGetWindowAttribute(hwnd,9,(&mut r as *mut Rect).cast(),std::mem::size_of::<Rect>() as u32)!=0 && GetWindowRect(hwnd,(&mut r as *mut Rect).cast())==0{return Ok(false)}
        let Some(parts)=strips(r) else{return Ok(false)};
        for (&h,(x,y,w,height)) in self.borders.iter().zip(parts){
            if SetWindowPos(h as isize,-1,x,y,w,height,0x10|0x40|0x200)==0{self.hide();return Err("Impossibile evidenziare una finestra sullo schermo.".into())}
            UpdateWindow(h);
        }
        Ok(true)
    }}
    pub fn flash(&self,hwnd:isize)->Result<(),String>{
        if self.show(hwnd)? {thread::sleep(FLASH_DURATION);self.hide();}
        Ok(())
    }
}
impl Drop for Overlay {fn drop(&mut self){unsafe{
    for h in &mut self.borders{if !h.is_null(){crate::DestroyWindow(*h);*h=null_mut();}}
    if self.registered{UnregisterClassW(self.name.as_ptr(),self.instance);}
    if !self.brush.is_null(){DeleteObject(self.brush);self.brush=null_mut();}
}}}
fn strips(r:Rect)->Option<[(i32,i32,i32,i32);4]>{
    let width=r.right.checked_sub(r.left)?;let height=r.bottom.checked_sub(r.top)?;
    if width<=0||height<=0{return None}let t=4.min(width).min(height);
    Some([(r.left,r.top,width,t),(r.left,r.bottom-t,width,t),(r.left,r.top,t,height),(r.right-t,r.top,t,height)])
}

#[cfg(test)] mod tests {use super::*;
    #[test] fn border_covers_edges_on_negative_monitors(){
        assert_eq!(strips(Rect{left:-400,top:-50,right:-100,bottom:100}),Some([(-400,-50,300,4),(-400,96,300,4),(-400,-50,4,150),(-104,-50,4,150)]));
        assert!(strips(Rect::default()).is_none());assert_eq!(FLASH_DURATION,Duration::from_millis(200));
    }
    // An isolated, non-activating test window is shown only for this opt-in test.
    #[test] #[ignore] fn native_overlay_is_yellow_clickthrough_and_temporary(){unsafe{
        #[link(name="kernel32")] unsafe extern "system" {fn SetErrorMode(mode:u32)->u32;}
        #[link(name="user32")] unsafe extern "system" {
            fn GetSysColorBrush(index:i32)->Handle;
            fn GetWindowLongW(hwnd:isize,index:i32)->i32;
            fn GetDC(hwnd:Handle)->Handle;
            fn ReleaseDC(hwnd:Handle,dc:Handle)->i32;
        }
        #[link(name="gdi32")] unsafe extern "system" {fn GetPixel(dc:Handle,x:i32,y:i32)->u32;}
        struct Target(Handle);impl Drop for Target{fn drop(&mut self){unsafe{crate::DestroyWindow(self.0);}}}
        SetErrorMode(2);let instance=crate::GetModuleHandleW(null());let name=crate::wide("myDesktopSaveHighlightTest");
        let class=crate::WndClassW{style:0,wnd_proc:Some(crate::DefWindowProcW),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:GetSysColorBrush(5),menu_name:null(),class_name:name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let target=Target(crate::CreateWindowExW(0x08000080,name.as_ptr(),crate::wide("Test myDesktop").as_ptr(),0x80000000,60,60,220,140,null_mut(),null_mut(),instance,null_mut()));assert!(!target.0.is_null());
        assert_ne!(SetWindowPos(target.0 as isize,-1,60,60,220,140,0x10|0x40),0);
        let mut before=Rect::default();assert_ne!(GetWindowRect(target.0 as isize,(&mut before as *mut Rect).cast()),0);
        let overlay=Overlay::new().unwrap();assert!(overlay.show(target.0 as isize).unwrap());
        for &h in &overlay.borders {assert_ne!(IsWindowVisible(h as isize),0);let flags=GetWindowLongW(h as isize,-20) as u32;assert_eq!(flags&0x080800a8,0x080800a8);}
        let dc=GetDC(overlay.borders[0]);assert!(!dc.is_null());assert_eq!(GetPixel(dc,1,1),YELLOW);ReleaseDC(overlay.borders[0],dc);
        let mut visible=Rect::default();if DwmGetWindowAttribute(target.0 as isize,9,(&mut visible as *mut Rect).cast(),std::mem::size_of::<Rect>() as u32)!=0{visible=before;}
        assert_eq!(crate::picker::test_root_at(visible.left+20,visible.top+1),target.0 as isize,"Il bordo intercetta il mouse");
        overlay.hide();let started=std::time::Instant::now();overlay.flash(target.0 as isize).unwrap();assert!(started.elapsed()>=FLASH_DURATION);
        for &h in &overlay.borders {assert_eq!(IsWindowVisible(h as isize),0);}
        let mut after=Rect::default();assert_ne!(GetWindowRect(target.0 as isize,(&mut after as *mut Rect).cast()),0);assert_eq!(after,before);
    }}
}

//! Select one existing top-level window without activating or clicking it.
use std::{ffi::c_void, ptr::{null,null_mut}, sync::atomic::{AtomicBool,AtomicIsize,Ordering}};
type Handle=*mut c_void;
pub const TIMER:usize=0x4d48;
pub const FINISHED:u32=crate::WM_APP+3;
static OWNER:AtomicIsize=AtomicIsize::new(0);
static MOUSE_HOOK:AtomicIsize=AtomicIsize::new(0);
static KEY_HOOK:AtomicIsize=AtomicIsize::new(0);
static HOVER:AtomicIsize=AtomicIsize::new(0);
static PRESSED:AtomicIsize=AtomicIsize::new(0);
static FINISH_POSTED:AtomicBool=AtomicBool::new(false);
static BORDERS:[AtomicIsize;4]=[const {AtomicIsize::new(0)};4];
#[repr(C)] #[derive(Clone,Copy,Default)] struct Point{x:i32,y:i32}
#[repr(C)] #[derive(Clone,Copy,Default)] struct Rect{left:i32,top:i32,right:i32,bottom:i32}
#[repr(C)] struct MouseEvent{point:Point,mouse_data:u32,flags:u32,time:u32,extra:usize}
#[repr(C)] struct Paint{dc:Handle,erase:i32,rect:Rect,restore:i32,update:i32,reserved:[u8;32]}
#[link(name="user32")] unsafe extern "system" {
    fn SetWindowsHookExW(id:i32,callback:Option<unsafe extern "system" fn(i32,usize,isize)->isize>,module:Handle,thread:u32)->Handle;
    fn UnhookWindowsHookEx(hook:Handle)->i32;
    fn CallNextHookEx(hook:Handle,code:i32,w:usize,l:isize)->isize;
    fn WindowFromPoint(point:Point)->isize;
    fn GetAncestor(hwnd:isize,flags:u32)->isize;
    fn GetWindowRect(hwnd:isize,rect:*mut c_void)->i32;
    fn SetLayeredWindowAttributes(hwnd:isize,key:u32,alpha:u8,flags:u32)->i32;
    fn SetWindowPos(hwnd:isize,after:isize,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
    fn ShowWindow(hwnd:isize,cmd:i32)->i32;
    fn SetTimer(hwnd:Handle,id:usize,ms:u32,callback:*const c_void)->usize;
    fn KillTimer(hwnd:Handle,id:usize)->i32;
    fn BeginPaint(hwnd:Handle,paint:*mut Paint)->Handle;
    fn EndPaint(hwnd:Handle,paint:*const Paint)->i32;
    fn GetClientRect(hwnd:Handle,rect:*mut Rect)->i32;
    fn FillRect(dc:Handle,rect:*const Rect,brush:Handle)->i32;
    fn GetSysColorBrush(index:i32)->Handle;
}

unsafe fn root_at(p:Point)->isize {unsafe{let child=WindowFromPoint(p);if child==0{0}else{GetAncestor(child,2)}}}
unsafe fn finish(confirm:usize,hwnd:isize){unsafe{
    if FINISH_POSTED.compare_exchange(false,true,Ordering::AcqRel,Ordering::Relaxed).is_ok() &&
       crate::PostMessageW(OWNER.load(Ordering::Relaxed) as Handle,FINISHED,confirm,hwnd)==0 {
        FINISH_POSTED.store(false,Ordering::Release);
    }
}}
unsafe extern "system" fn mouse_proc(code:i32,w:usize,l:isize)->isize {unsafe{
    if code==0 && l!=0 {
        match w as u32 {
            0x201=>{ // Consume down and up, so the selected application does not receive the click.
                let p=(*(l as *const MouseEvent)).point;let hover=HOVER.load(Ordering::Relaxed);
                PRESSED.store(if hover!=0 && root_at(p)==hover {hover}else{0},Ordering::Relaxed);return 1;
            },
            0x202=>{let h=PRESSED.swap(0,Ordering::Relaxed);if h>0{finish(1,h);}return 1;},
            0x204=>{PRESSED.store(-1,Ordering::Relaxed);return 1;},
            0x205=>{if PRESSED.swap(0,Ordering::Relaxed)==-1{finish(0,0);}return 1;},
            _=>()
        }
    }
    CallNextHookEx(null_mut(),code,w,l)
}}
unsafe extern "system" fn key_proc(code:i32,w:usize,l:isize)->isize {unsafe{
    if code==0 && l!=0 && (w==0x100 || w==0x104) && *(l as *const u32)==0x1b {
        finish(0,0);return 1;
    }
    CallNextHookEx(null_mut(),code,w,l)
}}
unsafe extern "system" fn border_proc(hwnd:Handle,msg:u32,w:usize,l:isize)->isize {unsafe{
    match msg {
        0x84=>-1, // HTTRANSPARENT
        0x21=>3,  // MA_NOACTIVATE
        0x14=>1,  // WM_ERASEBKGND; painting fills the entire rectangle.
        0x0f=>{let mut p:Paint=std::mem::zeroed();let dc=BeginPaint(hwnd,&mut p);if !dc.is_null(){let mut r=Rect::default();GetClientRect(hwnd,&mut r);FillRect(dc,&r,GetSysColorBrush(13));}EndPaint(hwnd,&p);0},
        _=>crate::DefWindowProcW(hwnd,msg,w,l)
    }
}}
unsafe fn hide_borders(){unsafe{for h in &BORDERS {let h=h.load(Ordering::Relaxed);if h!=0{ShowWindow(h,0);}}}}
unsafe fn outline(r:Rect){unsafe{
    let width=(r.right-r.left).max(1);let height=(r.bottom-r.top).max(1);let t=4.min(width).min(height);
    let parts=[(r.left,r.top,width,t),(r.left,r.bottom-t,width,t),(r.left,r.top,t,height),(r.right-t,r.top,t,height)];
    for (h,(x,y,w,hgt)) in BORDERS.iter().zip(parts){let hwnd=h.load(Ordering::Relaxed);if hwnd!=0{SetWindowPos(hwnd,-1,x,y,w,hgt,0x10|0x40|0x200);}}
}}
pub unsafe fn begin(owner:Handle)->Result<(),String>{unsafe{
    if OWNER.load(Ordering::Relaxed)!=0{return Err("Selezione già in corso.".into())}
    let class_name=crate::wide("myDesktopPickerBorder");let instance=crate::GetModuleHandleW(null());
    let class=crate::WndClassW{style:0,wnd_proc:Some(border_proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu_name:null(),class_name:class_name.as_ptr()};
    crate::RegisterClassW(&class);
    FINISH_POSTED.store(false,Ordering::Relaxed);
    OWNER.store(owner as isize,Ordering::Relaxed);
    for slot in &BORDERS {
        let hwnd=crate::CreateWindowExW(0x080800a8,class_name.as_ptr(),null(),0x80000000,0,0,0,0,owner,null_mut(),instance,null_mut());
        if hwnd.is_null(){stop(owner);return Err("Impossibile creare l'evidenziazione delle finestre.".into())}
        slot.store(hwnd as isize,Ordering::Relaxed);
        if SetLayeredWindowAttributes(hwnd as isize,0,255,2)==0 {stop(owner);return Err("Impossibile mostrare l'evidenziazione delle finestre.".into())}
    }
    let mouse=SetWindowsHookExW(14,Some(mouse_proc),instance,0);
    if mouse.is_null(){stop(owner);return Err("Impossibile attivare la selezione con il mouse.".into())}
    MOUSE_HOOK.store(mouse as isize,Ordering::Relaxed);
    let keyboard=SetWindowsHookExW(13,Some(key_proc),instance,0);
    if keyboard.is_null(){stop(owner);return Err("Impossibile attivare l'annullamento con Esc.".into())}
    KEY_HOOK.store(keyboard as isize,Ordering::Relaxed);
    if SetTimer(owner,TIMER,50,null())==0{stop(owner);return Err("Impossibile avviare la selezione delle finestre.".into())}
    Ok(())
}}
pub unsafe fn tick(owner:Handle){unsafe{
    if OWNER.load(Ordering::Relaxed)!=owner as isize{return}
    let mut pt=crate::Point{x:0,y:0};if crate::GetCursorPos(&mut pt)==0{return}
    let root=root_at(Point{x:pt.x,y:pt.y});let old=HOVER.load(Ordering::Relaxed);
    let target=if root==old{root}else if root!=0 && crate::core::selectable(root){root}else{0};
    if target==0{HOVER.store(0,Ordering::Relaxed);hide_borders();return}
    let mut rect=Rect::default();if GetWindowRect(target,(&mut rect as *mut Rect).cast())==0{HOVER.store(0,Ordering::Relaxed);hide_borders();return}
    HOVER.store(target,Ordering::Relaxed);outline(rect);
}}
pub unsafe fn stop(owner:Handle){unsafe{
    KillTimer(owner,TIMER);HOVER.store(0,Ordering::Relaxed);PRESSED.store(0,Ordering::Relaxed);
    let mouse=MOUSE_HOOK.swap(0,Ordering::Relaxed) as Handle;if !mouse.is_null(){UnhookWindowsHookEx(mouse);}
    let key=KEY_HOOK.swap(0,Ordering::Relaxed) as Handle;if !key.is_null(){UnhookWindowsHookEx(key);}
    for h in &BORDERS {let h=h.swap(0,Ordering::Relaxed) as Handle;if !h.is_null(){crate::DestroyWindow(h);}}
    OWNER.store(0,Ordering::Relaxed);
}}
pub fn active()->bool{OWNER.load(Ordering::Relaxed)!=0}

#[cfg(test)] mod tests {use super::*;
    // Uses only a temporary test window. The hooks are always removed by Guard.
    #[test] #[ignore] fn native_picker_highlights_and_consumes_confirmation(){unsafe{
        #[link(name="kernel32")] unsafe extern "system" {fn SetErrorMode(mode:u32)->u32;}
        #[link(name="user32")] unsafe extern "system" {
            fn PeekMessageW(msg:*mut crate::Msg,hwnd:Handle,min:u32,max:u32,remove:u32)->i32;
        }
        struct Guard{owner:Handle,target:Handle}
        impl Drop for Guard {fn drop(&mut self){unsafe{stop(self.owner);if !self.target.is_null(){crate::DestroyWindow(self.target);}if !self.owner.is_null(){crate::DestroyWindow(self.owner);}}}}
        SetErrorMode(2);
        let instance=crate::GetModuleHandleW(null());let class_name=crate::wide("myDesktopPickerTestTarget");
        let class=crate::WndClassW{style:0,wnd_proc:Some(crate::DefWindowProcW),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:GetSysColorBrush(5),menu_name:null(),class_name:class_name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let owner=crate::CreateWindowExW(0,class_name.as_ptr(),null(),0,0,0,0,0,null_mut(),null_mut(),instance,null_mut());assert!(!owner.is_null());
        let x=60;let y=60;
        let target=crate::CreateWindowExW(0x08000080,class_name.as_ptr(),crate::wide("Test myDesktop").as_ptr(),0x80000000,x,y,220,140,null_mut(),null_mut(),instance,null_mut());
        let guard=Guard{owner,target};assert!(!guard.target.is_null());
        assert_ne!(SetWindowPos(target as isize,-1,x,y,220,140,0x10|0x40),0);
        begin(owner).unwrap();
        let mut rect=Rect::default();assert_ne!(GetWindowRect(target as isize,(&mut rect as *mut Rect).cast()),0);
        HOVER.store(target as isize,Ordering::Relaxed);outline(rect);
        let border=Point{x:rect.left+20,y:rect.top+1};assert_eq!(root_at(border),target as isize,"Highlight intercepts mouse hit-testing");
        let event=MouseEvent{point:Point{x:rect.left+100,y:rect.top+70},mouse_data:0,flags:0,time:0,extra:0};
        let ptr=(&event as *const MouseEvent) as isize;
        assert_eq!(mouse_proc(0,0x201,ptr),1);assert_eq!(mouse_proc(0,0x202,ptr),1);
        let escape=0x1bu32;assert_eq!(key_proc(0,0x100,(&escape as *const u32) as isize),1);
        let mut msg:crate::Msg=std::mem::zeroed();assert_ne!(PeekMessageW(&mut msg,owner,FINISHED,FINISHED,1),0);
        assert_eq!(msg.wparam,1);assert_eq!(msg.lparam,target as isize);
        assert_eq!(PeekMessageW(&mut msg,owner,FINISHED,FINISHED,1),0,"A second completion was queued");
        stop(owner);begin(owner).unwrap();assert_eq!(key_proc(0,0x100,(&escape as *const u32) as isize),1);
        assert_ne!(PeekMessageW(&mut msg,owner,FINISHED,FINISHED,1),0);assert_eq!(msg.wparam,0);
        drop(guard);
        assert_eq!(MOUSE_HOOK.load(Ordering::Relaxed),0);assert_eq!(KEY_HOOK.load(Ordering::Relaxed),0);
    }}
}

//! Menu descriptions in a small, non-activating native window.
//!
//! A menu is modal, so the description is driven by WM_MENUSELECT and a timer
//! on its owning UI thread. No common-control tooltip or cross-thread UI calls
//! are needed. Handles and the selected command are only used on that thread.
use std::{ffi::c_void, mem::size_of, ptr::{null, null_mut}, sync::atomic::{AtomicIsize, AtomicU32, Ordering}};
type Handle = *mut c_void;
pub const TIMER: usize = 0x4d47;
const WM_PAINT: u32 = 0x0f;
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static MENU: AtomicIsize = AtomicIsize::new(0);
static COMMAND: AtomicU32 = AtomicU32::new(0);
#[cfg(test)] static PAINTED: AtomicU32 = AtomicU32::new(0);
#[repr(C)] #[derive(Clone, Copy, Default)] struct Rect { left:i32, top:i32, right:i32, bottom:i32 }
#[repr(C)] struct MonitorInfo { size:u32, monitor:Rect, work:Rect, flags:u32 }
#[repr(C)] struct Paint { dc:Handle, erase:i32, rect:Rect, restore:i32, inc_update:i32, reserved:[u8;32] }
#[link(name="user32")] unsafe extern "system" {
    fn SetTimer(hwnd:Handle,id:usize,ms:u32,cb:*const c_void)->usize;
    fn KillTimer(hwnd:Handle,id:usize)->i32;
    fn ShowWindow(hwnd:isize,cmd:i32)->i32;
    fn SetWindowPos(hwnd:isize,after:isize,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
    fn GetMenuItemCount(menu:Handle)->i32;
    fn GetMenuItemID(menu:Handle,pos:i32)->u32;
    fn GetMenuItemRect(owner:Handle,menu:Handle,pos:u32,rect:*mut Rect)->i32;
    fn MonitorFromRect(rect:*const Rect,flags:u32)->isize;
    fn GetMonitorInfoW(monitor:isize,info:*mut c_void)->i32;
    fn GetDpiForWindow(hwnd:Handle)->u32;
    fn BeginPaint(hwnd:Handle,paint:*mut Paint)->Handle;
    fn EndPaint(hwnd:Handle,paint:*const Paint)->i32;
    fn GetClientRect(hwnd:Handle,rect:*mut Rect)->i32;
    fn FillRect(dc:Handle,rect:*const Rect,brush:Handle)->i32;
    fn GetSysColorBrush(index:i32)->Handle;
    fn GetSysColor(index:i32)->u32;
    fn DrawTextW(dc:Handle,text:*const u16,count:i32,rect:*mut Rect,format:u32)->i32;
    fn InvalidateRect(hwnd:Handle,rect:*const Rect,erase:i32)->i32;
    #[cfg(test)] fn IsWindowVisible(hwnd:isize)->i32;
}
#[link(name="gdi32")] unsafe extern "system" {
    fn SetBkMode(dc:Handle,mode:i32)->i32;
    fn SetTextColor(dc:Handle,color:u32)->u32;
    fn GetStockObject(index:i32)->Handle;
    fn SelectObject(dc:Handle,object:Handle)->Handle;
}
fn help(id:u32)->Option<&'static str> { Some(match id {
    1=>"Salva la disposizione attuale come configurazione predefinita.",
    2=>"Ripristina la disposizione e riapre le finestre mancanti quando possibile.",
    8=>"Ripristina solo le finestre già aperte, senza avviare applicazioni.",
    9=>"Ripristina e chiede di chiudere le app assenti dal salvataggio.",
    6=>"Salva la disposizione in un file scelto da te.",
    7=>"Ripristina una configurazione da un file scelto da te.",
    5=>"Gestisce l'avvio al login e il servizio per le finestre elevate.",
    10=>"Mostra una spiegazione dei comandi disponibili.",
    4=>"Mostra la versione e la data di compilazione.",
    3=>"Chiude myDesktop; il servizio installato rimane attivo.",
    _=>return None,
}) }
unsafe extern "system" fn tip_proc(hwnd:Handle,msg:u32,w:usize,l:isize)->isize {unsafe{
    if msg!=WM_PAINT {return crate::DefWindowProcW(hwnd,msg,w,l)}
    let mut paint:Paint=std::mem::zeroed();
    let dc=BeginPaint(hwnd,&mut paint);
    if !dc.is_null() {
        let mut rect=Rect::default();GetClientRect(hwnd,&mut rect);
        FillRect(dc,&rect,GetSysColorBrush(24)); // COLOR_INFOBK
        rect.left+=8;rect.top+=6;rect.right-=8;rect.bottom-=6;
        SetBkMode(dc,1);SetTextColor(dc,GetSysColor(23)); // TRANSPARENT, COLOR_INFOTEXT
        let previous=SelectObject(dc,GetStockObject(17)); // DEFAULT_GUI_FONT
        if let Some(text)=help(COMMAND.load(Ordering::Relaxed)) {
            let text=crate::wide(text);let drawn=DrawTextW(dc,text.as_ptr(),-1,&mut rect,0x10|0x800); // WORDBREAK | NOPREFIX
            #[cfg(test)] if drawn>0 {PAINTED.fetch_or(1<<COMMAND.load(Ordering::Relaxed),Ordering::Relaxed);}
            #[cfg(not(test))] let _=drawn;
        }
        if !previous.is_null(){SelectObject(dc,previous);}
    }
    EndPaint(hwnd,&paint);0
}}
/// Construct the description window before entering TrackPopupMenu.
pub unsafe fn prepare(owner:Handle) {unsafe{
    if WINDOW.load(Ordering::Relaxed)!=0{return}
    let name=crate::wide("myDesktopMenuDescription");let instance=crate::GetModuleHandleW(null());
    let class=crate::WndClassW{style:0,wnd_proc:Some(tip_proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu_name:null(),class_name:name.as_ptr()};
    crate::RegisterClassW(&class); // Class may already exist after a prior menu.
    // WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_TRANSPARENT
    let tip=crate::CreateWindowExW(0x080000a8,name.as_ptr(),null(),0x80800000,0,0,0,0,owner,null_mut(),instance,null_mut());
    if !tip.is_null(){WINDOW.store(tip as isize,Ordering::Relaxed);}
}}
/// Selection changes cannot block: the UI thread owns the window and state.
pub unsafe fn select(owner:Handle,menu:Handle,id:u32) {unsafe{
    KillTimer(owner,TIMER);MENU.store(0,Ordering::Relaxed);COMMAND.store(0,Ordering::Relaxed);
    let tip=WINDOW.load(Ordering::Relaxed) as Handle;
    if !tip.is_null(){ShowWindow(tip as isize,0);}
    if !menu.is_null() && help(id).is_some() && !tip.is_null(){
        MENU.store(menu as isize,Ordering::Relaxed);COMMAND.store(id,Ordering::Relaxed);
        SetTimer(owner,TIMER,650,null());
    }
}}
fn position(item:Rect,work:Rect,width:i32,height:i32)->(i32,i32){
    let right=item.right.saturating_add(8);let left=item.left.saturating_sub(width).saturating_sub(8);
    let x=if right.saturating_add(width)<=work.right {right}else{left};
    (x.clamp(work.left,(work.right-width).max(work.left)),item.top.clamp(work.top,(work.bottom-height).max(work.top)))
}
pub unsafe fn show(owner:Handle) {unsafe{
    KillTimer(owner,TIMER);
    let tip=WINDOW.load(Ordering::Relaxed) as Handle;let menu=MENU.load(Ordering::Relaxed) as Handle;let id=COMMAND.load(Ordering::Relaxed);
    if tip.is_null()||menu.is_null()||help(id).is_none(){return}
    let count=GetMenuItemCount(menu);let Some(pos)=(0..count).find(|&i|GetMenuItemID(menu,i)==id) else{return};
    let mut rect=Rect::default();if GetMenuItemRect(owner,menu,pos as u32,&mut rect)==0{return}
    let monitor=MonitorFromRect(&rect,2);let mut info=MonitorInfo{size:size_of::<MonitorInfo>() as u32,monitor:Rect::default(),work:Rect::default(),flags:0};
    if monitor==0 || GetMonitorInfoW(monitor,(&mut info as *mut MonitorInfo).cast())==0{return}
    let dpi=GetDpiForWindow(owner).max(96) as i32;let width=320*dpi/96;let height=62*dpi/96;
    let (x,y)=position(rect,info.work,width,height);
    InvalidateRect(tip,null(),1);
    SetWindowPos(tip as isize,-1,x,y,width,height,0x10|0x40|0x200); // NOACTIVATE | SHOWWINDOW | NOOWNERZORDER
}}
pub unsafe fn hide(owner:Handle){unsafe{select(owner,null_mut(),0);}}
pub unsafe fn destroy(owner:Handle){unsafe{
    hide(owner);let tip=WINDOW.swap(0,Ordering::Relaxed) as Handle;if !tip.is_null(){crate::DestroyWindow(tip);}
}}

#[cfg(test)] mod tests {use super::*;
    #[test] fn all_commands_have_help(){for id in [1,2,3,4,5,6,7,8,9,10]{assert!(help(id).unwrap().len()<150)}assert!(help(0).is_none());}
    #[test] fn positions_fit_work_area(){let work=Rect{left:-1000,top:0,right:0,bottom:800};let item=Rect{left:-200,top:780,right:-10,bottom:800};let(x,y)=position(item,work,250,50);assert!(x>=work.left&&x+250<=work.right&&y>=work.top&&y+50<=work.bottom);}
    // Explicitly opt in: the genuine Win32 modal menu briefly appears on the desktop.
    #[test] #[ignore] fn interactive_menu_shows_help_and_closes(){unsafe{
        use std::sync::atomic::Ordering;
        #[link(name="kernel32")] unsafe extern "system" {fn SetErrorMode(mode:u32)->u32;}
        static SHOWN:AtomicU32=AtomicU32::new(0);
        unsafe extern "system" fn owner_proc(hwnd:Handle,msg:u32,w:usize,l:isize)->isize {unsafe{
            let result=crate::wnd_proc(hwnd,msg,w,l);
            if msg==crate::WM_TIMER && w==TIMER {
                let tip=WINDOW.load(Ordering::Relaxed);
                if tip!=0 && IsWindowVisible(tip)!=0 {SHOWN.fetch_or(1<<COMMAND.load(Ordering::Relaxed),Ordering::SeqCst);}
            }
            result
        }}
        SetErrorMode(0x0002); // The isolated test must never leave a crash dialog on the user's desktop.
        let name=crate::wide("myDesktopMenuLoopTest");let instance=crate::GetModuleHandleW(null());
        let class=crate::WndClassW{style:0,wnd_proc:Some(owner_proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu_name:null(),class_name:name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let owner=crate::CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,0,0,null_mut(),null_mut(),instance,null_mut());assert!(!owner.is_null());
        let menu=crate::CreatePopupMenu();assert!(!menu.is_null());
        assert_ne!(crate::AppendMenuW(menu,0,1,crate::wide("Memorizza finestre").as_ptr()),0);
        assert_ne!(crate::AppendMenuW(menu,0,8,crate::wide("Ripristina finestre (solo presenti)").as_ptr()),0);
        prepare(owner);assert_ne!(WINDOW.load(Ordering::Relaxed),0);
        let owner_id=owner as usize;let menu_id=menu as isize;
        let sender=std::thread::spawn(move||{
            std::thread::sleep(std::time::Duration::from_millis(350));
            crate::PostMessageW(owner_id as Handle,crate::WM_MENUSELECT,1,menu_id);
            std::thread::sleep(std::time::Duration::from_millis(1050));
            crate::PostMessageW(owner_id as Handle,crate::WM_MENUSELECT,8,menu_id);
            std::thread::sleep(std::time::Duration::from_millis(1050));
            crate::PostMessageW(owner_id as Handle,0x1f,0,0); // WM_CANCELMODE
        });
        let mut p=crate::Point{x:0,y:0};crate::GetCursorPos(&mut p);crate::SetForegroundWindow(owner);
        crate::TrackPopupMenu(menu,0x102,p.x,p.y,0,owner,null());
        sender.join().unwrap();
        let tip=WINDOW.load(Ordering::Relaxed);hide(owner);
        assert_eq!(IsWindowVisible(tip),0,"Description remained visible after closing the menu");
        crate::DestroyMenu(menu);crate::DestroyWindow(owner);
        let expected=(1<<1)|(1<<8);
        assert_eq!(SHOWN.load(Ordering::SeqCst)&expected,expected,"Descriptions were not displayed during the menu loop");
        assert_eq!(PAINTED.load(Ordering::SeqCst)&expected,expected,"Descriptions were not painted during the menu loop");
    }}
}

//! Native tracking tooltips for the tray popup menu.
use std::{ffi::c_void, mem::size_of, ptr::{null,null_mut}, sync::Mutex};
type Handle=*mut c_void;
pub const TIMER:usize=0x4d47;
const TTM_ADDTOOLW:u32=0x400+50;
const TTM_UPDATETIPTEXTW:u32=0x400+57;
const TTM_TRACKACTIVATE:u32=0x400+17;
const TTM_TRACKPOSITION:u32=0x400+18;
const TTM_SETMAXTIPWIDTH:u32=0x400+24;
const TTM_GETBUBBLESIZE:u32=0x400+30;
#[repr(C)] #[derive(Clone,Copy,Default)] struct Rect {left:i32,top:i32,right:i32,bottom:i32}
#[repr(C)] struct ToolInfo {size:u32,flags:u32,owner:Handle,id:usize,rect:Rect,instance:Handle,text:*mut u16,param:isize,reserved:*mut c_void}
#[repr(C)] struct InitControls {size:u32,classes:u32}
#[repr(C)] struct MonitorInfo {size:u32,monitor:Rect,work:Rect,flags:u32}
#[link(name="user32")] unsafe extern "system" {
    fn SendMessageW(hwnd:Handle,msg:u32,w:usize,l:isize)->isize;
    fn SetTimer(hwnd:Handle,id:usize,ms:u32,cb:*const c_void)->usize;
    fn KillTimer(hwnd:Handle,id:usize)->i32;
    fn GetMenuItemCount(menu:Handle)->i32;
    fn GetMenuItemID(menu:Handle,pos:i32)->u32;
    fn GetMenuItemRect(owner:Handle,menu:Handle,pos:u32,rect:*mut Rect)->i32;
    fn MonitorFromRect(rect:*const Rect,flags:u32)->isize;
    fn GetMonitorInfoW(monitor:isize,info:*mut c_void)->i32;
    #[cfg(test)] fn IsWindowVisible(hwnd:isize)->i32;
}
#[link(name="comctl32")] unsafe extern "system" {fn InitCommonControlsEx(info:*const InitControls)->i32;}
struct State {tooltip:isize, menu:isize, id:u32, text:Vec<u16>}
static STATE:Mutex<State>=Mutex::new(State{tooltip:0,menu:0,id:0,text:Vec::new()});
fn help(id:u32)->Option<&'static str>{Some(match id {
    1=>"Salva la disposizione attuale come configurazione predefinita.",
    2=>"Ripristina la configurazione predefinita, riaprendo le finestre mancanti quando possibile.",
    8=>"Ripristina solo le finestre già aperte, senza avviare applicazioni.",
    9=>"Ripristina e chiede di chiudere le app assenti dal salvataggio, dopo conferma.",
    6=>"Salva la disposizione in un file scelto da te.",
    7=>"Ripristina una configurazione da un file scelto da te.",
    5=>"Attiva o disattiva l'avvio al login e il servizio per le finestre elevate.",
    4=>"Mostra la versione e la data di compilazione.",
    3=>"Chiude myDesktop; il servizio installato rimane attivo.",
    _=>return None,
})}
// The v2 TOOLINFO size is accepted by both classic and newer common controls.
fn tool(owner:Handle,text:*mut u16)->ToolInfo {ToolInfo{size:(size_of::<ToolInfo>()-size_of::<*mut c_void>()) as u32,flags:0x20|0x80,owner,id:1,rect:Rect::default(),instance:null_mut(),text,param:0,reserved:null_mut()}}
unsafe fn ensure(owner:Handle,state:&mut State)->bool {unsafe{
    if state.tooltip!=0{return true}
    let controls=InitControls{size:size_of::<InitControls>() as u32,classes:0xff};
    if InitCommonControlsEx(&controls)==0{#[cfg(test)] eprintln!("InitCommonControlsEx: {}",std::io::Error::last_os_error());return false}
    let class:Vec<u16>="tooltips_class32".encode_utf16().chain(Some(0)).collect();
    let tip=crate::CreateWindowExW(0x8|0x80|0x08000000,class.as_ptr(),null(),0x80000000|0x01|0x02,0,0,0,0,owner,null_mut(),crate::GetModuleHandleW(null()),null_mut());
    if tip.is_null(){#[cfg(test)] eprintln!("CreateWindowExW tooltip: {}",std::io::Error::last_os_error());return false}
    state.text=vec![b' ' as u16,0];let mut info=tool(owner,state.text.as_mut_ptr());
    if SendMessageW(tip,TTM_ADDTOOLW,0,(&mut info as *mut ToolInfo) as isize)==0 {#[cfg(test)] eprintln!("TTM_ADDTOOLW: {} size={}",std::io::Error::last_os_error(),info.size);crate::DestroyWindow(tip);return false}
    SendMessageW(tip,TTM_SETMAXTIPWIDTH,0,320);state.tooltip=tip as isize;true
}}
/// Arm a delayed tooltip for the selected command, without changing menu input.
pub unsafe fn select(owner:Handle,menu:Handle,id:u32) {unsafe{
    KillTimer(owner,TIMER);
    let mut s=STATE.lock().unwrap_or_else(|e|e.into_inner());
    if s.tooltip!=0 {let mut t=tool(owner,s.text.as_mut_ptr());SendMessageW(s.tooltip as Handle,TTM_TRACKACTIVATE,0,(&mut t as *mut ToolInfo) as isize);}
    let Some(message)=help(id) else{s.id=0;s.menu=0;return};
    if menu.is_null()||!ensure(owner,&mut s){return}
    let previous=std::mem::replace(&mut s.text,message.encode_utf16().chain(Some(0)).collect());
    let mut info=tool(owner,s.text.as_mut_ptr());
    SendMessageW(s.tooltip as Handle,TTM_UPDATETIPTEXTW,0,(&mut info as *mut ToolInfo) as isize);
    drop(previous);s.id=id;s.menu=menu as isize;
    SetTimer(owner,TIMER,650,null());
}}
fn position(item:Rect,work:Rect,width:i32,height:i32)->(i32,i32){
    let right=item.right.saturating_add(8);let left=item.left.saturating_sub(width).saturating_sub(8);
    let x=if right.saturating_add(width)<=work.right {right}else{left};
    (x.clamp(work.left,(work.right-width).max(work.left)),item.top.clamp(work.top,(work.bottom-height).max(work.top)))
}
pub unsafe fn show(owner:Handle) {unsafe{
    KillTimer(owner,TIMER);
    let mut s=STATE.lock().unwrap_or_else(|e|e.into_inner());
    if s.tooltip==0||s.menu==0||s.id==0{return}
    let menu=s.menu as Handle;let count=GetMenuItemCount(menu);
    let pos=(0..count).find(|&i|GetMenuItemID(menu,i)==s.id);
    let Some(pos)=pos else{return};let mut rect=Rect::default();
    if GetMenuItemRect(owner,menu,pos as u32,&mut rect)==0{return}
    let mut info=tool(owner,s.text.as_mut_ptr());let tip=s.tooltip as Handle;
    SendMessageW(tip,TTM_UPDATETIPTEXTW,0,(&mut info as *mut ToolInfo) as isize);
    let size=SendMessageW(tip,TTM_GETBUBBLESIZE,0,(&mut info as *mut ToolInfo) as isize) as u32;
    let width=((size&0xffff) as i32).max(80);let height=((size>>16) as i32).max(25);
    let mon=MonitorFromRect(&rect,2);let mut mi=MonitorInfo{size:size_of::<MonitorInfo>() as u32,monitor:Rect::default(),work:Rect::default(),flags:0};
    let work=if mon!=0&&GetMonitorInfoW(mon,(&mut mi as *mut MonitorInfo).cast())!=0 {mi.work} else {Rect{left:rect.left-1000,top:rect.top-1000,right:rect.right+1000,bottom:rect.bottom+1000}};
    let (x,y)=position(rect,work,width,height);
    let packed=((x as u16 as u32)|((y as u16 as u32)<<16)) as isize;
    SendMessageW(tip,TTM_TRACKPOSITION,0,packed);
    SendMessageW(tip,TTM_TRACKACTIVATE,1,(&mut info as *mut ToolInfo) as isize);
}}
pub unsafe fn hide(owner:Handle) {unsafe{
    KillTimer(owner,TIMER);let mut s=STATE.lock().unwrap_or_else(|e|e.into_inner());s.menu=0;s.id=0;
    if s.tooltip!=0 {let mut t=tool(owner,s.text.as_mut_ptr());SendMessageW(s.tooltip as Handle,TTM_TRACKACTIVATE,0,(&mut t as *mut ToolInfo) as isize);}
}}
pub unsafe fn destroy(owner:Handle) {unsafe{hide(owner);let mut s=STATE.lock().unwrap_or_else(|e|e.into_inner());if s.tooltip!=0{crate::DestroyWindow(s.tooltip as Handle);s.tooltip=0;}s.text.clear();}}
#[cfg(test)] mod tests {use super::*;
    #[test] fn all_menu_commands_have_short_help(){for id in [1,2,3,4,5,6,7,8,9]{let text=help(id).unwrap();assert!(!text.is_empty()&&text.len()<150);}assert!(help(0).is_none());}
    #[test] fn positions_help_within_monitor_work_area(){let work=Rect{left:-1000,top:0,right:0,bottom:800};let item=Rect{left:-200,top:780,right:-10,bottom:800};let (x,y)=position(item,work,250,50);assert!(x>=work.left&&x+250<=work.right&&y>=work.top&&y+50<=work.bottom);}
    #[test] fn native_tooltip_registers_and_is_cleaned_up(){unsafe{
        let name:Vec<u16>="myDesktopMenuHelpTest".encode_utf16().chain(Some(0)).collect();let instance=crate::GetModuleHandleW(null());
        let class=crate::WndClassW{style:0,wnd_proc:Some(crate::DefWindowProcW),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu_name:null(),class_name:name.as_ptr()};
        assert_ne!(crate::RegisterClassW(&class),0);
        let owner=crate::CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,0,0,null_mut(),null_mut(),instance,null_mut());assert!(!owner.is_null());
        let menu=crate::CreatePopupMenu();assert!(!menu.is_null());
        let title:Vec<u16>="Salva".encode_utf16().chain(Some(0)).collect();assert_ne!(crate::AppendMenuW(menu,0,1,title.as_ptr()),0);
        select(owner,menu,1);
        {let mut s=STATE.lock().unwrap();assert_ne!(s.tooltip,0);assert_eq!(s.id,1);assert_eq!(SendMessageW(s.tooltip as Handle,0x400+13,0,0),1);
            let mut text=[0u16;256];let mut info=tool(owner,text.as_mut_ptr());SendMessageW(s.tooltip as Handle,0x400+56,0,(&mut info as *mut ToolInfo) as isize);
            let n=text.iter().position(|&c|c==0).unwrap();assert_eq!(String::from_utf16_lossy(&text[..n]),help(1).unwrap());
            let mut info=tool(owner,s.text.as_mut_ptr());let tip=s.tooltip as Handle;
            let pos=((-32000i16 as u16 as u32)|((-32000i16 as u16 as u32)<<16)) as isize;
            SendMessageW(tip,TTM_TRACKPOSITION,0,pos);SendMessageW(tip,TTM_TRACKACTIVATE,1,(&mut info as *mut ToolInfo) as isize);
            assert_ne!(IsWindowVisible(tip as isize),0);}
        hide(owner);{let s=STATE.lock().unwrap();assert_eq!(s.id,0);}
        destroy(owner);crate::DestroyMenu(menu);crate::DestroyWindow(owner);
    }}
}

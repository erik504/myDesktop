#![windows_subsystem="windows"]
//! Small standalone fixture used only by the opt-in restore integration test.
use std::{ffi::c_void,ptr::{null,null_mut},sync::atomic::{AtomicBool,AtomicUsize,Ordering},time::{Duration,SystemTime,UNIX_EPOCH}};
type H=*mut c_void;
#[repr(C)] struct Class{style:u32,proc:Option<unsafe extern "system" fn(H,u32,usize,isize)->isize>,cls:i32,extra:i32,instance:H,icon:H,cursor:H,background:H,menu:*const u16,name:*const u16}
#[repr(C)] struct Point{x:i32,y:i32}
#[repr(C)] struct Msg{hwnd:H,msg:u32,w:usize,l:isize,time:u32,point:Point,private:u32}
#[repr(C)] #[derive(Default)] struct Rect{left:i32,top:i32,right:i32,bottom:i32}
static CREATED:AtomicBool=AtomicBool::new(false);static CHANGES:AtomicUsize=AtomicUsize::new(0);
fn marker(suffix:&str)->std::path::PathBuf{let exe=std::env::current_exe().unwrap();exe.with_extension(suffix)}
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
#[link(name="user32")] unsafe extern "system" {
    fn RegisterClassW(class:*const Class)->u16;
    fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,x:i32,y:i32,w:i32,h:i32,parent:H,menu:H,instance:H,param:*mut c_void)->H;
    fn DefWindowProcW(hwnd:H,msg:u32,w:usize,l:isize)->isize;
    fn GetWindowRect(hwnd:H,rect:*mut Rect)->i32;
    fn GetMessageW(msg:*mut Msg,hwnd:H,min:u32,max:u32)->i32;
    fn TranslateMessage(msg:*const Msg)->i32;
    fn DispatchMessageW(msg:*const Msg)->isize;
    fn SetTimer(hwnd:H,id:usize,ms:u32,callback:*const c_void)->usize;
    fn DestroyWindow(hwnd:H)->i32;
    fn PostQuitMessage(code:i32);
}
#[link(name="kernel32")] unsafe extern "system" {fn GetModuleHandleW(name:*const u16)->H;}
unsafe extern "system" fn proc(hwnd:H,msg:u32,w:usize,l:isize)->isize{unsafe{
    if msg==0x46 && CREATED.load(Ordering::SeqCst){let n=CHANGES.fetch_add(1,Ordering::SeqCst)+1;let _=std::fs::write(marker("changes"),n.to_string());}
    if msg==0x113 {DestroyWindow(hwnd);return 0}
    if msg==2 {PostQuitMessage(0);return 0}
    DefWindowProcW(hwnd,msg,w,l)
}}
fn main(){unsafe{
    let started=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    std::fs::write(marker("started"),started.to_string()).unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    let instance=GetModuleHandleW(null());let name=wide("myDesktopRestoreFixture");
    let class=Class{style:0,proc:Some(proc),cls:0,extra:0,instance,icon:null_mut(),cursor:null_mut(),background:null_mut(),menu:null(),name:name.as_ptr()};
    assert_ne!(RegisterClassW(&class),0);
    let exe=std::env::current_exe().unwrap();let title=exe.file_stem().unwrap().to_string_lossy();
    let use_default=title.ends_with("_a");let d=i32::MIN;
    let hwnd=CreateWindowExW(0x08000000,name.as_ptr(),wide(&title).as_ptr(),0x10cf0000,if use_default{d}else{20},if use_default{d}else{20},if use_default{d}else{300},if use_default{d}else{160},null_mut(),null_mut(),instance,null_mut());
    assert!(!hwnd.is_null());let mut rect=Rect::default();assert_ne!(GetWindowRect(hwnd,&mut rect),0);
    std::fs::write(marker("initial"),format!("{} {} {} {}",rect.left,rect.top,rect.right,rect.bottom)).unwrap();
    std::fs::write(marker("shown"),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis().to_string()).unwrap();
    CREATED.store(true,Ordering::SeqCst);SetTimer(hwnd,1,6000,null());
    let mut msg:Msg=std::mem::zeroed();while GetMessageW(&mut msg,null_mut(),0,0)>0{TranslateMessage(&msg);DispatchMessageW(&msg);}
}}

//! Temporary success mark, exclusively managed on the tray window's thread.
use std::{ffi::c_void, ptr::null, sync::{Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
type Handle = *mut c_void;
pub const TIMER_ID: usize = 0x4d45;
const DISPLAY_MS:u32 = 1250;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
#[link(name="user32")]
unsafe extern "system" {
    fn SetTimer(hwnd: Handle, id: usize, interval: u32, callback: *const c_void) -> usize;
    fn KillTimer(hwnd: Handle, id: usize) -> i32;
    fn CreateIcon(instance: Handle, width: i32, height: i32, planes: u8, bits: u8, and_mask: *const u8, xor_mask: *const u8) -> Handle;
    fn DestroyIcon(icon: Handle) -> i32;
}
pub fn active() -> bool { ACTIVE.load(Ordering::Acquire) }
pub fn expired() -> bool { DEADLINE.lock().unwrap_or_else(|e|e.into_inner()).is_some_and(|t|Instant::now()>=t) }
pub unsafe fn start(hwnd: Handle) {
    ACTIVE.store(true, Ordering::Release);
    *DEADLINE.lock().unwrap_or_else(|e|e.into_inner())=Some(Instant::now()+Duration::from_millis(u64::from(DISPLAY_MS)));
    unsafe { draw(hwnd); if SetTimer(hwnd, TIMER_ID, DISPLAY_MS, null()) == 0 { stop(hwnd); } }
}
pub unsafe fn stop(hwnd: Handle) {
    ACTIVE.store(false, Ordering::Release);
    *DEADLINE.lock().unwrap_or_else(|e|e.into_inner())=None;
    unsafe { KillTimer(hwnd, TIMER_ID); let mut data=crate::tray_data(hwnd); data.flags=crate::NIF_ICON; crate::Shell_NotifyIconW(1, &mut data); }
}
// The 32-pixel frame is an uncompressed 32-bit DIB inside the application's ICO.
// Convert its bottom-up rows to CreateIcon's top-down rows before adding the badge.
fn original_pixels()->Option<[u8;4096]> {
    let ico=include_bytes!("../icon.ico");
    let count=u16::from_le_bytes([*ico.get(4)?,*ico.get(5)?]) as usize;
    for i in 0..count {
        let base=6+i*16; let entry=ico.get(base..base+16)?;
        if entry[0]!=32 || entry[1]!=32 {continue;}
        let offset=u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        let header=ico.get(offset..offset+40)?;
        if u32::from_le_bytes(header[0..4].try_into().ok()?)!=40 || u16::from_le_bytes(header[14..16].try_into().ok()?)!=32 || u32::from_le_bytes(header[16..20].try_into().ok()?)!=0 {return None;}
        let dib=ico.get(offset+40..offset+40+4096)?;let mut pixels=[0u8;4096];
        for y in 0..32 {pixels[y*128..(y+1)*128].copy_from_slice(&dib[(31-y)*128..(32-y)*128]);}
        return Some(pixels);
    }
    None
}
#[cfg(test)]
#[test] fn embedded_icon_has_expected_frame() { assert!(original_pixels().is_some()); assert_eq!(DISPLAY_MS,1250); }
// CreateIcon converts these straight BGRA values for Windows alpha blending.
fn fade_base(pixel:&mut [u8]) {
    let alpha=(u16::from(pixel[3])*70+50)/100;
    pixel[3]=alpha as u8;
}
/// Shared background for all tray status signals, in straight BGRA.
pub(crate) fn faded_icon_pixels()->Option<[u8;4096]> {
    let mut pixels=original_pixels()?;
    for pixel in pixels.chunks_exact_mut(4) {fade_base(pixel);}
    Some(pixels)
}
#[cfg(test)]
#[test] fn fades_base_to_seventy_percent_without_double_multiplying_color() {
    let mut white=[255,255,255,255];fade_base(&mut white);assert_eq!(white,[255,255,255,179]);
    let mut invisible=[50,100,200,0];fade_base(&mut invisible);assert_eq!(invisible,[50,100,200,0]);
    let original=original_pixels().unwrap();let faded=faded_icon_pixels().unwrap();
    for (before,after) in original.chunks_exact(4).zip(faded.chunks_exact(4)) {
        let mut expected=[before[0],before[1],before[2],before[3]];fade_base(&mut expected);assert_eq!(after,expected);
    }
}
fn success_pixels()->Option<[u8;4096]> {
    const N:usize=32;
    let Some(mut pixels)=faded_icon_pixels() else {return None};
    for y in 0..N { for x in 0..N {
        let at=(y*N+x)*4;
        let left=x>=8 && x<=15; let right=x>=14 && x<=24;
        let distance1=(y as i32-(x as i32+4)).abs();
        let distance2=(y as i32-(34-x as i32)).abs();
        if (left && distance1<=3) || (right && distance2<=3) {
            let inside=(left && distance1<=2) || (right && distance2<=2);
            // A dark border preserves contrast on the original white artwork.
            pixels[at..at+4].copy_from_slice(if inside {&[30,200,35,255]} else {&[5,60,5,255]});
        }
    }}
    Some(pixels)
}
#[cfg(test)]
#[test] fn success_keeps_white_icon_visible_behind_green_tick() {
    let base=faded_icon_pixels().unwrap();let marked=success_pixels().unwrap();
    let corner=(5*32+5)*4;assert_eq!(&marked[corner..corner+4],&base[corner..corner+4]);
    assert_eq!(marked[corner+3],179);
    let check=(14*32+10)*4;assert_eq!(&marked[check..check+4],&[30,200,35,255]);
}
#[cfg(test)]
#[test] fn windows_renders_the_white_background_and_green_tick() {
    #[repr(C)] struct Header {size:u32,width:i32,height:i32,planes:u16,bits:u16,compression:u32,image_size:u32,xppm:i32,yppm:i32,used:u32,important:u32}
    #[repr(C)] struct Info {header:Header,colors:[u32;1]}
    #[link(name="gdi32")] unsafe extern "system" {
        fn CreateCompatibleDC(dc:Handle)->Handle; fn DeleteDC(dc:Handle)->i32;
        fn CreateDIBSection(dc:Handle,info:*const Info,usage:u32,bits:*mut *mut c_void,section:Handle,offset:u32)->Handle;
        fn SelectObject(dc:Handle,object:Handle)->Handle;fn DeleteObject(object:Handle)->i32;
    }
    #[link(name="user32")] unsafe extern "system" {
        fn DrawIconEx(dc:Handle,x:i32,y:i32,icon:Handle,width:i32,height:i32,step:u32,brush:Handle,flags:u32)->i32;
    }
    unsafe {
        let pixels=success_pixels().unwrap();let mut mask=[0xffu8;128];
        for y in 0..32 {for x in 0..32 {if pixels[(y*32+x)*4+3]!=0 {mask[y*4+x/8]&=!(0x80>>(x%8));}}}
        let icon=CreateIcon(std::ptr::null_mut(),32,32,1,32,mask.as_ptr(),pixels.as_ptr());assert!(!icon.is_null());
        let dc=CreateCompatibleDC(std::ptr::null_mut());assert!(!dc.is_null());
        let info=Info{header:Header{size:40,width:32,height:-32,planes:1,bits:32,compression:0,image_size:0,xppm:0,yppm:0,used:0,important:0},colors:[0]};
        let mut bits=std::ptr::null_mut();let bitmap=CreateDIBSection(dc,&info,0,&mut bits,std::ptr::null_mut(),0);assert!(!bitmap.is_null()&&!bits.is_null());
        let old=SelectObject(dc,bitmap);let frame=std::slice::from_raw_parts_mut(bits as *mut u8,4096);
        frame.fill(0);assert_ne!(DrawIconEx(dc,0,0,icon,32,32,0,std::ptr::null_mut(),3),0);
        let white=(5*32+5)*4;let tick=(14*32+10)*4;
        assert!(frame[white]>150 && frame[white+1]>150 && frame[white+2]>150,"white background should remain visible on dark tray: {:?}",&frame[white..white+4]);
        assert!(frame[tick+1]>frame[tick+2]+80,"check should remain green");
        frame.fill(255);assert_ne!(DrawIconEx(dc,0,0,icon,32,32,0,std::ptr::null_mut(),3),0);
        assert!(frame[white]>220 && frame[white+1]>220 && frame[white+2]>220,"white background should remain visible on light tray");
        SelectObject(dc,old);DeleteObject(bitmap);DeleteDC(dc);DestroyIcon(icon);
    }
}
pub unsafe fn draw(hwnd: Handle) {
    const N:usize=32; let Some(pixels)=success_pixels() else {return};
    let mut mask=[0xffu8; N*4];
    for y in 0..N { for x in 0..N {
        let at=(y*N+x)*4;
        if pixels[at+3]!=0 {mask[y*4+x/8]&=!(0x80>>(x%8));}
    }}
    unsafe {
        let icon=CreateIcon(std::ptr::null_mut(),N as i32,N as i32,1,32,mask.as_ptr(),pixels.as_ptr());
        if !icon.is_null() { let mut data=crate::tray_data(hwnd); data.flags=crate::NIF_ICON; data.icon=icon; crate::Shell_NotifyIconW(1,&mut data); DestroyIcon(icon); }
    }
}

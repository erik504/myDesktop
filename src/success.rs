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
// DrawIconEx uses premultiplied alpha for 32-bit icons.
fn fade_base(pixel:&mut [u8]) {
    let alpha=(u16::from(pixel[3])*40+50)/100;
    for channel in &mut pixel[..3] { *channel=((u16::from(*channel)*alpha+127)/255) as u8; }
    pixel[3]=alpha as u8;
}
/// Shared background for all tray status signals, in premultiplied BGRA.
pub(crate) fn faded_icon_pixels()->Option<[u8;4096]> {
    let mut pixels=original_pixels()?;
    for pixel in pixels.chunks_exact_mut(4) {fade_base(pixel);}
    Some(pixels)
}
#[cfg(test)]
#[test] fn fades_base_to_forty_percent_with_premultiplied_color() {
    let mut white=[255,255,255,255];fade_base(&mut white);assert_eq!(white,[102,102,102,102]);
    let mut invisible=[50,100,200,0];fade_base(&mut invisible);assert_eq!(invisible,[0,0,0,0]);
    let original=original_pixels().unwrap();let faded=faded_icon_pixels().unwrap();
    for (before,after) in original.chunks_exact(4).zip(faded.chunks_exact(4)) {
        let mut expected=[before[0],before[1],before[2],before[3]];fade_base(&mut expected);assert_eq!(after,expected);
    }
}
pub unsafe fn draw(hwnd: Handle) {
    const N: usize=32;
    let mut mask=[0xffu8; N*4];
    let Some(mut pixels)=faded_icon_pixels() else {return};
    for y in 0..N { for x in 0..N {
        let dx=x as i32-16; let dy=y as i32-16;
        let at=(y*N+x)*4;
        if dx*dx+dy*dy<=11*11 {
            let white=(x>=8 && x<=15 && (y as i32-(x as i32+4)).abs()<=2)
                || (x>=14 && x<=24 && (y as i32-(34-x as i32)).abs()<=2);
            pixels[at..at+4].copy_from_slice(if white {&[255,255,255,255]} else {&[35,135,25,255]});
        }
        if pixels[at+3]!=0 {mask[y*4+x/8]&=!(0x80>>(x%8));}
    }}
    unsafe {
        let icon=CreateIcon(std::ptr::null_mut(),N as i32,N as i32,1,32,mask.as_ptr(),pixels.as_ptr());
        if !icon.is_null() { let mut data=crate::tray_data(hwnd); data.flags=crate::NIF_ICON; data.icon=icon; crate::Shell_NotifyIconW(1,&mut data); DestroyIcon(icon); }
    }
}

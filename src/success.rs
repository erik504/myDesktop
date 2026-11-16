//! Temporary success mark, exclusively managed on the tray window's thread.
use std::{ffi::c_void, ptr::null, sync::{Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
type Handle = *mut c_void;
pub const TIMER_ID: usize = 0x4d45;
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
    *DEADLINE.lock().unwrap_or_else(|e|e.into_inner())=Some(Instant::now()+Duration::from_millis(2500));
    unsafe { draw(hwnd); if SetTimer(hwnd, TIMER_ID, 2500, null()) == 0 { stop(hwnd); } }
}
pub unsafe fn stop(hwnd: Handle) {
    ACTIVE.store(false, Ordering::Release);
    *DEADLINE.lock().unwrap_or_else(|e|e.into_inner())=None;
    unsafe { KillTimer(hwnd, TIMER_ID); let mut data=crate::tray_data(hwnd); data.flags=crate::NIF_ICON; crate::Shell_NotifyIconW(1, &mut data); }
}
pub unsafe fn draw(hwnd: Handle) {
    const N: usize=32;
    let mut mask=[0xffu8; N*4];
    let mut pixels=[0u8;N*N*4];
    for y in 0..N { for x in 0..N {
        let dx=x as i32-16; let dy=y as i32-16;
        if dx*dx+dy*dy>14*14 { continue }
        // Two strokes of a tick, on a green disc visible against either tray theme.
        let white=(x>=7 && x<=14 && (y as i32-(x as i32+7)).abs()<=2)
            || (x>=12 && x<=25 && (y as i32-(33-x as i32)).abs()<=2);
        let color=if white {[255,255,255,255]} else {[35,135,25,255]};
        let row=N-1-y; let at=(row*N+x)*4;
        pixels[at..at+4].copy_from_slice(&color); mask[row*4+x/8]&=!(0x80>>(x%8));
    }}
    unsafe {
        let icon=CreateIcon(std::ptr::null_mut(),N as i32,N as i32,1,32,mask.as_ptr(),pixels.as_ptr());
        if !icon.is_null() { let mut data=crate::tray_data(hwnd); data.flags=crate::NIF_ICON; data.icon=icon; crate::Shell_NotifyIconW(1,&mut data); DestroyIcon(icon); }
    }
}

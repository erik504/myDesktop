//! Animated tray icon while a background job is running. Owned by the UI thread.
use std::ffi::c_void;
use std::ptr::null;
use std::sync::atomic::{AtomicUsize, Ordering};

type Handle = *mut c_void;
pub const TIMER_ID: usize = 0x4d44;
const NIM_MODIFY: u32 = 1;
const NIF_ICON: u32 = 2;
static FRAME: AtomicUsize = AtomicUsize::new(0);

#[link(name = "user32")]
unsafe extern "system" {
    fn SetTimer(hwnd: Handle, id: usize, interval_ms: u32, callback: *const c_void) -> usize;
    fn KillTimer(hwnd: Handle, id: usize) -> i32;
    fn CreateIcon(instance: Handle, width: i32, height: i32, planes: u8, bits: u8, and_mask: *const u8, xor_mask: *const u8) -> Handle;
    fn DestroyIcon(icon: Handle) -> i32;
}

/// Show the first frame immediately, then animate on the tray window's UI thread.
pub unsafe fn start(hwnd: Handle) {
    FRAME.store(0, Ordering::Relaxed);
    unsafe { tick(hwnd); SetTimer(hwnd, TIMER_ID, 110, null()); }
}

/// Stop the animation and restore the normal application icon.
pub unsafe fn stop(hwnd: Handle) {
    unsafe {
        KillTimer(hwnd, TIMER_ID);
        let mut data = crate::tray_data(hwnd);
        data.flags = NIF_ICON;
        crate::Shell_NotifyIconW(NIM_MODIFY, &mut data);
    }
}

/// Generate one high-contrast rotating ring, without retaining GDI handles.
pub unsafe fn tick(hwnd: Handle) {
    const N: usize = 32;
    const POINTS: [(i32, i32); 8] = [(16, 4), (24, 8), (28, 16), (24, 24), (16, 28), (8, 24), (4, 16), (8, 8)];
    // CreateIcon expects an AND mask and bottom-up BGRA color pixels.
    let mut and_mask = [0xffu8; N * 4];
    let mut pixels = [0u8; N * N * 4];
    let phase = FRAME.fetch_add(1, Ordering::Relaxed) % POINTS.len();
    for (i, &(cx, cy)) in POINTS.iter().enumerate() {
        let strength = ((i + POINTS.len() - phase) % POINTS.len()) as u8;
        let color = 40 + strength * 25;
        for y in (cy - 2)..=(cy + 2) {
            for x in (cx - 2)..=(cx + 2) {
                if (x - cx).pow(2) + (y - cy).pow(2) > 5 { continue; }
                let row = N - 1 - y as usize;
                let col = x as usize;
                let at = (row * N + col) * 4;
                pixels[at..at + 4].copy_from_slice(&[color, color, color, 255]);
                and_mask[row * 4 + col / 8] &= !(0x80 >> (col % 8));
            }
        }
    }
    unsafe {
        let icon = CreateIcon(std::ptr::null_mut(), N as i32, N as i32, 1, 32, and_mask.as_ptr(), pixels.as_ptr());
        if !icon.is_null() {
            let mut data = crate::tray_data(hwnd);
            data.flags = NIF_ICON;
            data.icon = icon;
            crate::Shell_NotifyIconW(NIM_MODIFY, &mut data);
            DestroyIcon(icon);
        }
    }
}

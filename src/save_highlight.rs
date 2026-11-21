//! A click-through screen overlay; the target windows are only queried.
use std::{
    cell::RefCell,
    ffi::c_void,
    ptr::{null, null_mut},
    sync::Mutex,
    time::{Duration, Instant},
};
type Handle = *mut c_void;
pub const SHOW: u32 = crate::WM_APP + 4;
pub const TIMER: usize = 0x4d49;
const FLASH_DURATION: Duration = Duration::from_millis(100);
const YELLOW: u32 = 0x0000ffff; // Win32 COLORREF: red + green.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
static PENDING: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
thread_local! { static OVERLAY:RefCell<Option<Overlay>>=const { RefCell::new(None) }; }
#[link(name = "user32")]
unsafe extern "system" {
    fn IsWindowVisible(hwnd: isize) -> i32;
    fn IsIconic(hwnd: isize) -> i32;
    fn GetWindowRect(hwnd: isize, rect: *mut c_void) -> i32;
    fn SetLayeredWindowAttributes(hwnd: isize, key: u32, alpha: u8, flags: u32) -> i32;
    fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn UpdateWindow(hwnd: Handle) -> i32;
    fn UnregisterClassW(name: *const u16, instance: Handle) -> i32;
    fn SetTimer(hwnd: Handle, id: usize, ms: u32, callback: *const c_void) -> usize;
    fn KillTimer(hwnd: Handle, id: usize) -> i32;
}
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(hwnd: isize, attr: u32, value: *mut u32, size: u32) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> Handle;
    fn DeleteObject(object: Handle) -> i32;
}

unsafe extern "system" fn overlay_proc(hwnd: Handle, msg: u32, w: usize, l: isize) -> isize {
    unsafe {
        match msg {
            0x84 => -1,
            0x21 => 3,
            _ => crate::DefWindowProcW(hwnd, msg, w, l),
        } // click-through, no activation
    }
}

struct Border([Handle; 4]);
impl Drop for Border {
    fn drop(&mut self) {
        for &hwnd in &self.0 {
            if !hwnd.is_null() {
                unsafe {
                    crate::DestroyWindow(hwnd);
                }
            }
        }
    }
}

struct Flash {
    _border: Border,
    expires: Instant,
}
struct Overlay {
    name: Vec<u16>,
    instance: Handle,
    flashes: Vec<Flash>,
}
impl Overlay {
    fn new() -> Option<Self> {
        unsafe {
            let name = crate::wide("myDesktopSaveHighlight");
            let instance = crate::GetModuleHandleW(null());
            let brush = CreateSolidBrush(YELLOW);
            if brush.is_null() {
                return None;
            }
            let class = crate::WndClassW {
                style: 0,
                wnd_proc: Some(overlay_proc),
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: null_mut(),
                cursor: null_mut(),
                background: brush,
                menu_name: null(),
                class_name: name.as_ptr(),
            };
            if crate::RegisterClassW(&class) == 0 {
                DeleteObject(brush);
                return None;
            }
            // Windows owns the class background brush after registration.
            Some(Self {
                name,
                instance,
                flashes: Vec::new(),
            })
        }
    }
    fn show(&mut self, rect: Rect) {
        unsafe {
            let Some(parts) = strips(rect) else { return };
            let mut border = Border([null_mut(); 4]);
            for slot in &mut border.0 {
                // TOPMOST | TRANSPARENT | TOOLWINDOW | LAYERED | NOACTIVATE.
                let hwnd = crate::CreateWindowExW(
                    0x080800a8,
                    self.name.as_ptr(),
                    null(),
                    0x80000000,
                    0,
                    0,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    self.instance,
                    null_mut(),
                );
                if hwnd.is_null() {
                    return;
                }
                *slot = hwnd;
                if SetLayeredWindowAttributes(hwnd as isize, 0, 255, 2) == 0 {
                    return;
                }
            }
            for (&h, (x, y, w, height)) in border.0.iter().zip(parts) {
                if SetWindowPos(h as isize, -1, x, y, w, height, 0x10 | 0x40 | 0x200) == 0 {
                    return;
                }
                UpdateWindow(h);
            }
            self.flashes.push(Flash {
                _border: border,
                expires: Instant::now() + FLASH_DURATION,
            });
        }
    }
    fn expire(&mut self, now: Instant) {
        self.flashes.retain(|flash| flash.expires > now);
    }
}
impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            self.flashes.clear();
            UnregisterClassW(self.name.as_ptr(), self.instance);
        }
    }
}

fn visible_frame(hwnd: isize) -> Option<Rect> {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return None;
        }
        let mut rect = Rect::default();
        // Use the visible frame to avoid the invisible resize margins.
        let ptr = (&mut rect as *mut Rect).cast();
        if DwmGetWindowAttribute(hwnd, 9, ptr, std::mem::size_of::<Rect>() as u32) != 0
            && GetWindowRect(hwnd, ptr.cast()) == 0
        {
            return None;
        }
        Some(rect)
    }
}

/// Snapshot the screen coordinates on the capture thread and wake the tray.
pub fn enqueue(owner: isize, hwnd: isize) {
    let Some(rect) = visible_frame(hwnd) else {
        return;
    };
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let should_notify = pending.is_empty();
    pending.push(rect);
    if should_notify && unsafe { crate::PostMessageW(owner as Handle, SHOW, 0, 0) } == 0 {
        pending.clear();
    }
}

pub fn show_pending(owner: Handle) {
    let pending = std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()));
    if pending.is_empty() {
        return;
    }
    OVERLAY.with_borrow_mut(|state| {
        if state.is_none() {
            let Some(overlay) = Overlay::new() else {
                return;
            };
            if unsafe { SetTimer(owner, TIMER, 10, null()) } == 0 {
                return;
            }
            *state = Some(overlay);
        }
        if let Some(overlay) = state {
            for rect in pending {
                overlay.expire(Instant::now());
                overlay.show(rect);
            }
        }
    });
}

pub fn tick(owner: Handle) {
    OVERLAY.with_borrow_mut(|state| {
        if let Some(overlay) = state {
            overlay.expire(Instant::now());
            if !overlay.flashes.is_empty() {
                return;
            }
        }
        unsafe {
            KillTimer(owner, TIMER);
        }
        *state = None;
    });
}

pub fn destroy(owner: Handle) {
    unsafe {
        KillTimer(owner, TIMER);
    }
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).clear();
    OVERLAY.with_borrow_mut(|state| *state = None);
}

fn strips(r: Rect) -> Option<[(i32, i32, i32, i32); 4]> {
    let width = r.right.checked_sub(r.left)?;
    let height = r.bottom.checked_sub(r.top)?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let t = 4.min(width).min(height);
    Some([
        (r.left, r.top, width, t),
        (r.left, r.bottom - t, width, t),
        (r.left, r.top, t, height),
        (r.right - t, r.top, t, height),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn border_covers_edges_on_negative_monitors() {
        assert_eq!(
            strips(Rect {
                left: -400,
                top: -50,
                right: -100,
                bottom: 100
            }),
            Some([
                (-400, -50, 300, 4),
                (-400, 96, 300, 4),
                (-400, -50, 4, 150),
                (-104, -50, 4, 150)
            ])
        );
        assert!(strips(Rect::default()).is_none());
    }
    // An isolated, non-activating test window is shown only for this opt-in test.
    #[test]
    #[ignore]
    fn native_overlays_overlap_and_expire_independently() {
        unsafe {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn SetErrorMode(mode: u32) -> u32;
            }
            #[link(name = "user32")]
            unsafe extern "system" {
                fn GetSysColorBrush(index: i32) -> Handle;
                fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
                fn GetDC(hwnd: Handle) -> Handle;
                fn ReleaseDC(hwnd: Handle, dc: Handle) -> i32;
                fn PeekMessageW(
                    msg: *mut crate::Msg,
                    hwnd: Handle,
                    min: u32,
                    max: u32,
                    remove: u32,
                ) -> i32;
            }
            #[link(name = "gdi32")]
            unsafe extern "system" {
                fn GetPixel(dc: Handle, x: i32, y: i32) -> u32;
            }
            struct Target(Handle);
            impl Drop for Target {
                fn drop(&mut self) {
                    unsafe {
                        crate::DestroyWindow(self.0);
                    }
                }
            }
            SetErrorMode(2);
            let instance = crate::GetModuleHandleW(null());
            let name = crate::wide("myDesktopSaveHighlightTest");
            let class = crate::WndClassW {
                style: 0,
                wnd_proc: Some(crate::DefWindowProcW),
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: null_mut(),
                cursor: null_mut(),
                background: GetSysColorBrush(5),
                menu_name: null(),
                class_name: name.as_ptr(),
            };
            assert_ne!(crate::RegisterClassW(&class), 0);
            let target = Target(crate::CreateWindowExW(
                0x08000080,
                name.as_ptr(),
                crate::wide("Test myDesktop").as_ptr(),
                0x80000000,
                60,
                60,
                220,
                140,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            ));
            assert!(!target.0.is_null());
            assert_ne!(
                SetWindowPos(target.0 as isize, -1, 60, 60, 220, 140, 0x10 | 0x40),
                0
            );
            let mut before = Rect::default();
            assert_ne!(
                GetWindowRect(target.0 as isize, (&mut before as *mut Rect).cast()),
                0
            );
            let visible = visible_frame(target.0 as isize).unwrap();
            let second = Rect {
                left: visible.left + 40,
                top: visible.top + 40,
                right: visible.right + 40,
                bottom: visible.bottom + 40,
            };
            let mut overlay = Overlay::new().unwrap();
            overlay.show(visible);
            overlay.show(second);
            assert_eq!(overlay.flashes.len(), 2);
            let first = overlay.flashes[0]._border.0;
            let next = overlay.flashes[1]._border.0;
            for &h in first.iter().chain(&next) {
                assert_ne!(IsWindowVisible(h as isize), 0);
                let flags = GetWindowLongW(h as isize, -20) as u32;
                assert_eq!(flags & 0x080800a8, 0x080800a8);
            }
            let dc = GetDC(first[0]);
            assert!(!dc.is_null());
            assert_eq!(GetPixel(dc, 1, 1), YELLOW);
            ReleaseDC(first[0], dc);
            assert_eq!(
                crate::picker::test_root_at(visible.left + 20, visible.top + 1),
                target.0 as isize,
                "Il bordo intercetta il mouse"
            );
            overlay.expire(overlay.flashes[0].expires);
            assert_eq!(overlay.flashes.len(), 1);
            for h in first {
                assert_eq!(IsWindowVisible(h as isize), 0);
            }
            for h in next {
                assert_ne!(IsWindowVisible(h as isize), 0);
            }
            overlay.expire(overlay.flashes[0].expires);
            assert!(overlay.flashes.is_empty());
            for h in next {
                assert_eq!(IsWindowVisible(h as isize), 0);
            }
            drop(overlay);

            enqueue(target.0 as isize, target.0 as isize);
            let mut message: crate::Msg = std::mem::zeroed();
            assert_ne!(PeekMessageW(&mut message, target.0, SHOW, SHOW, 1), 0);
            let started = Instant::now();
            crate::wnd_proc(target.0, message.message, message.wparam, message.lparam);
            assert!(
                OVERLAY.with_borrow(|state| state.as_ref().is_some_and(|o| o.flashes.len() == 1))
            );
            while started.elapsed() < Duration::from_secs(2) {
                if PeekMessageW(&mut message, target.0, crate::WM_TIMER, crate::WM_TIMER, 1) != 0 {
                    crate::wnd_proc(target.0, message.message, message.wparam, message.lparam);
                }
                if OVERLAY.with_borrow(|state| state.is_none()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                OVERLAY.with_borrow(|state| state.is_none()),
                "Il timer non ha rimosso il bordo"
            );
            assert!(started.elapsed() >= FLASH_DURATION);
            let mut after = Rect::default();
            assert_ne!(
                GetWindowRect(target.0 as isize, (&mut after as *mut Rect).cast()),
                0
            );
            assert_eq!(after, before);
        }
    }
}

//! The error badge expires while the tooltip remains available until read.
use std::{
    ffi::c_void,
    sync::Mutex,
    time::{Duration, Instant},
};
type Handle = *mut c_void;
pub const TIMER: usize = 0x4d46;
const BADGE_DURATION: Duration = Duration::from_secs(10);
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Identifier {
    size: u32,
    hwnd: Handle,
    id: u32,
    guid: crate::Guid,
}
struct State {
    pages: Vec<String>,
    page: usize,
    hover: Option<Instant>,
    seen: bool,
    badge_until: Option<Instant>,
}
impl State {
    fn new(text: &str, now: Instant) -> Self {
        Self {
            pages: pages(text),
            page: 0,
            hover: None,
            seen: false,
            badge_until: Some(now + BADGE_DURATION),
        }
    }

    fn badge_visible(&self, now: Instant) -> bool {
        self.badge_until.is_some_and(|until| now < until)
    }

    fn expire_badge(&mut self, now: Instant) -> bool {
        if self.badge_until.is_some_and(|until| now >= until) {
            self.badge_until = None;
            return true;
        }
        false
    }
}
static ERROR: Mutex<Option<State>> = Mutex::new(None);
#[link(name = "user32")]
unsafe extern "system" {
    fn SetTimer(hwnd: Handle, id: usize, ms: u32, callback: *const c_void) -> usize;
    fn KillTimer(hwnd: Handle, id: usize) -> i32;
    fn GetCursorPos(p: *mut Point) -> i32;
    fn CreateIcon(
        instance: Handle,
        width: i32,
        height: i32,
        planes: u8,
        bits: u8,
        and_mask: *const u8,
        xor_mask: *const u8,
    ) -> Handle;
    fn DestroyIcon(icon: Handle) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn Shell_NotifyIconGetRect(id: *const Identifier, rect: *mut Rect) -> i32;
}
pub fn badge_active() -> bool {
    ERROR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|state| state.badge_visible(Instant::now()))
}
fn pages(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut part = String::new();
    let mut units = 0;
    for ch in text.replace(['\r', '\n'], " ").chars() {
        let n = ch.len_utf16();
        if units + n > 105 {
            result.push(std::mem::take(&mut part));
            units = 0;
        }
        part.push(ch);
        units += n;
    }
    if !part.is_empty() || result.is_empty() {
        result.push(part);
    }
    result
}
#[cfg(test)]
#[test]
fn long_errors_fit_tooltip_and_keep_all_text() {
    let input = "finestra 🪟 ".repeat(40);
    let p = pages(&input);
    assert!(p.len() > 1);
    assert_eq!(p.concat(), input);
    assert!(p.iter().all(|s| s.encode_utf16().count() <= 105));
}
#[cfg(test)]
#[test]
fn error_badge_expires_without_discarding_the_tooltip() {
    let reported = Instant::now();
    let mut state = State::new("Finestra non disponibile", reported);
    assert!(!state.expire_badge(reported + Duration::from_millis(9_999)));
    assert!(state.badge_visible(reported + Duration::from_millis(9_999)));
    let deadline = reported + Duration::from_secs(10);
    assert!(state.expire_badge(deadline));
    assert!(!state.badge_visible(deadline));
    assert!(!state.expire_badge(deadline));
    assert_eq!(state.pages, ["Finestra non disponibile"]);
    assert!(!state.seen);
}
pub unsafe fn report(hwnd: Handle, text: &str) {
    *ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(State::new(text, Instant::now()));
    unsafe {
        draw(hwnd);
        SetTimer(hwnd, TIMER, 150, std::ptr::null());
    }
}
pub unsafe fn draw(hwnd: Handle) {
    let lock = ERROR.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = lock.as_ref() else { return };
    let mut d = unsafe { crate::tray_data(hwnd) };
    d.flags = crate::NIF_TIP;
    d.tip.fill(0);
    let message = if state.pages.len() > 1 {
        format!(
            "Errore ({}/{}): {}",
            state.page + 1,
            state.pages.len(),
            state.pages[state.page]
        )
    } else {
        format!("Errore: {}", state.pages[0])
    };
    let mut chars = message.encode_utf16();
    for dst in d.tip.iter_mut().take(127) {
        if let Some(ch) = chars.next() {
            *dst = ch
        } else {
            break;
        }
    }
    if !state.badge_visible(Instant::now())
        || crate::BUSY.load(std::sync::atomic::Ordering::Acquire)
    {
        unsafe {
            crate::Shell_NotifyIconW(1, &mut d);
        }
        return;
    }
    d.flags |= crate::NIF_ICON;
    let mut mask = [0xffu8; 128];
    let Some(mut pixels) = crate::success::faded_icon_pixels() else {
        return;
    };
    for y in 0..32usize {
        for x in 0..32usize {
            let dx = x as i32 - 16;
            let dy = y as i32 - 16;
            let i = (y * 32 + x) * 4;
            if dx * dx + dy * dy <= 11 * 11 {
                let white = (dx - dy).abs() <= 2 || (dx + dy).abs() <= 2;
                let color = if white {
                    [255, 255, 255, 255]
                } else {
                    [35, 35, 210, 255]
                };
                pixels[i..i + 4].copy_from_slice(&color);
            }
            if pixels[i + 3] != 0 {
                mask[y * 4 + x / 8] &= !(0x80 >> (x % 8));
            }
        }
    }
    unsafe {
        let icon = CreateIcon(
            std::ptr::null_mut(),
            32,
            32,
            1,
            32,
            mask.as_ptr(),
            pixels.as_ptr(),
        );
        if !icon.is_null() {
            d.icon = icon;
            crate::Shell_NotifyIconW(1, &mut d);
            DestroyIcon(icon);
        }
    }
}
/// Return true when the tray needs to replace the error icon.
pub unsafe fn tick(hwnd: Handle, busy: bool) -> bool {
    let mut state = ERROR.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = state.as_mut() else {
        unsafe {
            KillTimer(hwnd, TIMER);
        }
        return false;
    };
    let now = Instant::now();
    let mut hide_badge = e.expire_badge(now);
    if busy {
        return hide_badge;
    }
    let id = Identifier {
        size: std::mem::size_of::<Identifier>() as u32,
        hwnd,
        id: 1,
        guid: crate::Guid {
            d1: 0,
            d2: 0,
            d3: 0,
            d4: [0; 8],
        },
    };
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut p = Point { x: 0, y: 0 };
    let inside = unsafe { Shell_NotifyIconGetRect(&id, &mut r) == 0 && GetCursorPos(&mut p) != 0 }
        && p.x >= r.left
        && p.x < r.right
        && p.y >= r.top
        && p.y < r.bottom;
    if inside {
        let start = e.hover.get_or_insert(now);
        if !e.seen && now.duration_since(*start) >= Duration::from_millis(2500) {
            if e.page + 1 < e.pages.len() {
                e.page += 1;
                e.hover = Some(now);
            } else {
                e.seen = true;
                hide_badge |= e.badge_until.take().is_some();
            }
            drop(state);
            unsafe { draw(hwnd) };
        }
    } else if e.seen {
        *state = None;
        drop(state);
        unsafe {
            KillTimer(hwnd, TIMER);
            let mut d = crate::tray_data(hwnd);
            d.flags = crate::NIF_TIP;
            crate::Shell_NotifyIconW(1, &mut d);
        }
    } else {
        e.hover = None;
    }
    hide_badge
}

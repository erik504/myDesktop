//! Per-user startup setting. Does not modify settings until the user selects the menu item.
use std::{ffi::{c_void, OsStr}, os::windows::ffi::OsStrExt, ptr};
type Hkey = *mut c_void;
const CURRENT_USER: Hkey = 0x80000001u32 as i32 as isize as Hkey;
const KEY_QUERY_VALUE: u32 = 1;
const KEY_SET_VALUE: u32 = 2;
const REG_SZ: u32 = 1;
const NOT_FOUND: i32 = 2;
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "myDesktop";

#[link(name="advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(key: Hkey, sub: *const u16, options: u32, access: u32, result: *mut Hkey) -> i32;
    fn RegCreateKeyExW(key: Hkey, sub: *const u16, reserved: u32, class: *mut u16, options: u32, access: u32, security: *const c_void, result: *mut Hkey, disposition: *mut u32) -> i32;
    fn RegQueryValueExW(key: Hkey, value: *const u16, reserved: *mut u32, kind: *mut u32, data: *mut u8, size: *mut u32) -> i32;
    fn RegSetValueExW(key: Hkey, value: *const u16, reserved: u32, kind: u32, data: *const u8, size: u32) -> i32;
    fn RegDeleteValueW(key: Hkey, value: *const u16) -> i32;
    fn RegCloseKey(key: Hkey) -> i32;
}
struct Key(Hkey);
impl Drop for Key { fn drop(&mut self) { unsafe { RegCloseKey(self.0); } } }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn err(code: i32) -> String { format!("Impossibile modificare l'avvio automatico: {}", std::io::Error::from_raw_os_error(code)) }
fn command(path: &OsStr) -> Vec<u16> {
    let mut result = vec![b'"' as u16];
    result.extend(path.encode_wide());
    result.extend([b'"' as u16, 0]);
    result
}
fn current_command() -> Result<Vec<u16>, String> {
    Ok(command(std::env::current_exe().map_err(|e|format!("Impossibile trovare l'eseguibile: {e}"))?.as_os_str()))
}
fn open(access: u32, create: bool) -> Result<Option<Key>, String> {
    let mut handle = ptr::null_mut();
    let code = unsafe {
        if create { RegCreateKeyExW(CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, ptr::null_mut(), 0, access, ptr::null(), &mut handle, ptr::null_mut()) }
        else { RegOpenKeyExW(CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut handle) }
    };
    if code == NOT_FOUND { Ok(None) } else if code == 0 { Ok(Some(Key(handle))) } else { Err(err(code)) }
}
fn matching(a: &[u16], b: &[u16]) -> bool {
    let a = String::from_utf16_lossy(a).trim_end_matches('\0').trim().to_lowercase();
    let b = String::from_utf16_lossy(b).trim_end_matches('\0').trim().to_lowercase();
    a == b
}
pub fn enabled() -> Result<bool, String> {
    let Some(key) = open(KEY_QUERY_VALUE, false)? else { return Ok(false) };
    let mut kind = 0; let mut size = 0;
    let code = unsafe { RegQueryValueExW(key.0, wide(VALUE).as_ptr(), ptr::null_mut(), &mut kind, ptr::null_mut(), &mut size) };
    if code == NOT_FOUND { return Ok(false) }
    if code != 0 { return Err(err(code)); }
    if kind != REG_SZ || size % 2 != 0 || size > 65536 { return Ok(false) }
    let mut data = vec![0u16; (size as usize + 1) / 2];
    let code = unsafe { RegQueryValueExW(key.0, wide(VALUE).as_ptr(), ptr::null_mut(), &mut kind, data.as_mut_ptr().cast(), &mut size) };
    if code != 0 { return Err(err(code)); }
    Ok(kind == REG_SZ && matching(&data, &current_command()?))
}
pub fn set(value: bool) -> Result<(), String> {
    let Some(key) = open(KEY_SET_VALUE, value)? else { return Ok(()) };
    let code = if value {
        let command = current_command()?;
        unsafe { RegSetValueExW(key.0, wide(VALUE).as_ptr(), 0, REG_SZ, command.as_ptr().cast(), (command.len() * 2) as u32) }
    } else { unsafe { RegDeleteValueW(key.0, wide(VALUE).as_ptr()) } };
    if code == 0 || (!value && code == NOT_FOUND) { Ok(()) } else { Err(err(code)) }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn executable_path_is_quoted() {
        assert_eq!(String::from_utf16(&command(OsStr::new(r"C:\Program Files\myDesktop.exe"))).unwrap(), "\"C:\\Program Files\\myDesktop.exe\"\0");
    }
    #[test] fn compares_registry_values_without_null_or_case_differences() {
        assert!(matching(&command(OsStr::new(r"C:\AI\myDesktop.exe")), &command(OsStr::new(r"c:\ai\MYDESKTOP.exe"))));
        assert!(!matching(&command(OsStr::new(r"C:\Other\myDesktop.exe")), &command(OsStr::new(r"C:\AI\myDesktop.exe"))));
    }
}

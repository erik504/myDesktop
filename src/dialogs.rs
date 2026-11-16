//! Native Windows file dialogs for named layouts.
use std::{ffi::{c_void, OsString}, mem::size_of, os::windows::ffi::OsStringExt, path::PathBuf, ptr::{null, null_mut}};
type Handle = *mut c_void;
#[repr(C)]
struct OpenFileName {
    size: u32, owner: Handle, instance: Handle, filter: *const u16,
    custom_filter: *mut u16, max_custom_filter: u32, filter_index: u32,
    file: *mut u16, max_file: u32, file_title: *mut u16, max_file_title: u32,
    initial_dir: *const u16, title: *const u16, flags: u32,
    file_offset: u16, file_extension: u16, default_extension: *const u16,
    custom_data: isize, hook: *mut c_void, template_name: *const u16,
    reserved: *mut c_void, reserved_u32: u32, flags_ex: u32,
}
#[link(name="comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileName) -> i32;
    fn GetSaveFileNameW(ofn: *mut OpenFileName) -> i32;
    fn CommDlgExtendedError() -> u32;
}
fn w(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
pub fn choose(owner: Handle, save: bool) -> Result<Option<PathBuf>, String> {
    let mut file = vec![0u16; 32768];
    let filter: Vec<u16> = "Configurazioni myDesktop (*.json)\0*.json\0Tutti i file (*.*)\0*.*\0\0".encode_utf16().collect();
    let title = w(if save { "Salva configurazione con nome" } else { "Apri configurazione" });
    let extension = w("json");
    // The common dialog owns no pointers beyond this synchronous call.
    let mut ofn = OpenFileName { size: size_of::<OpenFileName>() as u32, owner, instance: null_mut(), filter: filter.as_ptr(), custom_filter: null_mut(), max_custom_filter: 0, filter_index: 1, file: file.as_mut_ptr(), max_file: file.len() as u32, file_title: null_mut(), max_file_title: 0, initial_dir: null(), title: title.as_ptr(), flags: 0x00080000 | 0x00000800 | 0x00000008 | if save { 0x00000002 } else { 0x00001000 }, file_offset: 0, file_extension: 0, default_extension: extension.as_ptr(), custom_data: 0, hook: null_mut(), template_name: null(), reserved: null_mut(), reserved_u32: 0, flags_ex: 0 };
    let ok = unsafe { if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) } };
    if ok == 0 { let code=unsafe { CommDlgExtendedError() }; return if code==0 { Ok(None) } else { Err(format!("Impossibile aprire la finestra di scelta file (codice {code:#x}).")) }; }
    let n = file.iter().position(|&v| v == 0).ok_or("Percorso non valido")?;
    if n == 0 { return Err("Percorso non valido".into()); }
    Ok(Some(PathBuf::from(OsString::from_wide(&file[..n]))))
}

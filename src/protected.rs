//! Conservative, handle-based checks for the service installation path.
//! Any unrecognized ACL form is rejected. Handles are kept open without
//! FILE_SHARE_DELETE for the duration of installation to prevent replacement.
use std::{ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, path::{Component, Path, PathBuf, Prefix}, ptr::{null, null_mut}};
type Handle=*mut c_void;
#[repr(C)] struct AttributeTag { attributes:u32, tag:u32 }
#[repr(C)] struct AclSize { count:u32, bytes_in_use:u32, bytes_free:u32 }
#[link(name="kernel32")] unsafe extern "system" {
    fn CreateFileW(path:*const u16,access:u32,share:u32,attrs:*const c_void,creation:u32,flags:u32,template:Handle)->Handle;
    fn GetFileInformationByHandleEx(handle:Handle,class:u32,info:*mut c_void,size:u32)->i32;
    fn CloseHandle(handle:Handle)->i32;
    fn LocalFree(ptr:*mut c_void)->*mut c_void;
}
#[link(name="advapi32")] unsafe extern "system" {
    fn GetSecurityInfo(handle:Handle,kind:u32,info:u32,owner:*mut *mut c_void,group:*mut *mut c_void,dacl:*mut *mut c_void,sacl:*mut *mut c_void,descriptor:*mut *mut c_void)->u32;
    fn GetSecurityDescriptorControl(descriptor:*mut c_void,control:*mut u16,revision:*mut u32)->i32;
    fn GetAclInformation(acl:*mut c_void,info:*mut c_void,len:u32,class:u32)->i32;
    fn GetAce(acl:*mut c_void,index:u32,ace:*mut *mut c_void)->i32;
    fn ConvertSidToStringSidW(sid:*mut c_void,text:*mut *mut u16)->i32;
}
struct HandleGuard(Handle);
impl Drop for HandleGuard { fn drop(&mut self){unsafe{CloseHandle(self.0);}} }
struct Descriptor(*mut c_void);
impl Drop for Descriptor {fn drop(&mut self){unsafe{LocalFree(self.0);}}}
pub struct Guard { _handles:Vec<HandleGuard> }
fn wide(s:&std::ffi::OsStr)->Vec<u16>{s.encode_wide().chain(Some(0)).collect()}
fn error(message:&str)->String{format!("{message}: {}",std::io::Error::last_os_error())}
fn trusted(sid:&str)->bool { matches!(sid,"S-1-5-18"|"S-1-5-32-544"|"S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464") }
fn unsafe_allow(sid:&str,mask:u32,base:bool,strict:bool)->bool {
    let mut dangerous=0x10000|0x40000|0x80000|0x40|0x10|0x100|0x10000000|0x40000000;
    if base||strict {dangerous|=0x2|0x4;}
    !trusted(sid) && mask&dangerous!=0
}
unsafe fn sid_text(sid:*mut c_void)->Result<String,String>{unsafe{
    if sid.is_null(){return Err("SID assente".into())} let mut ptr=null_mut();if ConvertSidToStringSidW(sid,&mut ptr)==0{return Err(error("SID non valido"))}
    let mut n=0;while *ptr.add(n)!=0 {n+=1} let text=String::from_utf16_lossy(std::slice::from_raw_parts(ptr,n));LocalFree(ptr.cast());Ok(text)
}}
fn open_checked(path:&Path,base:bool,strict:bool)->Result<HandleGuard,String>{unsafe{
    // Block deletion/rename while installation is proceeding; reject reparse points
    // instead of following them. READ_CONTROL is required for descriptor review.
    let h=CreateFileW(wide(path.as_os_str()).as_ptr(),0x20080,3,null(),3,0x02200000,null_mut());
    if h as isize == -1 {return Err(error(&format!("Impossibile verificare {}",path.display())))}let handle=HandleGuard(h);
    let mut attrs=AttributeTag{attributes:0,tag:0};if GetFileInformationByHandleEx(h,9,(&mut attrs as *mut AttributeTag).cast(),size_of::<AttributeTag>() as u32)==0{return Err(error("Impossibile verificare gli attributi"))}
    if attrs.attributes&0x400!=0{return Err(format!("Il percorso {} contiene un punto di reindirizzamento",path.display()))}
    let mut owner=null_mut();let mut dacl=null_mut();let mut sd=null_mut();
    let rc=GetSecurityInfo(h,1,0x1|0x4,&mut owner,null_mut(),&mut dacl,null_mut(),&mut sd);
    if rc!=0{return Err(format!("Impossibile verificare i permessi di {}: {}",path.display(),std::io::Error::from_raw_os_error(rc as i32)))}
    let _descriptor=Descriptor(sd);if !trusted(&sid_text(owner)?) {return Err(format!("Proprietario non attendibile: {}",path.display()))} if dacl.is_null(){return Err("ACL vuota non protetta".into())}
    if strict {let mut control=0u16;let mut revision=0;if GetSecurityDescriptorControl(sd,&mut control,&mut revision)==0||control&0x1000==0{return Err(format!("ACL non protetta: {}",path.display()))}}
    let mut info=AclSize{count:0,bytes_in_use:0,bytes_free:0}; if GetAclInformation(dacl,(&mut info as *mut AclSize).cast(),size_of::<AclSize>() as u32,2)==0{return Err(error("Impossibile leggere ACL"))}
    for i in 0..info.count {let mut raw=null_mut();if GetAce(dacl,i,&mut raw)==0{return Err(error("ACE non leggibile"))}let p=raw as *const u8;let typ=*p;let flags=*p.add(1);let len=std::ptr::read_unaligned(p.add(2).cast::<u16>()) as usize;
        // Deny ACE cannot grant access. For ancestors ignore inherit-only ACEs;
        // for strict directories also check inheritable-only permissions.
        if typ==1 {continue} if typ!=0 || len<12{return Err(format!("ACL non supportata: {}",path.display()))} if flags&0x8!=0 && !strict {continue}
        let mask=std::ptr::read_unaligned(p.add(4).cast::<u32>());let sid=sid_text(p.add(8) as *mut c_void)?;
        if unsafe_allow(&sid,mask,base,strict){return Err(format!("Permessi di scrittura non attendibili su {} (SID {})",path.display(),sid))}
    }
    Ok(handle)
}}
/// Lock and validate every ancestor, including the Program Files directory.
pub fn parent_chain(program_files:&Path)->Result<Guard,String>{
    let mut components=program_files.components();
    match components.next(){Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_))=>{},_=>return Err("Program Files deve essere su un disco locale".into())}
    let mut chain=Vec::<PathBuf>::new();let mut p=program_files;loop{chain.push(p.to_path_buf());match p.parent(){Some(parent) if parent!=p => p=parent,_=>break}}
    chain.reverse();let mut handles=Vec::new();for entry in chain {if entry.as_os_str().is_empty(){continue} if entry.parent().is_none() && entry.as_os_str().len()==2{continue}handles.push(open_checked(&entry,entry==program_files,false)?)}
    Ok(Guard{_handles:handles})
}
/// Validate and lock the explicit protected installation directory.
pub fn installation_dir(dir:&Path)->Result<Guard,String>{Ok(Guard{_handles:vec![open_checked(dir,true,true)?]})}
/// Validate a previously installed binary; it must not be a reparse point and
/// must have an administrator-owned ACL that never grants writes to other users.
pub fn installed_file(path:&Path)->Result<(),String>{drop(open_checked(path,false,true)?);Ok(())}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn real_program_files_path_is_protected() {
        let target=crate::service::installed_path().unwrap();
        let program_files=target.parent().unwrap().parent().unwrap();
        parent_chain(program_files).unwrap();
    }
    #[test] fn remote_path_is_rejected() {
        assert!(parent_chain(Path::new(r"\\server\share\Program Files")).is_err());
    }
    #[test] fn dangerous_user_rights_are_rejected() {
        for mask in [0x10000,0x40000,0x80000,0x40,0x10,0x100,0x10000000,0x40000000,0x2,0x4] { assert!(unsafe_allow("S-1-5-32-545",mask,true,false)); }
        assert!(!unsafe_allow("S-1-5-18",0x10000000,true,true));
        assert!(!unsafe_allow("S-1-5-32-545",0x1200a9,true,true));
        assert!(!unsafe_allow("S-1-5-11",0x4,false,false)); // ancestor may permit creation, not deletion
    }
    #[test] fn user_owned_installation_dir_is_rejected() {
        let test_dir=std::env::current_dir().unwrap().join(format!("target\\mydesktop-untrusted-{}",std::process::id()));
        std::fs::create_dir(&test_dir).unwrap();
        let actual=installation_dir(&test_dir);
        std::fs::remove_dir(&test_dir).unwrap();
        assert!(actual.is_err());
    }
}

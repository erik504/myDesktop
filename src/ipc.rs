#![allow(non_snake_case, dead_code)]
use std::{ffi::{c_void, OsStr}, io, mem::size_of, os::windows::ffi::OsStrExt, ptr::{null,null_mut}, thread, time::{Duration, Instant}};
type Handle = *mut c_void;
type Sid = *mut c_void;
const INVALID: Handle = -1isize as Handle;
const QUERY: u32 = 0x1000;
const TOKEN_QUERY: u32 = 8;
const TOKEN_USER: u32 = 1;
const TOKEN_GROUPS: u32 = 2;
const TOKEN_STATISTICS: u32 = 10;
const TOKEN_SESSION: u32 = 12;
const TOKEN_ELEVATION: u32 = 20;
const REQ_SIZE: usize = 52;
const MAGIC: &[u8;4] = b"MDW1";

#[repr(C)] #[derive(Default,Clone,Copy)] struct FileTime { low:u32, high:u32 }
#[repr(C)] #[derive(Default,Clone,Copy)] struct Luid { low:u32, high:i32 }
#[repr(C)] struct Stats { token_id:Luid, authentication_id:Luid, expiration:i64, token_type:u32, impersonation:u32, dynamic_charged:u32, dynamic_available:u32, group_count:u32, privilege_count:u32, modified_id:Luid }
#[repr(C)] struct SidAttributes { sid:Sid, attributes:u32 }
#[repr(C)] struct TokenUser { user:SidAttributes }
#[repr(C)] struct TokenGroups { count:u32, groups:[SidAttributes;1] }
#[repr(C)] struct SecurityAttributes { length:u32, descriptor:*mut c_void, inherit:i32 }
#[repr(C)] #[derive(Default)] struct Rect { left:i32, top:i32, right:i32, bottom:i32 }
#[repr(C)] struct Placement { len:u32, flags:u32, show:u32, min:[i32;2], max:[i32;2], normal:Rect }

#[link(name="kernel32")] unsafe extern "system" {
 fn GetCurrentProcess()->Handle; fn GetCurrentThread()->Handle; fn OpenProcess(access:u32,inherit:i32,pid:u32)->Handle; fn CloseHandle(h:Handle)->i32;
 fn GetCurrentProcessId()->u32; fn ProcessIdToSessionId(pid:u32,session:*mut u32)->i32;
 fn GetProcessTimes(h:Handle,create:*mut FileTime,exit:*mut FileTime,kernel:*mut FileTime,user:*mut FileTime)->i32;
 fn QueryFullProcessImageNameW(h:Handle,flags:u32,name:*mut u16,size:*mut u32)->i32;
 fn CreateNamedPipeW(name:*const u16,open:u32,mode:u32,max:u32,outsize:u32,insize:u32,timeout:u32,sa:*const SecurityAttributes)->Handle;
 fn ConnectNamedPipe(h:Handle,overlapped:*mut c_void)->i32; fn DisconnectNamedPipe(h:Handle)->i32;
 fn GetNamedPipeClientProcessId(h:Handle,pid:*mut u32)->i32; fn GetNamedPipeServerProcessId(h:Handle,pid:*mut u32)->i32;
 fn PeekNamedPipe(h:Handle,buffer:*mut c_void,size:u32,read:*mut u32,available:*mut u32,left:*mut u32)->i32;
 fn ReadFile(h:Handle,buffer:*mut c_void,count:u32,read:*mut u32,overlapped:*mut c_void)->i32;
 fn WriteFile(h:Handle,buffer:*const c_void,count:u32,written:*mut u32,overlapped:*mut c_void)->i32;
 fn CreateFileW(path:*const u16,access:u32,share:u32,sa:*const c_void,disposition:u32,flags:u32,template:Handle)->Handle;
 fn WaitNamedPipeW(name:*const u16,timeout:u32)->i32; fn LocalFree(h:*mut c_void)->*mut c_void;
 fn GetCurrentThreadId()->u32;
 fn GetModuleHandleW(name:*const u16)->Handle; fn GetProcAddress(module:Handle,name:*const u8)->*const c_void;
}
#[link(name="advapi32")] unsafe extern "system" {
 fn OpenProcessToken(h:Handle,access:u32,token:*mut Handle)->i32; fn OpenThreadToken(h:Handle,access:u32,self_:i32,token:*mut Handle)->i32;
 fn GetTokenInformation(token:Handle,class:u32,buf:*mut c_void,len:u32,needed:*mut u32)->i32;
 fn GetLengthSid(sid:Sid)->u32; fn IsValidSid(sid:Sid)->i32;
 fn ConvertSidToStringSidW(sid:Sid,string:*mut *mut u16)->i32;
 fn ConvertStringSecurityDescriptorToSecurityDescriptorW(s:*const u16,rev:u32,sd:*mut *mut c_void,size:*mut u32)->i32;
 fn ImpersonateNamedPipeClient(h:Handle)->i32; fn RevertToSelf()->i32;
}
#[link(name="user32")] unsafe extern "system" {
 fn GetWindowThreadProcessId(h:isize,pid:*mut u32)->u32; fn IsWindowVisible(h:isize)->i32;
 fn GetThreadDesktop(thread:u32)->Handle; fn GetProcessWindowStation()->Handle; fn GetUserObjectInformationW(h:Handle,index:i32,data:*mut c_void,len:u32,needed:*mut u32)->i32;
 fn GetSystemMetrics(index:i32)->i32; fn SetWindowPos(h:isize,after:isize,x:i32,y:i32,w:i32,height:i32,flags:u32)->i32;
 fn ShowWindowAsync(h:isize,show:i32)->i32; fn GetWindowPlacement(h:isize,p:*mut c_void)->i32; fn GetWindowRect(h:isize,p:*mut c_void)->i32;
}
#[link(name="shell32")] unsafe extern "system" { fn SHGetFolderPathW(hwnd:Handle,folder:i32,token:Handle,flags:u32,path:*mut u16)->i32; }

struct Owned(Handle); impl Drop for Owned {fn drop(&mut self){unsafe {CloseHandle(self.0);}}}
struct Local(*mut c_void); impl Drop for Local {fn drop(&mut self){unsafe {LocalFree(self.0);}}}
fn wide(s:&str)->Vec<u16>{OsStr::new(s).encode_wide().chain(Some(0)).collect()}
fn last(msg:&str)->String{format!("{msg}: {}",io::Error::last_os_error())}
fn own(h:Handle)->Result<Owned,String>{if h.is_null()||h==INVALID {Err(last("handle non disponibile"))}else{Ok(Owned(h))}}
fn open_process(pid:u32)->Result<Owned,String>{unsafe{own(OpenProcess(QUERY,0,pid))}}
fn token(process:Handle)->Result<Owned,String>{unsafe{let mut h=null_mut();if OpenProcessToken(process,TOKEN_QUERY,&mut h)==0{return Err(last("token non disponibile"))}own(h)}}
fn info(token:Handle,class:u32)->Result<Vec<usize>,String>{unsafe{let mut n=0;GetTokenInformation(token,class,null_mut(),0,&mut n);if n==0||n>65536{return Err(last("token non leggibile"))}let mut b=vec![0usize;(n as usize+size_of::<usize>()-1)/size_of::<usize>()];let size=n;if GetTokenInformation(token,class,b.as_mut_ptr().cast(),size,&mut n)==0{return Err(last("token non leggibile"))}Ok(b)}}
#[derive(PartialEq,Eq,Debug)] struct Identity {sid:Vec<u8>,session:u32,logon:(u32,i32), elevated:bool}
fn identity(h:Handle)->Result<Identity,String>{unsafe{let u=info(h,TOKEN_USER)?;let sid=(*(u.as_ptr() as *const TokenUser)).user.sid;if sid.is_null()||IsValidSid(sid)==0{return Err("SID non valido".into())}let len=GetLengthSid(sid) as usize;if len==0||len>1024{return Err("SID non valido".into())}let sid=std::slice::from_raw_parts(sid.cast::<u8>(),len).to_vec();let session=info(h,TOKEN_SESSION)?;let stats=info(h,TOKEN_STATISTICS)?;let elevation=info(h,TOKEN_ELEVATION)?;let s=&*(stats.as_ptr() as *const Stats);Ok(Identity{sid,session:*(session.as_ptr() as *const u32),logon:(s.authentication_id.low,s.authentication_id.high),elevated:*(elevation.as_ptr() as *const u32)!=0})}}
// UAC can create a distinct authentication LUID for the same user in the same
// interactive session. The pipe ACL separately restricts clients to the logon SID.
fn same_user(a:&Identity,b:&Identity)->bool{a.sid==b.sid && a.session==b.session}
#[cfg(test)]
#[test]
fn same_session_accepts_different_uac_logons_but_not_other_users_or_sessions() {
    let a=Identity { sid:vec![1], session:1, logon:(1,0), elevated:false };
    let b=Identity { sid:vec![1], session:1, logon:(2,0), elevated:true };
    let other_user=Identity { sid:vec![2], session:1, logon:(1,0), elevated:true };
    let other_session=Identity { sid:vec![1], session:2, logon:(1,0), elevated:true };
    assert!(same_user(&a,&b));
    assert!(!same_user(&a,&other_user));
    assert!(!same_user(&a,&other_session));
}
fn current()->Result<Identity,String>{unsafe{identity(token(GetCurrentProcess())?.0)}}
fn process_identity(pid:u32)->Result<(Owned,Identity,u64),String>{unsafe{let p=open_process(pid)?;let id=identity(token(p.0)?.0)?;let(mut c,mut e,mut k,mut u)=(FileTime::default(),FileTime::default(),FileTime::default(),FileTime::default());if GetProcessTimes(p.0,&mut c,&mut e,&mut k,&mut u)==0{return Err(last("identità processo non leggibile"))}let created=(u64::from(c.high)<<32)|u64::from(c.low);Ok((p,id,created))}}
fn process_path(p:Handle)->Result<String,String>{unsafe{let mut b=vec![0u16;32768];let mut n=b.len() as u32;if QueryFullProcessImageNameW(p,0,b.as_mut_ptr(),&mut n)==0{return Err(last("percorso processo non leggibile"))}Ok(String::from_utf16_lossy(&b[..n as usize]))}}
fn installed_path()->Result<String,String>{unsafe{let mut p=[0u16;260];if SHGetFolderPathW(null_mut(),0x26,null_mut(),0,p.as_mut_ptr())!=0{return Err("Cartella Programmi non disponibile".into())}let n=p.iter().position(|x|*x==0).ok_or("Percorso Programmi non valido")?;Ok(format!("{}\\myDesktop\\myDesktop.exe",String::from_utf16_lossy(&p[..n])))}}
fn pipe_name(session:u32)->Vec<u16>{wide(&format!(r"\\.\pipe\myDesktop-WindowMover-v1-{session}"))}
fn object_name(h:Handle)->Option<String>{unsafe{if h.is_null(){return None}let mut b=[0u16;256];let mut n=0;if GetUserObjectInformationW(h,2,b.as_mut_ptr().cast(),(b.len()*2) as u32,&mut n)==0{return None}let end=b.iter().position(|c|*c==0).unwrap_or(b.len());Some(String::from_utf16_lossy(&b[..end]))}}
fn desktop_is_default(tid:u32)->bool{unsafe{let target=GetThreadDesktop(tid);let own=GetThreadDesktop(GetCurrentThreadId());let module=GetModuleHandleW(wide("kernelbase.dll").as_ptr());if module.is_null(){return false}let address=GetProcAddress(module,b"CompareObjectHandles\0".as_ptr());if address.is_null(){return false}let compare:unsafe extern "system" fn(Handle,Handle)->i32=std::mem::transmute(address);!target.is_null()&&!own.is_null()&&object_name(GetProcessWindowStation()).is_some_and(|n|n.eq_ignore_ascii_case("WinSta0"))&&object_name(own).is_some_and(|n|n.eq_ignore_ascii_case("Default"))&&compare(target,own)!=0}}

#[derive(Debug,Clone,Copy)] struct Request{hwnd:u64,pid:u32,created:u64,x:i32,y:i32,w:i32,h:i32,show:u32}
fn decode(b:&[u8])->Result<Request,String>{if b.len()!=REQ_SIZE||&b[..4]!=MAGIC||b[4..8]!=[1,0,1,0]||b[13..16]!=[0;3]||b[48..]!=[0;4]{return Err("Richiesta non valida".into())}let n=|i|i32::from_le_bytes(b[i..i+4].try_into().unwrap());let r=Request{pid:u32::from_le_bytes(b[8..12].try_into().unwrap()),show:b[12] as u32,hwnd:u64::from_le_bytes(b[16..24].try_into().unwrap()),created:u64::from_le_bytes(b[24..32].try_into().unwrap()),x:n(32),y:n(36),w:n(40),h:n(44)};if r.hwnd==0||r.pid==0||r.created==0||r.show>2||r.w<=0||r.h<=0||r.w>32768||r.h>32768||i64::from(r.x).abs()>1_000_000||i64::from(r.y).abs()>1_000_000{return Err("Geometria non valida".into())}Ok(r)}
fn encode(r:Request)->[u8;REQ_SIZE]{let mut b=[0u8;REQ_SIZE];b[..4].copy_from_slice(MAGIC);b[4]=1;b[6]=1;b[8..12].copy_from_slice(&r.pid.to_le_bytes());b[12]=r.show as u8;b[16..24].copy_from_slice(&r.hwnd.to_le_bytes());b[24..32].copy_from_slice(&r.created.to_le_bytes());for (off,v) in [(32,r.x),(36,r.y),(40,r.w),(44,r.h)]{b[off..off+4].copy_from_slice(&v.to_le_bytes())}b}
fn window_state(hwnd:isize)->Option<u32>{unsafe{let mut p:Placement=std::mem::zeroed();p.len=size_of::<Placement>() as u32;if GetWindowPlacement(hwnd,(&mut p as *mut Placement).cast())==0{None}else{Some(p.show)}}}
fn wait_for(mut f:impl FnMut()->bool,timeout:Duration)->bool{let until=Instant::now()+timeout;while Instant::now()<until {if f(){return true}thread::sleep(Duration::from_millis(25));}f()}
fn execute(req:Request,own_id:&Identity)->Result<(),String>{unsafe{let hwnd=isize::try_from(req.hwnd).map_err(|_|"Finestra non valida")?;let mut pid=0;let tid=GetWindowThreadProcessId(hwnd,&mut pid);if tid==0||pid!=req.pid||IsWindowVisible(hwnd)==0||!desktop_is_default(tid){return Err("Finestra non valida o non visibile".into())}let(_p,id,created)=process_identity(pid)?;if !same_user(&id,own_id)||created!=req.created{return Err("Finestra non autorizzata o non più valida".into())}let left=GetSystemMetrics(76);let top=GetSystemMetrics(77);let width=GetSystemMetrics(78);let height=GetSystemMetrics(79);if width<=0||height<=0||i64::from(req.x)>=i64::from(left)+i64::from(width)||i64::from(req.y)>=i64::from(top)+i64::from(height)||i64::from(req.x)+i64::from(req.w)<=i64::from(left)||i64::from(req.y)+i64::from(req.h)<=i64::from(top){return Err("Finestra fuori dal desktop".into())}let still_same=||{let mut verify=0;GetWindowThreadProcessId(hwnd,&mut verify)!=0 && verify==pid};if !still_same(){return Err("Finestra modificata durante la verifica".into())}
 ShowWindowAsync(hwnd,9);if !wait_for(||still_same() && window_state(hwnd)==Some(1),Duration::from_secs(2)){return Err("Impossibile riportare la finestra allo stato normale".into())}
 if !still_same()||SetWindowPos(hwnd,0,req.x,req.y,req.w,req.h,0x14|0x4000)==0{return Err(last("Impossibile spostare la finestra"))}
 if !wait_for(||{let mut r=Rect::default();still_same()&&GetWindowRect(hwnd,(&mut r as *mut Rect).cast())!=0&&r.left==req.x&&r.top==req.y&&i64::from(r.right)-i64::from(r.left)==i64::from(req.w)&&i64::from(r.bottom)-i64::from(r.top)==i64::from(req.h)},Duration::from_secs(2)){return Err("La finestra non ha accettato la posizione richiesta".into())}
 if req.show!=0 {let state=if req.show==1{3}else{6};ShowWindowAsync(hwnd,state);if !wait_for(||still_same()&&window_state(hwnd).is_some_and(|v|if req.show==1{v==3}else{[2,6,7,11].contains(&v)}),Duration::from_secs(2)){return Err("La finestra non ha accettato lo stato richiesto".into())}}
 Ok(())}}

/// Elevated helper, launched only from the protected service installation.
pub fn serve()->Result<(),String>{unsafe{
 let own_id=current()?;if !own_id.elevated{return Err("Il helper deve essere amministratore".into())}
 let groups=info(token(GetCurrentProcess())?.0,TOKEN_GROUPS)?;let group_header=&*(groups.as_ptr() as *const TokenGroups);if group_header.count>4096 || std::mem::offset_of!(TokenGroups,groups)+group_header.count as usize*size_of::<SidAttributes>()>groups.len()*size_of::<usize>(){return Err("Token gruppi non valido".into())}let start=std::ptr::addr_of!(group_header.groups).cast::<SidAttributes>();let mut sid=null_mut();for i in 0..group_header.count as usize {let group=&*start.add(i);if group.attributes&0xc0000000==0xc0000000{sid=group.sid;break}}if sid.is_null()||IsValidSid(sid)==0{return Err("SID sessione non disponibile".into())}let mut sid_string:*mut u16=null_mut();if ConvertSidToStringSidW(sid,&mut sid_string)==0{return Err(last("SID non convertibile"))}let _sid_guard=Local(sid_string.cast());let mut len=0;while *sid_string.add(len)!=0{len+=1;if len>1024{return Err("SID non valido".into())}}let sid_text=String::from_utf16_lossy(std::slice::from_raw_parts(sid_string,len));
 let sddl=wide(&format!("D:P(A;;GA;;;SY)(A;;0x0012008b;;;{sid_text})"));let mut descriptor=null_mut();if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut descriptor,null_mut())==0{return Err(last("ACL non valida"))}let _guard=Local(descriptor);let sa=SecurityAttributes{length:size_of::<SecurityAttributes>() as u32,descriptor,inherit:0};let name=pipe_name(own_id.session);
 let pipe=own(CreateNamedPipeW(name.as_ptr(),0x00000003|0x00080000,0x00000004|0x00000002|0x00000008,1,64,64,1000,&sa))?;
 loop{
  let connected=ConnectNamedPipe(pipe.0,null_mut())!=0||io::Error::last_os_error().raw_os_error()==Some(535);
  if !connected {thread::sleep(Duration::from_millis(100));continue}
  let until=Instant::now()+Duration::from_secs(3);let mut available=0;
  while Instant::now()<until && PeekNamedPipe(pipe.0,null_mut(),0,null_mut(),&mut available,null_mut())!=0 && available<REQ_SIZE as u32 {thread::sleep(Duration::from_millis(25));}
  let mut b=[0u8;REQ_SIZE];let mut n=0;let ok=available>=REQ_SIZE as u32&&ReadFile(pipe.0,b.as_mut_ptr().cast(),b.len() as u32,&mut n,null_mut())!=0&&n as usize==REQ_SIZE;
  let result=(||->Result<(),String>{if !ok{return Err("Richiesta incompleta".into())}let mut client_pid=0;if GetNamedPipeClientProcessId(pipe.0,&mut client_pid)==0{return Err(last("Client non identificato"))}if ImpersonateNamedPipeClient(pipe.0)==0{return Err(last("Client non autenticato"))}let client_token={let mut h=null_mut();let success=OpenThreadToken(GetCurrentThread(),TOKEN_QUERY,1,&mut h);if RevertToSelf()==0{std::process::abort()}if success==0{return Err(last("Token client non disponibile"))}own(h)?};let client=identity(client_token.0)?;let(_,pid_identity,_)=process_identity(client_pid)?;if !same_user(&client,&own_id)||!same_user(&client,&pid_identity){return Err("Client non autorizzato".into())}execute(decode(&b)?,&own_id)})();
  let code:u32=if result.is_ok(){0}else{1};let mut n=0;
  if WriteFile(pipe.0,code.to_le_bytes().as_ptr().cast(),4,&mut n,null_mut())!=0 && n==4 {
   // Client acknowledges reading the response before disconnect, with a bounded wait.
   let until=Instant::now()+Duration::from_secs(2);let mut ready=0;
   while Instant::now()<until&&PeekNamedPipe(pipe.0,null_mut(),0,null_mut(),&mut ready,null_mut())!=0&&ready<1 {thread::sleep(Duration::from_millis(25));}
   if ready>=1 {let mut ack=0u8;ReadFile(pipe.0,(&mut ack as *mut u8).cast(),1,&mut n,null_mut());}
  }
  DisconnectNamedPipe(pipe.0);
 }
}}

/// Request a placement from the authenticated elevated helper.
pub fn move_window(hwnd:isize,x:i32,y:i32,w:i32,h:i32,show:u32)->Result<(),String>{unsafe{
 let me=current()?;let mut pid=0;if GetWindowThreadProcessId(hwnd,&mut pid)==0{return Err("Finestra non valida".into())}let(_,target,created)=process_identity(pid)?;if !same_user(&me,&target){return Err("Finestra di un altro utente".into())}let state=if show==3{1}else if [2,6,7,11].contains(&show){2}else{0};let req=Request{hwnd:hwnd as u64,pid,created,x,y,w,h,show:state};let bytes=encode(req);decode(&bytes)?;
 let name=pipe_name(me.session);WaitNamedPipeW(name.as_ptr(),1000);let pipe=own(CreateFileW(name.as_ptr(),0x80000002,0,null(),3,0,null_mut())).map_err(|_|"Servizio privilegiato non disponibile".to_string())?;let mut server_pid=0;if GetNamedPipeServerProcessId(pipe.0,&mut server_pid)==0{return Err(last("Server non identificato"))}let(server,server_id,_)=process_identity(server_pid)?;if !same_user(&me,&server_id)||!server_id.elevated||!process_path(server.0)?.eq_ignore_ascii_case(&installed_path()?){return Err("Servizio non autenticato".into())}let mut n=0;if WriteFile(pipe.0,bytes.as_ptr().cast(),bytes.len() as u32,&mut n,null_mut())==0||n as usize!=bytes.len(){return Err(last("Invio richiesta fallito"))}let until=Instant::now()+Duration::from_secs(8);let mut available=0;while Instant::now()<until&&PeekNamedPipe(pipe.0,null_mut(),0,null_mut(),&mut available,null_mut())!=0&&available<4 {thread::sleep(Duration::from_millis(25));}let mut answer=[0u8;4];if available<4||ReadFile(pipe.0,answer.as_mut_ptr().cast(),4,&mut n,null_mut())==0||n!=4{return Err("Risposta del servizio assente o non valida".into())}let ack=[1u8];WriteFile(pipe.0,ack.as_ptr().cast(),1,&mut n,null_mut());if u32::from_le_bytes(answer)!=0{return Err("Il servizio ha rifiutato la richiesta di spostamento".into())}Ok(())
}}
#[cfg(test)] mod tests{use super::*;#[test]fn packet_roundtrip(){let r=Request{hwnd:20,pid:8,created:99,x:-50,y:10,w:400,h:300,show:1};let a=decode(&encode(r)).unwrap();assert_eq!((a.hwnd,a.pid,a.created,a.x,a.show),(20,8,99,-50,1));}#[test]fn rejects_invalid(){let r=Request{hwnd:20,pid:8,created:99,x:0,y:0,w:-1,h:300,show:0};assert!(decode(&encode(r)).is_err());let mut r=encode(Request{w:50,..r});r[6]=3;assert!(decode(&r).is_err());}}

//! Private desktop bring-up client; host previews never issue kernel syscalls.
use alloc::vec::Vec;
use nt61_abi::desktop_bridge::*;
#[cfg(target_os="none")]
#[allow(dead_code)]
#[path="../bin/explorer/arch/mod.rs"]
mod machine;

pub fn submit(request:&mut Request)->Result<(),u32>{
    #[cfg(target_os="none")]
    let status=machine::syscall5(SYS_DESKTOP_SERVICE,core::ptr::from_mut(request)as usize,0,0,0,0)as u32;
    #[cfg(not(target_os="none"))]
    let status={let _=&request;STATUS_NOT_SUPPORTED};
    if status==STATUS_SUCCESS{Ok(())}else{Err(status)}
}

fn exchange(operation:u32,path:&str,input:&[u8])->Result<(Vec<u8>,u32),u32>{
    if path.len()>MAX_PATH || input.len()>MAX_TRANSFER{return Err(STATUS_INVALID_PARAMETER);}
    let output=matches!(operation,LIST|READ|READ_AT|COMMAND|PROCESSES|DISKPART);
    let capacity=if operation==COMMAND{CWD_BYTES+65536}else if output{65536}else{input.len()};
    let mut buffer=alloc::vec![0;capacity];
    if input.len()>buffer.len(){return Err(STATUS_INVALID_PARAMETER);}
    buffer[..input.len()].copy_from_slice(input);
    let mut request=Request{version:VERSION,operation,path:path.as_ptr()as u64,path_length:path.len()as u64,
        data:buffer.as_mut_ptr()as u64,capacity:buffer.len()as u64,..Request::default()};
    let result=submit(&mut request);
    // A read can be retried with the required capacity. A command or write
    // must never be repeated: it may already have changed the system.
    if result==Err(STATUS_BUFFER_TOO_SMALL) && matches!(operation,LIST|READ|PROCESSES)
        && request.length>buffer.len()as u64 && request.length<=MAX_TRANSFER as u64 {
        buffer.resize(request.length as usize,0);
        request.data=buffer.as_mut_ptr()as u64;request.capacity=buffer.len()as u64;
        submit(&mut request)?;
    }else{result?;}
    if request.length>buffer.len()as u64{return Err(STATUS_BUFFER_TOO_SMALL);}
    buffer.truncate(request.length as usize);Ok((buffer,request.flags))
}

pub fn transfer(operation:u32,path:&str,input:&[u8])->Result<Vec<u8>,u32>{
    exchange(operation,path,input).map(|(bytes,_)|bytes)
}

pub fn logon(user:&str,password:&str)->Result<(),u32>{
    if user.len()>MAX_PATH || password.len()>256 {return Err(STATUS_INVALID_PARAMETER);}
    // LOGON has no output payload. Borrow the credential buffer instead of
    // leaving an extra password copy in a temporary heap allocation.
    submit(&mut Request{version:VERSION,operation:LOGON,path:user.as_ptr()as u64,
        path_length:user.len()as u64,data:password.as_ptr()as u64,
        capacity:password.len()as u64,..Request::default()})
}

pub struct CommandOutput {pub directory:alloc::string::String,pub bytes:Vec<u8>,pub exit:bool}
pub fn command(line:&str,directory:&str)->Result<CommandOutput,u32>{
    if directory.len()>=CWD_BYTES{return Err(STATUS_INVALID_PARAMETER);}
    let mut cwd=[0;CWD_BYTES];cwd[..directory.len()].copy_from_slice(directory.as_bytes());
    let (bytes,flags)=exchange(COMMAND,line,&cwd)?;
    if bytes.len()<CWD_BYTES{return Err(STATUS_INVALID_PARAMETER);}
    let n=bytes[..CWD_BYTES].iter().position(|b|*b==0).unwrap_or(CWD_BYTES);
    let directory=core::str::from_utf8(&bytes[..n]).map_err(|_|STATUS_INVALID_PARAMETER)?;
    Ok(CommandOutput{directory:directory.into(),bytes:bytes[CWD_BYTES..].to_vec(),exit:flags&COMMAND_EXIT!=0})
}

/// Range reads keep resource DLLs out of the shell heap; only selected icon
/// payloads are fetched, even when imageres.dll is tens of megabytes long.
pub fn read_at(path:&str,offset:u64,length:usize)->Result<Vec<u8>,u32>{
    if length>MAX_READ_CHUNK{return Err(STATUS_INVALID_PARAMETER);}
    let mut input=offset.to_le_bytes().to_vec();
    input.extend_from_slice(&(length as u32).to_le_bytes());
    transfer(READ_AT,path,&input)
}

pub fn processes()->Result<crate::applications::processes::Snapshot,u32>{
    crate::applications::processes::Snapshot::decode(&transfer(PROCESSES,"",&[])?)
}

pub fn diskpart(line:&str,focus:[u32;3])->Result<([u32;3],u32,alloc::string::String,bool),u32>{
    let input:Vec<u8>=focus.into_iter().flat_map(u32::to_le_bytes).collect();
    let(bytes,flags)=exchange(DISKPART,line,&input)?;
    if bytes.len()<DISKPART_HEADER{return Err(STATUS_INVALID_PARAMETER);}
    let mut state=[0;3];for(i,v)in state.iter_mut().enumerate(){*v=u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap());}
    let code=u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let output=core::str::from_utf8(&bytes[16..]).map_err(|_|STATUS_INVALID_PARAMETER)?;
    Ok((state,code,output.into(),flags&COMMAND_EXIT!=0))
}

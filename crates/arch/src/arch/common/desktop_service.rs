//! Bounded user-memory boundary for desktop file, console and logon services.
use alloc::{vec, vec::Vec};
use nt61_abi::desktop_bridge::*;

fn copy_input(address:u64,length:usize)->Result<Vec<u8>,u32>{
    let mut bytes=vec![0;length];
    if length!=0 {crate::mm::user_copy::copy_from_user(&mut bytes,address).map_err(|e|e as u32)?;}
    Ok(bytes)
}
fn execute(request:&mut Request)->Result<Vec<u8>,u32>{
    if request.version!=VERSION || request.path_length>MAX_PATH as u64 || request.capacity>MAX_TRANSFER as u64 {return Err(STATUS_INVALID_PARAMETER);}
    let bytes=copy_input(request.path,request.path_length as usize)?;
    let path=core::str::from_utf8(&bytes).map_err(|_|STATUS_INVALID_PARAMETER)?;
    match request.operation{
        LIST=>{
            let mut output=Vec::new();
            for entry in crate::fs::system_volume::list(path)? {
                if entry.name.len()>MAX_PATH || output.len()+ENTRY_HEADER+entry.name.len()>MAX_TRANSFER{return Err(STATUS_BUFFER_TOO_SMALL);}
                output.extend_from_slice(&(entry.name.len()as u32).to_le_bytes());
                output.extend_from_slice(&(if entry.directory{DIRECTORY}else{0}).to_le_bytes());
                output.extend_from_slice(&entry.size.to_le_bytes());
                output.extend_from_slice(entry.name.as_bytes());
            }
            Ok(output)
        },
        READ=>crate::fs::system_volume::read(path),
        READ_AT=>{
            if request.capacity<12 {return Err(STATUS_INVALID_PARAMETER);}
            let input=copy_input(request.data,12)?;
            let offset=u64::from_le_bytes(input[..8].try_into().unwrap());
            let length=u32::from_le_bytes(input[8..12].try_into().unwrap()) as usize;
            if length>MAX_READ_CHUNK || length>request.capacity as usize {return Err(STATUS_INVALID_PARAMETER);}
            crate::fs::system_volume::read_at(path,offset,length)
        },
        RENAME=>{
            let input=copy_input(request.data,request.capacity as usize)?;
            let destination=core::str::from_utf8(&input).map_err(|_|STATUS_INVALID_PARAMETER)?;
            crate::fs::system_volume::rename(path,destination)?;Ok(Vec::new())
        },
        CREATE=>{
            let data=copy_input(request.data,request.capacity as usize)?;
            crate::fs::system_volume::create(path,&data)?;Ok(Vec::new())
        },
        WRITE=>{
            let data=copy_input(request.data,request.capacity as usize)?;
            crate::fs::system_volume::write(path,&data)?;Ok(Vec::new())
        },
        MKDIR=>{crate::fs::system_volume::mkdir(path)?;Ok(Vec::new())},
        REMOVE=>{crate::fs::system_volume::remove(path)?;Ok(Vec::new())},
        COMMAND=>{
            if request.capacity<CWD_BYTES as u64{return Err(STATUS_INVALID_PARAMETER);}
            let directory=copy_input(request.data,CWD_BYTES)?;
            let n=directory.iter().position(|b|*b==0).unwrap_or(directory.len());
            let directory=core::str::from_utf8(&directory[..n]).map_err(|_|STATUS_INVALID_PARAMETER)?;
            let directory=crate::fs::system_volume::normalize(directory)?;
            let result=crate::servers::cmd::execute_captured_command(path,directory.as_bytes()).map_err(|_|STATUS_ACCESS_DENIED)?;
            let mut output=vec![0;CWD_BYTES];
            let n=result.directory.len().min(CWD_BYTES-1);output[..n].copy_from_slice(&result.directory[..n]);
            output.extend_from_slice(&result.output);
            request.flags=if result.exit{COMMAND_EXIT}else{0};
            Ok(output)
        },
        DISKPART=>{
            if request.capacity<DISKPART_HEADER as u64{return Err(STATUS_INVALID_PARAMETER);}
            let bytes=copy_input(request.data,12)?;let mut focus=[0;3];for(i,v)in focus.iter_mut().enumerate(){*v=u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap());}
            let focus_result=crate::fs::diskpart::DiskPartState::from_focus(focus);
            let (mut state,stale)=match focus_result {Ok(s)=>(s,None),Err(e)=>(crate::fs::diskpart::DiskPartState::new(),Some(e))};
            let command=if let Some(e)=stale {Err(e.into())} else {crate::fs::diskpart::execute_command(path,&mut state)};
            let (code,text,exit)=match command{Ok(text)=>(0u32,text,false),Err(text)if text=="EXIT"=>(0,"Leaving DiskPart.".into(),true),Err(text)=>(1,text,false)};
            let mut output:Vec<u8>=state.focus().into_iter().flat_map(u32::to_le_bytes).collect();output.extend_from_slice(&code.to_le_bytes());output.extend_from_slice(text.as_bytes());request.flags=if exit{COMMAND_EXIT}else{0};Ok(output)
        },
        PROCESSES=>{
            let processes=crate::ps::process_mgmt::get_all_processes().map_err(|e|e as u32)?;
            if processes.len()>MAX_PROCESSES {return Err(STATUS_BUFFER_TOO_SMALL);}
            let mut output=Vec::new();
            output.extend_from_slice(&crate::ke::scheduler::get_cpu_count().to_le_bytes());
            output.extend_from_slice(&(processes.len()as u32).to_le_bytes());
            for process in processes {
                if process.name.len()>MAX_PATH || process.user_name.len()>MAX_PATH
                    || output.len()+PROCESS_HEADER+process.name.len()+process.user_name.len()>MAX_TRANSFER {
                    return Err(STATUS_BUFFER_TOO_SMALL);
                }
                output.extend_from_slice(&process.pid.to_le_bytes());
                output.extend_from_slice(&process.memory_usage.to_le_bytes());
                output.extend_from_slice(&process.cpu_time.to_le_bytes());
                output.extend_from_slice(&process.session_id.to_le_bytes());
                output.extend_from_slice(&(process.name.len()as u32).to_le_bytes());
                output.extend_from_slice(&(process.user_name.len()as u32).to_le_bytes());
                output.extend_from_slice(process.name.as_bytes());
                output.extend_from_slice(process.user_name.as_bytes());
            }
            Ok(output)
        },
        LOGON=>{
            // Only the pending Session-1 LogonUI may install Explorer's token.
            let thread=crate::ps::thread::get_current_ethread();
            if thread.is_null(){return Err(STATUS_ACCESS_DENIED);}
            // SAFETY: the current ETHREAD and its owning process are live.
            let process=unsafe{(*thread).threads_process};
            if process.is_null() || unsafe{(*process).unique_process_id}!=crate::servers::smss::PID_LOGONUI {
                return Err(STATUS_ACCESS_DENIED);
            }
            if request.capacity>256{return Err(STATUS_INVALID_PARAMETER);}
            let explorer=crate::ps::process::get_by_pid(crate::servers::smss::PID_EXPLORER).ok_or(STATUS_NOT_FOUND)?;
            let mut password=copy_input(request.data,request.capacity as usize)?;
            let token=crate::se::logon::interactive_logon(path,&password,1);
            for byte in &mut password {
                // SAFETY: each pointer refers to an exclusively borrowed live byte.
                // Volatile stores prevent credentials surviving optimized erasure.
                unsafe {core::ptr::write_volatile(byte,0);}
            }
            let token=token.ok_or(STATUS_ACCESS_DENIED)?;
            explorer.set_token(token);
            crate::ps::process::authorize_logon_handoff();
            crate::hal::serial::write_string_always("[LOGON] interactive session 1 authenticated\r\n");
            Ok(Vec::new())
        },
        _=>Err(STATUS_NOT_SUPPORTED),
    }
}

/// # Safety
/// `address` is an untrusted user address; all accesses use checked user copies.
pub unsafe fn dispatch(address:u64)->u64{
    let mut request=Request::default();
    // SAFETY: Request is repr(C), has no references or invalid integer bit patterns,
    // and every byte (including output fields) is overwritten by copy_from_user.
    let bytes=unsafe{core::slice::from_raw_parts_mut(core::ptr::from_mut(&mut request).cast::<u8>(),core::mem::size_of::<Request>())};
    if crate::mm::user_copy::copy_from_user(bytes,address).is_err(){return STATUS_INVALID_PARAMETER as u64;}
    request.length=0;request.flags=0;
    match execute(&mut request){
        Ok(output)=>{
            request.length=output.len()as u64;
            request.status=if output.len()>request.capacity as usize {STATUS_BUFFER_TOO_SMALL}
                else if output.is_empty(){STATUS_SUCCESS}
                else {match crate::mm::user_copy::copy_to_user(request.data,&output){Ok(())=>STATUS_SUCCESS,Err(e)=>e as u32}};
        },
        Err(status)=>request.status=status,
    }
    // SAFETY: repr(C) Request is fully initialized, without padding (56 bytes).
    let bytes=unsafe{core::slice::from_raw_parts(core::ptr::from_ref(&request).cast::<u8>(),core::mem::size_of::<Request>())};
    match crate::mm::user_copy::copy_to_user(address,bytes){Ok(())=>request.status as u64,Err(e)=>e as u64}
}

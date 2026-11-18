//! Private desktop bring-up client; host previews never issue kernel syscalls.
use alloc::vec::Vec;
use nt61_abi::desktop_bridge::*;
#[cfg(target_os = "none")]
#[allow(dead_code)]
#[path = "../bin/explorer/arch/mod.rs"]
mod machine;

pub fn submit(request: &mut Request) -> Result<(), u32> {
    #[cfg(target_os = "none")]
    let status = machine::syscall5(
        SYS_DESKTOP_SERVICE,
        core::ptr::from_mut(request) as usize,
        0,
        0,
        0,
        0,
    ) as u32;
    #[cfg(not(target_os = "none"))]
    let status = {
        let _ = &request;
        STATUS_NOT_SUPPORTED
    };
    if status == STATUS_SUCCESS {
        Ok(())
    } else {
        Err(status)
    }
}

fn exchange(operation: u32, path: &str, input: &[u8]) -> Result<(Vec<u8>, u32), u32> {
    exchange_with(operation, path, input, submit)
}

fn exchange_with(
    operation: u32,
    path: &str,
    input: &[u8],
    mut submit_request: impl FnMut(&mut Request) -> Result<(), u32>,
) -> Result<(Vec<u8>, u32), u32> {
    if path.len() > MAX_PATH || input.len() > MAX_TRANSFER {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let output = matches!(
        operation,
        LIST | READ | READ_AT | COMMAND | PROCESSES | DISKPART | nt61_abi::dialog_bridge::POLL | nt61_abi::dialog_bridge::COMMAND_POLL
    );
    let capacity = if operation == VOLUME_SPACE {
        16
    } else if operation == nt61_abi::session_bridge::DISPLAY_MODES {256
    } else if operation == nt61_abi::session_bridge::HOTKEYS {4
    } else if operation == nt61_abi::session_bridge::MEDIA {input.len().max(16)
    } else if operation == nt61_abi::session_bridge::SOUND {16
    } else if operation == nt61_abi::session_bridge::SESSION && path=="user" {128
    } else if operation == nt61_abi::session_bridge::SESSION && path=="wallpaper-get" {512
    } else if matches!(operation,nt61_abi::session_bridge::MONITOR|nt61_abi::session_bridge::SERVICES|nt61_abi::session_bridge::ACCOUNTS|nt61_abi::session_bridge::NETWORK) {65536
    } else if matches!(operation, COMMAND | nt61_abi::dialog_bridge::COMMAND_POLL) {
        CWD_BYTES + 65536
    } else if operation == nt61_abi::dialog_bridge::PRINT_WRITE { input.len().max(4)
    } else if output {
        65536
    } else {
        input.len()
    };
    let mut buffer = alloc::vec![0;capacity];
    if input.len() > buffer.len() {
        return Err(STATUS_INVALID_PARAMETER);
    }
    buffer[..input.len()].copy_from_slice(input);
    let mut request = Request {
        version: VERSION,
        operation,
        path: path.as_ptr() as u64,
        path_length: path.len() as u64,
        data: buffer.as_mut_ptr() as u64,
        capacity: buffer.len() as u64,
        ..Request::default()
    };
    let result = submit_request(&mut request);
    // A read can be retried with the required capacity. A command or write
    // must never be repeated: it may already have changed the system.
    if result == Err(STATUS_BUFFER_TOO_SMALL)
        && matches!(operation, LIST | READ | PROCESSES)
        && request.length > buffer.len() as u64
        && request.length <= MAX_TRANSFER as u64
    {
        buffer.resize(request.length as usize, 0);
        request.data = buffer.as_mut_ptr() as u64;
        request.capacity = buffer.len() as u64;
        submit_request(&mut request)?;
    } else {
        result?;
    }
    if request.length > buffer.len() as u64 {
        return Err(STATUS_BUFFER_TOO_SMALL);
    }
    buffer.truncate(request.length as usize);
    Ok((buffer, request.flags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_query_reserves_output_without_input_payload() {
        let expected: Vec<u8> = [536870912u64, 340103168]
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect();
        let (actual, _) = exchange_with(VOLUME_SPACE, "C:\\", &[], |request| {
            request.length = expected.len() as u64;
            if request.capacity < request.length {
                return Err(STATUS_BUFFER_TOO_SMALL);
            }
            // SAFETY: exchange_with keeps the owned output buffer live for
            // this synchronous call, and capacity was checked above.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    expected.as_ptr(),
                    request.data as *mut u8,
                    expected.len(),
                );
            }
            Ok(())
        })
        .expect("a no-input query must still receive its volume information");
        assert_eq!(actual, expected);
    }
}

pub fn transfer(operation: u32, path: &str, input: &[u8]) -> Result<Vec<u8>, u32> {
    exchange(operation, path, input).map(|(bytes, _)| bytes)
}

pub fn logon(user: &str, password: &str) -> Result<(), u32> {
    if user.len() > MAX_PATH || password.len() > 256 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    // LOGON has no output payload. Borrow the credential buffer instead of
    // leaving an extra password copy in a temporary heap allocation.
    submit(&mut Request {
        version: VERSION,
        operation: LOGON,
        path: user.as_ptr() as u64,
        path_length: user.len() as u64,
        data: password.as_ptr() as u64,
        capacity: password.len() as u64,
        ..Request::default()
    })
}

pub struct CommandOutput {
    pub directory: alloc::string::String,
    pub bytes: Vec<u8>,
    pub exit: bool,
}
pub fn command(line: &str, directory: &str) -> Result<CommandOutput, u32> {
    if directory.len() >= CWD_BYTES {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut cwd = [0; CWD_BYTES];
    cwd[..directory.len()].copy_from_slice(directory.as_bytes());
    let (bytes, flags) = exchange(COMMAND, line, &cwd)?;
    if bytes.len() < CWD_BYTES {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let n = bytes[..CWD_BYTES]
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(CWD_BYTES);
    let directory = core::str::from_utf8(&bytes[..n]).map_err(|_| STATUS_INVALID_PARAMETER)?;
    Ok(CommandOutput {
        directory: directory.into(),
        bytes: bytes[CWD_BYTES..].to_vec(),
        exit: flags & COMMAND_EXIT != 0,
    })
}

/// Range reads keep resource DLLs out of the shell heap; only selected icon
/// payloads are fetched, even when imageres.dll is tens of megabytes long.
pub fn read_at(path: &str, offset: u64, length: usize) -> Result<Vec<u8>, u32> {
    if length > MAX_READ_CHUNK {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut input = offset.to_le_bytes().to_vec();
    input.extend_from_slice(&(length as u32).to_le_bytes());
    transfer(READ_AT, path, &input)
}

pub fn processes() -> Result<crate::applications::processes::Snapshot, u32> {
    crate::applications::processes::Snapshot::decode(&transfer(PROCESSES, "", &[])?)
}

pub fn diskpart(
    line: &str,
    focus: [u32; 3],
) -> Result<([u32; 3], u32, alloc::string::String, bool), u32> {
    let input: Vec<u8> = focus.into_iter().flat_map(u32::to_le_bytes).collect();
    let (bytes, flags) = exchange(DISKPART, line, &input)?;
    if bytes.len() < DISKPART_HEADER {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut state = [0; 3];
    for (i, v) in state.iter_mut().enumerate() {
        *v = u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    }
    let code = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let output = core::str::from_utf8(&bytes[16..]).map_err(|_| STATUS_INVALID_PARAMETER)?;
    Ok((state, code, output.into(), flags & COMMAND_EXIT != 0))
}

/// Start command execution without blocking the compositor or modal input.
pub fn command_start(line:&str,directory:&str)->Result<u64,u32>{
    if directory.len()>=CWD_BYTES{return Err(STATUS_INVALID_PARAMETER);}
    let mut input=[0;CWD_BYTES];input[..directory.len()].copy_from_slice(directory.as_bytes());
    let (bytes,_)=exchange(nt61_abi::dialog_bridge::COMMAND_START,line,&input)?;
    Ok(u64::from_le_bytes(bytes.as_slice().try_into().map_err(|_|STATUS_INVALID_PARAMETER)?))
}
pub fn command_poll(id:u64)->Result<Option<CommandOutput>,u32>{
    let (bytes,flags)=exchange(nt61_abi::dialog_bridge::COMMAND_POLL,&alloc::format!("{id}"),&[])?;
    if bytes.is_empty(){return Ok(None);}
    if bytes.len()<CWD_BYTES{return Err(STATUS_INVALID_PARAMETER);}
    let n=bytes[..CWD_BYTES].iter().position(|b|*b==0).unwrap_or(CWD_BYTES);
    let directory=core::str::from_utf8(&bytes[..n]).map_err(|_|STATUS_INVALID_PARAMETER)?.into();
    Ok(Some(CommandOutput{directory,bytes:bytes[CWD_BYTES..].to_vec(),exit:flags&COMMAND_EXIT!=0}))
}

pub fn session(action:&str,input:&[u8])->Result<Vec<u8>,u32>{
    // Passwords borrow caller storage. Never clone them into exchange's buffer.
    if matches!(action,"user"|"wallpaper-get") {return transfer(nt61_abi::session_bridge::SESSION,action,input);}
    submit(&mut Request{version:VERSION,operation:nt61_abi::session_bridge::SESSION,path:action.as_ptr()as u64,path_length:action.len()as u64,data:input.as_ptr()as u64,capacity:input.len()as u64,..Request::default()})?;
    Ok(Vec::new())
}
pub fn desktop_hotkeys()->Result<u32,u32>{let b=transfer(nt61_abi::session_bridge::HOTKEYS,"",&[])?;Ok(u32::from_le_bytes(b.as_slice().try_into().map_err(|_|STATUS_INVALID_PARAMETER)?))}

pub fn monitor()->Result<crate::applications::system_monitor::Sample,u32>{crate::applications::system_monitor::Sample::decode(&transfer(nt61_abi::session_bridge::MONITOR,"",&[])?) }
pub fn service_list()->Result<Vec<crate::applications::system_monitor::Service>,u32>{crate::applications::system_monitor::decode_services(&transfer(nt61_abi::session_bridge::SERVICES,"",&[])?) }
pub fn task_control(pid:u64,command:&str)->Result<(),u32>{transfer(nt61_abi::session_bridge::TASK_CONTROL,&alloc::format!("{pid}"),command.as_bytes()).map(|_|())}
pub fn service_control(name:&str,start:bool)->Result<(),u32>{transfer(nt61_abi::session_bridge::SERVICE_CONTROL,name,if start{b"start"}else{b"stop"}).map(|_|())}

pub fn display_modes()->Result<Vec<(u32,u32)>,u32>{
    let (data,_)=exchange(nt61_abi::session_bridge::DISPLAY_MODES,"",&[])?;
    if data.len()%8!=0{return Err(STATUS_INVALID_PARAMETER);}
    Ok(data.chunks_exact(8).map(|b|(u32::from_le_bytes(b[..4].try_into().unwrap()),u32::from_le_bytes(b[4..].try_into().unwrap()))).collect())
}
pub fn display_set(w:u32,h:u32)->Result<(),u32>{
    exchange(nt61_abi::session_bridge::DISPLAY_SET,&alloc::format!("{w}x{h}"),&[]).map(|_|())
}

pub fn display_preference(command:&str)->Result<(),u32>{
    exchange(nt61_abi::session_bridge::DISPLAY_SET,command,&[]).map(|_|())
}

pub fn accounts()->Result<Vec<crate::accounts::Account>,u32>{crate::accounts::decode(&transfer(nt61_abi::session_bridge::ACCOUNTS,"",&[])?) }
pub fn account_control(name:&str,command:&str,value:&[u8])->Result<(),u32>{
    if name.len()>64||command.len()>16||value.len()>64{return Err(STATUS_INVALID_PARAMETER);}
    let mut input=[0u8;82];input[..command.len()].copy_from_slice(command.as_bytes());input[command.len()+1..command.len()+1+value.len()].copy_from_slice(value);
    let bytes=&mut input[..command.len()+1+value.len()];
    let result=submit(&mut Request{version:VERSION,operation:nt61_abi::session_bridge::ACCOUNT_CONTROL,path:name.as_ptr()as u64,path_length:name.len()as u64,data:bytes.as_ptr()as u64,capacity:bytes.len()as u64,..Request::default()});
    for b in bytes {unsafe{core::ptr::write_volatile(b,0);}} result
}

pub fn play_sound(event:&str)->Result<(),u32>{transfer(nt61_abi::session_bridge::SOUND,event,&[]).map(|_|())}

pub fn sound_settings()->Result<alloc::vec::Vec<u8>,u32>{transfer(nt61_abi::session_bridge::SOUND,"settings",&[])}

pub fn media(action:&str,input:&[u8])->Result<[u32;4],u32>{
    let mut packet=(input.len() as u32).to_le_bytes().to_vec();packet.extend_from_slice(input);
    let (bytes,_)=exchange(nt61_abi::session_bridge::MEDIA,action,&packet)?;
    if bytes.len()!=16 {return Err(STATUS_INVALID_PARAMETER);}
    Ok(core::array::from_fn(|i|u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap())))
}

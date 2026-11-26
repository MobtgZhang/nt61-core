use super::{environment, nls, put, read, string, word, ApiError, DecodeMode, GuestImage, GuestMemory, GuestServices, NOACCESS};
use crate::arch::common::btl::{abi::GuestAbi, nt61::ChildRequest};
use alloc::string::String;

pub(super) fn process_string(memory: &mut dyn GuestMemory, address: u64, wide: bool) -> Result<Option<String>, u32> {
    if address == 0 { return Ok(None); }
    if wide { return string(memory, address, true).map(Some); }
    let mut value = String::new();
    for index in 0..32767 {
        let address = address.checked_add(index).ok_or(NOACCESS)?;
        let mut byte = [0];
        if memory.read_virt(address, &mut byte) != 1 { return Err(NOACCESS); }
        if byte[0] == 0 { return Ok(Some(value)); }
        value.push(char::from_u32(u32::from(nls::decode_ansi(byte[0]))).ok_or(1113u32)?);
    }
    Err(87)
}

pub(super) fn create(
    image: &mut GuestImage, args: &[u64], services: &mut dyn GuestServices, wide: bool,
) -> Result<u64, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let startup_size = if x86 { 68 } else { 104 };
    let startup = read(&mut image.memory, args[8], startup_size)?;
    if u32::from_le_bytes(startup[..4].try_into().unwrap()) as usize != startup_size {
        return Err(87u32.into());
    }
    let flags = args[5] as u32;
    if args[2..5].iter().any(|&argument| argument != 0) || flags & !0x400 != 0
        || startup[4..].iter().any(|&byte| byte != 0) {
        return Err(120u32.into());
    }
    let information_size = width * 2 + 8;
    if !image.memory.probe_write(args[9], information_size) { return Err(NOACCESS.into()); }
    let application = process_string(&mut image.memory, args[0], wide)?;
    let command_line = process_string(&mut image.memory, args[1], wide)?;
    if application.is_none() && command_line.as_deref().is_none_or(str::is_empty) {
        return Err(87u32.into());
    }
    let parameters = word(&mut image.memory, image.peb + if x86 { 0x10 } else { 0x20 }, width)?;
    let parent_directory = if parameters == 0 {
        String::from("C:\\")
    } else {
        let slot = parameters.checked_add(if x86 { 0x24 } else { 0x38 }).ok_or(NOACCESS)?;
        let buffer = word(&mut image.memory, slot + width as u64, width)?;
        string(&mut image.memory, buffer, true)?
    };
    let directory = process_string(&mut image.memory, args[7], wide)?.unwrap_or_else(|| parent_directory.clone());
    let search_path = if application.is_none() && parameters != 0 {
        let name: alloc::vec::Vec<u16> = "PATH".encode_utf16().collect();
        environment::lookup(image, &name)?.map(|value| String::from_utf16_lossy(&value))
    } else { None };
    let environment = if args[6] != 0 {
        Some(environment::capture(&mut image.memory, args[6], flags & 0x400 != 0)?)
    } else if parameters != 0 {
        Some(environment::snapshot(image)?)
    } else { None };
    let abi = if x86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
    let information = services.create_process(ChildRequest {
        application, command_line, directory, parent_directory, search_path, environment, abi,
        redirect: !image.win32.local.fs_redirection_disabled,
    })?;
    let mut bytes = [0; 24];
    bytes[..4].copy_from_slice(&information[0].to_le_bytes());
    bytes[width..width + 4].copy_from_slice(&information[1].to_le_bytes());
    bytes[width * 2..width * 2 + 4].copy_from_slice(&information[2].to_le_bytes());
    bytes[width * 2 + 4..information_size].copy_from_slice(&information[3].to_le_bytes());
    if !image.memory.write_virt(args[9], &bytes[..information_size]) { return Err(NOACCESS.into()); }
    Ok(1)
}

pub(super) fn child_info(
    image: &mut GuestImage, args: &[u64], services: &dyn GuestServices, name: &str,
) -> Result<u64, ApiError> {
    let thread = matches!(name, "GetThreadId" | "GetExitCodeThread" | "GetProcessIdOfThread");
    let (id, process_id, code) = services.child_info(args[0], thread)?;
    match name {
        "GetProcessIdOfThread" => Ok(u64::from(process_id)),
        "GetExitCodeThread" | "GetExitCodeProcess" => {
            put(&mut image.memory, args[1], u64::from(code), 4)?;
            Ok(1)
        }
        _ => Ok(u64::from(id)),
    }
}

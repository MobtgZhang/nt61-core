//! Checked shell paths and COM task memory in the isolated guest address space.
//! Current-user profile folders use this process's provisioned environment.
//! Registry folder redirection, other tokens and shell namespace objects are
//! not supported; failures are HRESULTs and never change Win32 LastError.
use super::*;

#[derive(Default)]
pub(super) struct State {
    task_blocks: Vec<(u64, usize)>,
}

fn task_alloc(image: &mut GuestImage, size: u64) -> Result<u64, u32> {
    let size = usize::try_from(size).map_err(|_| 8u32)?;
    if size > MAX_BUFFER { return Err(8); }
    image.win32.shell.task_blocks.try_reserve(1).map_err(|_| 8u32)?;
    let pointer = allocate(image, 0, size.max(1) as u64, READ | WRITE)?;
    image.win32.shell.task_blocks.push((pointer, size));
    Ok(pointer)
}

fn task_free(image: &mut GuestImage, pointer: u64) -> Result<(), ApiError> {
    if pointer == 0 { return Ok(()); }
    let index = image.win32.shell.task_blocks.iter().position(|b| b.0 == pointer)
        .ok_or(ApiError::MemoryFault)?;
    free(image, pointer).map_err(|_| ApiError::MemoryFault)?;
    image.win32.shell.task_blocks.remove(index);
    Ok(())
}

fn profile_path(image: &mut GuestImage, variable: &str, verify: bool, services: &mut dyn GuestServices)
    -> Result<Result<Vec<u16>, u64>, ApiError> {
    let name: Vec<u16> = variable.encode_utf16().collect();
    let Some(path) = environment::lookup(image, &name)? else { return Ok(Err(0x80070002)); };
    if path.len() < 3 || path[1..3] != [b':' as u16, b'\\' as u16] || path.contains(&0) {
        return Ok(Err(0x80070003));
    }
    if verify {
        let text = String::from_utf16(&path).map_err(|_| ApiError::MemoryFault)?;
        let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
        if let Err(status) = services.directory_entries(abi, &text) {
            return Ok(Err(0x80070000 | u64::from(status_error(status))));
        }
    }
    Ok(Ok(path))
}

fn known_folder(image: &mut GuestImage, a: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
    put(&mut image.memory, a[3], 0, width).map_err(|_| ApiError::MemoryFault)?;
    let guid = read(&mut image.memory, a[0], 16).map_err(|_| ApiError::MemoryFault)?;
    // GUID fields Data1/Data2/Data3 are little endian in Windows memory.
    let variable = match guid.as_slice() {
        [0xdb,0x85,0xb6,0x3e,0xf9,0x65,0xf6,0x4c,0xa0,0x3a,0xe3,0xef,0x65,0x72,0x9f,0x3d] => "APPDATA",
        [0x85,0x27,0xb3,0xf1,0xba,0x6f,0xcf,0x4f,0x9d,0x55,0x7b,0x8e,0x7f,0x15,0x70,0x91] => "LOCALAPPDATA",
        [0x8f,0x85,0x6c,0x5e,0x22,0x0e,0x60,0x47,0x9a,0xfe,0xea,0x33,0x17,0xb6,0x71,0x73] => "USERPROFILE",
        _ => return Ok(0x80070057), // E_INVALIDARG: unknown FOLDERID
    };
    // Only current process token and KF_FLAG_DONT_VERIFY are available.
    // CREATE, DEFAULT_PATH, impersonation and redirected folders need the
    // persistent user hive/token backend, rather than an invented path.
    if a[2] != 0 || a[1] as u32 & !0x4000 != 0 { return Ok(0x80004001); }
    let path = match profile_path(image, variable, a[1] as u32 & 0x4000 == 0, services)? {
        Ok(path) => path,
        Err(hr) => return Ok(hr),
    };
    let bytes: Vec<u8> = path.into_iter().chain([0]).flat_map(u16::to_le_bytes).collect();
    copy_to_task_block(image, &bytes, a[3], width)
}

fn copy_to_task_block(image: &mut GuestImage, bytes: &[u8], output: u64, width: usize) -> Result<u64, ApiError> {
    let pointer = match task_alloc(image, bytes.len() as u64) {
        Ok(pointer) => pointer,
        Err(_) => return Ok(0x8007000e),
    };
    if !image.memory.write_virt(pointer, bytes) || put(&mut image.memory, output, pointer, width).is_err() {
        task_free(image, pointer)?;
        return Err(ApiError::MemoryFault);
    }
    Ok(0)
}

fn folder_path(image: &mut GuestImage, a: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    put(&mut image.memory, a[4], 0, 2).map_err(|_| ApiError::MemoryFault)?;
    let folder = a[1] as u32;
    if a[2] != 0 || a[3] as u32 != 0 || folder & !0x40ff != 0 { return Ok(0x80004001); }
    let variable = match folder & 0xff {
        0x1a => "APPDATA",
        0x1c => "LOCALAPPDATA",
        0x28 => "USERPROFILE",
        _ => return Ok(0x80070057),
    };
    let path = match profile_path(image, variable, folder & 0x4000 == 0, services)? {
        Ok(path) => path,
        Err(hr) => return Ok(hr),
    };
    if path.len() >= 260 { return Ok(0x800700ce); }
    let bytes: Vec<u8> = path.into_iter().chain([0]).flat_map(u16::to_le_bytes).collect();
    if !image.memory.probe_write(a[4], bytes.len()) || !image.memory.write_virt(a[4], &bytes) {
        return Err(ApiError::MemoryFault);
    }
    Ok(0)
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    match name {
        "SHGetKnownFolderPath" => known_folder(image, a, services),
        "SHGetFolderPathW" => folder_path(image, a, services),
        "PathIsFileSpecW" => {
            if a[0] == 0 { return Ok(0); }
            let bytes = wide_z_bytes(&mut image.memory, a[0])?;
            Ok(u64::from(!bytes.chunks_exact(2).any(|unit| matches!(unit, [b':' | b'\\', 0]))))
        }
        "SHStrDupW" => {
            let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
            put(&mut image.memory, a[1], 0, width).map_err(|_| ApiError::MemoryFault)?;
            if a[0] == 0 { return Ok(0x80070057); }
            let bytes = wide_z_bytes(&mut image.memory, a[0])?;
            copy_to_task_block(image, &bytes, a[1], width)
        }
        "CoTaskMemAlloc" => Ok(task_alloc(image, a[0]).unwrap_or(0)),
        "CoTaskMemFree" => { task_free(image, a[0])?; Ok(0) }
        "CoTaskMemRealloc" => {
            if a[0] == 0 { return Ok(task_alloc(image, a[1]).unwrap_or(0)); }
            let old = image.win32.shell.task_blocks.iter().find(|b| b.0 == a[0])
                .map(|b| b.1).ok_or(ApiError::MemoryFault)?;
            if a[1] == 0 { task_free(image, a[0])?; return Ok(0); }
            let pointer = match task_alloc(image, a[1]) { Ok(p) => p, Err(_) => return Ok(0) };
            let length = old.min(a[1] as usize);
            let bytes = read(&mut image.memory, a[0], length).map_err(|_| ApiError::MemoryFault)?;
            if !image.memory.write_virt(pointer, &bytes) {
                task_free(image, pointer)?;
                return Err(ApiError::MemoryFault);
            }
            task_free(image, a[0])?;
            Ok(pointer)
        }
        _ => Err(ApiError::Unsupported),
    }
}

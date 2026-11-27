//! Checked shell paths and COM task memory in the isolated guest address space.
//! Current-user profile folders use this process's provisioned environment.
//! Registry folder redirection and other tokens are unsupported. Path and
//! special-folder HRESULTs never change Win32 LastError.
use super::*;

pub(super) const MAX_PATH: usize = 260;
const PIDL_TAG: [u8; 4] = *b"NT61";
const PIDL_HEADER: usize = 2 + PIDL_TAG.len();
const PIDL_MAX_ITEM: usize = PIDL_HEADER + (MAX_PATH - 1) * 2;
const CSIDL_FLAG_CREATE: u32 = 0x8000;

#[derive(Default)]
pub(super) struct State {
    task_blocks: Vec<(u64, usize)>,
}

pub(super) fn task_alloc(image: &mut GuestImage, size: u64) -> Result<u64, u32> {
    let size = usize::try_from(size).map_err(|_| 8u32)?;
    if size > MAX_BUFFER { return Err(8); }
    image.win32.shell.task_blocks.try_reserve(1).map_err(|_| 8u32)?;
    let pointer = allocate(image, 0, size.max(1) as u64, READ | WRITE)?;
    image.win32.shell.task_blocks.push((pointer, size));
    Ok(pointer)
}

pub(super) fn task_free(image: &mut GuestImage, pointer: u64) -> Result<(), ApiError> {
    if pointer == 0 { return Ok(()); }
    let index = image.win32.shell.task_blocks.iter().position(|b| b.0 == pointer)
        .ok_or(ApiError::MemoryFault)?;
    free(image, pointer).map_err(|_| ApiError::MemoryFault)?;
    image.win32.shell.task_blocks.remove(index);
    Ok(())
}

fn file_info(image: &mut GuestImage, args: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    const USE_FILE_ATTRIBUTES: u32 = 0x10;
    const DISPLAY_NAME: u32 = 0x200;
    let flags = args[4] as u32;
    if args[0] == 0 || args[2] == 0 {
        return Ok(0);
    }
    if flags & !(USE_FILE_ATTRIBUTES | DISPLAY_NAME) != 0 {
        return Err(120u32.into());
    }
    let (abi, width) = if image.thread.context.mode == DecodeMode::X86 {
        (GuestAbi::X86_32, 4)
    } else {
        (GuestAbi::X86_64, 8)
    };
    let input = string(&mut image.memory, args[0], true)?;
    let output = args[2];
    let attributes = output.checked_add(width as u64 + 4).ok_or(NOACCESS)?;
    let display = attributes.checked_add(4).ok_or(NOACCESS)?;
    let type_name = display.checked_add((2 * MAX_PATH) as u64).ok_or(NOACCESS)?;
    if ![(output, width), (display, 2), (type_name, 2)].into_iter()
        .all(|(address, length)| image.memory.probe_write(address, length))
    {
        return Err(NOACCESS.into());
    }
    put(&mut image.memory, output, 0, width)?;
    put(&mut image.memory, display, 0, 2)?;
    put(&mut image.memory, type_name, 0, 2)?;
    if input.is_empty() && flags == 0 {
        error(image, INVALID_PARAMETER);
        return Ok(1);
    }
    if input.is_empty() || input.chars().any(|character| character < ' ' || "<>\"|?*".contains(character)) {
        return Err(123u32.into());
    }
    let path = directory::path(image, &input)?;
    if flags & USE_FILE_ATTRIBUTES == 0 {
        services.file_attributes(abi, &path).map_err(status_error)?;
    }
    if flags & DISPLAY_NAME != 0 {
        let name = path.trim_end_matches('\\').rsplit('\\').next().unwrap_or(&path);
        let units = name.encode_utf16().count();
        if units >= MAX_PATH {
            return Err(206u32.into());
        }
        if !image.memory.probe_write(attributes, 4 + (units + 1) * 2) {
            return Err(NOACCESS.into());
        }
        write_string(&mut image.memory, display, name, true)?;
        put(&mut image.memory, attributes, 0, 4)?;
    }
    Ok(1)
}

fn profile_path(image: &mut GuestImage, variable: &str) -> Result<Result<Vec<u16>, u64>, ApiError> {
    let name: Vec<u16> = variable.encode_utf16().collect();
    let Some(path) = environment::lookup(image, &name)? else { return Ok(Err(0x80070002)); };
    if path.len() < 3 || path[1..3] != [b':' as u16, b'\\' as u16] || path.contains(&0) {
        return Ok(Err(0x80070003));
    }
    Ok(Ok(path))
}

fn verify_directory(image: &GuestImage, path: &[u16], services: &mut dyn GuestServices)
    -> Result<Result<(), u32>, ApiError> {
    let text = String::from_utf16(path).map_err(|_| ApiError::MemoryFault)?;
    let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
    Ok(services.directory_entries(abi, &text).map(|_| ()))
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
    let path = match profile_path(image, variable)? {
        Ok(path) => path,
        Err(hr) => return Ok(hr),
    };
    if a[1] as u32 & 0x4000 == 0 {
        if let Err(status) = verify_directory(image, &path, services)? {
            return Ok(0x80070000 | u64::from(status_error(status)));
        }
    }
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
    let path = match profile_path(image, variable)? {
        Ok(path) => path,
        Err(hr) => return Ok(hr),
    };
    if folder & 0x4000 == 0 {
        if let Err(status) = verify_directory(image, &path, services)? {
            return Ok(0x80070000 | u64::from(status_error(status)));
        }
    }
    if path.len() >= MAX_PATH { return Ok(0x800700ce); }
    let bytes: Vec<u8> = path.into_iter().chain([0]).flat_map(u16::to_le_bytes).collect();
    if !image.memory.probe_write(a[4], bytes.len()) || !image.memory.write_virt(a[4], &bytes) {
        return Err(ApiError::MemoryFault);
    }
    Ok(0)
}

fn special_folder_path(image: &mut GuestImage, folder: u32) -> Result<Result<Vec<u16>, u64>, ApiError> {
    let (variable, suffix) = match folder {
        0x02 => ("APPDATA", "\\Microsoft\\Windows\\Start Menu\\Programs"),
        0x0b => ("APPDATA", "\\Microsoft\\Windows\\Start Menu"),
        0x00 | 0x10 => ("USERPROFILE", "\\Desktop"),
        0x16 => ("ProgramData", "\\Microsoft\\Windows\\Start Menu"),
        0x17 => ("ProgramData", "\\Microsoft\\Windows\\Start Menu\\Programs"),
        0x19 => return Ok(Ok("C:\\Users\\Public\\Desktop".encode_utf16().collect())),
        0x1a => ("APPDATA", ""),
        0x1c => ("LOCALAPPDATA", ""),
        0x23 => ("ProgramData", ""),
        0x26 if image.thread.context.mode != DecodeMode::X86 => {
            return Ok(Ok("C:\\Program Files".encode_utf16().collect()));
        }
        0x26 | 0x2a => return Ok(Ok("C:\\Program Files (x86)".encode_utf16().collect())),
        0x28 => ("USERPROFILE", ""),
        _ => return Ok(Err(0x80070057)),
    };
    let mut path = match profile_path(image, variable)? {
        Ok(path) => path,
        Err(hr) => return Ok(Err(hr)),
    };
    if !suffix.is_empty() {
        while path.last() == Some(&u16::from(b'\\')) {
            path.pop();
        }
        path.extend(suffix.encode_utf16());
    }
    Ok(Ok(path))
}

fn special_folder_location(image: &mut GuestImage, folder: u32, output: u64, services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    if output == 0 { return Ok(0x80070057); }
    let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
    put(&mut image.memory, output, 0, width).map_err(|_| ApiError::MemoryFault)?;
    if folder & !(0xff | CSIDL_FLAG_CREATE) != 0 { return Ok(0x80070057); }
    let csidl = folder & 0xff;
    if csidl == 0x00 { return copy_to_task_block(image, &[0, 0], output, width); }
    let path = if csidl == 0x11 {
        Vec::new()
    } else {
        let path = match special_folder_path(image, csidl)? {
            Ok(path) => path,
            Err(hr) => return Ok(hr),
        };
        if path.len() >= MAX_PATH { return Ok(0x800700ce); }
        if let Err(status) = verify_directory(image, &path, services)? {
            if folder & CSIDL_FLAG_CREATE != 0 && matches!(status, 0xc0000034 | 0xc000003a) {
                return Ok(0x80004001);
            }
            return Ok(0x80070000 | u64::from(status_error(status)));
        }
        path
    };
    let bytes = path_pidl(&path);
    copy_to_task_block(image, &bytes, output, width)
}

pub(super) fn path_pidl(path: &[u16]) -> Vec<u8> {
    let item_size = (PIDL_HEADER + path.len() * 2) as u16;
    let mut bytes = Vec::with_capacity(usize::from(item_size) + 2);
    bytes.extend(item_size.to_le_bytes());
    bytes.extend(PIDL_TAG);
    bytes.extend(path.iter().copied().flat_map(u16::to_le_bytes));
    bytes.extend([0, 0]);
    bytes
}

pub(super) fn pidl_path(image: &mut GuestImage, pidl: u64) -> Result<Option<Vec<u16>>, ApiError> {
    let mut size = [0u8; 2];
    let path = if image.memory.read_virt(pidl, &mut size) != size.len() {
        None
    } else if size == [0, 0] {
        match special_folder_path(image, 0x00)? {
            Ok(path) if path.len() < MAX_PATH => Some(path),
            _ => None,
        }
    } else {
        let size = usize::from(u16::from_le_bytes(size));
        if size <= PIDL_HEADER || size > PIDL_MAX_ITEM || size % 2 != 0 {
            None
        } else {
            read(&mut image.memory, pidl, size + 2).ok().and_then(|bytes| {
                if bytes[2..PIDL_HEADER] != PIDL_TAG || bytes[size..] != [0, 0] {
                    return None;
                }
                let path: Vec<u16> = bytes[PIDL_HEADER..size].chunks_exact(2)
                    .map(|unit| u16::from_le_bytes([unit[0], unit[1]])).collect();
                if path.len() < 3 || path[1..3] != [u16::from(b':'), u16::from(b'\\')]
                    || path.contains(&0) || String::from_utf16(&path).is_err()
                {
                    return None;
                }
                Some(path)
            })
        }
    };
    Ok(path)
}

fn path_from_id_list(image: &mut GuestImage, pidl: u64, output: u64) -> Result<u64, ApiError> {
    if output == 0 { return Ok(0); }
    let path = pidl_path(image, pidl)?;
    let success = u64::from(path.is_some());
    let bytes: Vec<u8> = path.unwrap_or_default().into_iter().chain([0]).flat_map(u16::to_le_bytes).collect();
    if !image.memory.write_virt(output, &bytes) { return Err(ApiError::MemoryFault); }
    Ok(success)
}

fn copy_paths(image: &mut GuestImage, pointer: u64) -> Result<Vec<String>, ApiError> {
    if pointer == 0 { return Err(INVALID_PARAMETER.into()); }
    let mut paths = Vec::new();
    let mut path = Vec::new();
    let mut pointer = pointer;
    loop {
        let unit = word(&mut image.memory, pointer, 2)? as u16;
        if unit == 0 {
            if path.is_empty() {
                return if paths.is_empty() { Err(INVALID_PARAMETER.into()) } else { Ok(paths) };
            }
            let path_string = String::from_utf16(&path).map_err(|_| 1113u32)?;
            paths.push(directory::path(image, &path_string)?);
            path.clear();
        } else {
            if path.len() >= MAX_PATH - 1 || paths.len() >= 64 { return Err(120.into()); }
            path.push(unit);
        }
        pointer = pointer.checked_add(2).ok_or(NOACCESS)?;
    }
}

fn copy_operation(image: &mut GuestImage, operation: u64, services: &mut dyn GuestServices) -> Result<(), ApiError> {
    const FO_COPY: u32 = 2;
    const FOF_NOCONFIRMATION: u16 = 0x10;
    const FOF_FILESONLY: u16 = 0x80;
    const FOF_NO_UI_FLAGS: u16 = 0x0604;
    const FOF_NOCOPYSECURITYATTRIBS: u16 = 0x0800;
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let (abi, width, source_offset) = if x86 { (GuestAbi::X86_32, 4, 8) } else { (GuestAbi::X86_64, 8, 16) };
    let layout = read(&mut image.memory, operation, if x86 { 30 } else { 56 })?;
    let field = |offset: usize, length: usize| {
        let mut value = [0; 8];
        value[..length].copy_from_slice(&layout[offset..offset + length]);
        u64::from_le_bytes(value)
    };
    let flags_offset = source_offset + width * 2;
    let aborted = operation.checked_add((flags_offset + if x86 { 2 } else { 4 }) as u64).ok_or(NOACCESS)?;
    put(&mut image.memory, aborted, 0, 4)?;
    let function = field(width, 4) as u32;
    let flags = field(flags_offset, 2) as u16;
    if function != FO_COPY || flags & !(FOF_NOCONFIRMATION | FOF_FILESONLY | FOF_NO_UI_FLAGS | FOF_NOCOPYSECURITYATTRIBS) != 0 {
        return Err(120.into());
    }
    let sources = copy_paths(image, field(source_offset, width))?;
    let destinations = copy_paths(image, field(source_offset + width, width))?;
    let [destination] = destinations.as_slice() else { return Err(120.into()); };
    if destination.contains(['*', '?']) { return Err(123.into()); }
    let destination_is_directory = match services.file_attributes(abi, destination) {
        Ok(attributes) if attributes & 0x400 == 0 => attributes & 0x10 != 0,
        Ok(_) => return Err(120.into()),
        Err(0xc0000034 | 0xc000000f) => false,
        Err(status) => return Err(status_error(status).into()),
    };
    if sources.len() > 1 && !destination_is_directory { return Err(0x72.into()); }
    let mut copies = Vec::new();
    for source in sources {
        if source.contains(['*', '?']) {
            let (parent, pattern) = source.rsplit_once('\\').ok_or(123u32)?;
            if !destination_is_directory || flags & FOF_FILESONLY == 0 || parent.contains(['*', '?']) {
                return Err(120.into());
            }
            let pattern: Vec<_> = pattern.encode_utf16().collect();
            let parent_path = if parent.ends_with(':') { &source[..parent.len() + 1] } else { parent };
            for entry in services.directory_entries(abi, parent_path).map_err(status_error)? {
                if entry.attributes & 0x10 == 0 && directory::matches(&pattern, &entry.name)? {
                    let leaf = String::from_utf16(&entry.name).map_err(|_| 1113u32)?;
                    copies.push(alloc::format!("{parent}\\{leaf}"));
                    if copies.len() > 64 { return Err(120.into()); }
                }
            }
        } else {
            copies.push(source);
            if copies.len() > 64 { return Err(120.into()); }
        }
    }
    for source in copies {
        let attributes = services.file_attributes(abi, &source).map_err(status_error)?;
        if attributes & (0x10 | 0x400) != 0 { return Err(120.into()); }
        let target = if destination_is_directory {
            let leaf = source.rsplit('\\').next().filter(|leaf| !leaf.is_empty()).ok_or(123u32)?;
            alloc::format!("{}\\{leaf}", destination.trim_end_matches('\\'))
        } else {
            destination.clone()
        };
        let result = services.copy_file(abi, &source, &target, flags & FOF_NOCONFIRMATION != 0);
        match result {
            Ok(()) => {},
            Err(0xc000_00d4) => return Err(0x71.into()),
            Err(status) => return Err(status_error(status).into()),
        }
    }
    Ok(())
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    match name {
        "SHFileOperationW" => match copy_operation(image, a[0], services) {
            Ok(()) => Ok(0),
            Err(ApiError::Windows(code)) => Err(ApiError::Hresult(code, "Shell file copy")),
            Err(error) => Err(error),
        },
        "SHGetKnownFolderPath" => known_folder(image, a, services),
        "SHGetFolderPathW" => folder_path(image, a, services),
        "SHGetSpecialFolderLocation" => special_folder_location(image, a[1] as u32, a[2], services),
        "SHGetPathFromIDListW" => path_from_id_list(image, a[0], a[1]),
        "SHGetFileInfoW" => file_info(image, a, services),
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

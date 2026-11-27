//! Process-owned FindFirst/FindNext cursors over copied mounted-volume data.
use super::*;

struct Search { handle: u64, entries: Vec<nt61::DirectoryEntry>, position: usize }
#[derive(Default)]
pub(super) struct State { searches: Vec<Search>, next: u32 }

pub(super) fn path(image: &mut GuestImage, input: &str) -> Result<String, ApiError> {
    let input = input.replace('/', "\\");
    if input.starts_with("\\\\") || input.starts_with("\\?\\") { return Err(120u32.into()); }
    if input.as_bytes().get(1..3) == Some(b":\\") { return Ok(input); }
    // Drive-relative and rooted paths need per-drive current-directory state.
    if input.contains(':') || input.starts_with('\\') { return Err(120u32.into()); }
    let directory = current_directory(image)?;
    Ok(alloc::format!("{}\\{input}",directory.trim_end_matches('\\')))
}

fn current_directory(image: &mut GuestImage) -> Result<String, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 {4} else {8};
    let parameters = word(&mut image.memory,image.peb+if x86 {0x10}else{0x20},width)?;
    let descriptor = parameters.checked_add(if x86 {0x24}else{0x38}).ok_or(NOACCESS)?;
    let len = word(&mut image.memory,descriptor,2)? as usize;
    if len == 0 || len % 2 != 0 { return Err(INVALID_PARAMETER.into()); }
    let pointer = word(&mut image.memory,descriptor+width as u64,width)?;
    let bytes = read(&mut image.memory,pointer,len)?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|b|u16::from_le_bytes([b[0],b[1]])).collect();
    String::from_utf16(&units).map_err(|_|1113u32.into())
}

// The bounded initial matcher accepts literal names, '*' and '*.*'. Other
// DOS expressions require the volume's $UpCase/wildcard implementation.
pub(super) fn matches(pattern: &[u16], name: &[u16]) -> Result<bool, ApiError> {
    let star=b'*' as u16;
    let pattern=if pattern==[star,b'.' as u16,star] {&[b'*' as u16][..]} else {pattern};
    if pattern.iter().any(|v|matches!(*v,0x3f|0x3c|0x3e|0x22)) { return Err(120u32.into()); }
    let fold=|input:&[u16]| -> Result<Vec<u16>,ApiError> {
        input.iter().map(|&unit|nls::map_case(unit,true)).collect()
    };
    let pattern=fold(pattern)?;
    let name=fold(name)?;
    let mut row=vec![false;name.len()+1]; row[0]=true;
    for token in pattern {
        let mut next=vec![false;name.len()+1];
        if token==star {
            next[0]=row[0];
            for i in 1..next.len() {next[i]=row[i]||next[i-1];}
        } else {
            for i in 1..next.len() {next[i]=row[i-1]&&name[i-1]==token;}
        }
        row=next;
    }
    Ok(row[name.len()])
}

fn input(image: &mut GuestImage, address: u64, wide: bool) -> Result<String, ApiError> {
    if wide { return Ok(string(&mut image.memory, address, true)?); }
    let mut units = Vec::new();
    for index in 0..32768 {
        let pointer = address.checked_add(index).ok_or(NOACCESS)?;
        let byte = word(&mut image.memory, pointer, 1)? as u8;
        if byte == 0 { return String::from_utf16(&units).map_err(|_| 1113u32.into()); }
        units.push(nls::decode_ansi(byte));
    }
    Err(INVALID_PARAMETER.into())
}

pub(super) fn full_path_name(image: &mut GuestImage, args: &[u64], wide: bool) -> Result<u64, ApiError> {
    let path = input(image, args[0], wide)?.replace('/', "\\");
    if path.is_empty() { return Err(123u32.into()); }
    if path.starts_with("\\\\") { return Err(120u32.into()); }
    let absolute = |path: &str| path.as_bytes().get(1..3) == Some(b":\\")
        && path.as_bytes()[0].is_ascii_alphabetic();
    let joined = if absolute(&path) {
        path
    } else {
        let cwd = current_directory(image)?;
        if !absolute(&cwd) { return Err(120u32.into()); }
        if let Some(relative) = path.strip_prefix('\\') {
            alloc::format!("{}\\{relative}", &cwd[..2])
        } else {
            let relative = if path.as_bytes().get(1) == Some(&b':') {
                let drive = path.get(..2).ok_or(123u32)?;
                if !drive.eq_ignore_ascii_case(&cwd[..2]) { return Err(120u32.into()); }
                &path[2..]
            } else { &path };
            if relative.is_empty() { cwd }
            else { alloc::format!("{}\\{relative}", cwd.trim_end_matches('\\')) }
        }
    };
    let trailing = joined.ends_with('\\');
    let mut components = Vec::new();
    for component in joined[3..].split('\\') {
        match component {
            "" | "." => {},
            ".." => { components.pop(); },
            part if part.ends_with([' ', '.']) || part.contains(':') => return Err(120u32.into()),
            part => components.push(part),
        }
    }
    let mut resolved = alloc::format!("{}{}", &joined[..3], components.join("\\"));
    let leaf = if trailing || components.is_empty() { None }
        else { Some(&resolved[..resolved.rfind('\\').ok_or(123u32)? + 1]) };
    let unit = if wide { 2 } else { 1 };
    let leaf_units = leaf.map(|prefix| prefix.encode_utf16().count());
    if trailing && !resolved.ends_with('\\') { resolved.push('\\'); }
    let units = resolved.encode_utf16().count() + 1;
    if (args[1] as u32 as usize) < units { return Ok(units as u64); }
    let bytes = if wide {
        resolved.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<_>>()
    } else {
        resolved.encode_utf16().map(|character| nls::encode_ansi(character).ok_or(1113u32))
            .chain([Ok(0)]).collect::<Result<Vec<_>, _>>()?
    };
    let ptr_width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
    let output = args[2];
    let file_part = args[3];
    let leaf_address = match leaf_units {
        Some(units) => output.checked_add((units * unit) as u64).ok_or(NOACCESS)?,
        None => 0,
    };
    if !image.memory.probe_write(output, bytes.len())
        || (file_part != 0 && !image.memory.probe_write(file_part, ptr_width)) {
        return Err(NOACCESS.into());
    }
    if !image.memory.write_virt(output, &bytes) { return Err(NOACCESS.into()); }
    if file_part != 0 { put(&mut image.memory, file_part, leaf_address, ptr_width)?; }
    Ok((units - 1) as u64)
}

fn attribute_data(attributes: u32, times: [u64; 3], size: u64) -> [u8; 36] {
    let mut bytes = [0; 36];
    bytes[..4].copy_from_slice(&attributes.to_le_bytes());
    for (field, time) in bytes[4..28].chunks_exact_mut(8).zip(times) {
        field.copy_from_slice(&time.to_le_bytes());
    }
    bytes[28..32].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
    bytes[32..36].copy_from_slice(&(size as u32).to_le_bytes());
    bytes
}

fn output(image: &mut GuestImage, address: u64, entry: &nt61::DirectoryEntry, wide: bool) -> Result<(), ApiError> {
    if entry.name.len()>259 || entry.name.contains(&0) { return Err(206u32.into()); }
    let mut bytes=[0u8;592]; // WIN32_FIND_DATAW is identical in both guest ABIs.
    bytes[..36].copy_from_slice(&attribute_data(entry.attributes,
        [entry.creation_time, entry.access_time, entry.write_time], entry.size));
    // Reparse tags and alternate (8.3) names are not captured by this backend.
    if entry.attributes & 0x400 != 0 { return Err(120u32.into()); }
    let length = if wide {
        for (index, unit) in entry.name.iter().enumerate() {
            bytes[44 + index * 2..46 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        592
    } else {
        for (index, unit) in entry.name.iter().enumerate() {
            bytes[44 + index] = nls::encode_ansi(*unit).ok_or(1113u32)?;
        }
        320
    };
    if !image.memory.write_virt(address, &bytes[..length]) {
        return Err(NOACCESS.into());
    }
    Ok(())
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64], services: &mut dyn GuestServices) -> Result<u64,ApiError> {
    let wide = !name.ends_with('A');
    if matches!(name, "MoveFileA" | "MoveFileW") {
        let move_path = |image: &mut GuestImage, address| -> Result<String, ApiError> {
            let value = input(image, address, wide)?;
            if value.is_empty() || value.ends_with(['\\', '/']) || value.chars().enumerate().any(|(index, character)| {
                character < ' ' || "<>\"|?*".contains(character) || (character == ':' && index != 1)
            }) {
                return Err(123u32.into());
            }
            let full = path(image, &value)?;
            if !full.as_bytes().get(..3).is_some_and(|drive| drive.eq_ignore_ascii_case(b"C:\\")) {
                return Err(120u32.into());
            }
            Ok(full)
        };
        let source = move_path(image, a[0])?;
        let destination = move_path(image, a[1])?;
        let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
        services.move_file(abi, &source, &destination).map_err(status_error)?;
        return Ok(1);
    }
    if matches!(name, "GetFileAttributesA" | "GetFileAttributesW" | "GetFileAttributesExA" | "GetFileAttributesExW"
        | "SetFileAttributesW" | "DeleteFileA" | "DeleteFileW") {
        let input = input(image, a[0], wide)?;
        if input.is_empty() || input.chars().any(|character| character < ' ' || "<>\"|?*".contains(character)) {
            return Err(123u32.into());
        }
        let delete = matches!(name, "DeleteFileA" | "DeleteFileW");
        if delete && input.bytes().enumerate().any(|(offset, byte)| {
            byte == b':' && (offset != 1 || !input.as_bytes()[0].is_ascii_alphabetic())
        }) {
            return Err(123u32.into());
        }
        let full = path(image, &input)?;
        let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
        if matches!(name, "GetFileAttributesExA" | "GetFileAttributesExW") {
            if a[1] as u32 != 0 { return Err(INVALID_PARAMETER.into()); }
            if !image.memory.probe_write(a[2], 36) { return Err(NOACCESS.into()); }
            let info = services.file_basic_information(abi, &full).map_err(status_error)?;
            if !image.memory.write_virt(a[2], &attribute_data(info.attributes, info.times, info.size)) {
                return Err(NOACCESS.into());
            }
            return Ok(1);
        }
        if delete {
            services.delete_file(abi, &full).map_err(status_error)?;
            return Ok(1);
        }
        if name == "SetFileAttributesW" {
            services.set_file_attributes(abi, &full, a[1] as u32).map_err(status_error)?;
            return Ok(1);
        }
        return services.file_attributes(abi, &full).map(u64::from)
            .map_err(|status| status_error(status).into());
    }
    if matches!(name, "CreateDirectoryA" | "CreateDirectoryW" | "RemoveDirectoryA" | "RemoveDirectoryW") {
        let remove = matches!(name, "RemoveDirectoryA" | "RemoveDirectoryW");
        // Explicit security descriptors and inheritable handles require the
        // native security-aware object creation path, which is not exposed by
        // this bounded mounted-volume backend.
        if !remove && a[1] != 0 { return Err(120u32.into()); }
        let input = input(image, a[0], wide)?;
        if input.is_empty() { return Err(123u32.into()); }
        let mut full = path(image, &input)?;
        if remove && full.len() > 3 && full.ends_with('\\') { full.pop(); }
        let tail = full.get(3..).filter(|_| full.as_bytes().get(1..3) == Some(b":\\")
            && full.as_bytes()[0].is_ascii_alphabetic()).ok_or(123u32)?;
        if remove && tail.is_empty() { return Err(5u32.into()); }
        if tail.is_empty() || tail.split('\\').any(|part|
            part.is_empty() || part == "." || part == ".." || part.ends_with([' ', '.'])
            || part.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c))) {
            return Err(123u32.into());
        }
        let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
        if remove {
            services.remove_directory(abi, &full).map_err(status_error)?;
            return Ok(1);
        }
        services.create_directory(abi, &full).map_err(|status| {
            // CreateDirectory reports ERROR_ALREADY_EXISTS, rather than
            // CreateFile(CREATE_NEW)'s ERROR_FILE_EXISTS.
            if status == 0xc0000035 { ApiError::Windows(183) } else { status_error(status).into() }
        })?;
        return Ok(1);
    }
    if name=="FindClose" {
        let index=image.win32.directory.searches.iter().position(|s|s.handle==a[0]).ok_or(6u32)?;
        image.win32.directory.searches.remove(index);return Ok(1);
    }
    if matches!(name, "FindNextFileA" | "FindNextFileW") {
        let index=image.win32.directory.searches.iter().position(|s|s.handle==a[0]).ok_or(6u32)?;
        let search=&image.win32.directory.searches[index];
        let entry=search.entries.get(search.position).ok_or(18u32)?.clone();
        output(image, a[1], &entry, wide)?;
        image.win32.directory.searches[index].position+=1;
        return Ok(1);
    }
    let extended=name=="FindFirstFileExW";
    // FindExInfoBasic omits the alternate filename, as does this backend.
    // Standard requests need short-name capture before full support.
    let output_address=if extended {
        if a[1] as u32!=1 || a[3] as u32>1 || a[4]!=0 || a[5] as u32 & !2 != 0 {return Err(120u32.into());}
        a[2]
    } else {a[1]};
    if !image.memory.probe_write(output_address, if wide { 592 } else { 320 }) {return Err(NOACCESS.into());}
    let input = input(image, a[0], wide)?;
    let full=path(image,&input)?;
    let (parent,pattern)=full.rsplit_once('\\').ok_or(INVALID_PARAMETER)?;
    if pattern.is_empty() {return Err(2u32.into());}
    if parent.contains(['*','?']) {return Err(123u32.into());}
    let parent=if parent.ends_with(':') {alloc::format!("{parent}\\")} else {String::from(parent)};
    let pattern:Vec<u16>=pattern.encode_utf16().collect();
    // Validate the pattern even if the directory has no entries.
    let _=matches(&pattern,&[])?;
    let abi=if image.thread.context.mode==DecodeMode::X86 {GuestAbi::X86_32}else{GuestAbi::X86_64};
    let entries=services.directory_entries(abi,&parent).map_err(status_error)?;
    let mut selected=Vec::new();
    for entry in entries {
        if extended && a[3] as u32==1 && entry.attributes & 0x10==0 {continue;}
        if matches(&pattern,&entry.name)? {selected.push(entry);}
    }
    let first=selected.first().ok_or(2u32)?;
    if image.win32.directory.searches.len()>=64 {return Err(4u32.into());}
    let next=image.win32.directory.next.checked_add(1).filter(|v|*v<=0x00ff_ffff).ok_or(4u32)?;
    let handle=0x4700_0000|u64::from(next);
    output(image, output_address, first, wide)?;
    image.win32.directory.searches.push(Search{handle,entries:selected,position:1});
    image.win32.directory.next=next;
    Ok(handle)
}

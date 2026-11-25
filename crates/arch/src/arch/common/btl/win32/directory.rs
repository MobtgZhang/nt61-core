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
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 {4} else {8};
    let parameters = word(&mut image.memory,image.peb+if x86 {0x10}else{0x20},width)?;
    let descriptor = parameters.checked_add(if x86 {0x24}else{0x38}).ok_or(NOACCESS)?;
    let len = word(&mut image.memory,descriptor,2)? as usize;
    if len == 0 || len % 2 != 0 { return Err(INVALID_PARAMETER.into()); }
    let pointer = word(&mut image.memory,descriptor+width as u64,width)?;
    let bytes = read(&mut image.memory,pointer,len)?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|b|u16::from_le_bytes([b[0],b[1]])).collect();
    let directory = String::from_utf16(&units).map_err(|_|1113u32)?;
    Ok(alloc::format!("{}\\{input}",directory.trim_end_matches('\\')))
}

// The bounded initial matcher accepts literal names, '*' and '*.*'. Other
// DOS expressions require the volume's $UpCase/wildcard implementation.
fn matches(pattern: &[u16], name: &[u16]) -> Result<bool, ApiError> {
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

fn output(image: &mut GuestImage, address: u64, entry: &nt61::DirectoryEntry, wide: bool) -> Result<(), ApiError> {
    if entry.name.len()>259 || entry.name.contains(&0) { return Err(206u32.into()); }
    let mut bytes=[0u8;592]; // WIN32_FIND_DATAW is identical in both guest ABIs.
    bytes[..4].copy_from_slice(&entry.attributes.to_le_bytes());
    bytes[4..12].copy_from_slice(&entry.creation_time.to_le_bytes());
    bytes[12..20].copy_from_slice(&entry.access_time.to_le_bytes());
    bytes[20..28].copy_from_slice(&entry.write_time.to_le_bytes());
    bytes[28..32].copy_from_slice(&((entry.size>>32) as u32).to_le_bytes());
    bytes[32..36].copy_from_slice(&(entry.size as u32).to_le_bytes());
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
    if matches!(name, "GetFileAttributesA" | "GetFileAttributesW" | "SetFileAttributesW") {
        let input = input(image, a[0], wide)?;
        if input.is_empty() || input.chars().any(|character| character < ' ' || "<>\"|?*".contains(character)) {
            return Err(123u32.into());
        }
        let full = path(image, &input)?;
        let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
        if name == "SetFileAttributesW" {
            services.set_file_attributes(abi, &full, a[1] as u32).map_err(status_error)?;
            return Ok(1);
        }
        return services.file_attributes(abi, &full).map(u64::from)
            .map_err(|status| status_error(status).into());
    }
    if matches!(name, "CreateDirectoryA" | "CreateDirectoryW") {
        // Explicit security descriptors and inheritable handles require the
        // native security-aware object creation path, which is not exposed by
        // this bounded mounted-volume backend.
        if a[1] != 0 { return Err(120u32.into()); }
        let input = input(image, a[0], wide)?;
        if input.is_empty() { return Err(123u32.into()); }
        let full = path(image, &input)?;
        let tail = full.get(3..).filter(|_| full.as_bytes().get(1..3) == Some(b":\\")
            && full.as_bytes()[0].is_ascii_alphabetic()).ok_or(123u32)?;
        if tail.is_empty() || tail.split('\\').any(|part|
            part.is_empty() || part == "." || part == ".." || part.ends_with([' ', '.'])
            || part.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c))) {
            return Err(123u32.into());
        }
        let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
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

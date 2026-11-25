//! Win7 version comparisons for the isolated process's compatibility identity.
use super::*;

const OLD_VERSION: u32 = 1150;
const BAD_ARGUMENTS: u32 = 160;

struct VersionNode {
    end: usize,
    value: usize,
    length: usize,
    children: usize,
    text: bool,
}

fn node(bytes: &[u8], start: usize, bound: usize) -> Option<(VersionNode, &[u8])> {
    let header = bytes.get(start..start.checked_add(6)?)?;
    let size = usize::from(u16::from_le_bytes([header[0], header[1]]));
    let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
    let text = match &header[4..6] { [0, 0] => false, [1, 0] => true, _ => return None };
    let end = start.checked_add(size)?;
    if end > bound || size < 8 { return None; }
    let names = bytes.get(start + 6..end)?;
    let key_length = names.chunks_exact(2).position(|pair| pair == [0, 0])? * 2;
    let value = (start + 8 + key_length).next_multiple_of(4);
    let value_end = value + length * if text { 2 } else { 1 };
    if value_end > end && length != 0 { return None; }
    Some((VersionNode { end, value, length, children: value_end.next_multiple_of(4), text }, &names[..key_length]))
}

fn query(bytes: &[u8], path: &str) -> Option<(usize, usize)> {
    let (mut current, name) = node(bytes, 0, bytes.len())?;
    if !name.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .eq("VS_VERSION_INFO".encode_utf16()) { return None; }
    if !path.starts_with('\\') { return None; }
    for part in path.split('\\').filter(|part| !part.is_empty()) {
        let mut offset = current.children;
        let child = loop {
            let (candidate, key) = node(bytes, offset, current.end)?;
            let units = key.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
            let key = String::from_utf16(&units.collect::<Vec<_>>()).ok()?;
            if part.eq_ignore_ascii_case(&key) { break candidate; }
            offset = candidate.end.next_multiple_of(4);
        };
        current = child;
    }
    if current.length == 0 || current.value >= current.end { return None; }
    if current.text && bytes.get(current.value + current.length * 2 - 2..current.value + current.length * 2)? != [0, 0] {
        return None;
    }
    Some((current.value, current.length))
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, args: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    if name == "VerQueryValueW" {
        let path = string(&mut image.memory, args[1], true)?;
        let length = word(&mut image.memory, args[0], 2)? as usize;
        let bytes = read(&mut image.memory, args[0], length)?;
        let Some((offset, length)) = query(&bytes, &path) else { return Ok(0); };
        let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
        if !image.memory.probe_write(args[2], width) || !image.memory.probe_write(args[3], 4) {
            return Err(NOACCESS.into());
        }
        put(&mut image.memory, args[2], args[0].checked_add(offset as u64).ok_or(NOACCESS)?, width)?;
        put(&mut image.memory, args[3], length as u64, 4)?;
        return Ok(1);
    }
    let flags = args[0] as u32;
    if flags & !7 != 0 { return Err(INVALID_PARAMETER.into()); }
    let path = string(&mut image.memory, args[1], true)?;
    let file = services.read_library(&path, image.thread.context.mode).map_err(status_error)?;
    let language = image.win32.local.ui_languages.first().copied().unwrap_or(nls::DEFAULT_UI_LANGUAGE);
    let data = resources::file_version(&file, language)?;
    if name == "GetFileVersionInfoSizeExW" {
        if args[2] != 0 { put(&mut image.memory, args[2], 0, 4)?; }
        return Ok(data.len() as u64);
    }
    let length = data.len().min(args[3] as u32 as usize);
    if length == 0 { return Err(122u32.into()); }
    if !image.memory.probe_write(args[4], length) || !image.memory.write_virt(args[4], &data[..length]) {
        return Err(NOACCESS.into());
    }
    Ok(1)
}

fn compare(current: u64, requested: u64, condition: u8) -> Result<(), ApiError> {
    let matches = match condition {
        1 => current == requested,
        2 => current > requested,
        3 => current >= requested,
        4 => current < requested,
        5 => current <= requested,
        _ => false,
    };
    if matches { Ok(()) } else { Err(OLD_VERSION.into()) }
}

fn hierarchy_condition(inherited: &mut u8, requested: u8) -> u8 {
    // Lower-order fields may refine an equal condition or a comparison in
    // the same direction. Conflicting directions inherit the prior one.
    // A missing condition freezes inheritance for subsequent fields.
    let compatible = matches!((*inherited, requested),
        (1, 1..=5) | (2 | 3, 1..=3) | (4 | 5, 1 | 4 | 5));
    if *inherited == 0 || (*inherited == 1 && compatible) {
        *inherited = requested;
    }
    if compatible { return requested; }
    if requested == 0 { *inherited |= 16; }
    *inherited & 15
}

pub(super) fn verify(
    image: &mut GuestImage,
    pointer: u64,
    types: u32,
    mask: u64,
) -> Result<u64, ApiError> {
    if types == 0 || mask == 0 { return Err(BAD_ARGUMENTS.into()); }
    let condition = |bit: u32| ((mask >> (bit * 3)) & 7) as u8;
    let mut field = |offset: u64, width| {
        let address = pointer.checked_add(offset).ok_or(ApiError::MemoryFault)?;
        word(&mut image.memory, address, width).map_err(|_| ApiError::MemoryFault)
    };
    // Product, suite, platform and build are independent constraints. Match
    // the loader's order so invalid conditions and mismatches keep priority.
    if types & 0x80 != 0 { compare(1, field(282, 1)?, condition(7))?; }
    if types & 0x40 != 0 {
        let requested = field(280, 2)?;
        let matches = match condition(6) {
            6 => requested & 0x100 == requested,
            7 => requested == 0 || requested & 0x100 != 0,
            _ => return Err(BAD_ARGUMENTS.into()),
        };
        if !matches { return Err(OLD_VERSION.into()); }
    }
    if types & 8 != 0 { compare(2, field(16, 4)?, condition(3))?; }
    if types & 4 != 0 { compare(7601, field(12, 4)?, condition(2))?; }
    let mut inherited = 0;
    let mut result = Ok(());
    // Major, minor and service-pack fields form one ordered comparison;
    // a decisive earlier field suppresses subsequent version checks.
    for (bit, offset, width, current) in [(1,4,4,6), (0,8,4,1), (5,276,2,1), (4,278,2,0)] {
        if types & (1 << bit) == 0 { continue; }
        let requested = field(offset, width)?;
        let op = hierarchy_condition(&mut inherited, condition(bit));
        result = compare(current, requested, op);
        if current != requested || !(1..=5).contains(&op) { break; }
    }
    result?;
    Ok(1)
}

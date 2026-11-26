//! Checked snapshots of the isolated guest process's UTF-16 environment.
use super::*;

#[derive(Default)]
pub(super) struct State {
    blocks: Vec<(u64, bool)>,
}

pub(super) fn snapshot(image: &mut GuestImage) -> Result<Vec<u8>, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let parameters = word(&mut image.memory, image.peb + if x86 { 0x10 } else { 0x20 }, width)?;
    let slot = parameters.checked_add(if x86 { 0x48 } else { 0x80 }).ok_or(NOACCESS)?;
    let source = word(&mut image.memory, slot, width)?;
    capture(&mut image.memory, source, true)
}

pub(super) fn capture(memory: &mut dyn GuestMemory, source: u64, wide: bool) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    if source == 0 {
        bytes.extend_from_slice(&[0; 4]);
    } else {
        let mut previous_nul = false;
        loop {
            if bytes.len() >= MAX_BUFFER { return Err(INVALID_PARAMETER.into()); }
            let offset = if wide { bytes.len() } else { bytes.len() / 2 };
            let address = source.checked_add(offset as u64).ok_or(NOACCESS)?;
            let value = if wide {
                word(memory, address, 2)? as u16
            } else {
                let mut byte = [0];
                if memory.read_virt(address, &mut byte) != 1 { return Err(NOACCESS.into()); }
                nls::decode_ansi(byte[0])
            };
            bytes.extend_from_slice(&value.to_le_bytes());
            if previous_nul && value == 0 { break; }
            previous_nul = value == 0;
        }
    }
    Ok(bytes)
}

pub(super) fn get(image: &mut GuestImage, wide: bool) -> Result<u64, ApiError> {
    let mut bytes = snapshot(image)?;
    if !wide {
        bytes = bytes.chunks_exact(2).map(|unit| {
            let value = u16::from_le_bytes([unit[0], unit[1]]);
            u8::try_from(value).ok().filter(u8::is_ascii).ok_or(1113u32)
        }).collect::<Result<Vec<_>, _>>()?;
    }
    image.win32.environment.blocks.try_reserve(1).map_err(|_| 8u32)?;
    let result = allocate(image, 0, bytes.len() as u64, READ | WRITE)?;
    if !image.memory.write_virt(result, &bytes) {
        let _ = free(image, result);
        return Err(NOACCESS.into());
    }
    image.win32.environment.blocks.push((result, wide));
    Ok(result)
}

pub(super) fn variable(image: &mut GuestImage, a: &[u64], wide: bool) -> Result<u64, ApiError> {
    let mut name = Vec::new();
    let width = if wide { 2 } else { 1 };
    for index in 0..32768u64 {
        let address = a[0].checked_add(index * width).ok_or(NOACCESS)?;
        let raw = word(&mut image.memory, address, width as usize)? as u16;
        let unit = if wide { raw } else { nls::decode_ansi(raw as u8) };
        if unit == 0 { break; }
        if index == 32767 { return Err(INVALID_PARAMETER.into()); }
        name.push(unit);
    }
    if name.is_empty() { return Err(203u32.into()); }
    if let Some(value) = lookup(image, &name)? {
        let output: Vec<u8> = if wide {
            value.iter().copied().chain(core::iter::once(0)).flat_map(u16::to_le_bytes).collect()
        } else {
            // ACP is 1252 for this runtime. Uninstalled best-fit/default-char
            // tables must not silently substitute an invented environment.
            value.iter().copied().chain(core::iter::once(0))
                .map(|unit| nls::encode_ansi(unit).ok_or(ApiError::Unsupported))
                .collect::<Result<Vec<_>, _>>()?
        };
        let required = value.len() + 1;
        // CP1252 is single byte, so value.len() is also the ANSI byte count.
        // nSize is a DWORD count of characters; a short buffer is untouched.
        if u64::from(a[2] as u32) < required as u64 { return Ok(required as u64); }
        if !image.memory.write_virt(a[1], &output) { return Err(NOACCESS.into()); }
        return Ok(value.len() as u64);
    }
    Err(203u32.into()) // ERROR_ENVVAR_NOT_FOUND
}

pub(super) fn lookup(image: &mut GuestImage, name: &[u16]) -> Result<Option<Vec<u16>>, ApiError> {
    let bytes = snapshot(image)?;
    let units: Vec<u16> = bytes.chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    for entry in units.split(|&unit| unit == 0).take_while(|entry| !entry.is_empty()) {
        // Drive-current-directory variables start with '='; the following
        // equals sign separates the name and value in that case.
        let start = usize::from(entry[0] == b'=' as u16);
        let Some(separator) = entry[start..].iter().position(|&unit| unit == b'=' as u16)
            .map(|offset| start + offset) else { continue; };
        let candidate = &entry[..separator];
        if candidate.len() != name.len() { continue; }
        let mut same = true;
        for (&left, &right) in candidate.iter().zip(name) {
            if left != right && nls::map_case(left, true)? != nls::map_case(right, true)? {
                same = false;
                break;
            }
        }
        if !same { continue; }
        return Ok(Some(entry[separator + 1..].to_vec()));
    }
    Ok(None)
}


pub(super) fn temp_path(image: &mut GuestImage, a: &[u64], wide: bool) -> Result<u64, ApiError> {
    let mut path = None;
    for name in ["TMP", "TEMP", "USERPROFILE"] {
        let units: Vec<u16> = name.encode_utf16().collect();
        if let Some(value) = lookup(image, &units)? {
            if !value.is_empty() { path = Some(value); break; }
        }
    }
    // The mounted-volume runtime's Windows directory is fixed at setup;
    // WINDIR is an environment variable and must not override this fallback.
    let mut units = path.unwrap_or_else(|| "C:\\Windows".encode_utf16().collect());
    if units.last() != Some(&(b'\\' as u16)) { units.push(b'\\' as u16); }
    units.push(0);
    if u64::from(a[0] as u32) < units.len() as u64 { return Ok(units.len() as u64); }
    let bytes = if wide {
        units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
    } else {
        units.iter().map(|&unit| nls::encode_ansi(unit).ok_or(1113u32)).collect::<Result<Vec<_>, _>>()?
    };
    if !image.memory.write_virt(a[1], &bytes) { return Err(NOACCESS.into()); }
    Ok((units.len() - 1) as u64)
}

pub(super) fn release(image: &mut GuestImage, pointer: u64, wide: bool) -> Result<u64, ApiError> {
    let index = image.win32.environment.blocks.iter().position(|&block| block == (pointer, wide))
        .ok_or(INVALID_PARAMETER)?;
    free(image, pointer)?;
    image.win32.environment.blocks.remove(index);
    Ok(1)
}

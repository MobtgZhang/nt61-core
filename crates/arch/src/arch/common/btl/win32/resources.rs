//! Bounded PE string resources in loader-owned executable/DLL mappings.
//! Resource addresses stay guest addresses. No original Windows code or
//! external resource DLL is used by this implementation.
use super::*;

const INVALID_DATA: u32 = 13;

fn write(memory: &mut dyn GuestMemory, address: u64, bytes: &[u8]) -> Result<(), ApiError> {
    if !memory.probe_write(address, bytes.len()) || !memory.write_virt(address, bytes) {
        return Err(NOACCESS.into());
    }
    Ok(())
}

struct Resources<'a> {
    memory: &'a mut super::super::ImageMemory,
    base: u64,
    image_size: usize,
    root: usize,
    size: usize,
}

impl Resources<'_> {
    fn data(&mut self, offset: usize, length: usize) -> Result<Vec<u8>, ApiError> {
        if offset.checked_add(length).filter(|end| *end <= self.size).is_none() {
            return Err(INVALID_DATA.into());
        }
        self.image_data(self.root + offset, length)
    }

    fn image_data(&mut self, rva: usize, length: usize) -> Result<Vec<u8>, ApiError> {
        if length > MAX_BUFFER || rva.checked_add(length)
            .filter(|end| *end <= self.image_size).is_none() {
            return Err(INVALID_DATA.into());
        }
        read(self.memory, self.base + rva as u64, length)
            .map_err(|_| INVALID_DATA.into())
    }

    fn entries(&mut self, offset: usize) -> Result<Vec<(u32, u32)>, ApiError> {
        let header = self.data(offset, 16)?;
        let named = u16::from_le_bytes([header[12], header[13]]) as usize;
        let ids = u16::from_le_bytes([header[14], header[15]]) as usize;
        let count = named + ids;
        let bytes = self.data(offset + 16, count * 8)?;
        let mut result = Vec::new();
        for (index, entry) in bytes.chunks_exact(8).enumerate() {
            let name = u32::from_le_bytes(entry[..4].try_into().map_err(|_| INVALID_DATA)?);
            let target = u32::from_le_bytes(entry[4..].try_into().map_err(|_| INVALID_DATA)?);
            if (name & 0x80000000 != 0) != (index < named) {
                return Err(INVALID_DATA.into());
            }
            if index >= named {
                if name > u16::MAX as u32 || result.iter().any(|(id, _)| *id == name) {
                    return Err(INVALID_DATA.into());
                }
                result.push((name, target));
            }
        }
        Ok(result)
    }

    fn directory(&mut self, offset: usize, id: u32, missing: u32) -> Result<usize, ApiError> {
        let target = self.entries(offset)?.iter().find(|(name, _)| *name == id)
            .map(|(_, target)| *target).ok_or(missing)?;
        if target & 0x80000000 == 0 { return Err(INVALID_DATA.into()); }
        Ok((target & 0x7fffffff) as usize)
    }

    fn string(&mut self, id: u16) -> Result<(u64, Vec<u8>), ApiError> {
        let kind = self.directory(0, 6, 1813)?; // ERROR_RESOURCE_TYPE_NOT_FOUND
        let block = self.directory(kind, u32::from(id) / 16 + 1, 1814)?;
        let languages = self.entries(block)?;
        // No guessed thread/user language or MUI file search. Neutral and
        // single-language images have an unambiguous in-module candidate.
        let target = if let Some((_, target)) = languages.iter().find(|(lang, _)| *lang == 0) {
            *target
        } else if languages.len() == 1 { languages[0].1 }
        else if languages.is_empty() { return Err(1815u32.into()); }
        else { return Err(ApiError::Unsupported); };
        if target & 0x80000000 != 0 { return Err(INVALID_DATA.into()); }
        let entry = self.data(target as usize, 16)?;
        let rva = u32::from_le_bytes(entry[..4].try_into().map_err(|_| INVALID_DATA)?) as usize;
        let size = u32::from_le_bytes(entry[4..8].try_into().map_err(|_| INVALID_DATA)?) as usize;
        let bytes = self.image_data(rva, size)?;
        let mut offset = 0usize;
        for index in 0..=id % 16 {
            let length = bytes.get(offset..offset + 2).ok_or(INVALID_DATA)?;
            let length = u16::from_le_bytes([length[0], length[1]]) as usize;
            offset += 2;
            let end = offset.checked_add(length * 2).ok_or(INVALID_DATA)?;
            let text = bytes.get(offset..end).ok_or(INVALID_DATA)?;
            if index == id % 16 {
                return Ok((self.base + rva as u64 + offset as u64, text.to_vec()));
            }
            offset = end;
        }
        Err(INVALID_DATA.into())
    }
}

pub(super) fn load_string_w(image: &mut GuestImage, a: &[u64]) -> Result<u64, ApiError> {
    let capacity = a[3] as u32 as i32;
    if a[2] == 0 { return Err(INVALID_PARAMETER.into()); }
    // Negative INT sizes cannot describe a checked destination buffer.
    if capacity < 0 { return Err(INVALID_PARAMETER.into()); }
    let base = if a[0] == 0 { image.image_base } else { a[0] };
    let metadata = image.module_metadata.iter().find(|m| m.base == base).copied()
        .ok_or(ApiError::Windows(126))?;
    let (root, size) = metadata.directories[2];
    let loaded = if root == 0 || size == 0 { Err(1812u32.into()) }
    else if (root as usize).checked_add(size as usize).filter(|end| *end <= metadata.size).is_none() {
        Err(INVALID_DATA.into())
    } else {
        Resources { memory: &mut image.memory, base, image_size: metadata.size,
            root: root as usize, size: size as usize }.string(a[1] as u16)
    };
    let (pointer, text) = match loaded {
        Ok(value) => value,
        Err(e) => {
            if capacity > 0 { write(&mut image.memory, a[2], &[0, 0])?; }
            return Err(e);
        }
    };
    if capacity == 0 {
        let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
        write(&mut image.memory, a[2], &pointer.to_le_bytes()[..width])?;
        return Ok((text.len() / 2) as u64);
    }
    let length = (text.len() / 2).min(capacity as usize - 1);
    let mut output = text[..length * 2].to_vec();
    output.extend_from_slice(&[0, 0]);
    write(&mut image.memory, a[2], &output)?;
    Ok(length as u64)
}

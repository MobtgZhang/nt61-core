use alloc::vec::Vec;

use crate::windows_wim_lzx::decode_chunk;

const CHUNK_SIZE: usize = 32768;
const MAX_RESOURCE_SIZE: usize = 32 * 1024 * 1024;
const LOOKUP_ENTRY_SIZE: usize = 50;
const DIRECTORY_ENTRY_SIZE: usize = 102;

fn number<const SIZE: usize>(bytes: &[u8], offset: usize) -> Option<[u8; SIZE]> {
    bytes
        .get(offset..offset.checked_add(SIZE)?)?
        .try_into()
        .ok()
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(number(bytes, offset)?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(number(bytes, offset)?))
}

fn usize_at(bytes: &[u8], offset: usize) -> Option<usize> {
    usize::try_from(u64::from_le_bytes(number(bytes, offset)?)).ok()
}

fn resource(wim: &[u8], descriptor: &[u8]) -> Option<Vec<u8>> {
    let mut packed = number::<8>(descriptor, 0)?;
    let flags = core::mem::take(&mut packed[7]);
    if flags & !6 != 0 {
        return None;
    }
    let stored = usize::try_from(u64::from_le_bytes(packed)).ok()?;
    let offset = usize_at(descriptor, 8)?;
    let size = usize_at(descriptor, 16)?;
    if size == 0 || size > MAX_RESOURCE_SIZE {
        return None;
    }
    let data = wim.get(offset..offset.checked_add(stored)?)?;
    let mut output = Vec::new();
    output.try_reserve_exact(size).ok()?;
    if flags & 4 == 0 {
        return (size == stored).then(|| {
            output.extend_from_slice(data);
            output
        });
    }
    let chunks = size.div_ceil(CHUNK_SIZE);
    let data_start = (chunks - 1) * 4;
    let encoded = data.get(data_start..)?;
    let mut previous = 0;
    output.resize(size, 0);
    for (chunk, target) in output.chunks_mut(CHUNK_SIZE).enumerate() {
        let next = if chunk + 1 == chunks {
            encoded.len()
        } else {
            u32_at(data, chunk * 4)? as usize
        };
        let source = encoded.get(previous..next)?;
        match source.len().cmp(&target.len()) {
            core::cmp::Ordering::Equal => target.copy_from_slice(source),
            core::cmp::Ordering::Less => decode_chunk(source, target)?,
            core::cmp::Ordering::Greater => return None,
        }
        previous = next;
    }
    Some(output)
}

fn directory_entry(metadata: &[u8], position: usize) -> Option<&[u8]> {
    let size = usize_at(metadata, position)?;
    if size < DIRECTORY_ENTRY_SIZE || size % 8 != 0 {
        return None;
    }
    metadata.get(position..position.checked_add(size)?)
}

fn path_hash(metadata: &[u8], path: &str) -> Option<[u8; 20]> {
    let root = (u32_at(metadata, 0)? as usize).max(8).checked_add(7)? & !7;
    let mut entry = directory_entry(metadata, root)?;
    let mut components = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .peekable();
    while let Some(component) = components.next() {
        let mut position = usize_at(entry, 16)?;
        if position <= root || position % 8 != 0 {
            return None;
        }
        loop {
            entry = directory_entry(metadata, position)?;
            let name_len = usize::from(u16_at(entry, 100)?);
            let name = entry.get(DIRECTORY_ENTRY_SIZE..DIRECTORY_ENTRY_SIZE + name_len)?;
            let mut words = name
                .chunks_exact(2)
                .map(|word| u16::from_le_bytes([word[0], word[1]]));
            let name_matches = name_len % 2 == 0
                && component.encode_utf16().all(|expected| {
                    words.next().is_some_and(|actual| {
                        actual == expected
                            || (actual <= 127
                                && expected <= 127
                                && (actual as u8).eq_ignore_ascii_case(&(expected as u8)))
                    })
                })
                && words.next().is_none();
            if name_matches {
                let attributes = u32_at(entry, 8)?;
                if attributes & 0x400 != 0
                    || (attributes & 0x10 != 0) != components.peek().is_some()
                {
                    return None;
                }
                break;
            }
            position = position.checked_add(entry.len())?;
            for _ in 0..u16_at(entry, 96)? {
                let size = usize_at(metadata, position)?;
                if size < 40 || size % 8 != 0 {
                    return None;
                }
                position = position.checked_add(size)?;
            }
        }
    }
    let hash = number(entry, 64)?;
    (hash != [0; 20]).then_some(hash)
}

pub fn read_boot_file(wim: &[u8], path: &str) -> Option<Vec<u8>> {
    if wim.get(..8)? != b"MSWIM\0\0\0"
        || u32_at(wim, 8)? != 208
        || u32_at(wim, 12)? != 0x0001_0d00
        || u16_at(wim, 40)? != 1
        || u16_at(wim, 42)? != 1
    {
        return None;
    }
    let flags = u32_at(wim, 16)?;
    let compressed = flags & 2 != 0;
    if compressed && (flags & 0x000e_0000 != 0x0004_0000 || u32_at(wim, 20)? as usize != CHUNK_SIZE)
    {
        return None;
    }
    let boot_index = u32_at(wim, 120)?;
    if boot_index == 0 || boot_index > u32_at(wim, 44)? {
        return None;
    }
    let metadata = resource(wim, wim.get(96..120)?)?;
    let hash = path_hash(&metadata, path)?;
    let lookup = resource(wim, wim.get(48..72)?)?;
    if lookup.len() % LOOKUP_ENTRY_SIZE != 0 {
        return None;
    }
    let entry = lookup
        .chunks_exact(LOOKUP_ENTRY_SIZE)
        .find(|entry| entry[7] & 2 == 0 && u16_at(entry, 24) == Some(1) && entry[30..50] == hash)?;
    resource(wim, entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn descriptor(bytes: &mut [u8], offset: u64, length: u64, flags: u8) {
        bytes[..8].copy_from_slice(&length.to_le_bytes());
        bytes[7] = flags;
        bytes[8..16].copy_from_slice(&offset.to_le_bytes());
        bytes[16..24].copy_from_slice(&length.to_le_bytes());
    }

    #[test]
    fn resolves_the_boot_image_path_and_rejects_invalid_resources() {
        let mut wim = vec![0; 520];
        wim[..8].copy_from_slice(b"MSWIM\0\0\0");
        wim[8..12].copy_from_slice(&208u32.to_le_bytes());
        wim[12..16].copy_from_slice(&0x0001_0d00u32.to_le_bytes());
        wim[40..48].copy_from_slice(&[1, 0, 1, 0, 1, 0, 0, 0]);
        wim[120] = 1;
        descriptor(&mut wim[48..72], 458, 50, 2);
        descriptor(&mut wim[96..120], 208, 250, 2);
        let metadata = &mut wim[208..458];
        metadata[0] = 8;
        metadata[8] = 104;
        metadata[24] = 112;
        let child = &mut metadata[112..240];
        child[0] = 128;
        child[64..84].fill(42);
        child[100] = 22;
        for (bytes, word) in child[102..124]
            .chunks_exact_mut(2)
            .zip("winload.efi".encode_utf16())
        {
            bytes.copy_from_slice(&word.to_le_bytes());
        }
        let entry = &mut wim[458..508];
        descriptor(entry, 508, 12, 0);
        entry[24] = 1;
        entry[30..50].fill(42);
        wim[508..].copy_from_slice(b"test payload");
        assert_eq!(
            read_boot_file(&wim, "\\WINLOAD.EFI"),
            Some(b"test payload".to_vec())
        );
        assert_eq!(read_boot_file(&wim, "\\missing\\winload.efi"), None);
        assert_eq!(read_boot_file(&wim[..519], "winload.efi"), None);
        wim[120] = 0;
        assert_eq!(read_boot_file(&wim, "winload.efi"), None);
    }
}

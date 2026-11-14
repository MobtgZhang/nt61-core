//! Minimal ReFS boot reader for winload.efi.
//!
//! Uses the self-describing directory index produced by the in-tree
//! ReFS image builder (`tools/src/fs/refs.rs`). The index lives at byte
//! 0x2000 (LBA 16) from the partition start and consists of fixed-size
//! entries: 256-byte null-patched path + u64 start LBA + u64 byte length,
//! terminated by a zero-length path.

use alloc::vec::Vec;

const REFS_INDEX_LBA: u64 = 16;
const ENTRY_SIZE: usize = 256 + 16;
const CHUNK_SECTORS: u64 = 128; // 64 KiB per read

/// Read a file from a ReFS System partition.
pub fn read_refs_system_file(path: &str) -> Option<Vec<u8>> {
    use uefi::boot::OpenProtocolAttributes;
    use uefi::boot::OpenProtocolParams;
    use uefi::proto::media::block::BlockIO;

    let handles = uefi::boot::find_handles::<BlockIO>().ok()?;
    uefi::println!(
        "[REFS] read_refs_system_file: probing {} BlockIO handles",
        handles.len()
    );

    for handle in handles.iter() {
        let sp = unsafe {
            uefi::boot::open_protocol::<BlockIO>(
                OpenProtocolParams {
                    handle: *handle,
                    agent: uefi::boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        };
        let Ok(block) = sp else { continue };
        let Some(block_ref) = block.get() else {
            core::mem::forget(block);
            continue;
        };
        let media = block_ref.media();
        if media.block_size() != 512 {
            core::mem::forget(block);
            continue;
        }
        let media_id = media.media_id();
        let partition_start_lba = if media.is_logical_partition() {
            0u64
        } else {
            match find_refs_partition_start_lba(&*block_ref, media_id) {
                Some(lba) => lba,
                None => {
                    core::mem::forget(block);
                    continue;
                }
            }
        };

        // Verify boot sector
        let mut vbr = [0u8; 512];
        if block_ref
            .read_blocks(media_id, partition_start_lba, &mut vbr)
            .is_err()
        {
            core::mem::forget(block);
            continue;
        }
        if &vbr[3..11] != b"ReFS    " {
            core::mem::forget(block);
            continue;
        }

        let result = read_refs_file(&*block_ref, media_id, path, partition_start_lba);
        core::mem::forget(block);
        if result.is_some() {
            return result;
        }
    }
    None
}

/// Find a ReFS partition start LBA by reading the GPT.
fn find_refs_partition_start_lba(
    block: &uefi::proto::media::block::BlockIO,
    media_id: u32,
) -> Option<u64> {
    // Read GPT header at LBA 1
    let mut gpt_header = [0u8; 512];
    block.read_blocks(media_id, 1u64, &mut gpt_header).ok()?;
    if &gpt_header[0..8] != b"EFI PART" {
        return None;
    }
    let partition_entry_count = u32::from_le_bytes([
        gpt_header[80],
        gpt_header[81],
        gpt_header[82],
        gpt_header[83],
    ]) as usize;
    let partition_entry_size = u32::from_le_bytes([
        gpt_header[84],
        gpt_header[85],
        gpt_header[86],
        gpt_header[87],
    ]) as usize;
    let partition_entry_lba = u64::from_le_bytes([
        gpt_header[72],
        gpt_header[73],
        gpt_header[74],
        gpt_header[75],
        gpt_header[76],
        gpt_header[77],
        gpt_header[78],
        gpt_header[79],
    ]);

    let entries_buf_size = partition_entry_count * partition_entry_size;
    let mut entries_buf = alloc::vec![0u8; entries_buf_size];
    block
        .read_blocks(media_id, partition_entry_lba, &mut entries_buf)
        .ok()?;

    // Microsoft basic-data GUID (same for NTFS and ReFS)
    const BASIC_DATA_GUID: [u8; 16] = [
        0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99,
        0xC7,
    ];

    for i in 0..partition_entry_count {
        let entry_off = i * partition_entry_size;
        if entry_off + 48 > entries_buf.len() {
            break;
        }
        let g = &entries_buf[entry_off..entry_off + 16];
        if g != &BASIC_DATA_GUID {
            continue;
        }
        let start_lba = u64::from_le_bytes(
            entries_buf[entry_off + 32..entry_off + 40]
                .try_into()
                .ok()?,
        );
        // Distinguish ReFS from NTFS by reading the boot sector
        let mut vbr = [0u8; 512];
        if block.read_blocks(media_id, start_lba, &mut vbr).is_ok() {
            if &vbr[3..11] == b"ReFS    " {
                return Some(start_lba);
            }
        }
    }
    None
}

/// Read a file using the directory index.
pub(crate) fn read_refs_file(
    block: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    path: &str,
    partition_start_lba: u64,
) -> Option<Vec<u8>> {
    let mut superblock = [0u8; 512];
    block.read_blocks(media_id, partition_start_lba + 8, &mut superblock).ok()?;
    let table_bytes = u64::from_le_bytes(superblock[88..96].try_into().ok()?) as usize;
    if table_bytes < ENTRY_SIZE || table_bytes > 16 * 1024 * 1024 { return None; }
    let mut index = alloc::vec![0u8; table_bytes.div_ceil(512) * 512];
    block
        .read_blocks(media_id, partition_start_lba + REFS_INDEX_LBA, &mut index)
        .ok()?;

    // Normalise path: strip leading '\', '/', and drive letter
    let norm = path
        .trim_start_matches('\\')
        .trim_start_matches('/')
        .trim_start_matches("C:")
        .trim_start_matches("c:")
        .trim_start_matches('\\')
        .trim_start_matches('/')
        .replace('\\', "/");

    let mut offset = 0usize;
    while offset + ENTRY_SIZE <= index.len() {
        let path_field = &index[offset..offset + 256];
        let null_pos = path_field.iter().position(|&b| b == 0).unwrap_or(256);
        let entry_path = core::str::from_utf8(&path_field[..null_pos]).unwrap_or("");
        if entry_path.is_empty() {
            break;
        }
        let start_lba = u64::from_le_bytes(
            index[offset + 256..offset + 256 + 8]
                .try_into()
                .unwrap_or([0u8; 8]),
        );
        let file_len = u64::from_le_bytes(
            index[offset + 264..offset + 264 + 8]
                .try_into()
                .unwrap_or([0u8; 8]),
        );
        if entry_path.eq_ignore_ascii_case(&norm) {
            uefi::println!(
                "[REFS] found {} at LBA {} ({} bytes)",
                entry_path,
                start_lba,
                file_len
            );
            return read_file_data(block, media_id, partition_start_lba + start_lba, file_len);
        }
        offset += ENTRY_SIZE;
    }
    uefi::println!("[REFS] {} not found in directory index", path);
    None
}

/// Read `file_len` bytes starting at `base_lba` from the partition.
fn read_file_data(
    block: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    base_lba: u64,
    file_len: u64,
) -> Option<Vec<u8>> {
    if file_len > 256 * 1024 * 1024 { return None; }
    let mut data = alloc::vec![0u8; file_len.div_ceil(512) as usize * 512];
    let total_sectors = file_len.div_ceil(512);
    let mut sector_off = 0u64;
    while sector_off < total_sectors {
        let count = CHUNK_SECTORS.min(total_sectors - sector_off);
        let chunk_start = sector_off as usize * 512;
        let chunk_end = ((sector_off + count) as usize * 512).min(data.len());
        if chunk_start >= chunk_end {
            break;
        }
        let chunk = &mut data[chunk_start..chunk_end];
        if block
            .read_blocks(media_id, base_lba + sector_off, chunk)
            .is_err()
        {
            return None;
        }
        sector_off += count;
    }
    data.truncate(file_len as usize);
    Some(data)
}

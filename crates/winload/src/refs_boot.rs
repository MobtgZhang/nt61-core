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

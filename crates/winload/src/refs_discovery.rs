//! Discover a ReFS system volume for the OS loader.

use alloc::vec::Vec;
use crate::refs_boot::read_refs_file;

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


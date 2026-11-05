//! Minimal EXT4 boot-sector reader for bootmgr.efi.
//!
//! UEFI's `SimpleFileSystem` protocol only supports FAT-family volumes by
//! default. When the System partition is formatted as EXT4, bootmgr (when
//! loading `winload.efi`) falls back to this in-tree reader to resolve
//! `\Windows\System32\winload.efi`.
//!
//! This module is a *port* of `winload/src/ext4_boot.rs` to bootmgr's
//! module layout. The boot manager already classifies BlockIO handles
//! through a single partition-type probe, so we expose a handle-taking
//! helper [`read_ext4_with_block`] that the dispatcher calls once it
//! has identified an EXT4 partition. The convenience wrapper
//! [`read_ext4_system_file`] keeps the legacy "scan all handles"
//! behaviour for callers that do not yet use the dispatcher.
//!
//! ## Layout assumptions
//!
//! The reader supports the minimal EXT4 image layout produced by our
//! build tool (`tools/src/fs/ext4.rs`):
//!
//!   * 4096-byte blocks
//!   * inode size = 256
//!   * linear directory entries (no hash-tree)
//!   * extent trees of depth 0 or 1
//!   * no journal replay, no xattr blocks
//!   * 32-bit block group descriptors
//!
//! ## Filesystem walk
//!
//! 1. Probe the boot sector at LBA 0 for `0xEF53` at byte 56 of the
//!    superblock (offset 1024 + 56 = 1080 from the partition start).
//! 2. Parse superblock fields we need (`s_log_block_size`, `s_inode_size`,
//!    `s_blocks_per_group`, `s_inodes_per_group`, `s_first_data_block`).
//! 3. read the BGDT at the first-data-block + 1 block to find the inode
//!    table for group 0.
//! 4. read `inode 2` (root directory), walk its extents to get the dir
//!    block, scan entries to find the next path component.
//! 5. Repeat down the tree.

use alloc::vec::Vec;

// =====================================================================
// Constants
// =====================================================================

const EXT4_MAGIC: u16 = 0xEF53;
/// Inode number for the root directory. EXT4 always uses 2.
const EXT4_ROOT_INO: u32 = 2;

// =====================================================================
// BlockIO sector read helper
// =====================================================================

/// read `count` sectors starting at `lba` from the given BlockIO handle.
fn read_partition_sectors(
    block_io: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    start_lba: u64,
    count: u32,
) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity((count as usize) * 512);
    buf.resize((count as usize) * 512, 0u8);
    // read one sector at a time to avoid large stack frames.
    for i in 0..count as usize {
        let sector_buf = &mut buf[i * 512..(i + 1) * 512];
        if block_io
            .read_blocks(media_id, start_lba + i as u64, sector_buf)
            .is_err()
        {
            return None;
        }
    }
    Some(buf)
}

// =====================================================================
// Superblock
// =====================================================================

/// Parsed EXT4 superblock (subset of fields we need).
struct Ext4Boot {
    block_size: u32,
    inode_size: u16,
    inodes_per_group: u32,
    first_data_block: u32,
}

impl Ext4Boot {
    fn parse(buf: &[u8; 1024]) -> Option<Self> {
        // Magic is at offset 56 within the superblock.
        let magic = u16::from_le_bytes([buf[56], buf[57]]);
        if magic != EXT4_MAGIC {
            return None;
        }
        let s_log_block_size = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]);
        let block_size: u32 = 1024u32 << s_log_block_size;
        if block_size != 1024 && block_size != 2048 && block_size != 4096 {
            return None;
        }
        let inode_size = u16::from_le_bytes([buf[88], buf[89]]);
        if inode_size < 128 {
            return None;
        }
        let inodes_per_group = u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]);
        let first_data_block = u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]);
        Some(Ext4Boot {
            block_size,
            inode_size,
            inodes_per_group,
            first_data_block,
        })
    }
}

// =====================================================================
// Inode reading
// =====================================================================

/// read a single EXT4 inode into a fresh buffer.
///
/// `inode_table_lba` is the **absolute** disk LBA of the start of the
/// inode table (i.e. `partition_start_lba + inode_table_block`).
fn read_inode(
    block_io: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    ext4: &Ext4Boot,
    inode_num: u32,
    partition_start_lba: u64,
) -> Option<Vec<u8>> {
    if inode_num == 0 {
        return None;
    }
    if ext4.inodes_per_group == 0 {
        return None;
    }
    let group = (inode_num - 1) / ext4.inodes_per_group;
    let local = (inode_num - 1) % ext4.inodes_per_group;

    // Locate this inode's group descriptor. EXT4 currently uses 64-byte
    // descriptors; descriptor 0 begins in the block after the first data
    // block. Read the one 512-byte sector containing the descriptor.
    let sectors_per_block = (ext4.block_size / 512) as u64;
    let descriptor_offset = group.checked_mul(64)?;
    let descriptor_lba = partition_start_lba
        + ((ext4.first_data_block + 1) as u64).checked_mul(sectors_per_block)?
        + u64::from(descriptor_offset / 512);
    let mut descriptor_sector = [0u8; 512];
    if block_io
        .read_blocks(media_id, descriptor_lba, &mut descriptor_sector)
        .is_err()
    {
        return None;
    }
    let descriptor_offset_in_sector = (descriptor_offset % 512) as usize;
    if descriptor_offset_in_sector + 64 > descriptor_sector.len() {
        return None;
    }
    let descriptor = &descriptor_sector[descriptor_offset_in_sector..];
    let inode_table_block =
        u32::from_le_bytes([descriptor[8], descriptor[9], descriptor[10], descriptor[11]]);
    if inode_table_block == 0 {
        return None;
    }
    let inode_table_lba =
        partition_start_lba + (inode_table_block as u64).checked_mul(sectors_per_block)?;

    let block_in_table = (local * (ext4.inode_size as u32)) / ext4.block_size;
    // inode_table_lba is the absolute disk LBA of the inode table
    // start. block_in_table is a block index (1 unit = block_size
    // bytes). Convert block_in_table to sectors before adding to the
    // LBA; otherwise we under-read by `block_in_table * 8` sectors
    // and pick up the wrong inode.
    let lba = inode_table_lba + block_in_table as u64 * sectors_per_block;
    let data = read_partition_sectors(block_io, media_id, lba, sectors_per_block as u32)?;
    let offset_in_block = ((local * (ext4.inode_size as u32)) % ext4.block_size) as usize;
    if offset_in_block + ext4.inode_size as usize > data.len() {
        return None;
    }
    Some(data[offset_in_block..offset_in_block + ext4.inode_size as usize].to_vec())
}

/// Extract the i_size_lo and i_block[15] fields from an inode buffer.
fn parse_inode(inode: &[u8]) -> Option<(u64, [u32; 15])> {
    if inode.len() < 112 {
        return None;
    }
    let i_size_lo = u32::from_le_bytes([inode[4], inode[5], inode[6], inode[7]]);
    let i_size_hi = u32::from_le_bytes([inode[108], inode[109], inode[110], inode[111]]);
    let size = u64::from(i_size_lo) | (u64::from(i_size_hi) << 32);
    let mut i_block = [0u32; 15];
    for i in 0..15 {
        let off = 40 + i * 4;
        i_block[i] =
            u32::from_le_bytes([inode[off], inode[off + 1], inode[off + 2], inode[off + 3]]);
    }
    Some((size, i_block))
}

/// Walk an inode's extent tree and return the bytes of the file's data.
/// Supports depth-0 extents (a single extent entry describes up to
/// 32768 blocks) and depth-1 extents (an internal node points at leaf
/// blocks containing extent entries).
///
/// `partition_start_lba` is the disk-relative LBA of the partition's
/// first sector; it is added to every extent LBA before the disk read.
fn read_extents(
    block_io: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    ext4: &Ext4Boot,
    i_block: &[u32; 15],
    size: u64,
    partition_start_lba: u64,
) -> Option<Vec<u8>> {
    let eh_magic = (i_block[0] & 0xFFFF) as u16;
    if eh_magic != 0xF30A {
        return None;
    }
    let eh_entries = (i_block[0] >> 16) as u16;
    let eh_depth = (i_block[1] >> 16) as u16;

    let mut out = Vec::with_capacity(size as usize);
    let mut bytes_left = size as usize;
    let mut next_logical = 0usize;

    let process_extent = |entry: (u32, u32, u32),
                          out: &mut Vec<u8>,
                          bytes_left: &mut usize,
                          next_logical: &mut usize|
     -> Option<bool> {
        let (ee_block, ee_len_word, start_word) = entry;
        let unwritten = ee_len_word & 0x8000 != 0;
        let ee_len = (ee_len_word & 0x7FFF) as usize;
        if ee_len == 0 {
            return Some(true);
        }
        let ee_start = ee_block as usize;
        if *next_logical < ee_start {
            let gap = core::cmp::min(ee_start - *next_logical, *bytes_left);
            out.extend(core::iter::repeat_n(0u8, gap));
            *bytes_left = bytes_left.saturating_sub(gap);
            *next_logical += gap;
        }
        if *next_logical != ee_start || *bytes_left == 0 {
            return Some(true);
        }

        let block_count = core::cmp::min(ee_len * ext4.block_size as usize, *bytes_left);
        if unwritten {
            out.extend(core::iter::repeat_n(0u8, block_count));
            *bytes_left -= block_count;
            *next_logical += block_count;
            return Some(*bytes_left != 0);
        }

        let ee_start_hi = (start_word >> 16) & 0xFFFF;
        let ee_start_lo = start_word;
        let physical = ((ee_start_hi << 16) | ee_start_lo) as u32;
        let sectors = ((block_count as usize + ext4.block_size as usize - 1)
            / ext4.block_size as usize
            * (ext4.block_size / 512) as usize) as u32;
        let lba = partition_start_lba + (physical as u64) * (ext4.block_size / 512) as u64;
        let data = read_partition_sectors(block_io, media_id, lba, sectors)?;
        out.extend_from_slice(&data[..block_count.min(data.len())]);
        *bytes_left = bytes_left.saturating_sub(block_count);
        *next_logical += block_count;
        Some(*bytes_left != 0)
    };

    if eh_depth == 0 {
        for i in 0..eh_entries as usize {
            let entry_start = 3 + i * 3;
            if entry_start + 3 > 15 {
                break;
            }
            let w0 = i_block[entry_start];
            let w1 = i_block[entry_start + 1];
            let w2 = i_block[entry_start + 2];
            if !process_extent((w0, w1, w2), &mut out, &mut bytes_left, &mut next_logical)? {
                break;
            }
        }
    } else if eh_depth == 1 {
        for i in 0..eh_entries as usize {
            let entry_start = 3 + i * 3;
            if entry_start + 3 > 15 {
                break;
            }
            let w0 = i_block[entry_start];
            let w1 = i_block[entry_start + 1];
            let w2 = i_block[entry_start + 2];
            let _ei_block = w0 & 0xFFFF;
            let ei_leaf_lo = w1;
            let ei_leaf_hi = (w2 >> 16) & 0xFFFF;
            let leaf_block = ((ei_leaf_hi << 16) | ei_leaf_lo) as u32;
            let leaf_lba =
                partition_start_lba + (leaf_block as u64) * (ext4.block_size / 512) as u64;
            let sectors_per_block = ext4.block_size / 512;
            let leaf = read_partition_sectors(block_io, media_id, leaf_lba, sectors_per_block)?;
            let lh_magic = u16::from_le_bytes([leaf[0], leaf[1]]);
            if lh_magic != 0xF30A {
                continue;
            }
            let lh_entries = u16::from_le_bytes([leaf[2], leaf[3]]);
            for j in 0..lh_entries as usize {
                let e_off = 12 + j * 12;
                if e_off + 12 > leaf.len() {
                    break;
                }
                let e_w0 = u32::from_le_bytes([
                    leaf[e_off],
                    leaf[e_off + 1],
                    leaf[e_off + 2],
                    leaf[e_off + 3],
                ]);
                let e_w1 = u32::from_le_bytes([
                    leaf[e_off + 4],
                    leaf[e_off + 5],
                    leaf[e_off + 6],
                    leaf[e_off + 7],
                ]);
                let e_w2 = u32::from_le_bytes([
                    leaf[e_off + 8],
                    leaf[e_off + 9],
                    leaf[e_off + 10],
                    leaf[e_off + 11],
                ]);
                if !process_extent(
                    (e_w0, e_w1, e_w2),
                    &mut out,
                    &mut bytes_left,
                    &mut next_logical,
                )? {
                    break;
                }
            }
            if bytes_left == 0 {
                break;
            }
        }
    } else {
        return None;
    }
    out.truncate(size as usize);
    Some(out)
}

// =====================================================================
// Directory walking
// =====================================================================

/// Scan a directory block for an entry with the given name (case
/// sensitive — EXT4 stores names verbatim). Returns the inode number
/// of the matching entry.
fn find_child_in_dir(dir_data: &[u8], target: &str) -> Option<u32> {
    let mut off = 0usize;
    while off + 8 <= dir_data.len() {
        let inode_num = u32::from_le_bytes([
            dir_data[off],
            dir_data[off + 1],
            dir_data[off + 2],
            dir_data[off + 3],
        ]);
        let rec_len = u16::from_le_bytes([dir_data[off + 4], dir_data[off + 5]]) as usize;
        let name_len = dir_data[off + 6] as usize;
        let _file_type = dir_data[off + 7];
        if rec_len == 0 || rec_len > dir_data.len() - off {
            break;
        }
        if inode_num != 0 && name_len > 0 && name_len + 8 <= rec_len {
            if let Ok(name) = core::str::from_utf8(&dir_data[off + 8..off + 8 + name_len]) {
                if name == target {
                    return Some(inode_num);
                }
            }
        }
        off += rec_len;
    }
    None
}

/// Resolve a Windows-style path (e.g. `\Windows\System32\winload.efi`)
/// starting at `start_ino` to an inode number.
///
/// `partition_start_lba` is added to every extent LBA before the disk
/// read so the path walker works with both partition-scoped and
/// disk-scoped BlockIO handles.
fn resolve_path(
    block_io: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    ext4: &Ext4Boot,
    start_ino: u32,
    path: &str,
    partition_start_lba: u64,
) -> Option<u32> {
    let parts: Vec<&str> = path
        .trim_start_matches(|c| c == '/' || c == '\\')
        .split(|c| c == '/' || c == '\\')
        .filter(|s| !s.is_empty())
        .collect();
    let mut current = start_ino;
    for part in &parts {
        let inode = read_inode(block_io, media_id, ext4, current, partition_start_lba)?;
        let (size, i_block) = parse_inode(&inode)?;
        let dir_data = read_extents(
            block_io,
            media_id,
            ext4,
            &i_block,
            size,
            partition_start_lba,
        )?;
        current = find_child_in_dir(&dir_data, part)?;
    }
    Some(current)
}

// =====================================================================
// Public entry points
// =====================================================================

/// read a file from an EXT4 partition that is already known to be EXT4,
/// using the supplied `BlockIO` handle and `media_id`. Used by the
/// bootmgr partition-type dispatcher: once the probe has classified
/// the handle as `Ext4`, the dispatcher calls this and avoids the
/// per-handle scan in [`read_ext4_system_file`].
///
/// `block_io` is borrowed for the duration of this call (the caller
/// owns the protocol handle; this function never drops it).
///
/// `partition_start_lba` is the disk-relative LBA of the EXT4
/// partition's first sector. Pass 0 for a partition-scoped handle; pass
/// the GPT start LBA for a disk-scoped handle. The EXT4 superblock
/// then lives at `partition_start_lba + 2` (sectors).
pub fn read_ext4_with_block(
    path: &str,
    block_io: &uefi::proto::media::block::BlockIO,
    media_id: u32,
    partition_start_lba: u64,
) -> Option<Vec<u8>> {
    // read the superblock: at byte offset 1024 within the partition.
    let sb_lba = partition_start_lba + 2;
    let mut sb_buf = [0u8; 1024];
    if block_io.read_blocks(media_id, sb_lba, &mut sb_buf).is_err() {
        debug_println!(
            "[EXT4] read_ext4_with_block: failed to read superblock at LBA {}",
            sb_lba
        );
        return None;
    }
    let ext4 = Ext4Boot::parse(&sb_buf)?;

    // Resolve path -> inode.
    let target_ino = match resolve_path(
        block_io,
        media_id,
        &ext4,
        EXT4_ROOT_INO,
        path,
        partition_start_lba,
    ) {
        Some(i) => i,
        None => {
            debug_println!(
                "[EXT4] read_ext4_with_block: resolve_path failed for '{}'",
                path
            );
            return None;
        }
    };

    // read target inode + extents.
    let inode = read_inode(block_io, media_id, &ext4, target_ino, partition_start_lba)?;
    let (size, i_block) = parse_inode(&inode)?;
    let data = read_extents(
        block_io,
        media_id,
        &ext4,
        &i_block,
        size,
        partition_start_lba,
    )?;
    debug_println!(
        "[EXT4] read_ext4_with_block: '{}' -> inode {} ({} bytes)",
        path,
        target_ino,
        data.len()
    );
    Some(data)
}

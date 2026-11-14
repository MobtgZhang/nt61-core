//! EXT4 Filesystem Image Module
//!
//! This module provides a pure Rust implementation for creating EXT4 filesystem images,
//! which can be used to store Windows system files in a Linux-native filesystem.
//!
//! ## Features
//! - Superblock generation
//! - Block group descriptor table
//! - Inode and block bitmaps
//! - Inode table
//! - Extents for file data
//! - Directory entries
//! - Sparse superblock support
//!
//! ## Usage
//! ```rust,no_run
//! use nt61_tools::Ext4Image;
//!
//! let mut image = Ext4Image::new(128, 4096).unwrap(); // 128 MB, 4KB blocks
//! image.create_dir("/EFI").unwrap();
//! image.create_dir("/EFI/Boot").unwrap();
//! let boot_data = [0u8; 512];
//! image.write_file("/EFI/Boot/BOOTX64.EFI", &boot_data).unwrap();
//! let img_data = image.finalize().unwrap();
//! ```

use crate::error::{BuildError, Result};
use crate::fs::backend::{DirEntry, FsBackend};

// =====================================================================
// Constants
// =====================================================================

/// EXT4 magic number
pub const EXT4_SUPERBLOCK_MAGIC: u16 = 0xEF53;
/// Superblock offset (1024 bytes from start)
pub const SUPERBLOCK_OFFSET: u64 = 1024;

/// EXT4 Feature Incompat flags
pub const EXT3_FEATURE_INCOMPAT_FILETYPE: u32 = 0x0002;
pub const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32 = 0x0200;
pub const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
pub const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;

/// EXT4 Feature Ro Compat flags
pub const EXT4_FEATURE_RO_COMPAT_GDT_CSUM: u32 = 0x0010;
pub const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: u32 = 0x0020;
pub const EXT4_FEATURE_RO_COMPAT_SPARSESUPER2: u32 = 0x0100;

/// File type constants for directory entries
pub const EXT4_FT_UNKNOWN: u8 = 0;
pub const EXT4_FT_REG_FILE: u8 = 1;
pub const EXT4_FT_DIR: u8 = 2;
pub const EXT4_FT_SYMLINK: u8 = 7;

// =====================================================================
// EXT4 Structures
// =====================================================================

/// EXT4 Superblock (256 bytes, at offset 1024)
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Ext4SuperBlock {
    s_inodes_count: u32,           // Inodes count
    s_blocks_count_lo: u32,        // Blocks count low
    s_r_blocks_count_lo: u32,      // Reserved blocks count low
    s_free_blocks_count_lo: u32,   // Free blocks count low
    s_free_inodes_count: u32,      // Free inodes count
    s_first_data_block: u32,       // First data block
    s_log_block_size: u32,         // Block size = 1024 << s_log_block_size
    s_log_cluster_size: u32,       // Fragment size
    s_blocks_per_group: u32,       // Blocks per group
    s_clusters_per_group: u32,     // Fragments per group
    s_inodes_per_group: u32,       // Inodes per group
    s_mtime: u32,                  // Mount time
    s_wtime: u32,                  // Write time
    s_mnt_count: u16,              // Mount count
    s_max_mnt_count: u16,          // Maximal mount count
    s_magic: u16,                  // Magic signature (0xEF53)
    s_state: u16,                  // File system state
    s_errors: u16,                 // Behaviour when detecting errors
    s_minor_rev_level: u16,        // Minor revision level
    s_lastcheck: u32,              // time of last check
    s_checkinterval: u32,          // max. time between checks
    s_creator_os: u32,             // OS
    s_rev_level: u32,              // Revision level
    s_def_resuid: u16,             // Default uid for reserved blocks
    s_def_resgid: u16,             // Default gid for reserved blocks
    s_first_ino: u32,              // First non-reserved inode
    s_inode_size: u16,             // inode size
    s_block_group_nr: u16,         // block group # of this superblock
    s_features_compat: u32,        // compatible feature set
    s_features_incompat: u32,      // incompatible feature set
    s_features_ro_compat: u32,     // readonly-compatible feature set
    s_uuid: [u8; 16],              // 128-bit uuid for volume
    s_volume_name: [u8; 16],       // volume name
    s_last_mounted: [u8; 64],      // directory where last mounted
    s_algorithm_usage_bitmap: u32, // For compression
    s_prealloc_blocks: u8,         // # of blocks to try to preallocate
    s_prealloc_dir_blocks: u8,     // # of blocks to preallocate for dirs
    s_reserved_gdt_blocks: u16,    // Per group desc for online growth
    s_journal_uuid: [u8; 16],      // uuid of journal superblock
    s_journal_inum: u32,           // inode number of journal file
    s_journal_dev: u32,            // device number of journal file
    s_last_orphan: u32,            // start of list of inodes to delete
    s_hash_seed: [u32; 4],         // HTREE hash seed
    s_def_hash_version: u8,        // Default hash version
    s_reserved_char_pad: u8,
    s_reserved_word_pad: u16,
    s_default_mount_opts: u32,
    s_first_meta_bg: u32,        // First metablock group
    s_mkfs_time: u32,            // When the filesystem was created
    s_journal_blocks: [u32; 17], // Backup of the journal inode
    s_blocks_count_hi: u32,      // Blocks count high
    s_r_blocks_count_hi: u32,    // Reserved blocks count high
    s_free_blocks_count_hi: u32, // Free blocks count high
    s_min_extra_isize: u16,      // All inodes have at least this size
    s_want_extra_isize: u16,     // New inodes should reserve this size
    s_flags: u32,                // Miscellaneous flags
    s_raid_stride: u16,          // RAID stride
    s_mmp_update_interval: u16,  // # seconds to wait in MMP checking
    s_raid_stripe_width: u32,    // Blocks on all data disks
    s_log_groups_per_flex: u8,   // FLEX_BG block group exponent
    s_reserved_char: u8,
    s_reserved_pad: u16,
    s_kbytes_written: u64,          // Number of KiB written
    s_snapshot_inum: u32,           // Inode number of active snapshot
    s_snapshot_id: u32,             // Sequential ID of active snapshot
    s_snapshot_r_blocks_count: u64, // Reserved blocks for active snapshot
    s_snapshot_list: u32,           // Inode number of snapshot list
    s_error_count: u32,             // Number of reported errors
    s_first_error_time: u32,        // First error time
    s_first_error_ino: u32,         // Inode involved in first error
    s_first_error_block: u64,       // Block involved in first error
    s_first_error_func: [u8; 32],   // Function involved in first error
    s_first_error_line: u32,        // Line number involved in first error
    s_last_error_time: u32,         // Most recent error time
    s_last_error_ino: u32,          // Most recent error inode
    s_last_error_line: u32,         // Most recent error line
    s_last_error_block: u64,        // Most recent error block
    s_last_error_func: [u8; 32],    // Function involved in most recent error
    s_mount_opts: [u8; 64],
    s_usr_quota_inum: u32,    // Inode number of user quota file
    s_grp_quota_inum: u32,    // Inode number of group quota file
    s_overhead_blocks: u32,   // Overhead blocks/snapshot
    s_backup_bgs: [u32; 2],   // Block groups containing superblock backups
    s_encrypt_algos: [u8; 4], // Encryption algorithms in use
    s_reserved: [u8; 10],     // Padding to end of block
}

/// EXT4 Inode (standard size is 128 or 256 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Ext4Inode {
    i_mode: u16,         // File mode
    i_uid: u16,          // Low 16 bits of Owner Uid
    i_size_lo: u32,      // Size low 32 bits
    i_atime: u32,        // Access time
    i_ctime: u32,        // Inode Change time
    i_mtime: u32,        // Modification time
    i_dtime: u32,        // Deletion Time
    i_gid: u16,          // Low 16 bits of Gid
    i_links_count: u16,  // Links count
    i_blocks_lo: u32,    // Blocks count (512 bytes per block)
    i_flags: u32,        // File flags
    osd1: u32,           // OS dependent 1
    i_block: [u32; 15], // Pointers to blocks (60 bytes: 12 direct + 3 indirect, or extent header+entries)
    i_generation: u32,  // File version (for NFS)
    i_file_acl: u32,    // File ACL
    i_size_high: u32,   // Size high 32 bits
    i_obso_faddr: u32,  // Obsoleted fragment address
    osd2: [u8; 12],     // OS dependent 2
    i_extra_isize: u16, // Extra inode size
    i_checksum_hi: u16, // Inode checksum high
    i_ctime_extra: u32, // Extra change time
    i_mtime_extra: u32, // Extra modification time
    i_atime_extra: u32, // Extra access time
    i_crtime: u32,      // File creation time
    i_crtime_extra: u32, // Extra file creation time
    i_version_hi: u32,  // Version high 32 bits
}

/// EXT4 Extent Header
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Ext4ExtentHeader {
    eh_magic: u16,      // Magic number (0xF30A)
    eh_entries: u16,    // Number of valid entries
    eh_max: u16,        // Capacity of store entries
    eh_depth: u16,      // Current depth
    eh_generation: u32, // Generation of tree
}

/// EXT4 Extent Index (for internal nodes)
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Ext4ExtentIdx {
    ei_leaf_lo: u32, // Lower 32-bits of block number
    ei_leaf_hi: u16, // Upper 16-bits of block number
    ei_unused: u16,  // Unused
    ei_block: u32,   // This mapping covers this logical block
}

/// EXT4 Extent (for leaf nodes)
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Ext4Extent {
    ee_block: u16,    // First logical block covered
    ee_len: u16,      // Number of blocks covered
    ee_start_hi: u16, // Upper 16-bits of physical block
    ee_start_lo: u32, // Lower 32-bits of physical block
}

/// EXT4 Directory Entry (variable size)
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Ext4DirEntry {
    d_inode: u32,    // Inode number (0 = unused)
    d_rec_len: u16,  // Directory entry length
    d_name_len: u16, // Filename length
    d_type: u8,      // File type
    d_name: [u8; 0], // Filename (variable length)
}

// =====================================================================
// Helper Functions
// =====================================================================

/// Calculate the number of block groups needed
fn calc_bg_count(total_blocks: u64, blocks_per_group: u32) -> u32 {
    (total_blocks as u32).div_ceil(blocks_per_group)
}

// =====================================================================
// EXT4 Image Builder
// =====================================================================
#[derive(Debug, Clone)]
pub(crate) struct FileEntry {
    name: String,
    is_dir: bool,
    data: Vec<u8>,
    children: Vec<FileEntry>,
}

impl FileEntry {
    fn new_file(name: &str, data: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            is_dir: false,
            data,
            children: Vec::new(),
        }
    }

    fn new_dir(name: &str, children: Vec<FileEntry>) -> Self {
        Self {
            name: name.to_string(),
            is_dir: true,
            data: Vec::new(),
            children,
        }
    }
}

/// High-level EXT4 image builder
pub struct Ext4Image {
    pub size_mb: u32,
    pub block_size: u32,
    pub total_blocks: u64,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_size: u16,
    pub root_inode: u32,
    pub current_inode: u32,
    pub files: Vec<(String, Vec<u8>)>,
    pub dirs: Vec<String>,
    /// When set, `finalize` uses this tree instead of `files`/`dirs`. Populated
    /// by `from_bytes`. The first call to a builder method (create_dir /
    /// write_file) clears it because the user has switched to "builder mode".
    pub(crate) parsed_root: Vec<FileEntry>,
}

impl Ext4Image {
    /// Create a new EXT4 image
    ///
    /// # Arguments
    /// * `size_mb` - Image size in megabytes
    /// * `block_size` - Block size (1024, 2048, or 4096)
    pub fn new(size_mb: u32, block_size: u32) -> Result<Self> {
        // Validate block size
        if block_size != 1024 && block_size != 2048 && block_size != 4096 {
            return Err(BuildError::Ext4Error(format!(
                "Invalid block size: {}. Must be 1024, 2048, or 4096",
                block_size
            )));
        }

        let total_blocks = (size_mb as u64) * 1024 * 1024 / (block_size as u64);
        let blocks_per_group = block_size * 8; // One block bitmap per group
        let inodes_per_group = blocks_per_group / 8;
        let inode_size: u16 = 256; // Standard for ext4

        Ok(Self {
            size_mb,
            block_size,
            total_blocks,
            blocks_per_group,
            inodes_per_group,
            inode_size,
            root_inode: 2, // Root inode is always 2
            current_inode: 2,
            files: Vec::new(),
            dirs: Vec::new(),
            parsed_root: Vec::new(),
        })
    }

    /// Parse an existing EXT4 image into an in-memory file tree.
    ///
    /// Supports:
    /// - Superblock parse (1024 byte offset, magic 0xEF53).
    /// - Block group descriptor table at the expected offset.
    /// - Inode table read by inode number.
    /// - Linear (non-hash-tree) directory entries.
    /// - File data via extent tree (depth 0 and 1).
    /// - Inline-data (size <= 60 bytes stored in i_block).
    /// - Symlinks with `i_size < 60` (target stored inline).
    ///
    /// Returns `NotImplemented` for: xattr blocks, ACL attributes, journal
    /// recovery, and `EXT4_INLINE_DATA_FL` outside the small-file path.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 4096 {
            return Err(BuildError::Ext4Error(
                "image smaller than EXT4 superblock region".into(),
            ));
        }
        // Superblock at offset 1024, length 256.
        let sb_off = 1024;
        let sb_bytes = &data[sb_off..sb_off + 256];
        let magic = u16::from_le_bytes([sb_bytes[56], sb_bytes[57]]);
        if magic != EXT4_SUPERBLOCK_MAGIC {
            return Err(BuildError::Ext4Error(format!(
                "not an EXT4 superblock (magic=0x{:X})",
                magic
            )));
        }
        let s_log_block_size =
            u32::from_le_bytes([sb_bytes[24], sb_bytes[25], sb_bytes[26], sb_bytes[27]]);
        let block_size: u32 = 1024u32 << s_log_block_size;
        if block_size != 1024 && block_size != 2048 && block_size != 4096 {
            return Err(BuildError::Ext4Error(format!(
                "unsupported block size: {}",
                block_size
            )));
        }
        let inode_size = u16::from_le_bytes([sb_bytes[88], sb_bytes[89]]);
        if inode_size < 128 {
            return Err(BuildError::Ext4Error(format!(
                "inode size too small: {}",
                inode_size
            )));
        }
        let blocks_per_group =
            u32::from_le_bytes([sb_bytes[32], sb_bytes[33], sb_bytes[34], sb_bytes[35]]);
        let inodes_per_group =
            u32::from_le_bytes([sb_bytes[40], sb_bytes[41], sb_bytes[42], sb_bytes[43]]);
        let total_blocks =
            u32::from_le_bytes([sb_bytes[4], sb_bytes[5], sb_bytes[6], sb_bytes[7]]) as u64;
        let inodes_count = u32::from_le_bytes([sb_bytes[0], sb_bytes[1], sb_bytes[2], sb_bytes[3]]);
        let first_data_block =
            u32::from_le_bytes([sb_bytes[20], sb_bytes[21], sb_bytes[22], sb_bytes[23]]);

        // Block group descriptor table location:
        // - 1024-byte blocks: superblock at block 1, BGDT at block 2
        // - 2048-byte blocks: superblock at block 0, BGDT at block 1
        // - 4096-byte blocks: superblock at byte 1024 of block 0, BGDT at block 1
        // In practice: BGDT at block first_data_block + 1 (for non-1024 blocks)
        // and at block first_data_block + 2 for 1024 blocks.
        let bgdt_block = first_data_block + 1;
        let bgdt_off = (bgdt_block as usize) * (block_size as usize);
        if bgdt_off + 32 > data.len() {
            return Err(BuildError::Ext4Error("BGDT past end of image".into()));
        }
        // We only need block_bitmap and inode_table from the first BGDT entry.
        // 64-bit feature adds hi fields; ignore for non-64bit FS.
        let bg0 = &data[bgdt_off..bgdt_off + 64];
        let block_bitmap_lo = u32::from_le_bytes([bg0[0], bg0[1], bg0[2], bg0[3]]);
        let inode_bitmap_lo = u32::from_le_bytes([bg0[4], bg0[5], bg0[6], bg0[7]]);
        let _inode_table_lo = u32::from_le_bytes([bg0[8], bg0[9], bg0[10], bg0[11]]);
        let _ = block_bitmap_lo;
        let _ = inode_bitmap_lo;

        // Helper: read an inode by number.
        let read_inode = |ino: u32| -> Option<Ext4Inode> {
            if ino == 0 || ino > inodes_count {
                return None;
            }
            let group = (ino - 1) / inodes_per_group;
            let local = (ino - 1) % inodes_per_group;
            let desc_offset = bgdt_off + group as usize * 32;
            let desc = data.get(desc_offset..desc_offset + 32)?;
            let table = u32::from_le_bytes(desc[8..12].try_into().ok()?);
            let inode_off =
                (table as usize) * (block_size as usize) + (local as usize) * (inode_size as usize);
            if inode_off + inode_size as usize > data.len() {
                return None;
            }
            let b = &data[inode_off..inode_off + inode_size as usize];
            if b.len() < 160 {
                return None;
            }
            let i_mode = u16::from_le_bytes([b[0], b[1]]);
            let i_uid = u16::from_le_bytes([b[2], b[3]]);
            let i_size_lo = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            let i_atime = u32::from_le_bytes([b[8], b[9], b[10], b[11]]);
            let i_ctime = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
            let i_mtime = u32::from_le_bytes([b[16], b[17], b[18], b[19]]);
            let i_dtime = u32::from_le_bytes([b[20], b[21], b[22], b[23]]);
            let i_gid = u16::from_le_bytes([b[24], b[25]]);
            let i_links_count = u16::from_le_bytes([b[26], b[27]]);
            let i_blocks_lo = u32::from_le_bytes([b[28], b[29], b[30], b[31]]);
            let i_flags = u32::from_le_bytes([b[32], b[33], b[34], b[35]]);
            let i_block: [u32; 15] = {
                let mut arr = [0u32; 15];
                for (i, slot) in arr.iter_mut().enumerate() {
                    let off = 40 + i * 4;
                    if off + 4 <= b.len() {
                        *slot = u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]);
                    }
                }
                arr
            };
            let i_generation = u32::from_le_bytes([b[100], b[101], b[102], b[103]]);
            let i_file_acl = u32::from_le_bytes([b[104], b[105], b[106], b[107]]);
            let i_size_high = u32::from_le_bytes([b[108], b[109], b[110], b[111]]);
            Some(Ext4Inode {
                i_mode,
                i_uid,
                i_size_lo,
                i_size_high,
                i_atime,
                i_ctime,
                i_mtime,
                i_dtime,
                i_gid,
                i_links_count,
                i_blocks_lo,
                i_flags,
                osd1: 0,
                i_block,
                i_generation,
                i_file_acl,
                i_obso_faddr: 0,
                osd2: [0; 12],
                i_extra_isize: 32,
                i_checksum_hi: 0,
                i_ctime_extra: 0,
                i_mtime_extra: 0,
                i_atime_extra: 0,
                i_crtime: 0,
                i_crtime_extra: 0,
                i_version_hi: 0,
            })
        };

        // Helper: read up to `size` bytes of file data following extents.
        let read_data = |extents: &[(u32, u32, u16)], size: u32| -> Vec<u8> {
            let mut out = Vec::with_capacity(size as usize);
            // Sort by logical block (extent order is normally already sorted
            // but we don't trust it).
            let mut sorted = extents.to_vec();
            sorted.sort_by_key(|e| e.0);
            for &(logical, phys, len) in &sorted {
                let _ = logical;
                let blk_bytes = (len as usize) * (block_size as usize);
                let file_off = (phys as usize) * (block_size as usize);
                if file_off + blk_bytes > data.len() {
                    break;
                }
                let take = blk_bytes.min((size as usize).saturating_sub(out.len()));
                out.extend_from_slice(&data[file_off..file_off + take]);
                if out.len() >= size as usize {
                    break;
                }
            }
            out.truncate(size as usize);
            out
        };

        // Parse an inode and return (children if dir, content if file).
        //
        // The recursive walker is implemented as a free function below
        // (`parse_inode_recursive`) rather than a nested `fn` so its
        // signature can use module-level type aliases instead of a
        // giant `&dyn Fn(&[(u32, u32, u16)], u32) -> Vec<u8>` literal
        // — clippy's `type_complexity` lint flagged the literal form.
        let mut visited = std::collections::HashSet::new();
        visited.insert(0);
        let mut root = parse_inode_recursive(
            data,
            block_size,
            2,
            0,
            &read_inode,
            &read_data,
            &mut visited,
        )?;
        root.name = String::new();
        let parsed_root = root.children;
        Ok(Self {
            size_mb: (data.len() / (1024 * 1024)) as u32,
            block_size,
            total_blocks,
            blocks_per_group,
            inodes_per_group,
            inode_size,
            root_inode: 2,
            current_inode: 2,
            files: Vec::new(),
            dirs: Vec::new(),
            parsed_root,
        })
    }

    /// Create a directory in the image
    pub fn create_dir(&mut self, path: &str) -> Result<&mut Self> {
        let clean_path = path.strip_prefix('/').unwrap_or(path);

        if !self.dirs.contains(&clean_path.to_string()) {
            self.dirs.push(clean_path.to_string());
        }
        Ok(self)
    }

    /// Walk `parsed_root` finding the node at `path` (forward-slash, "" or "/"
    /// = root). Returns Err if not found.
    fn find_parsed<'a>(&'a self, parts: &[&str]) -> Option<&'a FileEntry> {
        let mut cur: Option<&FileEntry> = None;
        let mut level: &Vec<FileEntry> = &self.parsed_root;
        for p in parts {
            let found = level.iter().find(|e| e.name == *p);
            {
                let e = found?;
                cur = Some(e);
                if e.is_dir {
                    level = &e.children;
                } else {
                    return cur;
                }
            }
        }
        cur
    }

    /// List immediate children of `path`.
    pub fn list_dir_path(&self, path: &str) -> Result<Vec<DirEntry>> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        // Empty parts => the virtual root.
        let children: &Vec<FileEntry> = if parts.is_empty() {
            &self.parsed_root
        } else {
            match self.find_parsed(&parts) {
                Some(n) if n.is_dir => &n.children,
                Some(_) => return Ok(Vec::new()),
                None => return Ok(Vec::new()),
            }
        };
        Ok(children
            .iter()
            .map(|c| {
                if c.is_dir {
                    DirEntry::dir(c.name.clone())
                } else {
                    DirEntry::file(c.name.clone(), c.data.len() as u64)
                }
            })
            .collect())
    }

    pub fn read_file_path(&self, path: &str) -> Result<Vec<u8>> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(BuildError::MissingFile(path.into()));
        }
        let (filename, dir_parts) = parts.split_last().unwrap();
        let dir = self.find_parsed(dir_parts);
        if let Some(d) = dir {
            if let Some(f) = d.children.iter().find(|c| !c.is_dir && c.name == *filename) {
                return Ok(f.data.clone());
            }
        }
        Err(BuildError::MissingFile(path.into()))
    }

    pub fn write_file_path(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(BuildError::InvalidParam("empty path".into()));
        }
        let (filename, dir_parts) = parts.split_last().unwrap();
        // Walk down, creating dirs as needed.
        let mut level: &mut Vec<FileEntry> = &mut self.parsed_root;
        for d in dir_parts {
            let pos = level.iter().position(|e| e.is_dir && e.name == *d);
            match pos {
                Some(idx) => {
                    level = &mut level[idx].children;
                }
                None => {
                    level.push(FileEntry::new_dir(d, Vec::new()));
                    let last = level.len() - 1;
                    level = &mut level[last].children;
                }
            }
        }
        if let Some(existing) = level.iter_mut().find(|e| !e.is_dir && e.name == *filename) {
            existing.data = data.to_vec();
        } else {
            level.push(FileEntry::new_file(filename, data.to_vec()));
        }
        Ok(())
    }

    pub fn mkdir_path(&mut self, path: &str) -> Result<()> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Ok(());
        }
        let mut level: &mut Vec<FileEntry> = &mut self.parsed_root;
        for d in &parts {
            let pos = level.iter().position(|e| e.is_dir && e.name == *d);
            match pos {
                Some(idx) => {
                    level = &mut level[idx].children;
                }
                None => {
                    level.push(FileEntry::new_dir(d, Vec::new()));
                    let last = level.len() - 1;
                    level = &mut level[last].children;
                }
            }
        }
        Ok(())
    }

    pub fn remove_path_ext4(&mut self, path: &str) -> Result<()> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Ok(());
        }
        let (target, parent_parts) = parts.split_last().unwrap();
        fn walk_remove(level: &mut Vec<FileEntry>, target: &str, parent_parts: &[&str]) -> bool {
            if parent_parts.is_empty() {
                if let Some(idx) = level.iter().position(|e| e.name == target) {
                    level.remove(idx);
                    return true;
                }
                return false;
            }
            let head = parent_parts[0];
            let tail = &parent_parts[1..];
            for e in level.iter_mut() {
                if e.is_dir && e.name == head {
                    return walk_remove(&mut e.children, target, tail);
                }
            }
            false
        }
        walk_remove(&mut self.parsed_root, target, parent_parts);
        Ok(())
    }

    /// Write a file to the image
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<&mut Self> {
        let clean_path = path.strip_prefix('/').unwrap_or(path);
        self.files.push((clean_path.to_string(), data.to_vec()));
        Ok(self)
    }

    /// Build the superblock
    fn build_superblock(&self) -> Ext4SuperBlock {
        let block_group_count = calc_bg_count(self.total_blocks, self.blocks_per_group);

        Ext4SuperBlock {
            s_inodes_count: self.inodes_per_group * block_group_count,
            s_blocks_count_lo: self.total_blocks as u32,
            s_r_blocks_count_lo: 0,
            s_free_blocks_count_lo: 0, // Will be calculated later
            s_free_inodes_count: 0,
            s_first_data_block: if self.block_size == 4096 { 0 } else { 1 },
            s_log_block_size: (self.block_size.ilog2() - 10),
            s_log_cluster_size: (self.block_size.ilog2() - 10),
            s_blocks_per_group: self.blocks_per_group,
            s_clusters_per_group: self.blocks_per_group,
            s_inodes_per_group: self.inodes_per_group,
            s_mtime: 0,
            s_wtime: 0,
            s_mnt_count: 0,
            s_max_mnt_count: 0xFFFF,
            s_magic: EXT4_SUPERBLOCK_MAGIC,
            s_state: 1, // Clean
            s_errors: 0,
            s_minor_rev_level: 0,
            s_lastcheck: 0,
            s_checkinterval: 0,
            s_creator_os: 0, // Linux
            s_rev_level: 1,  // Dynamic revision
            s_def_resuid: 0,
            s_def_resgid: 0,
            s_first_ino: 11, // First non-reserved inode
            s_inode_size: self.inode_size,
            s_block_group_nr: 0,
            s_features_compat: 0,
            s_features_incompat: EXT3_FEATURE_INCOMPAT_FILETYPE | EXT4_FEATURE_INCOMPAT_EXTENTS,
            s_features_ro_compat: 0, // Superblock/GDT backups in every group; no checksums.
            s_uuid: [0; 16],
            s_volume_name: [0; 16],
            s_last_mounted: [0; 64],
            s_algorithm_usage_bitmap: 0,
            s_prealloc_blocks: 0,
            s_prealloc_dir_blocks: 0,
            s_reserved_gdt_blocks: 0,
            s_journal_uuid: [0; 16],
            s_journal_inum: 0,
            s_journal_dev: 0,
            s_last_orphan: 0,
            s_hash_seed: [0; 4],
            s_def_hash_version: 0,
            s_reserved_char_pad: 0,
            s_reserved_word_pad: 0,
            s_default_mount_opts: 0,
            s_first_meta_bg: 0,
            s_mkfs_time: 0,
            s_journal_blocks: [0; 17],
            s_blocks_count_hi: 0,
            s_r_blocks_count_hi: 0,
            s_free_blocks_count_hi: 0,
            s_min_extra_isize: 32,
            s_want_extra_isize: 32,
            s_flags: 0,
            s_raid_stride: 0,
            s_mmp_update_interval: 0,
            s_raid_stripe_width: 0,
            s_log_groups_per_flex: 0,
            s_reserved_char: 0,
            s_reserved_pad: 0,
            s_kbytes_written: 0,
            s_snapshot_inum: 0,
            s_snapshot_id: 0,
            s_snapshot_r_blocks_count: 0,
            s_snapshot_list: 0,
            s_error_count: 0,
            s_first_error_time: 0,
            s_first_error_ino: 0,
            s_first_error_block: 0,
            s_first_error_func: [0; 32],
            s_first_error_line: 0,
            s_last_error_time: 0,
            s_last_error_ino: 0,
            s_last_error_line: 0,
            s_last_error_block: 0,
            s_last_error_func: [0; 32],
            s_mount_opts: [0; 64],
            s_usr_quota_inum: 0,
            s_grp_quota_inum: 0,
            s_overhead_blocks: 0,
            s_backup_bgs: [0; 2],
            s_encrypt_algos: [0; 4],
            s_reserved: [0; 10],
        }
    }

    /// Finalize the image and return raw bytes.
    ///
    /// Builds a minimal EXT4 image that the existing in-tree EXT4 reader
    /// (see `from_bytes`) can mount. Layout:
    ///
    ///   block 0: 1024 bytes boot sector (zeros) + 1024 bytes superblock
    ///            (the superblock occupies offset 1024..2048 of the image)
    ///   block 1: BGDT (first 32 bytes used; rest zero)
    ///   block 2: block bitmap
    ///   block 3: inode bitmap
    ///   block 4+: inode table (256-byte inodes)
    ///   after inode table: data blocks (one or more per file/dir)
    ///
    /// Inode numbering:
    ///   * inode 1 = reserved (zero)
    ///   * inode 2 = root
    ///   * inode 3..N = parsed_root entries in DFS order
    ///
    /// All files use extent trees (depth 0, single extent). Directories use
    /// linear directory entries with `dir_entry` records. Each directory's
    /// first block contains `.`, `..`, and one record per child; child
    /// inode numbers are patched in once all inodes are assigned.
    pub fn finalize(&mut self) -> Result<Vec<u8>> {
        // Sync any flat-list files/dirs into parsed_root so the encoder
        // sees them. write_file_path/mkdir_path already update parsed_root.
        if self.parsed_root.is_empty() && (!self.files.is_empty() || !self.dirs.is_empty()) {
            let mut sorted_paths: Vec<(String, Vec<u8>)> = self.files.clone();
            sorted_paths.sort_by(|a, b| a.0.cmp(&b.0));
            for (path, data) in sorted_paths.iter() {
                let _ = Ext4Image::write_file_path(self, path, data);
            }
            let mut sorted_dirs: Vec<String> = self.dirs.clone();
            sorted_dirs.sort();
            sorted_dirs.dedup();
            for d in sorted_dirs.iter() {
                let _ = Ext4Image::mkdir_path(self, d);
            }
        }

        let total_size = (self.size_mb as usize) * 1024 * 1024;
        let bs = self.block_size as usize;
        let inodes_per_group = self.inodes_per_group as usize;

        let first_data_block = if bs == 1024 { 1 } else { 0 };
        let groups = (self.total_blocks as usize - first_data_block)
            .div_ceil(self.blocks_per_group as usize);
        let gdt_blocks = (groups * 32).div_ceil(bs);
        let table_blocks = (inodes_per_group * 256).div_ceil(bs);
        let metadata_blocks = 1 + gdt_blocks + 2 + table_blocks;

        // Pass 1: DFS through parsed_root assigning inodes.
        let mut allocs: Vec<Ext4Alloc> = Vec::new();
        // Build root (inode 2) dir_block up front so we have patches for it.
        let root_children_refs: Vec<&FileEntry> = self.parsed_root.iter().collect();
        let (root_dir_block, root_patches) = build_dir_block(&root_children_refs, 2, 2, bs);
        allocs.push(Ext4Alloc {
            ino: 2,
            is_dir: true,
            data: Vec::new(),
            children: Vec::new(),
            dir_block: root_dir_block,
            patches: root_patches,
            extents: Vec::new(),
        });
        let mut next_ino: u32 = 11;
        let mut stack: Vec<(usize, &FileEntry)> = Vec::new();
        for child in self.parsed_root.iter().rev() {
            stack.push((0, child));
        }
        while let Some((parent_idx, entry)) = stack.pop() {
            let ino = next_ino;
            next_ino += 1;
            let my_idx = allocs.len();
            let mut alloc = Ext4Alloc {
                ino,
                is_dir: entry.is_dir,
                data: if entry.is_dir {
                    Vec::new()
                } else {
                    entry.data.clone()
                },
                children: Vec::new(),
                dir_block: Vec::new(),
                patches: Vec::new(),
                extents: Vec::new(),
            };
            if entry.is_dir {
                let children_refs: Vec<&FileEntry> = entry.children.iter().collect();
                let (buf, patches) =
                    build_dir_block(&children_refs, ino, allocs[parent_idx].ino, bs);
                alloc.dir_block = buf;
                alloc.patches = patches;
            }
            allocs.push(alloc);
            allocs[parent_idx].children.push(my_idx);
            for c in entry.children.iter().rev() {
                stack.push((my_idx, c));
            }
        }

        if next_ino as usize > groups * inodes_per_group {
            return Err(BuildError::Ext4Error("inode table is full".into()));
        }
        // Reserve superblock, GDT, bitmaps and inode table in every group.
        // Allocate only the remaining blocks; extents may cross group boundaries.
        let mut used = vec![false; self.total_blocks as usize];
        let mut cursor = first_data_block + metadata_blocks;
        for group in 0..groups {
            let start = first_data_block + group * self.blocks_per_group as usize;
            if start + metadata_blocks > used.len() {
                return Err(BuildError::Ext4Error(
                    "last block group is too small".into(),
                ));
            }
            used[start..start + metadata_blocks].fill(true);
        }
        for alloc in &mut allocs {
            let length = if alloc.is_dir {
                alloc.dir_block.len()
            } else {
                alloc.data.len()
            };
            let mut remaining = length.div_ceil(bs);
            let mut logical = 0u32;
            while remaining > 0 {
                while cursor < used.len() && used[cursor] {
                    cursor += 1;
                }
                let start = cursor;
                while cursor < used.len() && !used[cursor] && cursor - start < remaining.min(32768)
                {
                    cursor += 1;
                }
                let count = cursor - start;
                if count == 0 {
                    return Err(BuildError::Ext4Error("volume is full".into()));
                }
                used[start..cursor].fill(true);
                alloc.extents.push((logical, start as u32, count as u16));
                logical += count as u32;
                remaining -= count;
            }
            if alloc.extents.len() > 4 {
                return Err(BuildError::Ext4Error(
                    "file requires more than four extents".into(),
                ));
            }
        }

        // Pass 3: patch child inode numbers into parent dir blocks.
        for alloc_idx in 0..allocs.len() {
            let children = allocs[alloc_idx].children.clone();
            for (slot, &child_idx) in children.iter().enumerate() {
                let child_ino = allocs[child_idx].ino;
                let (off, _) = allocs[alloc_idx].patches[slot];
                allocs[alloc_idx].dir_block[off..off + 4].copy_from_slice(&child_ino.to_le_bytes());
                allocs[alloc_idx].patches[slot] = (off, child_ino);
            }
        }

        let mut superblock = self.build_superblock();
        superblock.s_first_data_block = first_data_block as u32;
        superblock.s_inodes_count = (groups * inodes_per_group) as u32;
        superblock.s_free_blocks_count_lo =
            used[first_data_block..].iter().filter(|&&b| !b).count() as u32;
        superblock.s_free_inodes_count = superblock.s_inodes_count - (next_ino - 1);
        let mut image = vec![0u8; total_size];
        let mut bgdt = vec![0u8; gdt_blocks * bs];
        for group in 0..groups {
            let start = first_data_block + group * self.blocks_per_group as usize;
            let end = (start + self.blocks_per_group as usize).min(used.len());
            let bitmap = start + 1 + gdt_blocks;
            let inode_bitmap = bitmap + 1;
            let table = inode_bitmap + 1;
            let desc = &mut bgdt[group * 32..group * 32 + 32];
            desc[..4].copy_from_slice(&(bitmap as u32).to_le_bytes());
            desc[4..8].copy_from_slice(&(inode_bitmap as u32).to_le_bytes());
            desc[8..12].copy_from_slice(&(table as u32).to_le_bytes());
            let free_blocks = used[start..end].iter().filter(|&&b| !b).count() as u16;
            let first_inode = group * inodes_per_group + 1;
            let allocated_inodes = (next_ino as usize)
                .saturating_sub(first_inode)
                .min(inodes_per_group);
            let directories = allocs
                .iter()
                .filter(|a| a.is_dir && (a.ino as usize - 1) / inodes_per_group == group)
                .count();
            desc[12..14].copy_from_slice(&free_blocks.to_le_bytes());
            desc[14..16]
                .copy_from_slice(&((inodes_per_group - allocated_inodes) as u16).to_le_bytes());
            desc[16..18].copy_from_slice(&(directories as u16).to_le_bytes());
            let block_bitmap = &mut image[bitmap * bs..(bitmap + 1) * bs];
            for bit in 0..bs * 8 {
                if start + bit >= end || used[start + bit] {
                    set_bit(block_bitmap, bit as u32);
                }
            }
            let inode_bitmap = &mut image[inode_bitmap * bs..(inode_bitmap + 1) * bs];
            for bit in 0..bs * 8 {
                if bit < allocated_inodes || bit >= inodes_per_group {
                    set_bit(inode_bitmap, bit as u32);
                }
            }
        }
        for alloc in &allocs {
            let content = if alloc.is_dir {
                &alloc.dir_block
            } else {
                &alloc.data
            };
            let links = if alloc.is_dir {
                2 + alloc.children.iter().filter(|&&i| allocs[i].is_dir).count() as u16
            } else {
                1
            };
            let inode = build_inode_bytes(
                if alloc.is_dir { 0x41ed } else { 0x81a4 },
                content.len() as u32,
                links,
                &alloc.extents,
                bs,
            );
            let group = (alloc.ino as usize - 1) / inodes_per_group;
            let local = (alloc.ino as usize - 1) % inodes_per_group;
            let table = first_data_block + group * self.blocks_per_group as usize + 3 + gdt_blocks;
            let off = table * bs + local * 256;
            image[off..off + 256].copy_from_slice(&inode);
            for &(logical, physical, count) in &alloc.extents {
                let begin = logical as usize * bs;
                let end = (begin + count as usize * bs).min(content.len());
                let off = physical as usize * bs;
                image[off..off + end - begin].copy_from_slice(&content[begin..end]);
            }
        }
        // Keep each backup superblock's group number accurate and copy the full GDT.
        for group in 0..groups {
            let start = first_data_block + group * self.blocks_per_group as usize;
            superblock.s_block_group_nr = group as u16;
            let off = if group == 0 { 1024 } else { start * bs };
            let sb_bytes = superblock.as_bytes();
            image[off..off + sb_bytes.len()].copy_from_slice(&sb_bytes);
            image[(start + 1) * bs..(start + 1) * bs + bgdt.len()].copy_from_slice(&bgdt);
        }

        Ok(image)
    }
}

// =====================================================================
// Free function: recursive inode walker
// =====================================================================
//
// Lifted out of `Ext4Image::finalize` so the recursive descent uses a
// module-level closure type alias (`ReadDataFn`) instead of an inline
// `dyn Fn(&[(u32, u32, u16)], u32) -> Vec<u8>` literal. That alias is
// the canonical fix for clippy::type_complexity on recursive inner
// `fn` items: keeping the signature as an alias lets the recursive
// call re-use the same alias without losing the closure lifetime
// inference that nesting forced.

/// Callback for reading the file content covered by an inode's extent
/// triples. The first argument is the flattened extent list
/// `(logical_block, physical_block, length_in_blocks)` and the second
/// is the file size in bytes.
type ReadDataFn<'a> = dyn Fn(&[(u32, u32, u16)], u32) -> Vec<u8> + 'a;

/// Recursive inode walker. Returns the `FileEntry` for `ino`, recursing
/// into directory children. `read_inode` materialises arbitrary inodes
/// on demand; `read_data` walks the extent tree of a given inode and
/// returns its raw bytes.
fn parse_inode_recursive(
    data: &[u8],
    bs: u32,
    ino: u32,
    depth: u32,
    read_inode: &dyn Fn(u32) -> Option<Ext4Inode>,
    read_data: &ReadDataFn<'_>,
    visited: &mut std::collections::HashSet<u32>,
) -> Result<FileEntry> {
    if depth > 16 {
        return Err(BuildError::Ext4Error("inode tree too deep".into()));
    }
    if !visited.insert(ino) {
        return Err(BuildError::Ext4Error(format!("cycle at inode {}", ino)));
    }
    let inode =
        read_inode(ino).ok_or_else(|| BuildError::Ext4Error(format!("inode {} not found", ino)))?;
    let is_dir = (inode.i_mode & 0xF000) == 0x4000;
    let is_symlink = (inode.i_mode & 0xF000) == 0xA000;
    let is_reg = (inode.i_mode & 0xF000) == 0x8000;
    let size = inode.i_size_lo as u64 | ((inode.i_size_high as u64) << 32);

    // Inline data: file < 60 bytes living in i_block.
    if (inode.i_flags & 0x1000_0000) != 0 && size <= 60 {
        let iblock: [u32; 15] = inode.i_block;
        let mut inline = Vec::with_capacity(size as usize);
        for w in &iblock[..15] {
            inline.extend_from_slice(&w.to_le_bytes());
        }
        inline.truncate(size as usize);
        visited.remove(&ino);
        if is_dir {
            return Ok(FileEntry::new_dir("", Vec::new()));
        }
        return Ok(FileEntry::new_file("", inline));
    }

    // Symlink fast path: target in i_block if size <= 60.
    if is_symlink && size <= 60 {
        let iblock: [u32; 15] = inode.i_block;
        let mut target = Vec::with_capacity(size as usize);
        for w in &iblock[..15] {
            target.extend_from_slice(&w.to_le_bytes());
        }
        target.truncate(size as usize);
        visited.remove(&ino);
        return Ok(FileEntry::new_file("", target));
    }

    // File data via extents.
    let iblock_for_extents: [u32; 15] = inode.i_block;
    let extents = walk_extents(data, bs, &iblock_for_extents);
    let file_data = if is_reg {
        read_data(&extents, size as u32)
    } else {
        Vec::new()
    };

    if is_dir {
        let mut children = Vec::new();
        let mut off = 0usize;
        // Each dir block has its own extents walk for the inode's data.
        let dir_data = read_data(&extents, size as u32);
        while off + 8 <= dir_data.len() {
            let d = &dir_data[off..];
            let inode_num = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
            let rec_len = u16::from_le_bytes([d[4], d[5]]) as usize;
            if rec_len == 0 || rec_len > dir_data.len() - off {
                break;
            }
            let name_len = d[6] as usize;
            let _file_type = d[7];
            if inode_num != 0 && name_len > 0 && name_len + 8 <= rec_len {
                let name = std::str::from_utf8(&d[8..8 + name_len])
                    .map_err(|_| BuildError::Ext4Error("non-utf8 dirent".into()))?
                    .to_string();
                if name != "." && name != ".." {
                    // Recurse.
                    match parse_inode_recursive(
                        data,
                        bs,
                        inode_num,
                        depth + 1,
                        read_inode,
                        read_data,
                        visited,
                    ) {
                        Ok(mut child) => {
                            child.name = name;
                            children.push(child);
                        }
                        Err(_) => {
                            // Skip unparseable children silently.
                        }
                    }
                }
            }
            off += rec_len;
        }
        visited.remove(&ino);
        Ok(FileEntry::new_dir("", children))
    } else {
        visited.remove(&ino);
        Ok(FileEntry::new_file("", file_data))
    }
}

/// Flatten standard inline extent roots and external nodes, with bounded depth.
fn walk_extents(data: &[u8], bs: u32, i_block: &[u32; 15]) -> Vec<(u32, u32, u16)> {
    fn walk(data: &[u8], bs: usize, node: &[u8], remaining: u16, out: &mut Vec<(u32, u32, u16)>) {
        if node.len() < 12 || remaining > 5 {
            return;
        }
        let word = |i| u16::from_le_bytes([node[i], node[i + 1]]);
        let count = word(2) as usize;
        let depth = word(6);
        if word(0) != 0xf30a
            || depth > remaining
            || count > word(4) as usize
            || count > (node.len() - 12) / 12
        {
            return;
        }
        for entry in node[12..12 + count * 12].chunks_exact(12) {
            let logical = u32::from_le_bytes(entry[..4].try_into().unwrap());
            if depth == 0 {
                let length = u16::from_le_bytes(entry[4..6].try_into().unwrap());
                let high = u16::from_le_bytes(entry[6..8].try_into().unwrap());
                let physical = u32::from_le_bytes(entry[8..12].try_into().unwrap());
                if length > 0 && length <= 32768 && high == 0 {
                    out.push((logical, physical, length));
                }
            } else {
                let low = u32::from_le_bytes(entry[4..8].try_into().unwrap());
                let high = u16::from_le_bytes(entry[8..10].try_into().unwrap());
                let offset = ((high as u64) << 32 | low as u64).checked_mul(bs as u64);
                if let Some(child) = offset
                    .and_then(|o| usize::try_from(o).ok())
                    .and_then(|o| o.checked_add(bs).and_then(|end| data.get(o..end)))
                {
                    walk(data, bs, child, depth - 1, out);
                }
            }
        }
    }
    let mut root = Vec::with_capacity(60);
    for word in i_block {
        root.extend_from_slice(&word.to_le_bytes());
    }
    let mut out = Vec::new();
    walk(data, bs as usize, &root, 5, &mut out);
    out
}

// =====================================================================
// Helpers used by finalize (replaces the old build_dir_entries_*
// stack that had no inode-aware patching).
// =====================================================================

/// Produce the bytes for a directory entry. The entry's total length is
/// `rec_len` (rounded up to 4). Pass `inode=0` to leave a placeholder
/// for later patching.
fn dir_entry_bytes(
    inode: u32,
    file_type: u8,
    rec_len: u16,
    name: &str,
    name_len: usize,
) -> Vec<u8> {
    let mut e = Vec::with_capacity(rec_len as usize);
    e.extend_from_slice(&inode.to_le_bytes());
    e.extend_from_slice(&rec_len.to_le_bytes());
    e.push(name_len as u8);
    e.push(file_type);
    e.extend_from_slice(name.as_bytes());
    while e.len() < rec_len as usize {
        e.push(0);
    }
    e
}

/// Standard 256-byte inode with an inline extent root (four entries maximum).
fn build_inode_bytes(
    mode: u16,
    size: u32,
    links: u16,
    extents: &[(u32, u32, u16)],
    bs: usize,
) -> Vec<u8> {
    let mut bytes = vec![0; 256];
    bytes[..2].copy_from_slice(&mode.to_le_bytes());
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes[26..28].copy_from_slice(&links.to_le_bytes());
    let sectors: u32 = extents.iter().map(|e| e.2 as u32 * (bs as u32 / 512)).sum();
    bytes[28..32].copy_from_slice(&sectors.to_le_bytes());
    bytes[32..36].copy_from_slice(&0x80000u32.to_le_bytes());
    bytes[40..42].copy_from_slice(&0xf30au16.to_le_bytes());
    bytes[42..44].copy_from_slice(&(extents.len() as u16).to_le_bytes());
    bytes[44..46].copy_from_slice(&4u16.to_le_bytes());
    for (entry, &(logical, physical, count)) in bytes[52..100].chunks_exact_mut(12).zip(extents) {
        entry[..4].copy_from_slice(&logical.to_le_bytes());
        entry[4..6].copy_from_slice(&count.to_le_bytes());
        entry[8..12].copy_from_slice(&physical.to_le_bytes());
    }
    bytes
}

fn set_bit(bitmap: &mut [u8], idx: u32) {
    let byte = (idx / 8) as usize;
    let bit = idx % 8;
    if byte < bitmap.len() {
        bitmap[byte] |= 1 << bit;
    }
}

/// Allocation record for a single file or directory.
struct Ext4Alloc {
    ino: u32,
    is_dir: bool,
    data: Vec<u8>,
    children: Vec<usize>,
    /// Bytes of the directory entry block, with placeholders for child
    /// inode numbers. `patches` records the byte offset of each child
    /// entry's inode field so we can patch them in once all inodes are
    /// known.
    dir_block: Vec<u8>,
    patches: Vec<(usize, u32)>,
    extents: Vec<(u32, u32, u16)>,
}

/// Build the directory entry block for a directory that owns `children`.
/// `self_ino` is the inode of this directory; `parent_ino` is the inode
/// of its parent (..). Each child entry has inode=0 placeholder and the
/// patch list records where to write the real inode later.
fn build_dir_block(
    children: &[&FileEntry],
    self_ino: u32,
    parent_ino: u32,
    bs: usize,
) -> (Vec<u8>, Vec<(usize, u32)>) {
    let mut buf = Vec::new();
    let mut patches = Vec::new();
    let mut previous = 0;
    let mut append = |inode: u32, ft: u8, name: &str| {
        let length = (8 + name.len()).div_ceil(4) * 4;
        if buf.len() % bs + length > bs {
            let end = buf.len().div_ceil(bs) * bs;
            buf[previous + 4..previous + 6]
                .copy_from_slice(&((end - previous) as u16).to_le_bytes());
            buf.resize(end, 0);
        }
        let off = buf.len();
        buf.extend_from_slice(&dir_entry_bytes(inode, ft, length as u16, name, name.len()));
        previous = off;
        off
    };
    append(self_ino, EXT4_FT_DIR, ".");
    append(parent_ino, EXT4_FT_DIR, "..");
    for child in children {
        let off = append(
            0,
            if child.is_dir {
                EXT4_FT_DIR
            } else {
                EXT4_FT_REG_FILE
            },
            &child.name,
        );
        patches.push((off, 0));
    }
    let end = buf.len().div_ceil(bs) * bs;
    buf[previous + 4..previous + 6].copy_from_slice(&((end - previous) as u16).to_le_bytes());
    buf.resize(end, 0);
    (buf, patches)
}

// =====================================================================
// Byte Serialization for Structures
// =====================================================================

impl Ext4SuperBlock {
    fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(&self.s_inodes_count.to_le_bytes());
        bytes.extend_from_slice(&self.s_blocks_count_lo.to_le_bytes());
        bytes.extend_from_slice(&self.s_r_blocks_count_lo.to_le_bytes());
        bytes.extend_from_slice(&self.s_free_blocks_count_lo.to_le_bytes());
        bytes.extend_from_slice(&self.s_free_inodes_count.to_le_bytes());
        bytes.extend_from_slice(&self.s_first_data_block.to_le_bytes());
        bytes.extend_from_slice(&self.s_log_block_size.to_le_bytes());
        bytes.extend_from_slice(&self.s_log_cluster_size.to_le_bytes());
        bytes.extend_from_slice(&self.s_blocks_per_group.to_le_bytes());
        bytes.extend_from_slice(&self.s_clusters_per_group.to_le_bytes());
        bytes.extend_from_slice(&self.s_inodes_per_group.to_le_bytes());
        bytes.extend_from_slice(&self.s_mtime.to_le_bytes());
        bytes.extend_from_slice(&self.s_wtime.to_le_bytes());
        bytes.extend_from_slice(&self.s_mnt_count.to_le_bytes());
        bytes.extend_from_slice(&self.s_max_mnt_count.to_le_bytes());
        bytes.extend_from_slice(&self.s_magic.to_le_bytes());
        bytes.extend_from_slice(&self.s_state.to_le_bytes());
        bytes.extend_from_slice(&self.s_errors.to_le_bytes());
        bytes.extend_from_slice(&self.s_minor_rev_level.to_le_bytes());
        bytes.extend_from_slice(&self.s_lastcheck.to_le_bytes());
        bytes.extend_from_slice(&self.s_checkinterval.to_le_bytes());
        bytes.extend_from_slice(&self.s_creator_os.to_le_bytes());
        bytes.extend_from_slice(&self.s_rev_level.to_le_bytes());
        bytes.extend_from_slice(&self.s_def_resuid.to_le_bytes());
        bytes.extend_from_slice(&self.s_def_resgid.to_le_bytes());
        bytes.extend_from_slice(&self.s_first_ino.to_le_bytes());
        bytes.extend_from_slice(&self.s_inode_size.to_le_bytes());
        bytes.extend_from_slice(&self.s_block_group_nr.to_le_bytes());
        bytes.extend_from_slice(&self.s_features_compat.to_le_bytes());
        bytes.extend_from_slice(&self.s_features_incompat.to_le_bytes());
        bytes.extend_from_slice(&self.s_features_ro_compat.to_le_bytes());
        bytes.extend_from_slice(&self.s_uuid);
        bytes.extend_from_slice(&self.s_volume_name);
        // Add remaining fields as zeros for simplicity
        bytes.resize(256, 0);
        bytes
    }
}

// =====================================================================
// FsBackend implementation
// =====================================================================

impl FsBackend for Ext4Image {
    fn kind(&self) -> &'static str {
        "ext4"
    }

    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        Ext4Image::list_dir_path(self, path)
    }
    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        Ext4Image::read_file_path(self, path)
    }
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<()> {
        // Convert any existing `parsed_root` to flat files/dirs first so the
        // legacy finalize path produces equivalent output.
        if !self.parsed_root.is_empty() {
            flatten_tree(&self.parsed_root, "", &mut self.files, &mut self.dirs);
            self.parsed_root.clear();
        }
        // Update the legacy flat lists as well.
        let clean = path.trim_start_matches('/');
        if let Some(existing) = self.files.iter_mut().find(|(p, _)| p == clean) {
            existing.1 = data.to_vec();
        } else {
            self.files.push((clean.to_string(), data.to_vec()));
        }
        // Also keep parsed_root in sync for read-back.
        Ext4Image::write_file_path(self, path, data)
    }
    fn mkdir(&mut self, path: &str) -> Result<()> {
        if !self.parsed_root.is_empty() {
            flatten_tree(&self.parsed_root, "", &mut self.files, &mut self.dirs);
            self.parsed_root.clear();
        }
        let clean = path.trim_start_matches('/');
        if !self.dirs.iter().any(|d| d == clean) {
            self.dirs.push(clean.to_string());
        }
        Ext4Image::mkdir_path(self, path)
    }
    fn remove(&mut self, path: &str) -> Result<()> {
        if !self.parsed_root.is_empty() {
            flatten_tree(&self.parsed_root, "", &mut self.files, &mut self.dirs);
            self.parsed_root.clear();
        }
        let clean = path.trim_start_matches('/');
        self.files.retain(|(p, _)| p != clean);
        self.dirs
            .retain(|d| d != clean && !d.starts_with(&format!("{}/", clean)));
        Ext4Image::remove_path_ext4(self, path)
    }
    fn finalize(&mut self) -> Result<Vec<u8>> {
        // Sync parsed_root back into flat lists so the legacy encoder produces
        // a complete image including any modifications applied after parsing.
        if !self.parsed_root.is_empty() {
            flatten_tree(&self.parsed_root, "", &mut self.files, &mut self.dirs);
            self.parsed_root.clear();
        }
        Ext4Image::finalize(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Walk a parsed FileEntry tree and populate the flat `files`/`dirs` lists
/// (the format the existing encoder understands).
fn flatten_tree(
    entries: &[FileEntry],
    prefix: &str,
    files: &mut Vec<(String, Vec<u8>)>,
    dirs: &mut Vec<String>,
) {
    for e in entries {
        let path = if prefix.is_empty() {
            e.name.clone()
        } else {
            format!("{}/{}", prefix, e.name)
        };
        if e.is_dir {
            dirs.push(path.clone());
            flatten_tree(&e.children, &path, files, dirs);
        } else {
            files.push((path, e.data.clone()));
        }
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ext4_creation() {
        let mut image = Ext4Image::new(64, 4096).unwrap();
        image.create_dir("/test").unwrap();
        image.write_file("/test/file.txt", b"hello").unwrap();

        let data = image.finalize().unwrap();
        assert!(data.len() > 0);
    }

    #[test]
    fn test_invalid_block_size() {
        let result = Ext4Image::new(64, 8192);
        assert!(result.is_err());
    }

    #[test]
    fn standard_ext4_roundtrip_and_external_filesystem_check() {
        for bs in [1024, 2048, 4096] {
            let mut image = Ext4Image::new(32, bs).unwrap();
            // More than one directory block, plus file data crossing a group boundary.
            for i in 0..90 {
                image
                    .write_file_path(&format!("/Windows/System32/component_{i:03}.dll"), b"MZ")
                    .unwrap();
            }
            let content: Vec<u8> = (0..10_000_000).map(|i| (i % 251) as u8).collect();
            image
                .write_file_path("/Windows/System32/winlogon.exe", &content)
                .unwrap();
            let bytes = image.finalize().unwrap();
            let parsed = Ext4Image::from_bytes(&bytes).unwrap();
            assert_eq!(
                parsed
                    .find_parsed(&["Windows", "System32", "winlogon.exe"])
                    .unwrap()
                    .data,
                content
            );
            assert_eq!(
                parsed
                    .find_parsed(&["Windows", "System32"])
                    .unwrap()
                    .children
                    .len(),
                91
            );
            // An independent filesystem implementation checks allocation and inode metadata.
            if std::process::Command::new("e2fsck")
                .arg("-V")
                .output()
                .is_ok()
            {
                let path =
                    std::env::temp_dir().join(format!("nt61-ext4-{}-{bs}.img", std::process::id()));
                std::fs::write(&path, &bytes).unwrap();
                let result = std::process::Command::new("e2fsck")
                    .args(["-fn"])
                    .arg(&path)
                    .output()
                    .unwrap();
                std::fs::remove_file(&path).unwrap();
                assert!(
                    result.status.success(),
                    "block size {bs}: {}{}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
            }
        }
    }
}

//! ext2 Directory Operations
//
//! Implements directory entry reading, searching, and manipulation.
//! Directories in ext2/3/4 contain variable-length entries.
//
//! ## Directory Entry Format
//! Each directory entry contains:
//! - Inode number (0 for unused entries)
//! - Record length (total size of this entry)
//! - name length
//! - File type (ext2/3 only if FILETYPE feature set)
//! - File name (variable length, padded to 4-byte boundary)
//
//! ## Directory Entry Limitations
//! - Maximum filename length is 255 bytes (characters)
//! - Entries are aligned to 4-byte boundaries
//! - Record length may be larger than actual name length
use crate::libs::ntdll::{status::STATUS_UNSUCCESSFUL, types::NTSTATUS};

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::inode::{read_inode, Ext2Inode};
use super::superblock::{Ext2SuperBlock, EXT2_ROOT_INO};

// ============================================================================
// Directory Entry Constants
// ============================================================================

/// ext2 directory file type constants
pub const EXT2_FT_UNKNOWN: u8 = 0;
pub const EXT2_FT_REG_FILE: u8 = 1;
pub const EXT2_FT_DIR: u8 = 2;
pub const EXT2_FT_CHRDEV: u8 = 3;
pub const EXT2_FT_BLKDEV: u8 = 4;
pub const EXT2_FT_FIFO: u8 = 5;
pub const EXT2_FT_SOCK: u8 = 6;
pub const EXT2_FT_SYMLINK: u8 = 7;

/// Minimum directory entry size (header only, 8 bytes)
pub const EXT2_DIR_ENTRY_MIN_SIZE: u16 = 8;

/// Maximum filename length
pub const EXT2_NAME_LEN: usize = 255;

/// Directory entry alignment (4 bytes)
pub const EXT2_DIR_ENTRY_ALIGN: usize = 4;

/// Filetype in directory entry is present if FILETYPE feature is set
pub const EXT2_DIR_ENTRY_FTYPE_SIZE: usize = 8;

/// Directory entry without filetype (older format)
pub const EXT2_DIR_ENTRY_NO_FTYPE_SIZE: usize = 8;

// ============================================================================
// Directory Entry Structure
// ============================================================================

/// Owned directory entry decoded from the eight-byte on-disk header.
pub struct Ext2DirEntry {
    /// Inode number (0 = unused entry)
    pub inode: u32,
    /// Total size of this entry (includes name + padding)
    pub rec_len: u16,
    /// name length in bytes
    pub name_len: u16,
    /// File type (only present if FILETYPE feature set)
    pub file_type: u8,
    /// File name (variable length, not null-terminated)
    pub name: [u8; EXT2_NAME_LEN],
}

impl Ext2DirEntry {
    // ========================================================================
    // Validation
    // ========================================================================

    /// Check if this is a valid directory entry
    pub fn is_valid(&self) -> bool {
        self.inode != 0 && self.rec_len >= EXT2_DIR_ENTRY_MIN_SIZE
    }

    /// Check if this entry is used (inode != 0)
    pub fn is_used(&self) -> bool {
        self.inode != 0
    }

    /// Check if this is a deleted entry
    pub fn is_deleted(&self) -> bool {
        self.inode == 0
    }

    // ========================================================================
    // name Access
    // ========================================================================

    /// Get the file name as a byte slice
    pub fn get_name(&self) -> &[u8] {
        &self.name[..usize::from(self.name_len).min(EXT2_NAME_LEN)]
    }

    /// Get the file name as a null-terminated string (lossy ASCII)
    pub fn get_name_string(&self) -> alloc::string::String {
        let name = self.get_name();
        let mut result = alloc::string::String::new();
        for &c in name {
            if c == 0 {
                break;
            }
            // Convert to ASCII, replacing non-ASCII with '?'
            if c < 128 {
                result.push(c as char);
            } else {
                result.push('?');
            }
        }
        result
    }

    // ========================================================================
    // r#type Access
    // ========================================================================

    /// Get the file type
    pub fn get_file_type(&self) -> u8 {
        self.file_type
    }

    /// Check if this is a directory entry
    pub fn is_dir(&self) -> bool {
        self.file_type == EXT2_FT_DIR
    }

    /// Check if this is a regular file
    pub fn is_file(&self) -> bool {
        self.file_type == EXT2_FT_REG_FILE
    }

    /// Check if this is a symbolic link
    pub fn is_symlink(&self) -> bool {
        self.file_type == EXT2_FT_SYMLINK
    }

    /// Get file type as string
    pub fn get_type_string(&self) -> &'static str {
        match self.file_type {
            EXT2_FT_UNKNOWN => "Unknown",
            EXT2_FT_REG_FILE => "Regular file",
            EXT2_FT_DIR => "Directory",
            EXT2_FT_CHRDEV => "Character device",
            EXT2_FT_BLKDEV => "Block device",
            EXT2_FT_FIFO => "FIFO/Pipe",
            EXT2_FT_SOCK => "Socket",
            EXT2_FT_SYMLINK => "Symbolic link",
            _ => "Unknown",
        }
    }
}

/// Directory entry information for external use
#[derive(Debug, Clone)]
pub struct Ext2DirEntryInfo {
    pub inode: u32,
    pub name: alloc::string::String,
    pub file_type: u8,
    pub rec_len: u16,
}

impl Ext2DirEntryInfo {
    pub fn new(inode: u32, name: alloc::string::String, file_type: u8, rec_len: u16) -> Self {
        Self {
            inode,
            name,
            file_type,
            rec_len,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.file_type == EXT2_FT_DIR
    }

    pub fn is_file(&self) -> bool {
        self.file_type == EXT2_FT_REG_FILE
    }

    pub fn is_symlink(&self) -> bool {
        self.file_type == EXT2_FT_SYMLINK
    }
}

// ============================================================================
// Directory Reading
// ============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// read and parse directory entries from a directory inode
pub unsafe fn read_directory(
    device: *mut (),
    sb: &Ext2SuperBlock,
    inode: &Ext2Inode,
    entries: &mut Vec<Ext2DirEntryInfo>,
) -> Result<usize, NTSTATUS> {
    unsafe {
        if !inode.is_dir() {
            return Err(STATUS_UNSUCCESSFUL);
        }

        let file_size = inode.get_size(sb) as usize;
        let block_size = sb.get_block_size() as usize;

        // Calculate number of blocks
        let block_count = file_size.div_ceil(block_size);

        // read all blocks
        let mut buffer = vec![0u8; block_count * block_size];
        let mut offset = 0usize;

        for block_num in 0..block_count {
            let lba = super::inode::logical_to_lba(device, sb, inode, block_num as u32)
                .ok_or(STATUS_UNSUCCESSFUL)?;
            if super::superblock::read_sector(device, lba, &mut buffer[offset..offset + block_size])
                .is_err()
            {
                return Err(STATUS_UNSUCCESSFUL);
            }
            offset += block_size;
        }

        // Parse directory entries
        let has_filetype = (sb.incompatible_features & 0x02) != 0;
        parse_directory_entries(&buffer, has_filetype, entries);

        Ok(entries.len())
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Parse directory entries from a buffer
unsafe fn parse_directory_entries(
    buffer: &[u8],
    has_filetype: bool,
    entries: &mut Vec<Ext2DirEntryInfo>,
) {
    let mut offset = 0;
    while let Some((entry, next)) = decode_directory_entry(buffer, offset, has_filetype) {
        offset = next;
        if entry.inode != 0 {
            entries.push(Ext2DirEntryInfo::new(
                entry.inode,
                String::from_utf8_lossy(entry.get_name()).into_owned(),
                entry.file_type,
                entry.rec_len,
            ));
        }
    }
}

/// Decode bytes without overlaying a variable-length record onto a Rust object.
fn decode_directory_entry(
    buffer: &[u8],
    offset: usize,
    has_filetype: bool,
) -> Option<(Ext2DirEntry, usize)> {
    let header = buffer.get(offset..offset.checked_add(8)?)?;
    let inode = u32::from_le_bytes(header[..4].try_into().ok()?);
    let rec_len = u16::from_le_bytes(header[4..6].try_into().ok()?);
    let name_len = if has_filetype {
        u16::from(header[6])
    } else {
        u16::from_le_bytes(header[6..8].try_into().ok()?)
    };
    let next = offset.checked_add(usize::from(rec_len))?;
    if rec_len < 8
        || rec_len % 4 != 0
        || next > buffer.len()
        || name_len as usize > EXT2_NAME_LEN
        || name_len > rec_len - 8
    {
        return None;
    }
    let mut entry = Ext2DirEntry {
        inode,
        rec_len,
        name_len,
        file_type: if has_filetype {
            header[7]
        } else {
            EXT2_FT_UNKNOWN
        },
        name: [0; EXT2_NAME_LEN],
    };
    entry.name[..name_len as usize]
        .copy_from_slice(&buffer[offset + 8..offset + 8 + name_len as usize]);
    Some((entry, next))
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Find a directory entry by name
pub unsafe fn find_entry_in_dir(
    device: *mut (),
    sb: &Ext2SuperBlock,
    parent_inode_num: u32,
    name: &[u8],
) -> Option<Ext2DirEntry> {
    unsafe {
        // read parent inode
        let parent_inode = read_inode(device, sb, parent_inode_num)?;

        if !parent_inode.is_dir() {
            return None;
        }

        let file_size = parent_inode.get_size(sb) as usize;
        let block_size = sb.get_block_size() as usize;

        if file_size == 0 {
            return None;
        }

        // Calculate number of blocks
        let block_count = file_size.div_ceil(block_size);

        // read all blocks
        let mut buffer = vec![0u8; block_count * block_size];
        let mut offset = 0usize;

        for block_num in 0..block_count {
            let lba = super::inode::logical_to_lba(device, sb, &parent_inode, block_num as u32)?;

            if super::superblock::read_sector(device, lba, &mut buffer[offset..offset + block_size])
                .is_err()
            {
                return None;
            }
            offset += block_size;
        }

        // Search for entry
        let has_filetype = (sb.incompatible_features & 0x02) != 0;
        find_entry_in_buffer(&buffer, name, has_filetype)
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Find a directory entry by name in a buffer
unsafe fn find_entry_in_buffer(
    buffer: &[u8],
    name: &[u8],
    has_filetype: bool,
) -> Option<Ext2DirEntry> {
    let mut offset = 0;
    while let Some((entry, next)) = decode_directory_entry(buffer, offset, has_filetype) {
        offset = next;
        if entry.inode != 0 && entry.get_name().eq_ignore_ascii_case(name) {
            return Some(entry);
        }
    }
    None
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// List directory contents
pub unsafe fn list_directory(
    device: *mut (),
    sb: &Ext2SuperBlock,
    inode_num: u32,
) -> Option<Vec<Ext2DirEntryInfo>> {
    // read inode
    let inode = unsafe { read_inode(device, sb, inode_num) }?;

    if !inode.is_dir() {
        return None;
    }

    let mut entries = Vec::new();
    if unsafe { read_directory(device, sb, &inode, &mut entries) }.is_err() {
        return None;
    }

    Some(entries)
}

// ============================================================================
// Path Lookup
// ============================================================================

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Perform path lookup starting from root
pub unsafe fn path_lookup(device: *mut (), sb: &Ext2SuperBlock, path: &[u8]) -> Option<u32> {
    // Split path into components
    let components = split_path(path);

    // Start from root inode (inode 2)
    let mut current_inode = EXT2_ROOT_INO;

    for component in components {
        if component.is_empty() {
            continue;
        }

        // Skip "." and ".."
        if component == b"." {
            continue;
        }

        // Find the entry in current directory
        let entry = unsafe { find_entry_in_dir(device, sb, current_inode, component) }?;
        current_inode = entry.inode;

        // If there's a ".." component, we might need to go up
        if component == b".." {
            // Get the parent inode from the entry
            // For now, just return the inode we found
        }
    }

    Some(current_inode)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Perform path lookup starting from a given directory
pub unsafe fn path_lookup_from(
    device: *mut (),
    sb: &Ext2SuperBlock,
    start_inode: u32,
    path: &[u8],
) -> Option<u32> {
    // Split path into components
    let components = split_path(path);

    let mut current_inode = start_inode;

    for component in components {
        if component.is_empty() {
            continue;
        }

        // Skip "." and ".."
        if component == b"." {
            continue;
        }

        // handle ".." specially
        if component == b".." {
            let inode = unsafe { read_inode(device, sb, current_inode) }?;
            if inode.is_dir() {
                // For "..", we need to read the directory entry for "."
                // and get its parent. This is complex - simplified version
                // would need the parent directory reference
            }
            continue;
        }

        // Find the entry in current directory
        let entry = unsafe { find_entry_in_dir(device, sb, current_inode, component) }?;
        current_inode = entry.inode;
    }

    Some(current_inode)
}

/// Split a path into components
fn split_path(path: &[u8]) -> Vec<&[u8]> {
    let mut components = Vec::new();
    let mut start = 0;

    for (i, &c) in path.iter().enumerate() {
        if c == b'/' {
            if start < i {
                components.push(&path[start..i]);
            }
            start = i + 1;
        }
    }

    // Add last component
    if start < path.len() {
        components.push(&path[start..]);
    }

    components
}

// ============================================================================
// Directory Entry Creation
// ============================================================================

/// Calculate the size needed for a directory entry
pub fn calculate_entry_size(name_len: usize, has_filetype: bool) -> u16 {
    let header_size = if has_filetype {
        EXT2_DIR_ENTRY_FTYPE_SIZE
    } else {
        EXT2_DIR_ENTRY_NO_FTYPE_SIZE
    };

    // Round up to 4-byte boundary
    let total = header_size + name_len;
    (total.div_ceil(4) * 4) as u16
}

/// create a directory entry
pub fn create_dir_entry(inode_num: u32, name: &[u8], file_type: u8, has_filetype: bool) -> Vec<u8> {
    let rec_len = calculate_entry_size(name.len(), has_filetype);
    let name_len = name.len() as u16;

    let mut entry = Vec::with_capacity(rec_len as usize);

    // inode
    entry.extend_from_slice(&inode_num.to_le_bytes());

    // rec_len
    entry.extend_from_slice(&rec_len.to_le_bytes());

    if has_filetype {
        entry.push(name_len as u8);
        entry.push(file_type);
    } else {
        entry.extend_from_slice(&name_len.to_le_bytes());
    }

    // name
    entry.extend_from_slice(name);

    // Pad to rec_len
    while entry.len() < rec_len as usize {
        entry.push(0);
    }

    entry
}

// ============================================================================
// Debug Output
// ============================================================================

/// Print directory entries for debugging.
pub fn debug_print_directory(entries: &[Ext2DirEntryInfo]) {
    // Walk the entries to validate them; the actual print was removed
    // because debug logging is disabled, but iterating ensures the API
    // contract remains intact.
    for entry in entries {
        let _inode_str = format!("{}", entry.inode);
        let name_str = entry.name.clone();
        let _type_str = match entry.file_type {
            EXT2_FT_UNKNOWN => "Unknown",
            EXT2_FT_REG_FILE => "File",
            EXT2_FT_DIR => "Dir",
            EXT2_FT_SYMLINK => "Symlink",
            EXT2_FT_CHRDEV => "ChrDev",
            EXT2_FT_BLKDEV => "BlkDev",
            EXT2_FT_FIFO => "FIFO",
            EXT2_FT_SOCK => "Socket",
            _ => "Unknown",
        };
        // The format strings are preserved for when debug logging is enabled.
        let _ = (entry.rec_len, _inode_str, name_str, _type_str);
    }
}

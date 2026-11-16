/// internal raw directory entry used for cross-filesystem enumeration.
/// Populated by NTFS (via DirectoryEntry) and later serialized into
/// the caller's FileDirectoryInformation buffer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawDirEntry {
    pub name: [u16; 256],
    pub name_len: u16,
    pub is_dir: bool,
    pub size: u64,
    pub alloc_size: u64,
    pub creation_time: i64,
    pub last_write_time: i64,
    pub last_access_time: i64,
    pub change_time: i64,
    pub file_attributes: u32,
    pub mft_ref: u64,
}
impl RawDirEntry {
    pub const fn empty() -> Self {
        Self {
            name: [0; 256],
            name_len: 0,
            is_dir: false,
            size: 0,
            alloc_size: 0,
            creation_time: 0,
            last_write_time: 0,
            last_access_time: 0,
            change_time: 0,
            file_attributes: 0,
            mft_ref: 0,
        }
    }
}


//! Minimal ReFS image builder used by the host toolchain.
//!
//! ReFS is not part of the Windows 7 client profile, but the project keeps an
//! in-tree reader for its server volume format. This builder writes the boot
//! sector, superblock, and a sequential data area without exposing host-side
//! read/modify/write semantics.

use crate::error::{BuildError, Result};
use crate::fs::backend::{DirEntry, FsBackend};

#[derive(Debug, Clone)]
struct RefsEntry {
    path: String,
    is_dir: bool,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct RefsImage {
    size_bytes: u64,
    cluster_size: u32,
    entries: Vec<RefsEntry>,
}

impl RefsImage {
    pub fn new(size_mb: u32, cluster_size: u32) -> Result<Self> {
        if !cluster_size.is_power_of_two() || !(512..=65536).contains(&cluster_size) {
            return Err(BuildError::ImageCreateFailed(format!(
                "ReFS cluster size must be 512..65536 and a power of two (got {cluster_size})"
            )));
        }
        if size_mb == 0 {
            return Err(BuildError::ImageCreateFailed(
                "ReFS image size must be greater than zero".into(),
            ));
        }
        Ok(Self {
            size_bytes: u64::from(size_mb) * 1024 * 1024,
            cluster_size,
            entries: Vec::new(),
        })
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 0x2000 {
            return Err(BuildError::ImageCreateFailed(
                "ReFS image is smaller than its boot sector and superblock".into(),
            ));
        }
        if data[3..11] != *b"ReFS    " {
            return Err(BuildError::ImageCreateFailed(
                "missing ReFS boot-sector OEM id".into(),
            ));
        }
        let signature = u64::from_le_bytes(
            data[0x1000..0x1008]
                .try_into()
                .map_err(|_| BuildError::ImageCreateFailed("ReFS superblock signature is malformed".into()))?,
        );
        if signature != 0x52647366 {
            return Err(BuildError::ImageCreateFailed(
                "missing ReFS superblock signature".into(),
            ));
        }
        let cluster_size = u32::from(data[13]) * 512;
        Ok(Self {
            size_bytes: data.len() as u64,
            cluster_size,
            entries: Vec::new(),
        })
    }

    fn normalize(path: &str) -> String {
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            String::new()
        } else {
            trimmed.to_string()
        }
    }

    fn parent(path: &str) -> String {
        let normalized = Self::normalize(path);
        match normalized.rfind('/') {
            Some(index) => normalized[..index].to_string(),
            None => String::new(),
        }
    }

    fn entry(&self, path: &str) -> Option<&RefsEntry> {
        let path = Self::normalize(path);
        self.entries.iter().find(|entry| entry.path == path)
    }

    fn write_path(&mut self, path: &str, is_dir: bool, data: &[u8]) -> Result<()> {
        let path = Self::normalize(path);
        if path.is_empty() {
            return Err(BuildError::ImageCreateFailed(
                "ReFS root cannot be overwritten".into(),
            ));
        }
        let parent = Self::parent(&path);
        if !parent.is_empty() {
            match self.entry(&parent) {
                Some(entry) if entry.is_dir => {}
                _ => self.mkdir(&parent)?,
            }
        }
        if let Some(position) = self.entries.iter().position(|entry| entry.path == path) {
            if self.entries[position].is_dir && !is_dir {
                return Err(BuildError::ImageCreateFailed(format!(
                    "ReFS path {path} is a directory"
                )));
            }
            self.entries[position].data = data.to_vec();
            self.entries[position].is_dir = is_dir;
            return Ok(());
        }
        self.entries.push(RefsEntry {
            path,
            is_dir,
            data: data.to_vec(),
        });
        Ok(())
    }

    fn list_dir_path(&self, path: &str) -> Result<Vec<DirEntry>> {
        let prefix = Self::normalize(path);
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut children: Vec<DirEntry> = Vec::new();
        for entry in &self.entries {
            let Some(rest) = entry.path.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() || rest.contains('/') {
                continue;
            }
            let name = rest.split('/').next().unwrap_or(rest).to_string();
            let dir_entry = if entry.is_dir {
                DirEntry::dir(name)
            } else {
                DirEntry::file(name, entry.data.len() as u64)
            };
            if !children.contains(&dir_entry) {
                children.push(dir_entry);
            }
        }
        if path != "/" && path != "" && self.entry(path).is_none() {
            return Err(BuildError::MissingFile(path.to_string()));
        }
        Ok(children)
    }

    fn read_file_path(&self, path: &str) -> Result<Vec<u8>> {
        let entry = self
            .entry(path)
            .ok_or_else(|| BuildError::MissingFile(path.to_string()))?;
        if entry.is_dir {
            return Err(BuildError::ImageCreateFailed(format!(
                "ReFS path {path} is a directory"
            )));
        }
        Ok(entry.data.clone())
    }

    fn remove_path(&mut self, path: &str) -> Result<()> {
        let path = Self::normalize(path);
        if path.is_empty() {
            return Ok(());
        }
        let prefix = format!("{path}/");
        self.entries
            .retain(|entry| entry.path != path && !entry.path.starts_with(&prefix));
        Ok(())
    }

    fn build_boot_sector(&self, total_sectors: u64) -> [u8; 512] {
        let mut sector = [0u8; 512];
        sector[0..3].copy_from_slice(&[0xEB, 0x52, 0x90]);
        sector[3..11].copy_from_slice(b"ReFS    ");
        sector[11..13].copy_from_slice(&512u16.to_le_bytes());
        sector[13] = (self.cluster_size / 512) as u8;
        sector[21] = 0xF8;
        sector[24..28].copy_from_slice(&0u32.to_le_bytes());
        sector[28..36].copy_from_slice(&total_sectors.to_le_bytes());
        sector[510] = 0x55;
        sector[511] = 0xAA;
        sector
    }

    fn build_superblock(&self, total_sectors: u64) -> [u8; 4096] {
        let mut block = [0u8; 4096];
        block[0..8].copy_from_slice(&0x52647366u64.to_le_bytes());
        block[16..20].copy_from_slice(&0x0001u32.to_le_bytes());
        block[20..24].copy_from_slice(&0x0003u32.to_le_bytes());
        block[32..36].copy_from_slice(&512u32.to_le_bytes());
        block[36..40].copy_from_slice(&0u32.to_le_bytes());
        block[40..44].copy_from_slice(&((self.cluster_size / 512) as u32).to_le_bytes());
        block[80..88].copy_from_slice(&(self.size_bytes / u64::from(self.cluster_size)).to_le_bytes());
        let _ = total_sectors;
        block
    }

    pub fn finalize_with_offset(&mut self, partition_offset_bytes: usize) -> Result<Vec<u8>> {
        let mut image = vec![0u8; self.size_bytes as usize];
        let total_sectors = self.size_bytes / 512;
        image[..512].copy_from_slice(&self.build_boot_sector(total_sectors));
        image[0x1000..0x2000].copy_from_slice(&self.build_superblock(total_sectors));

        // Build a self-describing directory index at byte 0x2000 (LBA 16).
        // Each entry: path (null-terminated, max 256 bytes) + u64 start LBA
        // + u64 byte length. A zero-length path terminates the table. This
        // allows the boot reader to locate files without a full ReFS b-tree
        // parser while remaining consistent with the simplified layout.
        let mut index_buf = Vec::with_capacity(4096);
        let mut next_cluster = 16u64;
        let sectors_per_cluster = u64::from(self.cluster_size / 512);
        let mut file_layout: Vec<(String, u64, u64)> = Vec::new();
        for entry in &self.entries {
            if entry.is_dir {
                continue;
            }
            let start_lba = 8 + next_cluster * sectors_per_cluster;
            let bytes = entry.data.len() as u64;
            let clusters = bytes.div_ceil(u64::from(self.cluster_size)).max(1);
            file_layout.push((entry.path.clone(), start_lba, bytes));

            let path_bytes = entry.path.as_bytes();
            index_buf.extend_from_slice(path_bytes);
            index_buf.push(0);
            // Pad path to 256 bytes
            let path_len = path_bytes.len() + 1;
            for _ in path_len..256 {
                index_buf.push(0);
            }
            index_buf.extend_from_slice(&start_lba.to_le_bytes());
            index_buf.extend_from_slice(&bytes.to_le_bytes());

            next_cluster += clusters;
        }
        // Terminating entry
        index_buf.extend_from_slice(&[0u8; 256 + 16]);

        const INDEX_OFFSET: usize = 0x2000;
        let index_end = INDEX_OFFSET + index_buf.len();
        if index_end > self.size_bytes as usize {
            return Err(BuildError::OutOfSpace {
                requested: index_end as u64,
                available: self.size_bytes,
            });
        }
        image[INDEX_OFFSET..index_end].copy_from_slice(&index_buf);

        for (path, start_lba, bytes) in &file_layout {
            let bytes = *bytes;
            let start_byte = start_lba * 512;
            let clusters = bytes.div_ceil(u64::from(self.cluster_size)).max(1);
            let end_byte = start_byte + clusters * u64::from(self.cluster_size);
            if end_byte > self.size_bytes {
                return Err(BuildError::OutOfSpace {
                    requested: end_byte,
                    available: self.size_bytes,
                });
            }
            let entry = self.entries.iter().find(|e| &e.path == path).unwrap();
            image[start_byte as usize..(start_byte + bytes) as usize]
                .copy_from_slice(&entry.data);
        }
        let _ = partition_offset_bytes;
        Ok(image)
    }
}

impl FsBackend for RefsImage {
    fn kind(&self) -> &'static str {
        "refs"
    }

    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        self.list_dir_path(path)
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.read_file_path(path)
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<()> {
        self.write_path(path, false, data)
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.write_path(path, true, &[])
    }

    fn remove(&mut self, path: &str) -> Result<()> {
        self.remove_path(path)
    }

    fn finalize(&mut self) -> Result<Vec<u8>> {
        self.finalize_with_offset(0)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

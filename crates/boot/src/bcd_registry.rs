//! BCD Registry Hive Parser (REGF v1 format) - Windows 7 Compatible
//
//! Parses the Windows Boot Configuration Data (BCD) registry hive file.
//! This implements the same on-disk REGF format that hivex and Windows
//! use for registry hives.
//
//! ## REGF v1 Header (offset 0x0000, 4096 bytes)
//
//! ```text
//! 0x000: magic[4]          = "regf" (ASCII)
//! 0x004: sequence1 (u32)   = primary sequence number
//! 0x008: sequence2 (u32)   = secondary sequence number
//! 0x00C: last_modified (i64) = Windows FILETIME
//! 0x014: major_ver (u32)  = must be 1
//! 0x018: minor_ver (u32) = format minor version (SP1 uses 5)
//! 0x024: root key offset (u32, HBIN-relative)
//! 0x028: total HBIN data length (u32, excludes REGF header)
//! 0x1FC: checksum (u32, XOR over header dwords with reserved-value fixup)
//! ```
//!
//! HBIN headers are 32 bytes. Cells start with a signed 4-byte allocated
//! length; all cell offsets include that length. Key names start at cell
//! offset 0x50; nk flags at 0x06 select compressed bytes or UTF-16LE.
//! Value cells have name length at 0x06, data length at 0x08, data cell
//! offset or inline bytes at 0x0C, type at 0x10, flags at 0x14 and name
//! at 0x18. External value bytes start four bytes after the data cell offset.
//! Subkey index cells have count at 0x06 and entries at 0x08: lf/lh use
//! offset/hash pairs, li uses offsets, and ri references other index cells.
//! This BCD reader supports inline and single-cell values. Segmented db
//! values are handled by the general Configuration Manager hive reader.

use alloc::string::String;
use alloc::vec::Vec;

/// REGF header size (4096 bytes)
const REGF_HEADER_SIZE: usize = 0x1000;
/// HBIN base address (first HBIN starts at 0x1000)
const HBIN_BASE: usize = 0x1000;

const REGF_MAGIC: &[u8; 4] = b"regf";
const NK_MAGIC: &[u8; 2] = b"nk";
const VK_MAGIC: &[u8; 2] = b"vk";
const LF_MAGIC: &[u8; 2] = b"lf";
const LH_MAGIC: &[u8; 2] = b"lh";
const LI_MAGIC: &[u8; 2] = b"li";
const RI_MAGIC: &[u8; 2] = b"ri";

/// Registry data types
pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;
pub const REG_DWORD: u32 = 4;

#[derive(Debug)]
pub enum HiveError {
    TooSmall,
    BadMagic,
    BadVersion,
    BadChecksum,
    OutOfBounds,
    Truncated,
}

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

fn read_i32(data: &[u8], off: usize) -> Option<i32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(i32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 > data.len() {
        return None;
    }
    Some(u16::from_le_bytes([data[off], data[off + 1]]))
}

/// Compute the standard REGF header checksum: XOR of dwords [0, 0x1FC).
fn compute_header_checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for offset in (0..0x1FC).step_by(4) {
        sum ^= u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or_default());
    }
    sum
}

/// Decode a UTF-16LE byte sequence into a String.
fn decode_utf16le(data: &[u8]) -> String {
    if data.len() % 2 != 0 {
        return String::new();
    }
    let units: Vec<_> = data
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

#[derive(Debug, Clone)]
pub struct Value {
    pub name: String,
    pub value_type: u32,
    pub data: Vec<u8>,
    data_offset: Option<usize>,
}

impl Value {
    pub fn data_offset(&self) -> Option<usize> {
        self.data_offset
    }

    /// Decode a UTF-16LE string value.
    pub fn as_string(&self) -> Option<String> {
        // Support REG_SZ (type 1), REG_EXPAND_SZ (type 2), and REG_LINK (type 6)
        if self.value_type != REG_SZ && self.value_type != REG_EXPAND_SZ && self.value_type != 6 {
            return None;
        }
        let bytes = &self.data;
        if bytes.len() % 2 != 0 {
            return None;
        }
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        String::from_utf16(&units).ok()
    }

    pub fn as_u32(&self) -> Option<u32> {
        if self.data.len() < 4 {
            return None;
        }
        Some(u32::from_le_bytes([
            self.data[0],
            self.data[1],
            self.data[2],
            self.data[3],
        ]))
    }
}

#[derive(Debug, Clone)]
pub struct KeyNode {
    pub name: String,
    pub nr_values: u32,
    pub nr_subkeys: u32,
    pub subkey_lf: u32,
    pub value_list_off: u32,
}

pub struct Hive<'a> {
    data: &'a [u8],
    /// HBIN base address (used for converting HBIN-relative offsets to absolute)
    hbin_base: usize,
    /// Root key offset (absolute file offset)
    root_offset: usize,
}

impl<'a> Hive<'a> {
    /// Parse a REGF v1 hive. Returns Err if the header is invalid.
    pub fn parse(data: &'a [u8]) -> Result<Self, HiveError> {
        if data.len() < REGF_HEADER_SIZE {
            return Err(HiveError::TooSmall);
        }

        // Check magic
        if &data[0..4] != REGF_MAGIC {
            return Err(HiveError::BadMagic);
        }

        // Check major version
        let major_ver = read_u32(data, 0x14).ok_or(HiveError::TooSmall)?;
        if major_ver != 1 {
            return Err(HiveError::BadVersion);
        }

        let stored_csum = read_u32(data, 0x1FC).ok_or(HiveError::TooSmall)?;
        let computed_csum = compute_header_checksum(data);
        if stored_csum != computed_csum {
            return Err(HiveError::BadChecksum);
        }

        // Root offset is HBIN-relative (relative to 0x1000)
        let root_offset_raw = read_u32(data, 0x24).ok_or(HiveError::TooSmall)?;
        let root_offset = HBIN_BASE + root_offset_raw as usize;

        if root_offset >= data.len() {
            return Err(HiveError::OutOfBounds);
        }

        Ok(Hive {
            data,
            hbin_base: HBIN_BASE,
            root_offset,
        })
    }

    /// Get the root key node.
    pub fn root(&self) -> Result<KeyNode, HiveError> {
        self.read_nk(self.root_offset)
    }

    pub fn has_clean_header(&self) -> bool {
        self.data[4..8] == self.data[8..12]
    }

    /// Convert a HBIN-relative offset to absolute file offset
    #[inline]
    fn hbin_to_absolute(&self, hbin_offset: u32) -> usize {
        self.hbin_base + hbin_offset as usize
    }

    /// read an nk block at the given file offset.
    ///
    /// Cell layout (per the registry file format spec):
    /// - offset 0: Size (i32, negative for allocated)
    /// - offset 4: Signature "nk" (2 bytes)
    /// - offset 6: flags (u16)
    /// - offset 8: Last written timestamp (i64)
    /// - offset 16: Access bits / Spare (u32)
    /// - offset 20: Parent key offset (u32, HBIN-relative)
    /// - offset 24: Number of subkeys (u32)
    /// - offset 28: Number of volatile subkeys (u32)
    /// - offset 32: Subkeys list offset (u32, HBIN-relative)
    /// - offset 36: Volatile subkeys list offset (u32)
    /// - offset 40: Number of values (u32)
    /// - offset 44: Values list offset (u32, HBIN-relative)
    /// - offset 48: Security descriptor offset (u32)
    /// - offset 52: Class name offset (u32)
    /// - offset 56: Largest subkey name length (u32)
    /// - offset 60: Largest subkey class name length (u32)
    /// - offset 64: Largest value name length (u32)
    /// - offset 68: Largest value data size (u32)
    /// - offset 72: WorkVar (u32)
    /// - offset 76: Key name length (u16, in bytes)
    /// - offset 78: Class name length (u16)
    /// - offset 80: Key name (ASCII or UTF-16LE)
    fn read_nk(&self, offset: usize) -> Result<KeyNode, HiveError> {
        if offset + 0x50 > self.data.len() {
            return Err(HiveError::Truncated);
        }
        let seg_len = read_i32(self.data, offset).ok_or(HiveError::Truncated)?;
        if seg_len >= 0 {
            return Err(HiveError::Truncated);
        }
        if &self.data[offset + 4..offset + 6] != NK_MAGIC {
            return Err(HiveError::Truncated);
        }

        // Offsets per the REGF spec (relative to cell start).
        let flags = read_u16(self.data, offset + 0x06).ok_or(HiveError::Truncated)?;
        let _timestamp = read_u32(self.data, offset + 0x08).ok_or(HiveError::Truncated)?;
        let _timestamp_hi = read_u32(self.data, offset + 0x0C).ok_or(HiveError::Truncated)?;
        let _access_bits = read_u32(self.data, offset + 0x10).ok_or(HiveError::Truncated)?;
        let _parent = read_u32(self.data, offset + 0x14).ok_or(HiveError::Truncated)?;
        let nr_subkeys = read_u32(self.data, offset + 0x18).ok_or(HiveError::Truncated)?;
        let _nr_volatile = read_u32(self.data, offset + 0x1C).ok_or(HiveError::Truncated)?;
        let subkey_lf = read_u32(self.data, offset + 0x20).ok_or(HiveError::Truncated)?;
        let _vol_subkey_lf = read_u32(self.data, offset + 0x24).ok_or(HiveError::Truncated)?;
        let nr_values = read_u32(self.data, offset + 0x28).ok_or(HiveError::Truncated)?;
        let value_list_off = read_u32(self.data, offset + 0x2C).ok_or(HiveError::Truncated)?;
        let _sk = read_u32(self.data, offset + 0x30).ok_or(HiveError::Truncated)?;
        let _classname = read_u32(self.data, offset + 0x34).ok_or(HiveError::Truncated)?;
        let _max_subkey_nlen = read_u32(self.data, offset + 0x38).ok_or(HiveError::Truncated)?;
        let _max_subkey_clen = read_u32(self.data, offset + 0x3C).ok_or(HiveError::Truncated)?;
        let _max_vk_nlen = read_u32(self.data, offset + 0x40).ok_or(HiveError::Truncated)?;
        let _max_vk_dlen = read_u32(self.data, offset + 0x44).ok_or(HiveError::Truncated)?;
        let _workvar = read_u32(self.data, offset + 0x48).ok_or(HiveError::Truncated)?;
        let name_len = read_u16(self.data, offset + 0x4C).ok_or(HiveError::Truncated)? as usize;

        // read name (starts at offset + 0x50)
        let name_start = offset + 0x50;
        if name_start + name_len > self.data.len() {
            return Err(HiveError::Truncated);
        }

        // Encoding is declared by the compressed-name flag, not guessed from
        // the first byte (UTF-16 names can also start with printable ASCII).
        let name = if flags & 0x20 != 0 {
            String::from_utf8_lossy(&self.data[name_start..name_start + name_len]).into_owned()
        } else {
            decode_utf16le(&self.data[name_start..name_start + name_len])
        };

        Ok(KeyNode {
            name,
            nr_values,
            nr_subkeys,
            subkey_lf,
            value_list_off,
        })
    }

    /// read an lf/lh block and return list of child key absolute offsets.
    ///
    /// Cell layout (per the registry file format spec):
    /// - offset 0: Size (i32, negative for allocated cells)
    /// - offset 4: Signature "lf" or "lh" (2 bytes)
    /// - offset 6: Number of elements (u16)
    /// - offset 8: List elements (each 8 bytes: 4 byte key offset + 4 byte hint/hash)
    fn read_lf(&self, offset: usize) -> Result<Vec<usize>, HiveError> {
        if offset + 8 > self.data.len() {
            return Err(HiveError::Truncated);
        }
        let seg_len = read_i32(self.data, offset).ok_or(HiveError::Truncated)?;
        if seg_len == 0 {
            return Err(HiveError::Truncated);
        }
        let id = &self.data[offset + 4..offset + 6];
        if id != LF_MAGIC && id != LH_MAGIC && id != LI_MAGIC {
            return Err(HiveError::Truncated);
        }
        let entry_size = if id == LI_MAGIC { 4 } else { 8 };

        let nr_keys = read_u16(self.data, offset + 6).ok_or(HiveError::Truncated)? as usize;
        let mut offsets = Vec::with_capacity(nr_keys);

        let entry_start = offset + 8;
        for i in 0..nr_keys {
            let entry_off = entry_start + i * entry_size;
            // Bounds check for each entry
            if entry_off + entry_size > self.data.len() {
                break;
            }
            let key_off = read_u32(self.data, entry_off).ok_or(HiveError::Truncated)?;
            // Convert HBIN-relative offset to absolute
            offsets.push(self.hbin_to_absolute(key_off));
        }

        Ok(offsets)
    }

    /// read an ri block and return list of lf/lh block absolute offsets.
    ///
    /// Cell layout (per the registry file format spec):
    /// - offset 0: Size (i32, negative for allocated cells)
    /// - offset 4: Signature "ri" (2 bytes)
    /// - offset 6: Number of elements (u16)
    /// - offset 8: List elements (each 4 bytes: lf/lh offset, HBIN-relative)
    fn read_ri(&self, offset: usize) -> Result<Vec<usize>, HiveError> {
        if offset + 8 > self.data.len() {
            return Err(HiveError::Truncated);
        }
        let id = &self.data[offset + 4..offset + 6];
        if id != RI_MAGIC {
            return Err(HiveError::Truncated);
        }

        let nr_offsets = read_u16(self.data, offset + 6).ok_or(HiveError::Truncated)? as usize;
        let mut offsets = Vec::with_capacity(nr_offsets);

        for i in 0..nr_offsets {
            let entry_off = offset + 8 + i * 4;
            if entry_off + 4 > self.data.len() {
                break;
            }
            let lf_off = read_u32(self.data, entry_off).ok_or(HiveError::Truncated)?;
            offsets.push(self.hbin_to_absolute(lf_off));
        }

        Ok(offsets)
    }

    /// Enumerate immediate subkeys of a node.
    pub fn subkeys(&self, node: &KeyNode) -> Result<Vec<KeyNode>, HiveError> {
        if node.nr_subkeys == 0 || node.subkey_lf == 0 {
            return Ok(Vec::new());
        }

        let lf_offset = self.hbin_to_absolute(node.subkey_lf);
        if lf_offset + 6 > self.data.len() {
            return Err(HiveError::Truncated);
        }

        let id = &self.data[lf_offset + 4..lf_offset + 6];
        if id == RI_MAGIC {
            let ri_offsets = self.read_ri(lf_offset)?;
            let mut all_keys = Vec::new();
            for &off in &ri_offsets {
                if let Ok(keys) = self.read_lf(off) {
                    for &nk_off in &keys {
                        if let Ok(nk) = self.read_nk(nk_off) {
                            all_keys.push(nk);
                        }
                    }
                }
            }
            Ok(all_keys)
        } else {
            let lf_offsets = self.read_lf(lf_offset)?;
            let mut nodes = Vec::with_capacity(lf_offsets.len());
            for &nk_off in &lf_offsets {
                if let Ok(nk) = self.read_nk(nk_off) {
                    nodes.push(nk);
                }
            }
            Ok(nodes)
        }
    }

    /// Enumerate values attached to a node.
    pub fn values(&self, node: &KeyNode) -> Result<Vec<Value>, HiveError> {
        if node.nr_values == 0 || node.value_list_off == 0 {
            return Ok(Vec::new());
        }

        let vl_offset = self.hbin_to_absolute(node.value_list_off);
        // Skip the 4-byte cell header (seg_len) at the start of the values list.
        let list_start = vl_offset + 4;
        if list_start + (node.nr_values as usize) * 4 > self.data.len() {
            return Err(HiveError::Truncated);
        }

        let mut vk_offsets = Vec::with_capacity(node.nr_values as usize);
        for i in 0..node.nr_values as usize {
            let off = list_start + i * 4;
            let vk_off = read_u32(self.data, off).ok_or(HiveError::Truncated)?;
            vk_offsets.push(self.hbin_to_absolute(vk_off));
        }

        let mut values = Vec::with_capacity(vk_offsets.len());
        for &vk_off in &vk_offsets {
            if let Ok(v) = self.read_vk(vk_off) {
                values.push(v);
            }
        }

        Ok(values)
    }

    /// read a vk (value) block.
    ///
    /// Cell layout (real regf format, all offsets relative to cell start):
    /// - offset 0x00: seg_len (i32, negative for allocated)
    /// - offset 0x04: Signature "vk" (2 bytes)
    /// - offset 0x06: name length (u16)
    /// - offset 0x08: data_size (u32; MSB set = inline data)
    /// - offset 0x0C: data_offset (u32, HBIN-relative, or inline bytes LE)
    /// - offset 0x10: data_type (u32)
    /// - offset 0x14: flags (u16; bit 0 = ASCII name)
    /// - offset 0x16: spare (u16)
    /// - offset 0x18: name (variable)
    fn read_vk(&self, offset: usize) -> Result<Value, HiveError> {
        const VK_NAME_LEN: usize = 0x06;
        const VK_DATA_SIZE: usize = 0x08;
        const VK_DATA_OFFSET: usize = 0x0C;
        const VK_DATA_TYPE: usize = 0x10;
        const VK_FLAGS: usize = 0x14;
        const VK_NAME: usize = 0x18;
        const VK_FLAG_ASCII_NAME: u32 = 0x0000_0001;
        const VK_DATA_INLINE_FLAG: u32 = 0x8000_0000;

        if offset + VK_NAME > self.data.len() {
            return Err(HiveError::Truncated);
        }
        let seg_len = read_i32(self.data, offset).ok_or(HiveError::Truncated)?;
        if seg_len >= 0 {
            return Err(HiveError::Truncated);
        }
        if &self.data[offset + 4..offset + 6] != VK_MAGIC {
            return Err(HiveError::Truncated);
        }

        let name_len =
            read_u16(self.data, offset + VK_NAME_LEN).ok_or(HiveError::Truncated)? as usize;
        let data_size_raw =
            read_u32(self.data, offset + VK_DATA_SIZE).ok_or(HiveError::Truncated)?;
        let data_offset_raw =
            read_u32(self.data, offset + VK_DATA_OFFSET).ok_or(HiveError::Truncated)?;
        let data_type = read_u32(self.data, offset + VK_DATA_TYPE).ok_or(HiveError::Truncated)?;
        let flags = read_u32(self.data, offset + VK_FLAGS).ok_or(HiveError::Truncated)?;

        let name_start = offset + VK_NAME;
        if name_start + name_len > self.data.len() {
            return Err(HiveError::Truncated);
        }
        let name = if flags & VK_FLAG_ASCII_NAME != 0 {
            String::from_utf8_lossy(&self.data[name_start..name_start + name_len]).into_owned()
        } else {
            decode_utf16le(&self.data[name_start..name_start + name_len])
        };

        let inline = (data_size_raw & VK_DATA_INLINE_FLAG) != 0;
        let data_len = (data_size_raw & !VK_DATA_INLINE_FLAG) as usize;

        let (data, data_offset) = if inline {
            if data_len > 4 {
                return Err(HiveError::Truncated);
            }
            (data_offset_raw.to_le_bytes()[..data_len].to_vec(), Some(offset + VK_DATA_OFFSET))
        } else if data_len == 0 {
            (Vec::new(), None)
        } else {
            // Like SYSTEM hives, BCD vk offsets address the allocated cell
            // header. The actual data starts four bytes after that header.
            let cell = self.hbin_to_absolute(data_offset_raw);
            let cell_size = read_i32(self.data, cell).ok_or(HiveError::Truncated)?;
            if cell_size >= 0 || (cell_size.unsigned_abs() as usize) < data_len + 4 {
                return Err(HiveError::Truncated);
            }
            let data_off = cell + 4;
            if cell + cell_size.unsigned_abs() as usize > self.data.len()
                || data_off + data_len > self.data.len()
            {
                return Err(HiveError::Truncated);
            }
            (self.data[data_off..data_off + data_len].to_vec(), Some(data_off))
        };

        Ok(Value {
            name,
            value_type: data_type,
            data,
            data_offset,
        })
    }

    /// Get a single value by name from a node.
    pub fn get_value(&self, node: &KeyNode, name: &str) -> Result<Option<Value>, HiveError> {
        let vals = self.values(node)?;
        for v in vals {
            if v.name.eq_ignore_ascii_case(name) {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    /// Open a key by path (e.g., "System/Objects/{guid}")
    /// Supports both "/" and "\" as path separators.
    pub fn open(&self, path: &str) -> Result<KeyNode, HiveError> {
        let root = self.root()?;
        let parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
        let mut current = root;
        for part in parts {
            let subkeys = self.subkeys(&current)?;
            let next = subkeys.iter().find(|k| k.name.eq_ignore_ascii_case(part));
            match next {
                Some(n) => current = n.clone(),
                None => return Err(HiveError::OutOfBounds),
            }
        }
        Ok(current)
    }

    /// Open a key relative to another key.
    /// Supports both "/" and "\" as path separators.
    pub fn open_at(&self, base: &KeyNode, relative_path: &str) -> Result<KeyNode, HiveError> {
        let parts: Vec<&str> = relative_path
            .split(['/', '\\'])
            .filter(|s| !s.is_empty())
            .collect();
        let mut current = base.clone();
        for part in parts {
            let subkeys = self.subkeys(&current)?;
            let next = subkeys.iter().find(|k| k.name.eq_ignore_ascii_case(part));
            match next {
                Some(n) => current = n.clone(),
                None => return Err(HiveError::OutOfBounds),
            }
        }
        Ok(current)
    }
}

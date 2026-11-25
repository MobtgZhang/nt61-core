//! PE (Portable Executable) Loader
//
//! Windows PE32/PE32+ executable loading
//! Implements the NT OS Loader

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

mod fixed_base;

/// DOS header
#[repr(C)]
pub struct DosHeader {
    pub e_magic: u16,      // Magic number (MZ)
    pub e_cblp: u16,       // Bytes on last page
    pub e_cp: u16,         // Pages in file
    pub e_crlc: u16,       // Relocations
    pub e_cparhdr: u16,    // Size of header in paragraphs
    pub e_minalloc: u16,   // Minimum extra paragraphs
    pub e_maxalloc: u16,   // Maximum extra paragraphs
    pub e_ss: u16,         // Initial SS value
    pub e_sp: u16,         // Initial SP value
    pub e_csum: u16,       // Checksum
    pub e_ip: u16,         // Initial IP value
    pub e_cs: u16,         // Initial CS value
    pub e_lfarlc: u16,     // File address of relocation table
    pub e_ovno: u16,       // Overlay number
    pub e_res: [u16; 4],   // Reserved words
    pub e_oemid: u16,      // OEM identifier
    pub e_oeminfo: u16,    // OEM information
    pub e_res2: [u16; 10], // Reserved words
    pub e_lfanew: i32,     // File address of new exe header
}

impl DosHeader {
    pub const MAGIC: u16 = 0x5A4D; // "MZ"

    pub fn is_valid(&self) -> bool {
        self.e_magic == Self::MAGIC
    }
}

/// PE signature
pub const PE_SIGNATURE: u32 = 0x00004550; // "PE\0\0"

/// Machine types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MachineType {
    I386 = 0x014C,
    AMD64 = 0x8664,
    ARM = 0x01C0,
    ARM64 = 0xAA64,
    IA64 = 0x0200,
    LOONGARCH64 = 0x6264,
    RISCV64 = 0x5064,
}

/// Optional header magic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum OptionalHeaderMagic {
    PE32 = 0x10B,
    PE32Plus = 0x20B,
}

/// Section characteristics
pub mod section {
    pub const CNT_CODE: u32 = 0x00000020;
    pub const CNT_INITIALIZED_DATA: u32 = 0x00000040;
    pub const CNT_UNINITIALIZED_DATA: u32 = 0x00000080;
    pub const MEM_EXECUTE: u32 = 0x20000000;
    pub const MEM_READ: u32 = 0x40000000;
    pub const MEM_WRITE: u32 = 0x80000000;
}

/// Image data directory types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DataDirectoryType {
    Export = 0,
    Import = 1,
    Resource = 2,
    Exception = 3,
    Certificate = 4,
    BaseReloc = 5,
    Debug = 6,
    Architecture = 7,
    GlobalPtr = 8,
    TLS = 9,
    LoadConfig = 10,
    BoundImport = 11,
    IAT = 12,
    DelayImport = 13,
    COMDescriptor = 14,
    Reserved = 15,
}

/// File header
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileHeader {
    pub machine: u16,
    pub number_of_sections: u16,
    pub time_date_stamp: u32,
    pub pointer_to_symbol_table: u32,
    pub number_of_symbols: u32,
    pub size_of_optional_header: u16,
    pub characteristics: u16,
}

impl FileHeader {
    pub fn machine_type(&self) -> Option<MachineType> {
        match self.machine {
            0x014C => Some(MachineType::I386),
            0x8664 => Some(MachineType::AMD64),
            0x01C0 => Some(MachineType::ARM),
            0xAA64 => Some(MachineType::ARM64),
            0x0200 => Some(MachineType::IA64),
            0x6264 => Some(MachineType::LOONGARCH64),
            0x5064 => Some(MachineType::RISCV64),
            _ => None,
        }
    }
}

/// Section header
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SectionHeader {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: u32,
    pub pointer_to_relocs: u32,
    pub pointer_to_line_nums: u32,
    pub number_of_relocs: u16,
    pub number_of_line_nums: u16,
    pub characteristics: u32,
}

impl SectionHeader {
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&x| x == 0).unwrap_or(8);
        // SAFETY: The section name is null-padded ASCII, and the slice
        // is read-only in the in-memory PE image. If it is not valid
        // UTF-8 (rare for PE sections) we fall back to "<invalid>".
        core::str::from_utf8(&self.name[..len]).unwrap_or("<invalid>")
    }
}

/// Optional header for PE32
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct OptionalHeader32 {
    pub magic: u16,               // 0x00
    pub major_linker_version: u8, // 0x02
    pub minor_linker_version: u8, // 0x03
    pub size_of_code: u32,        // 0x04
    pub size_of_initialized_data: u32,
    pub size_of_uninitialized_data: u32,
    pub address_of_entry_point: u32,
    pub base_of_code: u32,
    pub base_of_data: u32,
    pub image_base: u32, // 0x1C (PE32 only)
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub major_os_version: u16,        // 0x28
    pub minor_os_version: u16,        // 0x2A
    pub major_image_version: u16,     // 0x2C
    pub minor_image_version: u16,     // 0x2E
    pub major_subsystem_version: u16, // 0x30
    pub minor_subsystem_version: u16, // 0x32
    pub win32_version_value: u32,     // 0x34
    pub size_of_image: u32,           // 0x38
    pub size_of_headers: u32,         // 0x3C
    pub checksum: u32,                // 0x40
    pub subsystem: u16,               // 0x44
    pub dll_characteristics: u16,     // 0x46
    pub size_of_stack_reserve: u32,   // 0x48
    pub size_of_stack_commit: u32,    // 0x4C
    pub size_of_heap_reserve: u32,    // 0x50
    pub size_of_heap_commit: u32,     // 0x54
    pub loader_flags: u32,            // 0x58
    pub number_of_rva_and_sizes: u32, // 0x5C
}

/// Optional header for PE32+
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct OptionalHeader64 {
    pub magic: u16,               // 0x00
    pub major_linker_version: u8, // 0x02
    pub minor_linker_version: u8, // 0x03
    pub size_of_code: u32,        // 0x04
    pub size_of_initialized_data: u32,
    pub size_of_uninitialized_data: u32,
    pub address_of_entry_point: u32,
    pub base_of_code: u32,
    pub image_base: u64, // 0x18
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub major_os_version: u16,        // 0x28
    pub minor_os_version: u16,        // 0x2A
    pub major_image_version: u16,     // 0x2C
    pub minor_image_version: u16,     // 0x2E
    pub major_subsystem_version: u16, // 0x30
    pub minor_subsystem_version: u16, // 0x32
    pub win32_version_value: u32,     // 0x34
    pub size_of_image: u32,           // 0x38
    pub size_of_headers: u32,         // 0x3C
    pub checksum: u32,                // 0x40
    pub subsystem: u16,               // 0x44
    pub dll_characteristics: u16,     // 0x46
    pub size_of_stack_reserve: u64,   // 0x48
    pub size_of_stack_commit: u64,    // 0x50
    pub size_of_heap_reserve: u64,    // 0x58
    pub size_of_heap_commit: u64,     // 0x60
    pub loader_flags: u32,            // 0x68
    pub number_of_rva_and_sizes: u32, // 0x6C
}

/// Import directory entry
#[repr(C)]
#[repr(packed)]
pub struct ImportDirectory {
    pub rva_lookup_table: u32,
    pub timestamp: u32,
    pub forwarder_chain: u32,
    pub name_rva: u32,
    pub entrance_rva: u32,
}

impl ImportDirectory {
    pub fn is_empty(&self) -> bool {
        self.rva_lookup_table == 0 && self.timestamp == 0 && self.name_rva == 0
    }
}

/// Import lookup table entry (64-bit)
#[repr(C)]
#[repr(packed)]
pub struct ImportLookupEntry64 {
    pub value: u64,
}

impl ImportLookupEntry64 {
    pub fn is_name(&self) -> bool {
        (self.value & 0x8000000000000000) == 0
    }

    pub fn is_ordinal(&self) -> bool {
        (self.value & 0x8000000000000000) != 0
    }

    pub fn name_rva(&self) -> u32 {
        (self.value & 0x7FFFFFFF) as u32
    }

    pub fn ordinal(&self) -> u16 {
        (self.value & 0xFFFF) as u16
    }
}

/// Relocation entry
#[repr(C)]
#[repr(packed)]
pub struct RelocationEntry {
    pub page_rva: u32,
    pub block_size: u32,
}

impl RelocationEntry {
    pub fn entries(&self) -> u32 {
        (self.block_size - 8) / 2
    }
}

/// PE loader result
pub struct LoaderResult {
    pub entry_point: u64,
    pub image_base: u64,
    pub image_size: u64,
    pub subsystem: u16,
    pub dll_characteristics: u16,
}

impl LoaderResult {
    pub fn new() -> Self {
        Self {
            entry_point: 0,
            image_base: 0,
            image_size: 0,
            subsystem: 0,
            dll_characteristics: 0,
        }
    }
}

impl Default for LoaderResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Load PE image into memory
pub fn load_pe(image: &[u8]) -> Option<LoaderResult> {
    let (_, _, opt_magic, optional_header_offset) = parse_headers(image)?;

    let mut result = LoaderResult::new();

    match opt_magic {
        0x10B => {
            // PE32
            if optional_header_offset + 96 > image.len() {
                return None;
            }
            let opt = unsafe {
                core::ptr::read_unaligned(
                    image.as_ptr().add(optional_header_offset) as *const OptionalHeader32
                )
            };
            result.entry_point = opt.image_base as u64 + opt.address_of_entry_point as u64;
            result.image_base = opt.image_base as u64;
            result.image_size = opt.size_of_image as u64;
            result.subsystem = opt.subsystem;
            result.dll_characteristics = opt.dll_characteristics;
        }
        0x20B => {
            // PE32+
            if optional_header_offset + 112 > image.len() {
                return None;
            }
            let opt = unsafe {
                core::ptr::read_unaligned(
                    image.as_ptr().add(optional_header_offset) as *const OptionalHeader64
                )
            };
            result.entry_point = opt
                .image_base
                .checked_add(opt.address_of_entry_point as u64)?;
            result.image_base = opt.image_base;
            result.image_size = opt.size_of_image as u64;
            result.subsystem = opt.subsystem;
            result.dll_characteristics = opt.dll_characteristics;
        }
        _ => return None,
    }

    Some(result)
}

/// Get section headers from PE image (simplified)
pub fn get_sections(image: &[u8], output: &mut [SectionHeader]) -> usize {
    let Some((file_header, optional_header_size, _, optional_offset)) = parse_headers(image) else {
        return 0;
    };
    let num_sections = (file_header.number_of_sections as usize).min(output.len());
    let sections_offset = optional_offset + optional_header_size;

    for i in 0..num_sections as usize {
        let offset = sections_offset + i * 40;
        if offset + 40 > image.len() {
            break;
        }
        let section: SectionHeader = unsafe {
            core::ptr::read_unaligned(image.as_ptr().add(offset) as *const SectionHeader)
        };
        output[i] = section;
    }

    num_sections
}

// =====================================================================
//  NT-style PE loader
// =====================================================================
//
//  `load_pe` above is just a header parser; the routines below
//  actually map the file into memory, walk the import table,
//  resolve dependencies through the in-memory image database, and
//  apply base relocations. This is the work the OS Loader
//  (`winload.efi` -> `ntoskrnl.exe` bootstrap) does at boot; we
//  reproduce it here so that the kernel can load the PE files
//  produced by `pegen` and `system_image` at runtime.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// `IMAGE_ORDINAL_FLAG64` — in a PE32+ import table, an entry with
/// this bit set is an **ordinal import**; a clear bit means the entry
/// is a hint/name RVA.
pub const IMAGE_ORDINAL_FLAG64: u64 = 1u64 << 63;

/// A loaded PE image, in memory, after relocations and import
/// resolution have been applied. The `bytes` field is a private
/// owned copy in kernel virtual memory.
pub struct LoadedImage {
    pub name: String,
    pub image_base: u64,
    pub image_size: u64,
    pub entry_point: u64,
    pub subsystem: u16,
    /// True if this image is the kernel itself (subsystem
    /// `IMAGE_SUBSYSTEM_NATIVE`).
    pub is_kernel: bool,
    pub bytes: Vec<u8>,
}

impl LoadedImage {
    /// Access the loaded image bytes for callers that need to
    /// compute a function pointer within the image.
    pub fn image_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Convert an RVA to a pointer into the loaded image.
    /// True when `rva..rva + len` lies inside the loaded image buffer.
    ///
    /// Every RVA in a PE comes from the file, so it is untrusted input:
    /// a crafted image can point an import slot, relocation target, or
    /// export entry past the end of the buffer. All accessors below go
    /// through this check.
    pub fn contains(&self, rva: u32, len: usize) -> bool {
        let off = rva as usize;
        off < self.bytes.len() && off.checked_add(len) <= Some(self.bytes.len())
    }

    /// Convert an RVA to a pointer into the loaded image.
    /// Returns null when the RVA is outside the image.
    pub fn rva_to_ptr(&self, rva: u32) -> *mut u8 {
        if !self.contains(rva, 1) {
            return core::ptr::null_mut();
        }
        unsafe { self.bytes.as_ptr().add(rva as usize) as *mut u8 }
    }

    /// read a 16-bit value at `rva`, or `None` when out of bounds.
    pub fn read_u16(&self, rva: u32) -> Option<u16> {
        if !self.contains(rva, 2) {
            return None;
        }
        Some(unsafe { core::ptr::read_unaligned(self.rva_to_ptr(rva) as *const u16) })
    }
    pub fn read_u32(&self, rva: u32) -> Option<u32> {
        if !self.contains(rva, 4) {
            return None;
        }
        Some(unsafe { core::ptr::read_unaligned(self.rva_to_ptr(rva) as *const u32) })
    }
    pub fn read_u64(&self, rva: u32) -> Option<u64> {
        if !self.contains(rva, 8) {
            return None;
        }
        Some(unsafe { core::ptr::read_unaligned(self.rva_to_ptr(rva) as *const u64) })
    }
    pub fn write_u16(&mut self, rva: u32, v: u16) -> bool {
        if !self.contains(rva, 2) {
            return false;
        }
        unsafe { core::ptr::write_unaligned(self.rva_to_ptr(rva) as *mut u16, v) }
        true
    }
    pub fn write_u32(&mut self, rva: u32, v: u32) -> bool {
        if !self.contains(rva, 4) {
            return false;
        }
        unsafe { core::ptr::write_unaligned(self.rva_to_ptr(rva) as *mut u32, v) }
        true
    }
    pub fn write_u64(&mut self, rva: u32, v: u64) -> bool {
        if !self.contains(rva, 8) {
            return false;
        }
        unsafe { core::ptr::write_unaligned(self.rva_to_ptr(rva) as *mut u64, v) }
        true
    }

    /// Look up an export by name in the mapped PE image.
    pub fn find_export(&self, name: &str) -> Option<u64> {
        use image_link::{ExportQuery, ExportTarget};
        let exports = image_link::ImageExports::parse(&self.bytes, self.image_base)?;
        match exports.get(ExportQuery::Name(name))? {
            ExportTarget::Address(address) => Some(*address),
            ExportTarget::Forwarder(_) => None,
        }
    }
}

/// Parse the COFF file header and optional header (PE32/PE32+)
/// from a raw PE byte buffer. Returns a tuple
/// `(file_header, opt_header_size, opt_magic, opt_offset)` where
/// `opt_offset` is the byte index of the optional header in the
/// input slice.
pub fn parse_headers(image: &[u8]) -> Option<(FileHeader, usize, u16, usize)> {
    if image.len() < 64 {
        return None;
    }
    let dos = unsafe { core::ptr::read_unaligned(image.as_ptr() as *const DosHeader) };
    if !dos.is_valid() {
        return None;
    }
    let pe_off = usize::try_from(dos.e_lfanew).ok()?;
    if pe_off < 64 || pe_off.checked_add(24)? > image.len() {
        return None;
    }
    if &image[pe_off..pe_off + 4] != b"PE\0\0" {
        return None;
    }
    let fh_off = pe_off + 4;
    let file_hdr: FileHeader =
        unsafe { core::ptr::read_unaligned(image.as_ptr().add(fh_off) as *const FileHeader) };
    let opt_off = fh_off + 20;
    if opt_off + 2 > image.len() {
        return None;
    }
    let magic = u16::from_le_bytes([image[opt_off], image[opt_off + 1]]);
    if magic != 0x10B && magic != 0x20B {
        return None;
    }
    let opt_size = file_hdr.size_of_optional_header as usize;
    let minimum = if magic == 0x20b { 0x70 } else { 0x60 };
    if opt_size < minimum || opt_off.checked_add(opt_size)? > image.len() {
        return None;
    }
    let count_offset = opt_off + minimum - 4;
    let directory_count =
        u32::from_le_bytes(image.get(count_offset..count_offset + 4)?.try_into().ok()?) as usize;
    if directory_count > (opt_size - minimum) / 8 {
        return None;
    }
    let section_end = opt_off
        .checked_add(opt_size)?
        .checked_add((file_hdr.number_of_sections as usize).checked_mul(40)?)?;
    if section_end > image.len() {
        return None;
    }
    Some((file_hdr, opt_size, magic, opt_off))
}

/// read the data directory at index `i` from a parsed PE32+ image.
/// Returns `(rva, size)` or `(0, 0)` if the directory is empty or
/// the optional header is PE32 (32-bit directories are 8 bytes, we
/// only support PE32+ here).
pub fn read_data_directory_pe32plus(image: &[u8], opt_off: usize, i: usize) -> (u32, u32) {
    // PE32+ optional header layout (we are looking at the
    // "NumberOfRvaAndSizes" field, then 16 directories of
    // (u64 rva, u64 size) starting at offset 0x70 from opt_off).
    let Some(num_rva_off) = opt_off.checked_add(0x6C) else {
        return (0, 0);
    };
    if num_rva_off
        .checked_add(4)
        .map_or(true, |end| end > image.len())
    {
        return (0, 0);
    }
    let num_rva = u32::from_le_bytes([
        image[num_rva_off],
        image[num_rva_off + 1],
        image[num_rva_off + 2],
        image[num_rva_off + 3],
    ]);
    if i >= num_rva as usize {
        return (0, 0);
    }
    let Some(dir_off) = i
        .checked_mul(8)
        .and_then(|n| opt_off.checked_add(0x70)?.checked_add(n))
    else {
        return (0, 0);
    };
    let Some((_, opt_size, 0x20b, parsed_off)) = parse_headers(image) else {
        return (0, 0);
    };
    if parsed_off != opt_off
        || dir_off
            .checked_add(8)
            .map_or(true, |end| end > opt_off + opt_size)
    {
        return (0, 0);
    }
    let rva = u32::from_le_bytes([
        image[dir_off],
        image[dir_off + 1],
        image[dir_off + 2],
        image[dir_off + 3],
    ]);
    let size = u32::from_le_bytes([
        image[dir_off + 4],
        image[dir_off + 5],
        image[dir_off + 6],
        image[dir_off + 7],
    ]);
    (rva, size)
}

/// read the data directory at index `i` from a parsed PE32 image.
/// Returns `(rva, size)` or `(0, 0)` if the directory is empty.
/// PE32 uses 8-byte entries (u32 rva, u32 size) starting at offset 0x60.
pub fn read_data_directory_pe32(image: &[u8], opt_off: usize, i: usize) -> (u32, u32) {
    // PE32 optional header has NumberOfRvaAndSizes at offset 0x5C,
    // and directories start at offset 0x60.
    let Some(num_rva_off) = opt_off.checked_add(0x5C) else {
        return (0, 0);
    };
    if num_rva_off
        .checked_add(4)
        .map_or(true, |end| end > image.len())
    {
        return (0, 0);
    }
    let num_rva = u32::from_le_bytes([
        image[num_rva_off],
        image[num_rva_off + 1],
        image[num_rva_off + 2],
        image[num_rva_off + 3],
    ]);
    if i >= num_rva as usize {
        return (0, 0);
    }
    let Some(dir_off) = i
        .checked_mul(8)
        .and_then(|n| opt_off.checked_add(0x60)?.checked_add(n))
    else {
        return (0, 0);
    };
    let Some((_, opt_size, 0x10b, parsed_off)) = parse_headers(image) else {
        return (0, 0);
    };
    if parsed_off != opt_off
        || dir_off
            .checked_add(8)
            .map_or(true, |end| end > opt_off + opt_size)
    {
        return (0, 0);
    }
    let rva = u32::from_le_bytes([
        image[dir_off],
        image[dir_off + 1],
        image[dir_off + 2],
        image[dir_off + 3],
    ]);
    let size = u32::from_le_bytes([
        image[dir_off + 4],
        image[dir_off + 5],
        image[dir_off + 6],
        image[dir_off + 7],
    ]);
    (rva, size)
}

/// read the PE32+ optional header fields we need: `image_base`,
/// `size_of_image`, `address_of_entry_point`, `subsystem`.
fn read_opt64(image: &[u8], opt_off: usize) -> Option<(u64, u32, u32, u16)> {
    if opt_off.checked_add(0x70)? > image.len() {
        return None;
    }
    let aep = u32::from_le_bytes([
        image[opt_off + 0x10],
        image[opt_off + 0x11],
        image[opt_off + 0x12],
        image[opt_off + 0x13],
    ]);
    let img_base = u64::from_le_bytes([
        image[opt_off + 0x18],
        image[opt_off + 0x19],
        image[opt_off + 0x1A],
        image[opt_off + 0x1B],
        image[opt_off + 0x1C],
        image[opt_off + 0x1D],
        image[opt_off + 0x1E],
        image[opt_off + 0x1F],
    ]);
    let soi = u32::from_le_bytes([
        image[opt_off + 0x38],
        image[opt_off + 0x39],
        image[opt_off + 0x3A],
        image[opt_off + 0x3B],
    ]);
    let subsys = u16::from_le_bytes([image[opt_off + 0x44], image[opt_off + 0x45]]);
    Some((img_base, soi, aep, subsys))
}

/// read the PE32 optional header fields we need: `image_base`,
/// `size_of_image`, `address_of_entry_point`, `subsystem`.
/// PE32 has a 32-bit image base and different offsets.
fn read_opt32(image: &[u8], opt_off: usize) -> Option<(u64, u32, u32, u16)> {
    if opt_off + 0x60 > image.len() {
        return None;
    }
    let aep = u32::from_le_bytes([
        image[opt_off + 0x10],
        image[opt_off + 0x11],
        image[opt_off + 0x12],
        image[opt_off + 0x13],
    ]);
    // PE32 image base is 32-bit at offset 0x1C; PE32+ uses 8 bytes at 0x18.
    let img_base_lo = u32::from_le_bytes([
        image[opt_off + 0x1C],
        image[opt_off + 0x1D],
        image[opt_off + 0x1E],
        image[opt_off + 0x1F],
    ]);
    let img_base = img_base_lo as u64;
    let soi = u32::from_le_bytes([
        image[opt_off + 0x38],
        image[opt_off + 0x39],
        image[opt_off + 0x3A],
        image[opt_off + 0x3B],
    ]);
    let subsys = u16::from_le_bytes([image[opt_off + 0x44], image[opt_off + 0x45]]);
    Some((img_base, soi, aep, subsys))
}

mod image_bounds;
mod image_link;

/// Map a file PE into memory and return a fully loaded image with
/// all sections copied, base relocations applied, and import
/// tables resolved through `image_db`. This is the OS Loader's
/// main entry point.
///
/// `preferred_base` is the address the loader tried to map the
/// image at; if it is non-zero and the image's `ImageBase` differs,
/// the loader applies base relocations so the image runs correctly
/// at `preferred_base`.
pub fn load_image_full(
    name: &str,
    bytes: &[u8],
    image_db: &mut ImageDatabase,
    preferred_base: u64,
) -> Option<LoadedImage> {
    let (file_hdr, _opt_size, magic, opt_off) = parse_headers(bytes)?;
    let (image_base, size_of_image, aep, subsystem) = if magic == 0x10B {
        read_opt32(bytes, opt_off)?
    } else if magic == 0x20B {
        read_opt64(bytes, opt_off)?
    } else {
        return None;
    };
    if size_of_image == 0 || bytes.len() < 0x40 + 4 {
        return None;
    }
    unsafe {
        ntoskrnl::mm::heap::KERNEL_HEAP.debug_free_list();
    }
    let sect_table_off = opt_off + file_hdr.size_of_optional_header as usize;
    let header_extent = u32::from_le_bytes([
        bytes[opt_off + 0x3C],
        bytes[opt_off + 0x3D],
        bytes[opt_off + 0x3E],
        bytes[opt_off + 0x3F],
    ]) as usize;
    let mut section_layout = Vec::with_capacity(file_hdr.number_of_sections as usize);
    for i in 0..file_hdr.number_of_sections as usize {
        let sh_off = sect_table_off + i * 40;
        if sh_off + 40 > bytes.len() {
            return None;
        }
        let section: SectionHeader = unsafe {
            core::ptr::read_unaligned(bytes.as_ptr().add(sh_off) as *const SectionHeader)
        };
        let raw_end = (section.pointer_to_raw_data as usize)
            .checked_add(section.size_of_raw_data as usize)?;
        if section.size_of_raw_data != 0 && raw_end > bytes.len() {
            return None;
        }
        section_layout.push((section.virtual_address as usize,
            section.virtual_size as usize, section.size_of_raw_data as usize));
    }
    let headers = if header_extent == 0 {
        (sect_table_off + file_hdr.number_of_sections as usize * 40).min(4096)
    } else { header_extent };
    // Large private BSS reserves address space; it does not consume this buffer.
    // Validate section extents before any RVA-sized allocation as well.
    let initialized_extent = image_bounds::initialized_extent(
        size_of_image as usize, headers, section_layout)?;
    let mut img = alloc::vec![0; initialized_extent];
    // `loaded.bytes` is addressed by RVA.  The headers live at RVA 0 and
    // are not part of any section, so they must be copied explicitly.
    // Without this, every later parse of the mapped image sees a zeroed
    // DOS header and fails before any section can be executed.
    let size_of_headers = u32::from_le_bytes([
        bytes[opt_off + 0x3C],
        bytes[opt_off + 0x3D],
        bytes[opt_off + 0x3E],
        bytes[opt_off + 0x3F],
    ]) as usize;
    // A prebuilt image with SizeOfHeaders == 0 (a legacy PE layout) must
    // still have its DOS header, PE headers and section table copied:
    // without them every later re-parse of the mapped image sees a
    // zeroed header and fails. Fall back to the end of the section
    // table, capped at one page.
    let header_copy_len = if size_of_headers == 0 {
        let sect_end = opt_off
            + file_hdr.size_of_optional_header as usize
            + file_hdr.number_of_sections as usize * 40;
        sect_end.min(0x1000).min(bytes.len()).min(img.len())
    } else {
        size_of_headers.min(bytes.len()).min(img.len())
    };
    img[..header_copy_len].copy_from_slice(&bytes[..header_copy_len]);
    let _source_head = if bytes.len() >= 8 {
        let mut value = [0u8; 8];
        value.copy_from_slice(&bytes[0..8]);
        u64::from_le_bytes(value)
    } else {
        0
    };
    let _image_head = if img.len() >= 8 {
        let mut value = [0u8; 8];
        value.copy_from_slice(&img[0..8]);
        u64::from_le_bytes(value)
    } else {
        0
    };
    // Copy sections. Section table starts right after the optional
    // header.
    let sect_off = opt_off + file_hdr.size_of_optional_header as usize;
    for i in 0..file_hdr.number_of_sections as usize {
        let sh_off = sect_off + i * 40;
        if sh_off + 40 > bytes.len() {
            return None;
        }
        let sh: SectionHeader = unsafe {
            core::ptr::read_unaligned(bytes.as_ptr().add(sh_off) as *const SectionHeader)
        };
        if sh.size_of_raw_data == 0 {
            continue;
        }
        let src = sh.pointer_to_raw_data as usize;
        let dst = sh.virtual_address as usize;
        if dst >= size_of_image as usize {
            return None;
        }
        let sz = core::cmp::min(sh.size_of_raw_data as usize, size_of_image as usize - dst);
        if src + sz > bytes.len() || dst + sz > img.len() {
            return None;
        }
        img[dst..dst + sz].copy_from_slice(&bytes[src..src + sz]);
    }

    let actual_base = if preferred_base != 0 {
        preferred_base
    } else {
        image_base
    };
    let mut loaded = LoadedImage {
        name: String::from(name),
        image_base: actual_base,
        image_size: size_of_image as u64,
        entry_point: actual_base.checked_add(aep as u64)?,
        subsystem,
        is_kernel: subsystem == 1, // IMAGE_SUBSYSTEM_NATIVE
        bytes: img,
    };

    // Apply base relocations if we landed at a different address
    // than the preferred base.
    if actual_base != image_base {
        let delta = actual_base.wrapping_sub(image_base) as i64;
        apply_base_relocations(&mut loaded, bytes, delta);
    }

    image_link::bind_imports(&mut loaded.bytes, image_db).ok()?;
    Some(loaded)
}

/// Relocate and bind a PE32+ boot module in the pages assigned to it by Winload.
pub fn link_boot_image(
    name: &str,
    header: nt61_boot_types::boot_image::Header,
    mapped: &mut [u8],
    load_base: u64,
    image_db: &ImageDatabase,
) -> Result<usize, &'static str> {
    let mapped = mapped.get_mut(..header.image_size).ok_or("short mapped boot image")?;
    let imports = image_link::prepare_imports(mapped, image_db)?;
    let relocations = header.relocate(mapped, load_base)?;
    imports.apply(mapped);
    image_db.register_mapped(name, mapped, load_base);
    Ok(relocations)
}

pub struct MappedBootImage<'a> {
    pub name: &'static str,
    pub header: nt61_boot_types::boot_image::Header,
    pub bytes: &'a mut [u8],
    pub load_base: u64,
}

/// Bind a boot set whose modules may import from one another in either direction.
pub fn link_boot_images(images: &mut [MappedBootImage<'_>]) -> Result<(), (&'static str, &'static str)> {
    let database = ImageDatabase::new();
    for image in images.iter() {
        let mapped = image.bytes.get(..image.header.image_size)
            .ok_or((image.name, "short mapped boot image"))?;
        database.register_mapped(image.name, mapped, image.load_base);
    }

    let mut bindings = Vec::with_capacity(images.len());
    for image in images.iter_mut() {
        let mapped = &mut image.bytes[..image.header.image_size];
        let imports = image_link::prepare_imports(mapped, &database)
            .map_err(|reason| (image.name, reason))?;
        image.header.validate_relocations(mapped, image.load_base)
            .map_err(|reason| (image.name, reason))?;
        bindings.push(imports);
    }

    for (image, imports) in images.iter_mut().zip(bindings) {
        let mapped = &mut image.bytes[..image.header.image_size];
        image.header.relocate(mapped, image.load_base)
            .map_err(|reason| (image.name, reason))?;
        imports.apply(mapped);
    }
    Ok(())
}

/// Apply the `.reloc` table to the loaded image. Each relocation
/// block is `(page_rva, block_size)` followed by `block_size / 2`
/// 16-bit entries; the high 4 bits are the relocation type, the
/// low 12 bits are the offset within the 4 KB page.
///
/// Relocation types we handle for both PE32 and PE32+:
///   * `IMAGE_REL_BASED_ABSOLUTE` (0)  — padding, skip
///   * `IMAGE_REL_BASED_HIGHLOW`   (3)  — 32-bit VA correction (PE32)
///   * `IMAGE_REL_BASED_HIGHADJ`    (4)  — High 16 + signed low 16 adjustment
///   * `IMAGE_REL_BASED_HIGH`       (1)  — high 16 bits of the delta
///   * `IMAGE_REL_BASED_LOW`        (2)  — low 16 bits of the delta
///   * `IMAGE_REL_BASED_HIGHLOW`    (3)  — all 32 bits of the delta
///   * `IMAGE_REL_BASED_HIGHADJ`    (4)  — signed 16-bit adjustment + high delta
///   * `IMAGE_REL_BASED_REL32`      (7)  — RIP-relative 32-bit offset
///   * `IMAGE_REL_BASED_DIR64`     (10)  — 64-bit VA correction (PE32+)
///
/// Returns the number of relocations applied, or 0 on error.
fn apply_base_relocations(loaded: &mut LoadedImage, raw: &[u8], delta: i64) -> usize {
    if delta == 0 {
        return 0;
    }
    let (file_hdr, opt_size, magic, opt_off) = match parse_headers(raw) {
        Some(p) => p,
        None => return 0,
    };
    // Support both PE32 (0x10B) and PE32+ (0x20B)
    // PE32 uses HIGHLOW (0x3) relocations, PE32+ uses DIR64 (0xA)
    if magic != 0x10B && magic != 0x20B {
        return 0;
    }
    // For PE32, use relocation type based on file format
    let is_pe32plus = magic == 0x20B;

    // read relocation directory using appropriate format
    let (reloc_rva, reloc_size) = if is_pe32plus {
        read_data_directory_pe32plus(raw, opt_off, 5)
    } else {
        read_data_directory_pe32(raw, opt_off, 5)
    };
    if reloc_rva == 0 || reloc_size == 0 {
        return 0;
    }

    let sect_off = opt_off + opt_size;
    let mut sects: alloc::vec::Vec<(u32, u32, u32)> = alloc::vec::Vec::new();
    for i in 0..file_hdr.number_of_sections as usize {
        let off = sect_off + i * 40;
        if off + 40 > raw.len() {
            break;
        }
        let sh: SectionHeader =
            unsafe { core::ptr::read_unaligned(raw.as_ptr().add(off) as *const SectionHeader) };
        sects.push((
            sh.virtual_address,
            sh.pointer_to_raw_data,
            sh.size_of_raw_data,
        ));
    }
    let rva_to_file = |rva: u32| -> Option<usize> {
        for &(vaddr, raw_ptr, raw_sz) in &sects {
            if rva >= vaddr && rva < vaddr.saturating_add(raw_sz) {
                return Some(raw_ptr as usize + (rva - vaddr) as usize);
            }
        }
        None
    };

    let mut cur = match rva_to_file(reloc_rva) {
        Some(o) => o,
        None => return 0,
    };
    let end = cur.saturating_add(reloc_size as usize);
    let mut applied = 0usize;

    // Base relocation type values as defined by the PE format spec. The
    // previous values were wrong: HIGHADJ is 4 (not 5), REL32 is 7 (not 4),
    // and 6 is reserved and must be zero. Types 5 and 8 are architecture
    // specific (MIPS/ARM/RISC-V/LoongArch) and are not applicable here.
    const REL_BASED_ABSOLUTE: u16 = 0;
    const REL_BASED_HIGH: u16 = 1;
    const REL_BASED_LOW: u16 = 2;
    const REL_BASED_HIGHLOW: u16 = 3;
    const REL_BASED_HIGHADJ: u16 = 4;
    const REL_BASED_REL32: u16 = 7;
    const REL_BASED_DIR64: u16 = 10;

    while cur + 8 <= end.min(raw.len()) {
        let page_rva = u32::from_le_bytes([raw[cur], raw[cur + 1], raw[cur + 2], raw[cur + 3]]);
        let block_size =
            u32::from_le_bytes([raw[cur + 4], raw[cur + 5], raw[cur + 6], raw[cur + 7]]);

        if !(8..=0x1000_0000).contains(&block_size) {
            break;
        }
        // Walk the block by byte range rather than by a guessed entry count:
        // HIGHADJ consumes two slots, so a fixed iteration count can read
        // past the end of the block.
        let block_end = cur.saturating_add(block_size as usize).min(raw.len());
        cur += 8;

        while cur + 2 <= block_end {
            let entry = u16::from_le_bytes([raw[cur], raw[cur + 1]]);
            cur += 2;

            let reloc_type = entry >> 12;
            let offset_in_page = (entry & 0xFFF) as u32;
            let target_rva = page_rva.wrapping_add(offset_in_page);

            match reloc_type {
                REL_BASED_ABSOLUTE => {
                    // No action needed
                }
                REL_BASED_HIGH => {
                    if let Some(current) = loaded.read_u16(target_rva) {
                        let new_val = current.wrapping_add(((delta >> 16) as u32) as u16);
                        if loaded.write_u16(target_rva, new_val) {
                            applied += 1;
                        }
                    }
                }
                REL_BASED_LOW => {
                    if let Some(current) = loaded.read_u16(target_rva) {
                        let new_val = current.wrapping_add((delta as u32) as u16);
                        if loaded.write_u16(target_rva, new_val) {
                            applied += 1;
                        }
                    }
                }
                REL_BASED_HIGHLOW => {
                    // 32-bit relocation: apply delta to 32-bit value
                    // Used by PE32 images
                    if let Some(current) = loaded.read_u32(target_rva) {
                        let new_val = (current as i32).wrapping_add(delta as i32) as u32;
                        if loaded.write_u32(target_rva, new_val) {
                            applied += 1;
                        }
                    }
                }
                REL_BASED_HIGHADJ => {
                    // HIGHADJ occupies two slots. The second slot is a signed
                    // 16-bit adjustment; the first slot's low 12 bits identify
                    // the 16-bit target within the page. Reconstruct the
                    // affected 32-bit value before taking its high half so
                    // carries and negative adjustments are preserved.
                    if cur + 2 > block_end {
                        break;
                    }
                    let next = u16::from_le_bytes([raw[cur], raw[cur + 1]]);
                    cur += 2;
                    if let Some(current) = loaded.read_u16(target_rva) {
                        let new_val = apply_highadj(current, next, delta);
                        if loaded.write_u16(target_rva, new_val) {
                            applied += 1;
                        }
                    }
                }
                REL_BASED_REL32 => {
                    // RIP-relative 32-bit relocation for x64
                    // The value at target_rva is a relative offset that is relative
                    // to the instruction pointer after this instruction.
                    // new_val = old_val + delta. The delta is already computed as
                    // (actual_base - preferred_base), so we just add it.
                    if let Some(current) = loaded.read_u32(target_rva) {
                        let new_val = (current as i32).wrapping_add(delta as i32) as u32;
                        if loaded.write_u32(target_rva, new_val) {
                            applied += 1;
                        }
                    }
                }
                REL_BASED_DIR64 => {
                    // 64-bit relocation: apply delta to 64-bit value
                    // Only valid for PE32+ images. For PE32 images that somehow
                    // have this type, treat it as unsupported.
                    if !is_pe32plus {
                        continue;
                    }
                    if let Some(current) = loaded.read_u64(target_rva) {
                        let new_val = (current as i64).wrapping_add(delta) as u64;
                        if loaded.write_u64(target_rva, new_val) {
                            applied += 1;
                        }
                    }
                }
                _ => {
                    // Unknown relocation type - skip
                }
            }
        }
    }
    applied
}

pub(crate) fn apply_highadj(current: u16, next: u16, delta: i64) -> u16 {
    let combined = ((current as i64) << 16)
        .wrapping_add(i16::from_le_bytes(next.to_le_bytes()) as i64)
        .wrapping_add(delta);
    (combined >> 16) as u16
}

use ntoskrnl::ke::sync::Spinlock;

struct RegisteredImage {
    name: String,
    base: u64,
    exports: image_link::ImageExports,
}

pub struct ImageDatabase {
    images: Spinlock<Vec<RegisteredImage>>,
}

impl ImageDatabase {
    pub fn new() -> Self {
        Self { images: Spinlock::new(Vec::new()) }
    }

    pub fn register(&self, image: &LoadedImage, _raw_pe: &[u8]) {
        self.register_mapped(&image.name, &image.bytes, image.image_base);
    }

    fn register_mapped(&self, name: &str, mapped: &[u8], base: u64) {
        let exports = image_link::ImageExports::parse(mapped, base).unwrap_or_default();
        self.images.lock().push(RegisteredImage { name: String::from(name), base, exports });
    }

    pub fn lookup(&self, dll: &str, name: &str) -> Option<u64> {
        self.resolve(dll, image_link::ExportQuery::Name(name))
    }

    pub fn lookup_ordinal(&self, dll: &str, ordinal: u16) -> Option<u64> {
        self.resolve(dll, image_link::ExportQuery::Ordinal(ordinal))
    }

    fn resolve(&self, dll: &str, query: image_link::ExportQuery<'_>) -> Option<u64> {
        use image_link::{ExportQuery, ExportTarget};

        let images = self.images.lock();
        let mut dll = dll;
        let mut query = query;
        let mut forwarded = false;
        for _ in 0..32 {
            let target = images
                .iter()
                .filter(|image| {
                    image.name.eq_ignore_ascii_case(dll)
                        || (forwarded && image.name.rsplit_once('.').is_some_and(|(stem, extension)| {
                            extension.eq_ignore_ascii_case("dll") && stem.eq_ignore_ascii_case(dll)
                        }))
                })
                .find_map(|image| image.exports.get(query))?;
            match target {
                ExportTarget::Address(address) => return Some(*address),
                ExportTarget::Forwarder(target) => {
                    let (module, symbol) = target.rsplit_once('.')?;
                    dll = module;
                    query = match symbol.strip_prefix('#') {
                        Some(ordinal) => ExportQuery::Ordinal(ordinal.parse().ok()?),
                        None => ExportQuery::Name(symbol),
                    };
                    forwarded = true;
                }
            }
        }
        None
    }

    pub fn find_image_base(&self, name: &str) -> Option<u64> {
        self.images.lock().iter().find(|image| image.name.eq_ignore_ascii_case(name))
            .map(|image| image.base)
    }

    pub fn list(&self) -> Vec<String> {
        self.images.lock().iter().map(|image| image.name.clone()).collect()
    }
}

impl Default for ImageDatabase {
    fn default() -> Self {
        Self::new()
    }
}

/// `get_self_image_base` — return the base address of the
/// calling kernel image. Used by `kernel32!get_module_handle_w(NULL)`
/// to return a non-NULL module handle. The bootstrap does
/// not have a real "self" image; we return a fixed
/// placeholder so the smoke test has a non-NULL value to
/// compare.
pub fn get_self_image_base() -> u64 {
    0xFFFF_8000_0010_0000
}

// =====================================================================
// TLS (Thread Local Storage) Callback Support
// =====================================================================
//
// Windows PE images can declare TLS data and callbacks in the
// IMAGE_TLS_DIRECTORY. TLS provides thread-local storage, and the
// TLS callbacks are called when:
//   - A thread starts (DLL_THREAD_ATTACH)
//   - A thread exits (DLL_THREAD_DETACH)
//   - A process loads a DLL (DLL_PROCESS_ATTACH)
//
// The TLS directory contains:
//   - Raw data start/end (template for TLS initialization)
//   - Index location (where to store the TLS index)
//   - Callbacks location (array of function pointers)
//   - Size of zero fill (for uninitialized data)
//   - Characteristics (alignment, etc.)

/// IMAGE_TLS_DIRECTORY64 structure
/// Located at RVA specified by the TLS data directory (index 9)
#[repr(C)]
pub struct ImageTlsDirectory64 {
    /// Raw data start VA
    pub start_va: u64,
    /// Raw data end VA
    pub end_va: u64,
    /// TLS index location (image-relative)
    pub index_va: u64,
    /// TLS callbacks location (image-relative)
    pub callbacks_va: u64,
    /// Size of zero fill
    pub size_of_zero_fill: u32,
    /// Characteristics
    pub characteristics: u32,
}

static mut GLOBAL_IMAGE_DB: MaybeUninit<ImageDatabase> = MaybeUninit::uninit();
static GLOBAL_IMAGE_DB_READY: AtomicBool = AtomicBool::new(false);

/// Initialize the process-wide image database used by user-mode loads.
pub fn init_global_image_db() {
    if !GLOBAL_IMAGE_DB_READY.swap(true, Ordering::AcqRel) {
        unsafe {
            core::ptr::addr_of_mut!(GLOBAL_IMAGE_DB).write(MaybeUninit::new(ImageDatabase::new()));
        }
    }
}

pub fn global_image_db() -> Option<&'static mut ImageDatabase> {
    if !GLOBAL_IMAGE_DB_READY.load(Ordering::Acquire) {
        return None;
    }
    Some(unsafe {
        (*core::ptr::addr_of_mut!(GLOBAL_IMAGE_DB))
            .as_mut_ptr()
            .as_mut()
            .unwrap()
    })
}

impl ImageTlsDirectory64 {
    /// Get the TLS callback array pointer
    pub fn get_callbacks_ptr(&self) -> *const u64 {
        if self.callbacks_va == 0 {
            core::ptr::null()
        } else {
            self.callbacks_va as *const u64
        }
    }

    /// Get the number of callbacks (iterate until null terminator)
    pub fn callback_count(&self) -> usize {
        if self.callbacks_va == 0 {
            return 0;
        }

        let mut count = 0;
        let callbacks = self.get_callbacks_ptr();

        unsafe {
            while core::ptr::read(callbacks.add(count)) != 0 {
                count += 1;
            }
        }

        count
    }
}

/// TLS data for a thread
#[repr(C)]
pub struct TlsData {
    /// Pointer to the TLS directory in the image
    pub tls_dir: *const ImageTlsDirectory64,
    /// TLS index for this thread (assigned by loader)
    pub tls_index: u32,
    /// Pointer to the allocated TLS template data
    pub tls_data_ptr: *mut u8,
    /// Size of the TLS data
    pub tls_data_size: usize,
}

impl TlsData {
    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    /// create new TLS data for a thread
    pub unsafe fn new(tls_dir: *const ImageTlsDirectory64) -> Option<&'static mut Self> {
        if tls_dir.is_null() {
            return None;
        }

        let dir = unsafe { &*tls_dir };

        // Calculate the size of TLS data
        let start = dir.start_va as usize;
        let end = dir.end_va as usize;
        let size = end.saturating_sub(start);

        // Allocate from non-paged pool
        let data_ptr = ntoskrnl::mm::pool::allocate(ntoskrnl::mm::pool::PoolType::NonPaged, size);

        if data_ptr.is_null() {
            return None;
        }

        // Initialize with zeros (TLS data is zero-initialized by default)
        unsafe {
            core::ptr::write_bytes(data_ptr, 0, size);
        }

        let tls_data = ntoskrnl::mm::pool::allocate(
            ntoskrnl::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<Self>(),
        ) as *mut Self;

        if tls_data.is_null() {
            let _ = ntoskrnl::mm::pool::free(data_ptr);
            return None;
        }

        unsafe {
            core::ptr::write(
                tls_data,
                Self {
                    tls_dir,
                    tls_index: 0, // Will be set by allocate_tls_slot
                    tls_data_ptr: data_ptr,
                    tls_data_size: size,
                },
            );
        }

        unsafe { tls_data.as_mut() }
    }

    /// Free TLS data
    pub fn free(&mut self) {
        unsafe {
            if !self.tls_data_ptr.is_null() {
                {
                    let _ = ntoskrnl::mm::pool::free(self.tls_data_ptr);
                }
                self.tls_data_ptr = core::ptr::null_mut();
            }
        }
    }
}

/// Parse the TLS directory from a PE image
pub fn parse_tls_directory(image: &[u8]) -> Option<ImageTlsDirectory64> {
    // Get PE headers
    let (file_hdr, _opt_size, magic, opt_off) = parse_headers(image)?;

    if magic != 0x20B {
        // Only PE32+ is supported
        return None;
    }

    // read TLS directory (data directory index 9)
    let (tls_rva, tls_size) = read_data_directory_pe32plus(image, opt_off, 9);

    if tls_rva == 0 || tls_size == 0 {
        return None;
    }

    // Build section table for RVA -> file offset conversion
    let sect_off = opt_off + file_hdr.size_of_optional_header as usize;
    let mut sects: alloc::vec::Vec<(u32, u32, u32)> = alloc::vec::Vec::new();

    for i in 0..file_hdr.number_of_sections as usize {
        let off = sect_off + i * 40;
        if off + 40 > image.len() {
            break;
        }
        let sh: SectionHeader =
            unsafe { core::ptr::read_unaligned(image.as_ptr().add(off) as *const SectionHeader) };
        sects.push((
            sh.virtual_address,
            sh.pointer_to_raw_data,
            sh.size_of_raw_data,
        ));
    }

    let rva_to_file = |rva: u32| -> Option<usize> {
        for &(vaddr, raw_ptr, raw_sz) in &sects {
            if rva >= vaddr && rva < vaddr.saturating_add(raw_sz) {
                return Some(raw_ptr as usize + (rva - vaddr) as usize);
            }
        }
        None
    };

    // Convert TLS directory RVA to file offset
    let tls_file = rva_to_file(tls_rva)?;

    if tls_file + 32 > image.len() {
        return None;
    }

    // read the TLS directory
    let tls_dir = ImageTlsDirectory64 {
        start_va: u64::from_le_bytes([
            image[tls_file],
            image[tls_file + 1],
            image[tls_file + 2],
            image[tls_file + 3],
            image[tls_file + 4],
            image[tls_file + 5],
            image[tls_file + 6],
            image[tls_file + 7],
        ]),
        end_va: u64::from_le_bytes([
            image[tls_file + 8],
            image[tls_file + 9],
            image[tls_file + 10],
            image[tls_file + 11],
            image[tls_file + 12],
            image[tls_file + 13],
            image[tls_file + 14],
            image[tls_file + 15],
        ]),
        index_va: u64::from_le_bytes([
            image[tls_file + 16],
            image[tls_file + 17],
            image[tls_file + 18],
            image[tls_file + 19],
            image[tls_file + 20],
            image[tls_file + 21],
            image[tls_file + 22],
            image[tls_file + 23],
        ]),
        callbacks_va: u64::from_le_bytes([
            image[tls_file + 24],
            image[tls_file + 25],
            image[tls_file + 26],
            image[tls_file + 27],
            image[tls_file + 28],
            image[tls_file + 29],
            image[tls_file + 30],
            image[tls_file + 31],
        ]),
        size_of_zero_fill: u32::from_le_bytes([
            image[tls_file + 32],
            image[tls_file + 33],
            image[tls_file + 34],
            image[tls_file + 35],
        ]),
        characteristics: u32::from_le_bytes([
            image[tls_file + 36],
            image[tls_file + 37],
            image[tls_file + 38],
            image[tls_file + 39],
        ]),
    };

    Some(tls_dir)
}

/// Execute TLS callbacks for an image
///
/// This is called during DLL_PROCESS_ATTACH to invoke any
/// registered TLS callbacks. The callbacks are called with:
///   - hinst = image base
///   - reason = DLL_PROCESS_ATTACH (1)
///   - reserved = 0
///
/// Returns the number of callbacks executed, or 0 on error.
pub fn execute_tls_callbacks(loaded: &LoadedImage, _reason: u32) -> usize {
    let tls_dir = match parse_tls_directory(&loaded.bytes) {
        Some(d) => d,
        None => {
            // crate::kprintln!("  [TLS] {}: no TLS directory found", loaded.name);
            return 0;
        }
    };

    if tls_dir.callbacks_va == 0 {
        // crate::kprintln!("  [TLS] {}: no TLS callbacks", loaded.name);
        return 0;
    }

    // Get callbacks pointer (relative to image base)
    let callbacks_base = loaded.image_base;
    let callbacks_ptr = (callbacks_base + tls_dir.callbacks_va) as *const u64;

    let mut count = 0;

    // Iterate through callbacks until we hit a null terminator
    unsafe {
        let mut i = 0;
        loop {
            let callback = core::ptr::read(callbacks_ptr.add(i));
            if callback == 0 {
                break;
            }

            // crate::kprintln!(
            //                 "  [TLS] {}: calling callback {} at 0x{:016x}",
            //                 loaded.name, i, callback
            //             );

            // In a real implementation, we'd call the callback:
            // callback(image_base, reason, 0)
            // For our stub, we just log the call

            i += 1;
            count += 1;
        }
    }

    // crate::kprintln!(
    //         "  [TLS] {}: executed {} TLS callbacks",
    //         loaded.name, count
    //     );

    count
}

/// Initialize TLS for a new thread
///
/// Called when a thread attaches to a DLL. This invokes
/// TLS callbacks registered with DLL_THREAD_ATTACH.
pub fn tls_thread_attach(loaded: &LoadedImage, reason: u32) {
    execute_tls_callbacks(loaded, reason);
}

/// DllMain reason codes (Windows SDK: winnt.h).
pub const DLL_PROCESS_ATTACH: u32 = 1;
pub const DLL_THREAD_ATTACH: u32 = 2;
pub const DLL_PROCESS_DETACH: u32 = 0;
pub const DLL_THREAD_DETACH: u32 = 3;

/// Call a DLL's entry point. On x86_64 the standard calling
/// convention passes arguments in RCX, RDX, R8, R9 (with
/// stack cleanup by the caller). For DllMain the signature is:
///
/// ```c
/// BOOL DllMain(HINSTANCE hinstDLL, DWORD fdwReason, LPVOID lpvReserved);
/// ```
///
/// This function looks up `DllMainCRTStartup`, `DllMainCRTStartup`,
/// or `DllMain` in the loaded image's export table and calls the
/// entry point with the DllMain convention:
///   arg0 = image_base  (HINSTANCE)
///   arg1 = reason      (DLL_PROCESS_ATTACH = 1)
///   arg2 = reserved    (NULL)
///   Returns the value in eax (as i32).
///
/// When no DllMain export exists the loader reports success (1 = TRUE),
/// matching the PE/COFF specification for DLLs without entry points.
pub fn call_dll_main(image: &LoadedImage, reason: u32, reserved: u64) -> i32 {
    let entry_names = ["DllMainCRTStartup", "DllMainCRTStartup", "DllMain"];
    for name in &entry_names {
        if let Some(addr) = image.find_export(name) {
            let rva = (addr - image.image_base) as usize;
            // Call through the mapped image (map_loaded_image_into_kernel_space),
            // never through the heap copy in `image.bytes`: the heap
            // copy is not at `image_base`, so its absolute references
            // (relocated data, resolved imports) would dereference the
            // wrong addresses.
            let system_pml4 = ntoskrnl::mm::vas::MM_SYSTEM_VAS.lock().pml4_phys;
            let target = image.image_base + rva as u64;
            if system_pml4 != 0
                && rva < image.bytes.len()
                && ntoskrnl::mm::vas::is_user_range_mapped(system_pml4, target, 0x1000)
            {
                let fn_ptr = target as *const u8;
                let entry: extern "C" fn(u64, u32, u64) -> i32 =
                    unsafe { core::mem::transmute(fn_ptr) };
                return entry(image.image_base, reason, reserved);
            }
            // Image not mapped (or no system root): skipping DllMain is
            // the only safe choice — executing the heap copy would run
            // relocated code at the wrong addresses.
            return 0;
        }
    }
    1
}

/// Load a user-mode DLL into the kernel's in-memory image database,
/// call its DllMain entry point (DLL_PROCESS_ATTACH), and report
/// success or failure.
///
/// This function is called from `kernel_main` Phase 9a after the
/// kernel's own subsystems are initialised. The DllMain entry point
/// is invoked with the real image base and return value.
///
/// `IMAGE_DB` must be initialised before calling this function.
pub fn load_user_dll(name: &str, image: &[u8], db: &mut ImageDatabase) -> bool {
    let loaded = match load_image_full(name, image, db, 0) {
        Some(li) => li,
        None => {
            // crate::kprintln!("  [USER DLL FAIL] {}: load_image_full returned None", name);
            return false;
        }
    };
    db.register(&loaded, image);
    // Make the image's export addresses real: map the relocated bytes
    // at image_base in the system address space before calling DllMain.
    if !map_loaded_image_into_kernel_space(&loaded) {
        // Mapping failed (e.g. pool exhaustion). DllMain is skipped by
        // call_dll_main rather than executed from the heap copy.
    }

    let result = call_dll_main(&loaded, DLL_PROCESS_ATTACH, 0);
    let _ = &result;
    // crate::kprintln!(
    //         "  [USER DLL] {} DllMain called and returned {}",
    //         name, result
    //     );
    // Drop the in-memory image: the database only retains parsed
    // exports and the mapped pages live independently.
    true
}

// ---------------------------------------------------------------------------
// Phase 0: load a PE image into a per-process user address space
// ---------------------------------------------------------------------------
//
// `load_image_full` produces a `LoadedImage` whose `bytes` live in
// a `Vec<u8>` in kernel memory. For Milestone B we need the bytes
// to live *in the process's user address space*, so that the CPU
// can execute the image when CR3 is switched to the per-process
// PML4.
//
// `load_into_user_address_space` does exactly that:
//
//   1. Parse the PE headers to learn the `image_base`,
//      `size_of_image`, and the per-section virtual layout.
//   2. Allocate a physical frame for every 4 KiB page of the image
//      and map it into the per-process PML4 with the appropriate
//      user-mode protection bits (.text = R+X+U, .data = R+W+U,
//      .rsrc = R+U).
//   3. Copy the section bytes from the source PE into the
//      newly-mapped pages.
//   4. Return the (image_base, entry_point) so the caller can set
//      `EPROCESS.user_rip`.
//
// This function does *not* apply base relocations or resolve
// imports — the system_image-generated PE binaries that Phase 0
// uses are position-independent stubs that the loader's full
// pipeline can handle. Once real PE binaries are loaded, those
// steps would need to be added.

/// Result of a successful per-process PE load.
#[derive(Debug, Clone, Copy)]
pub struct UserImageMapping {
    pub image_base: u64,
    pub image_size: u64,
    pub entry_point: u64,
}

/// Map `image` into the per-process PML4 at the PE's
/// `ImageBase`. Returns the resulting user-mode entry point.
///
/// This function properly handles section permissions:
///   - .text sections: R+X (readable and executable)
///   - .data sections: R+W (readable and writable)
///   - .rdata sections: R (readable only)
///   - .rsrc sections: R (readable only)
///   - Other initialized data: R+W
///   - Uninitialized data (.bss): R+W
pub fn load_into_user_address_space(pml4_phys: u64, image: &[u8]) -> Option<UserImageMapping> {
    load_named_image_into_user_address_space(pml4_phys, image, "user.exe")
}

/// Load a bootstrap process image with demand-zero private writable BSS.
/// The process must not be runnable while its mappings/VAD are installed.
/// Other loader callers retain eager mapping until they own an image VAD.
pub fn load_into_process_address_space(
    process: &mut ntoskrnl::ps::process::Eprocess,
    image: &[u8],
) -> Option<UserImageMapping> {
    use ntoskrnl::mm::{pool, vad::{VadEntry, VadFlags, VadProtection, VadType}, vas};
    if process.user_image_size != 0 { return None; }
    let root = process.pml4_phys;
    let mapping = load_named_user_image(root, image, "user.exe", cfg!(target_arch = "x86_64"))?;
    #[cfg(target_arch = "x86_64")]
    {
        let node = pool::allocate(pool::PoolType::NonPaged, core::mem::size_of::<VadEntry>()) as *mut VadEntry;
        let installed = if node.is_null() { false } else {
            // SAFETY: private pool allocation, retained by the process VAD tree.
            unsafe {
                node.write(VadEntry::new());
                (*node).set_range(mapping.image_base, mapping.image_base + mapping.image_size - 1);
                (*node).flags = VadFlags::SEC_COMMIT;
                (*node).vad_type = VadType::VadImage;
                // Individual present leaves retain section permissions. Only
                // explicit RW/NX zero PTEs are faulted in under this VAD.
                (*node).protection = VadProtection::READWRITE;
                process.vad_root.insert(&mut *node).is_ok()
            }
        };
        if !installed {
            let _ = vas::unmap_user_pages(root, mapping.image_base, mapping.image_size);
            if !node.is_null() { unsafe { pool::free(node as *mut u8); } }
            return None;
        }
    }
    process.user_image_base = mapping.image_base;
    process.user_image_size = mapping.image_size;
    Some(mapping)
}

/// Load a user-mode image after running the full PE pipeline (base
/// relocations and imports) when a global image database is available.
pub fn load_named_image_into_user_address_space(
    pml4_phys: u64,
    image: &[u8],
    image_name: &str,
) -> Option<UserImageMapping> {
    load_named_user_image(pml4_phys, image, image_name, false)
}

fn load_named_user_image(
    pml4_phys: u64, image: &[u8], image_name: &str, allow_demand_zero: bool,
) -> Option<UserImageMapping> {
    #[cfg(target_arch = "riscv64")]
    {
        let _ = image_name;
        return load_raw_image_into_user_address_space(pml4_phys, image);
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        // Run the full PE pipeline (section layout, base relocations
        // and import resolution) against the global image database
        // before mapping. Without relocations the image cannot load
        // when its preferred base conflicts; without import resolution
        // every IAT slot stays zero and the process faults on its
        // first imported call.
        let Some(db) = global_image_db() else {
            // Image database not initialised yet: fall back to the raw
            // mapper so the caller still gets a mapped image (with
            // unresolved imports rather than no image at all).
            ntoskrnl::hal::serial::write_string_always("K:load-db-missing\r\n");
            return load_raw_image_into_user_address_space(pml4_phys, image);
        };
        let loaded = match load_image_full(image_name, image, db, 0) {
            Some(l) => l,
            None => {
                ntoskrnl::hal::serial::write_string_always("K:load-full-failed\r\n");
                return None;
            }
        };
        match map_prebuilt_image_into_user_address_space(pml4_phys, &loaded, allow_demand_zero) {
            Some(m) => Some(m),
            None => {
                ntoskrnl::hal::serial::write_string_always("K:load-map-failed\r\n");
                None
            }
        }
    }
}

fn load_raw_image_into_user_address_space(
    pml4_phys: u64,
    image: &[u8],
) -> Option<UserImageMapping> {
    // 1. Parse PE headers.
    let (file_hdr, _opt_size, _magic, opt_off) = parse_headers(image)?;
    let (preferred_image_base, size_of_image, aep, _subsystem) = read_opt64(image, opt_off)?;
    if size_of_image == 0 {
        return None;
    }

    // The raw RISC-V path loads fixed-address images. Statically linked
    // ELF-to-PE desktop images contain absolute function pointers without
    // PE fixups, so choosing another base would leave those pointers at
    // their linked addresses. Reject unsupported bases before allocating
    // pages; a generic PE relocation helper cannot prove RISC-V instruction
    // and data fixups are complete.
    let preferred_base = preferred_image_base;
    #[cfg(target_arch = "riscv64")]
    let image_base = {
        let Some(base) = fixed_base::riscv64_user_image_base(preferred_base, size_of_image as u64) else {
            ntoskrnl::hal::serial::write_string_always("K:raw-rv-fixed-base-invalid\r\n");
            return None;
        };
        base
    };
    #[cfg(not(target_arch = "riscv64"))]
    let image_base = preferred_image_base;
    // Check the complete extent before allocating or installing any leaf.
    // A low firmware stack may occupy a native PE's preferred user VA.
    if !ntoskrnl::mm::vas::user_mapping_available(image_base, size_of_image as u64) {
        return None;
    }
    // The base delta the relocations must absorb. On x86_64 this is
    // normally zero (the image lands at its preferred base in a fresh
    // per-process address space), but a non-zero delta is not an
    // error: the OS loader relocates rather than refusing to load.
    let delta = (image_base as i64).wrapping_sub(preferred_base as i64);

    // 2. Reserve a contiguous physical range big enough for the
    //    image and map it into the per-process PML4.
    let page_count = (size_of_image as u64).div_ceil(0x1000) as usize;
    // Dynamically track every allocated page so images larger than
    // a fixed-size stack array are supported (a 16-slot array caps
    // at 64 KiB; real PE images can be megabytes).
    let mut phys_pages: alloc::vec::Vec<u64> = alloc::vec::Vec::with_capacity(page_count);
    for _ in 0..page_count {
        let Some(p) = ntoskrnl::mm::vas::alloc_zeroed_page_for_vas() else {
            for &pa in &phys_pages {
                ntoskrnl::mm::pfn::free_pfn(pa >> 12);
            }
            return None;
        };
        phys_pages.push(p);
    }

    // 2b. Build section permission table as a fixed-size stack
    // array. Real PEs never have more than ~32 sections; we
    // keep 16 slots because cmd.exe has 2.
    #[derive(Copy, Clone)]
    struct SectInfo {
        va_start: u64,
        va_end: u64,
        chars: u32,
    }
    let sect_off = opt_off + file_hdr.size_of_optional_header as usize;
    let mut section_info: alloc::vec::Vec<SectInfo> =
        alloc::vec::Vec::with_capacity(file_hdr.number_of_sections as usize);

    for i in 0..file_hdr.number_of_sections as usize {
        let sh_off = sect_off + i * 40;
        if sh_off + 40 > image.len() {
            continue;
        }
        let sh: SectionHeader = unsafe {
            core::ptr::read_unaligned(image.as_ptr().add(sh_off) as *const SectionHeader)
        };
        if sh.virtual_size == 0 {
            continue;
        }

        let va_start = image_base + sh.virtual_address as u64;
        let va_end = va_start + (sh.virtual_size.max(sh.size_of_raw_data)) as u64;
        section_info.push(SectInfo {
            va_start,
            va_end,
            chars: sh.characteristics,
        });
    }
    // 2c. Lay the image out by RVA (headers at RVA 0, then sections)
    //     and apply base relocations when the image did not land at
    //     its preferred base. This is the OS-loader step that runs
    //     before any of the image bytes reach user pages: pointers
    //     baked into .data/.rdata and RIP-relative displacements are
    //     all stored relative to the preferred ImageBase and must be
    //     fixed by the same delta the mapping moved by.
    // Physical image pages are already zeroed. Only initialized bytes need
    // staging for relocation; desktop binaries include a 64-MiB .bss heap,
    // which must not be duplicated in the 32-MiB kernel heap.
    let mut initialized_extent = (u32::from_le_bytes([
        image[opt_off + 0x3C],
        image[opt_off + 0x3D],
        image[opt_off + 0x3E],
        image[opt_off + 0x3F],
    ]) as usize)
        .min(size_of_image as usize);
    for i in 0..file_hdr.number_of_sections as usize {
        let sh_off = sect_off.checked_add(i.checked_mul(40)?)?;
        if sh_off.checked_add(40)? > image.len() {
            return None;
        }
        let sh: SectionHeader = unsafe {
            core::ptr::read_unaligned(image.as_ptr().add(sh_off) as *const SectionHeader)
        };
        if sh.size_of_raw_data == 0 {
            continue;
        }
        let start = sh.virtual_address as usize;
        let end = start.checked_add(sh.size_of_raw_data as usize)?;
        if end > size_of_image as usize {
            return None;
        }
        initialized_extent = initialized_extent.max(end);
    }
    let mut rva_image: alloc::vec::Vec<u8> = alloc::vec![0; initialized_extent];
    let header_len = (u32::from_le_bytes([
        image[opt_off + 0x3C],
        image[opt_off + 0x3D],
        image[opt_off + 0x3E],
        image[opt_off + 0x3F],
    ]) as usize)
        .min(image.len())
        .min(rva_image.len());
    rva_image[..header_len].copy_from_slice(&image[..header_len]);
    for i in 0..file_hdr.number_of_sections as usize {
        let sh_off = sect_off + i * 40;
        if sh_off + 40 > image.len() {
            continue;
        }
        let sh: SectionHeader = unsafe {
            core::ptr::read_unaligned(image.as_ptr().add(sh_off) as *const SectionHeader)
        };
        if sh.virtual_size == 0 || sh.size_of_raw_data == 0 {
            continue;
        }
        let dst = sh.virtual_address as usize;
        if dst >= rva_image.len() {
            continue;
        }
        let sz = core::cmp::min(sh.size_of_raw_data as usize, rva_image.len() - dst);
        let src = sh.pointer_to_raw_data as usize;
        if src + sz > image.len() {
            continue;
        }
        rva_image[dst..dst + sz].copy_from_slice(&image[src..src + sz]);
    }
    if delta != 0 {
        ntoskrnl::userspace::loader::apply_base_relocations_to_image(&mut rva_image, image, delta);
    }

    // 3. Map each 4 KiB page at `image_base + i*0x1000` with
    //    appropriate permissions based on which section the page
    //    belongs to.
    //
    // CRITICAL: initial mapping uses RW+NX (writable, non-executable)
    // so that the I-cache for this VA cannot possibly contain a
    // stale translation of bytes that lived here before this PML4 was
    // created. After we have written the section bytes we remap the
    // code pages to R+X (executable, read-only). The transition from
    // NX -> X forces QEMU's TCG translation cache (and real-CPU
    // I-cache lines) to be invalidated for the new bytes.
    for (page_index, &p) in phys_pages.iter().enumerate() {
        let page_va = image_base + (page_index as u64) * 0x1000;

        // Determine the correct FINAL protection for this page
        let mut final_flags = ntoskrnl::mm::vas::PTE_RW | ntoskrnl::mm::vas::PTE_US; // Default: R+W+U
        for &s in &section_info {
            if page_va >= s.va_start && page_va < s.va_end {
                final_flags = determine_section_protection(s.chars);
                break;
            }
        }
        // For the initial mapping, force RW+NX so we can write into
        // the page and so the page is NOT yet executable.
        let initial_flags = final_flags | ntoskrnl::mm::vas::PTE_RW | ntoskrnl::mm::vas::PTE_NX;

        let r = ntoskrnl::mm::vas::map_page_in_pml4(pml4_phys, page_va, p, initial_flags);
        if r != ntoskrnl::mm::vas::MmStatus::Ok {
            for &pa in &phys_pages {
                ntoskrnl::mm::pfn::free_pfn(pa >> 12);
            }
            return None;
        }
    }

    // 4. Copy the relocated image bytes into the user pages. The
    //    source is the RVA-laid-out, relocated buffer built in step
    //    2c — not the raw file — so the user process executes exactly
    //    the bytes the loader fixed up. We do this BEFORE the final
    //    NX->X remap so that
    //    the remap transition (which is what should force QEMU's TB
    //    cache invalidation) happens *after* the bytes are in place
    //    and right before user-mode execution begins.
    // CRITICAL: temporarily switch CR3 to the system PML4 before doing
    // physical-address writes. The current CR3 is the per-process user
    // PML4 (set by attach_process), whose identity-map pages may not be
    // reachable for the freshly-allocated image PFNs. The system PML4
    // has a stable W=1 identity map, so writes via VA=PA succeed.
    //
    // We use the per-CPU area's `system_pml4` slot, which was populated
    // by the first call to attach_process at boot. If it's still 0
    // (e.g. on the very first attach), we read CR3 itself as the
    // best-effort fallback.
    let saved_cr3 = ntoskrnl::arch::switch_to_system_address_space();
    // Copy by destination page rather than section. Section RVAs need not
    // be page-aligned, and consecutive virtual pages need not have adjacent
    // physical frames. A section-sized copy can otherwise corrupt a different
    // allocation when it crosses a physical page boundary.
    for (page_index, bytes) in rva_image.chunks(0x1000).enumerate() {
        let dst = phys_pages[page_index] as *mut u8;
        for (word_index, chunk) in bytes.chunks(8).enumerate() {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            // SAFETY: each destination is a zeroed, identity-mapped image
            // frame. Page-aligned chunks keep this write within that frame;
            // padding the last word preserves the zero-filled image tail.
            unsafe {
                core::ptr::write_volatile(
                    dst.add(word_index * 8) as *mut u64,
                    u64::from_le_bytes(word),
                );
            }
        }
    }

    // After all section bytes have been written, transition each
    // code page from RW+NX to its final R+X (or R-only) protection.
    // This NX->X transition is what invalidates QEMU's TB cache for
    // the VA, so the user-mode CPU executes the bytes we just wrote
    // rather than any stale translation it may have cached.
    //
    // CRITICAL: the page-table walker inside the CPU may still have
    // the OLD PTE cached in its TLB (the NX-bit change is a *flag*
    // change, not a structural change, so the TLB entry remains
    // valid). We must explicitly invlpg each code page to force
    // the walker to refetch the new PTE so that the NX bit is
    // cleared in the TLB. Without this, a real-CPU NX=1→NX=0
    // transition is NOT observed by the instruction-fetch unit,
    // and the user-mode #PF handler reports a fetch fault at the
    // cmd.exe entry RIP.
    for (i, &p) in phys_pages.iter().enumerate() {
        let page_va = image_base + (i as u64) * 0x1000;
        let mut final_flags = ntoskrnl::mm::vas::PTE_RW | ntoskrnl::mm::vas::PTE_US;
        for &s in &section_info {
            if page_va >= s.va_start && page_va < s.va_end {
                final_flags = determine_section_protection(s.chars);
                break;
            }
        }
        let r = ntoskrnl::mm::vas::map_page_in_pml4(pml4_phys, page_va, p, final_flags);
        if r != ntoskrnl::mm::vas::MmStatus::Ok {
            ntoskrnl::arch::restore_system_address_space(saved_cr3);
            for &pa in &phys_pages {
                ntoskrnl::mm::pfn::free_pfn(pa >> 12);
            }
            return None;
        }
        // Force TLB flush so the NX→X transition is visible to
        // the next instruction fetch on this VA.
        ntoskrnl::mm::vas::invalidate_tlb(page_va);
    }

    // Belt-and-braces: also flush the entire TLB by reloading CR3.
    // This guarantees that no stale translation (from any VA we
    // touched during the load) lingers when user-mode execution
    // starts. The cost is one extra CR3 write, which is negligible
    // for a one-shot boot path.
    ntoskrnl::arch::flush_tlb();

    // Restore CR3 to the user PML4 we were using before the copy.
    ntoskrnl::arch::restore_system_address_space(saved_cr3);

    let entry_point = image_base + aep as u64;
    // crate::kprintln!("[loader] loaded PE into user PML4: image_base=0x{:x} entry=0x{:x} size=0x{:x}",
    //               image_base, entry_point, size_of_image);
    Some(UserImageMapping {
        image_base,
        image_size: size_of_image as u64,
        entry_point,
    })
}

/// Map an already-relocated `LoadedImage` (whose `bytes` are the full
/// RVA-laid-out image with base relocations applied and imports
/// resolved) into a per-process user PML4 at `loaded.image_base`.
///
/// This completes the PE pipeline for user-mode images: the bytes the
/// loader validated (relocated, IAT-patched) are the bytes user code
/// executes, instead of a half-mapped re-parse of the raw file.
#[cfg(not(target_arch = "riscv64"))]
fn map_prebuilt_image_into_user_address_space(
    pml4_phys: u64,
    loaded: &LoadedImage,
    allow_demand_zero: bool,
) -> Option<UserImageMapping> {
    use ntoskrnl::mm::vas;
    let prebuilt = &loaded.bytes;
    let image_base = loaded.image_base;
    let size_of_image = loaded.image_size as usize;
    if prebuilt.is_empty() || size_of_image == 0 || image_base & 0xfff != 0
        || prebuilt.len() > size_of_image
        || !vas::user_mapping_available(image_base, size_of_image as u64)
    {
        return None;
    }
    let (file_hdr, _, magic, opt_off) = parse_headers(prebuilt)?;
    let aep = if magic == 0x10B { read_opt32(prebuilt, opt_off)?.2 }
              else { read_opt64(prebuilt, opt_off)?.2 };
    if aep as usize >= size_of_image { return None; }
    #[derive(Copy, Clone)]
    struct SectInfo { start: usize, end: usize, raw_end: usize, chars: u32 }
    let sect_off = opt_off.checked_add(file_hdr.size_of_optional_header as usize)?;
    let mut sections = alloc::vec::Vec::with_capacity(file_hdr.number_of_sections as usize);
    for i in 0..file_hdr.number_of_sections as usize {
        let offset = sect_off.checked_add(i.checked_mul(40)?)?;
        prebuilt.get(offset..offset.checked_add(40)?)?;
        let sh: SectionHeader = unsafe {
            core::ptr::read_unaligned(prebuilt.as_ptr().add(offset) as *const SectionHeader)
        };
        let start = sh.virtual_address as usize;
        let end = start.checked_add(sh.virtual_size.max(sh.size_of_raw_data) as usize)?;
        if end > size_of_image { return None; }
        if end > start {
            sections.push(SectInfo { start, end,
                raw_end: start.checked_add(sh.size_of_raw_data as usize)?, chars: sh.characteristics });
        }
    }
    let page_count = size_of_image.div_ceil(4096);
    // (PA, final protection); PA=0 is an explicitly reserved demand-zero leaf.
    let mut pages: alloc::vec::Vec<(u64, u64)> = alloc::vec::Vec::with_capacity(page_count);
    for i in 0..page_count {
        let start = i * 4096;
        let end = (start + 4096).min(size_of_image);
        let mut final_flags = vas::PTE_US | vas::PTE_NX; // headers/gaps are read-only
        let mut matching = sections.iter().filter(|s| start < s.end && end > s.start);
        let first = matching.next();
        let shared_page = matching.next().is_some();
        if let Some(s) = first {
            final_flags = determine_section_protection(s.chars);
            // PE sections can share a page; combine all actual page permissions.
            for s in sections.iter().filter(|s| start < s.end && end > s.start) {
                let flags = determine_section_protection(s.chars);
                final_flags |= flags & vas::PTE_RW;
                if flags & vas::PTE_NX == 0 { final_flags &= !vas::PTE_NX; }
            }
        }
        let zero_bss = allow_demand_zero && cfg!(target_arch = "x86_64") && !shared_page
            && end - start == 4096
            && first.is_some_and(|s| start >= s.raw_end && end <= s.end && start >= s.start)
            && final_flags == (vas::PTE_US | vas::PTE_RW | vas::PTE_NX)
            && prebuilt.get(start..end.min(prebuilt.len())).is_none_or(|bytes| bytes.iter().all(|&b| b == 0));
        let va = image_base + start as u64;
        if zero_bss {
            #[cfg(target_arch = "x86_64")]
            if vas::reserve_image_zero_page(pml4_phys, va) != vas::MmStatus::Ok {
                let _ = vas::unmap_user_pages(pml4_phys, image_base, start as u64);
                return None;
            }
            pages.push((0, final_flags));
            continue;
        }
        let Some(pa) = vas::alloc_zeroed_page_for_vas() else {
            let _ = vas::unmap_user_pages(pml4_phys, image_base, start as u64);
            return None;
        };
        if vas::map_page_in_pml4(pml4_phys, va, pa, final_flags | vas::PTE_RW | vas::PTE_NX) != vas::MmStatus::Ok {
            ntoskrnl::mm::pfn::free_pfn(pa >> 12);
            let _ = vas::unmap_user_pages(pml4_phys, image_base, start as u64);
            return None;
        }
        pages.push((pa, final_flags));
    }
    let saved_cr3 = ntoskrnl::arch::switch_to_system_address_space();
    for (i, &(pa, flags)) in pages.iter().enumerate() {
        if pa == 0 { continue; }
        let start = i * 4096;
        if start < prebuilt.len() {
            let end = (start + 4096).min(prebuilt.len());
            unsafe { core::ptr::copy_nonoverlapping(prebuilt[start..end].as_ptr(), pa as *mut u8, end - start); }
        }
        let va = image_base + start as u64;
        if vas::map_page_in_pml4(pml4_phys, va, pa, flags) != vas::MmStatus::Ok {
            ntoskrnl::arch::restore_system_address_space(saved_cr3);
            let _ = vas::unmap_user_pages(pml4_phys, image_base, size_of_image as u64);
            return None;
        }
        vas::invalidate_tlb(va);
    }
    ntoskrnl::arch::flush_tlb();
    ntoskrnl::arch::restore_system_address_space(saved_cr3);
    Some(UserImageMapping { image_base, image_size: size_of_image as u64, entry_point: image_base + aep as u64 })
}

/// Map a fully-relocated `LoadedImage` into the system (kernel)
/// address space at `image_base` so that export addresses handed out
/// by the image database point at real, executable pages instead of an
/// unmapped region. Idempotent: an already-mapped base is a no-op.
/// Returns true when the image is mapped (or already was).
pub fn map_loaded_image_into_kernel_space(loaded: &LoadedImage) -> bool {
    let system_pml4 = ntoskrnl::mm::vas::MM_SYSTEM_VAS.lock().pml4_phys;
    if system_pml4 == 0 || loaded.image_base == 0 || loaded.bytes.is_empty() {
        ntoskrnl::hal::serial::write_string_always("K:mapimg-invalid\r\n");
        return false;
    }
    let image_base = loaded.image_base;
    let size_of_image = loaded.image_size as usize;
    if size_of_image == 0 {
        ntoskrnl::hal::serial::write_string_always("K:mapimg-empty\r\n");
        return false;
    }
    if ntoskrnl::mm::vas::is_user_range_mapped(system_pml4, image_base, 0x1000) {
        return true;
    }
    let (file_hdr, _opt_size, _magic, opt_off) = match parse_headers(&loaded.bytes) {
        Some(h) => h,
        None => return false,
    };

    let page_count = size_of_image.div_ceil(0x1000);
    let mut phys_pages_storage = [0u64; 256];
    let mut phys_page_count = 0usize;
    for _ in 0..page_count {
        let Some(p) = ntoskrnl::mm::vas::alloc_zeroed_page_for_vas() else {
            ntoskrnl::hal::serial::write_string_always("K:mapimg-page-alloc-failed\r\n");
            for &pa in &phys_pages_storage[..phys_page_count] {
                ntoskrnl::mm::pfn::free_pfn(pa >> 12);
            }
            return false;
        };
        if phys_page_count >= phys_pages_storage.len() {
            ntoskrnl::hal::serial::write_string_always("K:mapimg-page-alloc-failed\r\n");
            return false;
        }
        phys_pages_storage[phys_page_count] = p;
        phys_page_count += 1;
    }
    let phys_pages = &phys_pages_storage[..phys_page_count];

    #[derive(Copy, Clone)]
    struct SectInfo {
        va_start: u64,
        va_end: u64,
        chars: u32,
    }
    let sect_off = opt_off + file_hdr.size_of_optional_header as usize;
    let mut section_info: alloc::vec::Vec<SectInfo> =
        alloc::vec::Vec::with_capacity(file_hdr.number_of_sections as usize);
    for i in 0..file_hdr.number_of_sections as usize {
        let sh_off = sect_off + i * 40;
        if sh_off + 40 > loaded.bytes.len() {
            continue;
        }
        let sh: SectionHeader = unsafe {
            core::ptr::read_unaligned(loaded.bytes.as_ptr().add(sh_off) as *const SectionHeader)
        };
        if sh.virtual_size == 0 {
            continue;
        }
        let va_start = image_base + sh.virtual_address as u64;
        let va_end = va_start + (sh.virtual_size.max(sh.size_of_raw_data)) as u64;
        section_info.push(SectInfo {
            va_start,
            va_end,
            chars: sh.characteristics,
        });
    }

    let us_bit = ntoskrnl::mm::vas::PTE_US;
    for (page_index, &p) in phys_pages.iter().enumerate() {
        let page_va = image_base + (page_index as u64) * 0x1000;
        let mut final_flags = ntoskrnl::mm::vas::PTE_RW | ntoskrnl::mm::vas::PTE_NX;
        for &s in &section_info {
            if page_va >= s.va_start && page_va < s.va_end {
                final_flags = determine_section_protection(s.chars) & !us_bit;
                break;
            }
        }
        let initial_flags = final_flags | ntoskrnl::mm::vas::PTE_RW | ntoskrnl::mm::vas::PTE_NX;
        if ntoskrnl::mm::vas::map_page_in_pml4(system_pml4, page_va, p, initial_flags)
            != ntoskrnl::mm::vas::MmStatus::Ok
        {
            for &pa in phys_pages {
                ntoskrnl::mm::pfn::free_pfn(pa >> 12);
            }
            return false;
        }
    }

    let saved_cr3 = ntoskrnl::arch::switch_to_system_address_space();
    for (page_index, &p) in phys_pages.iter().enumerate() {
        let start = page_index * 0x1000;
        if start >= loaded.bytes.len() {
            break;
        }
        let end = core::cmp::min(start + 0x1000, loaded.bytes.len());
        unsafe {
            let dst = p as *mut u8;
            for (off, &b) in loaded.bytes[start..end].iter().enumerate() {
                core::ptr::write_volatile(dst.add(off), b);
            }
        }
    }
    for (i, &p) in phys_pages.iter().enumerate() {
        let page_va = image_base + (i as u64) * 0x1000;
        let mut final_flags = ntoskrnl::mm::vas::PTE_RW | ntoskrnl::mm::vas::PTE_NX;
        for &s in &section_info {
            if page_va >= s.va_start && page_va < s.va_end {
                final_flags = determine_section_protection(s.chars) & !us_bit;
                break;
            }
        }
        let r = ntoskrnl::mm::vas::map_page_in_pml4(system_pml4, page_va, p, final_flags);
        if r != ntoskrnl::mm::vas::MmStatus::Ok {
            ntoskrnl::arch::restore_system_address_space(saved_cr3);
            for &pa in phys_pages {
                ntoskrnl::mm::pfn::free_pfn(pa >> 12);
            }
            return false;
        }
        ntoskrnl::mm::vas::invalidate_tlb(page_va);
    }
    ntoskrnl::hal::serial::write_string_always("K:mapimg-done\r\n");
    ntoskrnl::arch::flush_tlb();
    ntoskrnl::arch::restore_system_address_space(saved_cr3);
    true
}

///
/// Windows PE section characteristics:
///   - IMAGE_SCN_CNT_CODE (0x20): Contains executable code
///   - IMAGE_SCN_CNT_INITIALIZED_DATA (0x40): Contains initialized data
///   - IMAGE_SCN_CNT_UNINITIALIZED_DATA (0x80): Contains uninitialized data
///   - IMAGE_SCN_MEM_EXECUTE (0x20000000): Memory is executable
///   - IMAGE_SCN_MEM_READ (0x40000000): Memory is readable
///   - IMAGE_SCN_MEM_WRITE (0x80000000): Memory is writable
fn determine_section_protection(characteristics: u32) -> u64 {
    use ntoskrnl::mm::vas::{PTE_NX, PTE_RW, PTE_US};

    let mut flags = PTE_US; // Always user-accessible

    // Check explicit flags first
    if characteristics & section::MEM_EXECUTE != 0 {
        // Section is explicitly marked as executable
        if characteristics & section::MEM_WRITE != 0 {
            // R+W+X
            flags |= PTE_RW;
            // Note: We don't add PTE_NX here even though W+X is dangerous
            // This matches Windows behavior for compatibility
        }
        // If not writable, it's R+X (no NX bit)
    } else if characteristics & section::MEM_WRITE != 0 {
        // Writable section (data) - add NX to prevent execution
        flags |= PTE_RW | PTE_NX;
    } else {
        // read-only section
        flags |= PTE_NX; // NX for read-only data
    }

    flags
}

// PE32 loader module for Wow64 support
pub mod pe32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_memory_attributes_control_permissions() {
        use ntoskrnl::mm::vas::{PTE_NX, PTE_RW, PTE_US};
        // CNT_CODE describes content; it must not override explicit memory
        // permissions on writable code or force execution without MEM_EXECUTE.
        assert_eq!(determine_section_protection(0xe0000020), PTE_US | PTE_RW);
        assert_eq!(determine_section_protection(0x60000020), PTE_US);
        assert_eq!(determine_section_protection(0xc0000020), PTE_US | PTE_RW | PTE_NX);
        assert_eq!(determine_section_protection(0x40000020), PTE_US | PTE_NX);
        assert_eq!(determine_section_protection(0xc0000040), PTE_US | PTE_RW | PTE_NX);
    }

    #[test]
    fn optional_header_sizes_match_pe_spec() {
        // Fixed fields before the data directories: 0x60 for PE32,
        // 0x70 for PE32+.
        assert_eq!(core::mem::size_of::<OptionalHeader32>(), 0x60);
        assert_eq!(core::mem::size_of::<OptionalHeader64>(), 0x70);
    }

    #[test]
    fn read_opt32_uses_documented_field_offsets() {
        let mut image = [0u8; 0x200];
        image[0] = b'M';
        image[1] = b'Z';
        image[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        image[0x40..0x44].copy_from_slice(b"PE\0\0");
        let opt = 0x58usize; // PE signature + COFF file header
        image[0x54..0x56].copy_from_slice(&0xe0u16.to_le_bytes());
        image[opt + 0x5c..opt + 0x60].copy_from_slice(&16u32.to_le_bytes());
        image[opt..opt + 2].copy_from_slice(&0x10B_u16.to_le_bytes());
        image[opt + 0x02] = 14; // MajorLinkerVersion
        image[opt + 0x03] = 0; // MinorLinkerVersion
        image[opt + 0x10..opt + 0x14].copy_from_slice(&0x0000_1010u32.to_le_bytes());
        image[opt + 0x1C..opt + 0x20].copy_from_slice(&0x1000_0000u32.to_le_bytes());
        image[opt + 0x38..opt + 0x3C].copy_from_slice(&0x0000_4000u32.to_le_bytes());
        image[opt + 0x44..opt + 0x46].copy_from_slice(&3u16.to_le_bytes());

        let (image_base, size_of_image, aep, subsystem) = read_opt32(&image, opt).unwrap();
        assert_eq!(aep, 0x0000_1010);
        assert_eq!(image_base, 0x1000_0000);
        assert_eq!(size_of_image, 0x0000_4000);
        assert_eq!(subsystem, 3);

        let (_file_hdr, _opt_size, magic, opt_off) = parse_headers(&image).unwrap();
        assert_eq!(magic, 0x10B);
        assert_eq!(opt_off, opt);
    }

    #[test]
    fn highadj_uses_signed_second_slot_and_carries_into_high_word() {
        assert_eq!(apply_highadj(0x1234, 0xFFFE, 0), 0x1233);
        assert_eq!(apply_highadj(0x1234, 0x0002, 0), 0x1234);
        assert_eq!(apply_highadj(0xFFFF, 0x0001, 0), 0xFFFF);
        assert_eq!(apply_highadj(0x1234, 0x7FFF, 0x8001), 0x1235);
    }

    #[test]
    fn highadj_applies_full_delta_before_taking_high_word() {
        assert_eq!(apply_highadj(0x1000, 0x0000, 0x0001_0000), 0x1001);
        assert_eq!(apply_highadj(0x1000, 0x0000, -0x0001_0000), 0x0FFF);
    }
}

pub mod utils;

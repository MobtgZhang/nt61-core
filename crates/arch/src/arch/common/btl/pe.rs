//! Checked PE32/PE32+ memory for the portable execution backend.
//!
//! Guest imports are linked only to checked guest DLL mappings, never to host
//! function pointers. Initial-thread executable TLS is supported; DLL
//! initialization remains an explicit unsupported boundary. Images are loaded
//! at their preferred base.

use super::{ir::DecodeMode, memory::GuestMemory, runtime::GuestThread};
use alloc::{vec, vec::Vec};

#[path = "pe/link.rs"]
mod link;
pub use link::{ImportResolver, ImportSymbol, LinkError};
#[path = "pe/source.rs"]
mod source;
pub use source::{read_image_file, ImageReadError};
#[path = "pe/tls.rs"]
mod tls;

const PAGE: usize = 4096;
const READ: u8 = 1;
const WRITE: u8 = 2;
const EXECUTE: u8 = 4;
const MAX_IMAGE: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    InvalidImage,
    UnsupportedMachine,
    ImportsRequired,
    TlsRequired,
    ImageTooLarge,
    OutOfMemory,
}

fn zeroed(length: usize) -> Result<Vec<u8>, LoadError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| LoadError::OutOfMemory)?;
    bytes.resize(length, 0);
    Ok(bytes)
}

#[cfg(test)]
#[test]
fn impossible_image_allocation_is_recoverable() {
    assert_eq!(zeroed(usize::MAX), Err(LoadError::OutOfMemory));
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, LoadError> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(2).ok_or(LoadError::InvalidImage)?)
            .ok_or(LoadError::InvalidImage)?
            .try_into()
            .map_err(|_| LoadError::InvalidImage)?,
    ))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, LoadError> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or(LoadError::InvalidImage)?)
            .ok_or(LoadError::InvalidImage)?
            .try_into()
            .map_err(|_| LoadError::InvalidImage)?,
    ))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, LoadError> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(8).ok_or(LoadError::InvalidImage)?)
            .ok_or(LoadError::InvalidImage)?
            .try_into()
            .map_err(|_| LoadError::InvalidImage)?,
    ))
}

struct Region {
    base: u64,
    bytes: Vec<u8>,
    permissions: Vec<u8>,
}
impl Region {
    fn new(base: u64, length: usize, permission: u8) -> Result<Self, LoadError> {
        let mut permissions = zeroed(length.div_ceil(PAGE))?;
        permissions.fill(permission);
        Ok(Self {
            base,
            bytes: zeroed(length)?,
            permissions,
        })
    }
    fn range(&self, va: u64, len: usize, permission: u8) -> Option<core::ops::Range<usize>> {
        let start = usize::try_from(va.checked_sub(self.base)?).ok()?;
        let end = start.checked_add(len)?;
        if end > self.bytes.len() {
            return None;
        }
        if len != 0
            && self.permissions[start / PAGE..=(end - 1) / PAGE]
                .iter()
                .any(|p| p & permission != permission)
        {
            return None;
        }
        Some(start..end)
    }
}

/// Owned, disjoint guest mappings. No guest address is a host pointer.
pub struct ImageMemory {
    regions: Vec<Region>,
}
impl GuestMemory for ImageMemory {
    fn probe_write(&mut self, va: u64, len: usize) -> bool {
        self.regions
            .iter()
            .any(|region| region.range(va, len, WRITE).is_some())
    }
    fn read_virt(&mut self, va: u64, buf: &mut [u8]) -> usize {
        for region in &self.regions {
            if let Some(range) = region.range(va, buf.len(), READ) {
                buf.copy_from_slice(&region.bytes[range]);
                return buf.len();
            }
        }
        0
    }
    fn write_virt(&mut self, va: u64, data: &[u8]) -> bool {
        for region in &mut self.regions {
            if let Some(range) = region.range(va, data.len(), WRITE) {
                region.bytes[range].copy_from_slice(data);
                return true;
            }
        }
        false
    }
    fn fetch_virt(&mut self, va: u64, buf: &mut [u8]) -> usize {
        for region in &self.regions {
            if let Some(range) = region.range(va, buf.len(), EXECUTE) {
                buf.copy_from_slice(&region.bytes[range]);
                return buf.len();
            }
        }
        0
    }
}

pub struct GuestImage {
    pub thread: GuestThread,
    pub memory: ImageMemory,
    pub image_base: u64,
    pub peb: u64,
    pub teb: u64,
    startup: Option<tls::Startup>,
    terminal: Option<ImageRunResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageRunResult {
    Yielded,
    Exited(u32),
    Fault(super::runtime::StopReason),
}

impl GuestImage {
    /// Execute a scheduler quantum, including guest ABI service conversion.
    pub fn run(
        &mut self,
        quantum: u64,
        services: &mut dyn super::nt61::GuestServices,
    ) -> ImageRunResult {
        use super::{abi::GuestAbi, ir::X86Reg, nt61::ServiceResult, runtime::StopReason};
        if let Some(result) = self.terminal {
            return result;
        }
        let mut remaining = quantum;
        loop {
            if remaining == 0 {
                return ImageRunResult::Yielded;
            }
            if let Some(startup) = &mut self.startup {
                if !startup.active() {
                    if let Err(reason) =
                        startup.begin(&mut self.thread, &mut self.memory, self.image_base)
                    {
                        let result = ImageRunResult::Fault(reason);
                        self.terminal = Some(result);
                        return result;
                    }
                    if !startup.active() {
                        self.startup = None;
                    }
                }
            }
            let outcome = self.thread.run(&mut self.memory, remaining);
            remaining -= outcome.retired;
            if let Some(startup) = &mut self.startup {
                if startup.complete(&mut self.thread, outcome.reason) {
                    continue;
                }
            }
            match outcome.reason {
                StopReason::SystemCall { number } => {
                    let abi = if self.thread.context.mode == DecodeMode::X86 {
                        GuestAbi::X86_32
                    } else {
                        GuestAbi::X86_64
                    };
                    match super::nt61::dispatch(
                        abi,
                        number,
                        &self.thread.context.regs,
                        &mut self.memory,
                        services,
                    ) {
                        ServiceResult::Return(status) => {
                            self.thread.context.set_reg(X86Reg::Rax, status as u64)
                        }
                        ServiceResult::ExitProcess(status) | ServiceResult::ExitThread(status) => {
                            let result = ImageRunResult::Exited(status);
                            self.terminal = Some(result);
                            return result;
                        }
                    }
                }
                StopReason::QuantumExpired => return ImageRunResult::Yielded,
                fault => {
                    let result = ImageRunResult::Fault(fault);
                    self.terminal = Some(result);
                    return result;
                }
            }
        }
    }

    /// Populate RTL_USER_PROCESS_PARAMETERS in the guest ABI, including UTF-16
    /// command line, current directory, environment and standard handles.
    /// Values live entirely in guest memory; no host strings/pointers leak in.
    pub fn set_process_parameters(
        &mut self,
        image_path: &str,
        command_line: &str,
        directory: &str,
    ) -> Result<(), LoadError> {
        let base = 0x7ffc_0000u64;
        let length = 0x10000usize;
        if self
            .memory
            .regions
            .iter()
            .any(|r| r.base < base + length as u64 && base < r.base + r.bytes.len() as u64)
        {
            return Err(LoadError::InvalidImage);
        }
        let x86 = self.thread.context.mode == DecodeMode::X86;
        let width = if x86 { 4 } else { 8 };
        let mut params = Region::new(base, length, READ | WRITE)?;
        let mut cursor = 0x1000;
        for (offset, text) in [
            (if x86 { 0x24 } else { 0x38 }, directory),
            (if x86 { 0x38 } else { 0x60 }, image_path),
            (if x86 { 0x40 } else { 0x70 }, command_line),
        ] {
            if text.contains('\0') {
                return Err(LoadError::InvalidImage);
            }
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let bytes = utf16.len().checked_mul(2).ok_or(LoadError::InvalidImage)?;
            if bytes > 32764 || cursor + bytes + 2 > length {
                return Err(LoadError::InvalidImage);
            }
            params.bytes[offset..offset + 2].copy_from_slice(&(bytes as u16).to_le_bytes());
            params.bytes[offset + 2..offset + 4]
                .copy_from_slice(&((bytes + 2) as u16).to_le_bytes());
            let ptr = offset + width;
            params.bytes[ptr..ptr + width]
                .copy_from_slice(&(base + cursor as u64).to_le_bytes()[..width]);
            for value in utf16 {
                params.bytes[cursor..cursor + 2].copy_from_slice(&value.to_le_bytes());
                cursor += 2;
            }
            cursor += 2;
        }
        let env =
            "SystemRoot=C:\\Windows\0WINDIR=C:\\Windows\0OS=Windows_NT\0USERNAME=Administrator\0\0";
        let env_offset = if x86 { 0x48 } else { 0x80 };
        params.bytes[env_offset..env_offset + width]
            .copy_from_slice(&(base + cursor as u64).to_le_bytes()[..width]);
        for value in env.encode_utf16() {
            if cursor + 2 > length {
                return Err(LoadError::InvalidImage);
            }
            params.bytes[cursor..cursor + 2].copy_from_slice(&value.to_le_bytes());
            cursor += 2;
        }
        let structure_size = if x86 { 0x290u32 } else { 0x400u32 };
        params.bytes[..4].copy_from_slice(&(length as u32).to_le_bytes());
        params.bytes[4..8].copy_from_slice(&structure_size.to_le_bytes());
        params.bytes[8..12].copy_from_slice(&1u32.to_le_bytes()); // RTL_USER_PROC_PARAMS_NORMALIZED
        for (offset, handle) in [
            (if x86 { 0x18 } else { 0x20 }, super::io::STD_INPUT),
            (if x86 { 0x1c } else { 0x28 }, super::io::STD_OUTPUT),
            (if x86 { 0x20 } else { 0x30 }, super::io::STD_ERROR),
        ] {
            params.bytes[offset..offset + width].copy_from_slice(&handle.to_le_bytes()[..width]);
        }
        let pointer = self.peb + if x86 { 0x10 } else { 0x20 };
        if !self
            .memory
            .write_virt(pointer, &base.to_le_bytes()[..width])
        {
            return Err(LoadError::InvalidImage);
        }
        self.memory.regions.push(params);
        Ok(())
    }

    /// Construct an isolated guest with no unresolved imports.
    pub fn load(file: &[u8]) -> Result<Self, LoadError> {
        let mapped = MappedImage::parse(file, false)?;
        if mapped.directories[1] != (0, 0) || mapped.directories[13] != (0, 0) {
            return Err(LoadError::ImportsRequired);
        }
        Self::from_mapped(mapped, Vec::new())
    }

    /// Link normal imports to guest PE DLLs supplied by a filesystem resolver.
    /// Delay imports remain guest code's responsibility. DLL initialization
    /// and per-module TLS must be implemented before such DLLs can run.
    pub fn load_linked(file: &[u8], resolver: &mut dyn ImportResolver) -> Result<Self, LinkError> {
        link::load(file, resolver)
    }

    fn from_mapped(mapped: MappedImage, libraries: Vec<Region>) -> Result<Self, LoadError> {
        let bad = LoadError::InvalidImage;
        let tls = tls::Plan::parse(&mapped)?;
        let MappedImage {
            region: image,
            mode,
            entry,
            ..
        } = mapped;
        let base = image.base;
        let (stack_base, peb, teb) = if mode == DecodeMode::X86 {
            (0x0010_0000, 0x7ffd_f000, 0x7ffd_e000)
        } else {
            (
                0x0000_0000_0100_0000,
                0x0000_0000_7ffd_f000,
                0x0000_0000_7ffd_e000,
            )
        };
        let stack_top = stack_base + 0x10_0000;
        let mut regions = vec![
            image,
            Region::new(stack_base, 0x10_0000, READ | WRITE)?,
            Region::new(peb, PAGE, READ | WRITE)?,
            Region::new(teb, PAGE, READ | WRITE)?,
        ];
        regions.sort_by_key(|region| region.base);
        if regions
            .windows(2)
            .any(|r| r[0].base + r[0].bytes.len() as u64 > r[1].base)
        {
            return Err(bad);
        }
        regions.extend(libraries);
        regions.sort_by_key(|region| region.base);
        if regions
            .windows(2)
            .any(|r| r[0].base + r[0].bytes.len() as u64 > r[1].base)
        {
            return Err(LoadError::InvalidImage);
        }
        let mut memory = ImageMemory { regions };
        // Win64 function entry has RSP % 16 == 8 (a return-address slot),
        // followed by the caller's 32-byte home area.
        let initial_sp = stack_top - if mode == DecodeMode::X86 { 0x40 } else { 0x48 };
        let mut thread = GuestThread::new(mode, base + entry as u64, initial_sp);
        // A return through an uninitialised process-start thunk is a fault,
        // not a call into host/kernel code. NT termination uses the syscall API.
        let mut put = |va, value: u64, width| memory.write_virt(va, &value.to_le_bytes()[..width]);
        if mode == DecodeMode::X86 {
            thread.context.fs_base = teb;
            for (offset, value) in [
                (0, 0xffff_ffff),
                (4, stack_top),
                (8, stack_base),
                (0x18, teb),
                (0x30, peb),
            ] {
                if !put(teb + offset, value, 4) {
                    return Err(bad);
                }
            }
            for (offset, value) in [(8, base), (0xa4, 6), (0xa8, 1)] {
                if !put(peb + offset, value, 4) {
                    return Err(bad);
                }
            }
            if !memory.write_virt(peb + 0xac, &7601u16.to_le_bytes()) {
                return Err(bad);
            }
        } else {
            thread.context.gs_base = teb;
            for (offset, value) in [(8, stack_top), (0x10, stack_base), (0x30, teb), (0x60, peb)] {
                if !put(teb + offset, value, 8) {
                    return Err(bad);
                }
            }
            for (offset, value, width) in [(0x10, base, 8), (0x118, 6, 4), (0x11c, 1, 4)] {
                if !put(peb + offset, value, width) {
                    return Err(bad);
                }
            }
            if !memory.write_virt(peb + 0x120, &7601u16.to_le_bytes()) {
                return Err(bad);
            }
        }
        let startup = if let Some(tls) = tls {
            Some(tls.install(&mut memory, &thread, teb)?)
        } else {
            None
        };
        Ok(Self {
            thread,
            memory,
            image_base: base,
            peb,
            teb,
            startup,
            terminal: None,
        })
    }
}

/// Validated file mapping reused by the guest DLL linker.
struct MappedImage {
    region: Region,
    mode: DecodeMode,
    entry: usize,
    directories: [(u32, u32); 16],
}
impl MappedImage {
    fn parse(file: &[u8], dll: bool) -> Result<Self, LoadError> {
        let bad = LoadError::InvalidImage;
        if file.get(..2) != Some(b"MZ") {
            return Err(bad);
        }
        let pe = u32_at(file, 0x3c)? as usize;
        if pe < 64 || file.get(pe..pe.checked_add(4).ok_or(bad)?) != Some(b"PE\0\0") {
            return Err(bad);
        }
        let coff = pe.checked_add(4).ok_or(bad)?;
        let count = u16_at(file, coff + 2)? as usize;
        let opt_len = u16_at(file, coff + 16)? as usize;
        let characteristics = u16_at(file, coff + 18)?;
        if characteristics & 2 == 0
            || (characteristics & 0x2000 != 0) != dll
            || count == 0
            || count > 96
        {
            return Err(bad);
        }
        let opt = coff.checked_add(20).ok_or(bad)?;
        let end = opt.checked_add(opt_len).ok_or(bad)?;
        let header = file.get(opt..end).ok_or(bad)?;
        let (mode, base, directory) = match (u16_at(file, coff)?, u16_at(header, 0)?) {
            (0x14c, 0x10b) => (DecodeMode::X86, u32_at(header, 28)? as u64, 96),
            (0x8664, 0x20b) => (DecodeMode::X64, u64_at(header, 24)?, 112),
            _ => return Err(LoadError::UnsupportedMachine),
        };
        let size = u32_at(header, 56)? as usize;
        let header_size = u32_at(header, 60)? as usize;
        let entry = u32_at(header, 16)? as usize;
        let section_alignment = u32_at(header, 32)? as usize;
        let file_alignment = u32_at(header, 36)? as usize;
        if size > MAX_IMAGE {
            return Err(LoadError::ImageTooLarge);
        }
        if size == 0
            || size % PAGE != 0
            || base < 0x10000
            || base % 0x10000 != 0
            || section_alignment < PAGE
            || !section_alignment.is_power_of_two()
            || !(512..=65536).contains(&file_alignment)
            || !file_alignment.is_power_of_two()
            || section_alignment < file_alignment
            || header_size == 0
            || header_size > size
            || header_size > file.len()
            || header_size % file_alignment != 0
            || entry >= size
        {
            return Err(bad);
        }
        let image_end = base.checked_add(size as u64).ok_or(bad)?;
        let limit = if mode == DecodeMode::X86 {
            if characteristics & 0x20 != 0 {
                0x1_0000_0000
            } else {
                0x8000_0000
            }
        } else {
            0x0000_8000_0000_0000
        };
        if image_end > limit {
            return Err(bad);
        }
        let directory_count = u32_at(header, directory - 4)? as usize;
        if directory_count > 16
            || directory.checked_add(directory_count * 8).ok_or(bad)? > header.len()
        {
            return Err(bad);
        }
        let mut directories = [(0, 0); 16];
        for (index, slot) in directories.iter_mut().enumerate().take(directory_count) {
            *slot = (
                u32_at(header, directory + index * 8)?,
                u32_at(header, directory + index * 8 + 4)?,
            );
        }
        if directories[14] != (0, 0) {
            return Err(LoadError::UnsupportedMachine);
        }
        let sections_end = end.checked_add(count * 40).ok_or(bad)?;
        if sections_end > header_size {
            return Err(bad);
        }
        let mut image = Region::new(base, size, 0)?;
        image.bytes[..header_size].copy_from_slice(&file[..header_size]);
        image.permissions[..header_size.div_ceil(PAGE)].fill(READ);
        for index in 0..count {
            let section = file
                .get(end + index * 40..end + (index + 1) * 40)
                .ok_or(bad)?;
            let virtual_size = u32_at(section, 8)? as usize;
            let rva = u32_at(section, 12)? as usize;
            let raw_size = u32_at(section, 16)? as usize;
            let raw = u32_at(section, 20)? as usize;
            let flags = u32_at(section, 36)?;
            let length = virtual_size.max(raw_size);
            if length == 0 {
                continue;
            }
            let section_end = rva.checked_add(length).ok_or(bad)?;
            if rva % section_alignment != 0
                || rva < header_size.div_ceil(PAGE) * PAGE
                || section_end > size
                || (raw_size != 0
                    && (raw % file_alignment != 0
                        || raw_size % file_alignment != 0
                        // Packed PEs can start raw data after the actual
                        // headers but before SizeOfHeaders' alignment padding.
                        || raw < sections_end))
            {
                return Err(bad);
            }
            let pages = &mut image.permissions[rva / PAGE..section_end.div_ceil(PAGE)];
            if pages.iter().any(|p| *p != 0) {
                return Err(bad);
            }
            let mut permission = 0;
            if flags & 0x4000_0000 != 0 {
                permission |= READ;
            }
            if flags & 0x8000_0000 != 0 {
                permission |= WRITE;
            }
            if flags & 0x2000_0000 != 0 {
                permission |= EXECUTE;
            }
            // Even a section with no access occupies its address range.
            pages.fill(permission | 8);
            let data = file
                .get(raw..raw.checked_add(raw_size).ok_or(bad)?)
                .ok_or(bad)?;
            image.bytes[rva..rva + raw_size].copy_from_slice(data);
        }
        if (entry != 0 || !dll) && image.permissions[entry / PAGE] & EXECUTE == 0 {
            return Err(bad);
        }
        Ok(MappedImage {
            region: image,
            mode,
            entry,
            directories,
        })
    }
}

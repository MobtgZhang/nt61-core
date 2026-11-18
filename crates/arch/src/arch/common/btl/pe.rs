//! Checked PE32/PE32+ memory for the portable execution backend.
//!
//! Guest imports are linked only to checked guest DLL mappings, never to host
//! function pointers. Static TLS and process-attach callbacks execute on the
//! initial guest thread in dependency order. Images use their preferred bases.

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
#[path = "pe/threads.rs"]
pub mod threads;
#[path = "win32.rs"]
pub mod win32;

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
    // Mapping metadata belongs to the loader/allocator, never mutable guest
    // PE headers. Keep Windows protections separately from interpreter bits
    // (which also contain the section-occupancy marker).
    protections: Vec<u32>,
    allocation_protection: u32,
    image: bool,
}
impl Region {
    fn new(base: u64, length: usize, permission: u8) -> Result<Self, LoadError> {
        let mut permissions = zeroed(length.div_ceil(PAGE))?;
        permissions.fill(permission);
        let allocation_protection = win32::win_protection(permission);
        let mut protections = Vec::new();
        protections.try_reserve_exact(permissions.len()).map_err(|_| LoadError::OutOfMemory)?;
        protections.resize(permissions.len(), allocation_protection);
        Ok(Self {
            base,
            bytes: zeroed(length)?,
            permissions,
            protections,
            allocation_protection,
            image: false,
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
    fn locked_compare_exchange(&mut self, va: u64, width: u8, expected: u64, replacement: u64)
        -> Result<u64, super::memory::AtomicMemoryError> {
        use super::memory::AtomicMemoryError::Fault;
        if !matches!(width, 1 | 2 | 4 | 8) { return Err(Fault); }
        let len = width as usize;
        let mut bytes = [0; 8];
        if !self.probe_write(va, len) || self.read_virt(va, &mut bytes[..len]) != len { return Err(Fault); }
        let old = u64::from_le_bytes(bytes);
        let mask = u64::MAX >> ((8 - len) * 8);
        // Owned Vec storage has no shared mappings; the exclusive borrow
        // excludes every observer for the complete comparison and write cycle.
        let value = if old == expected & mask { replacement } else { old };
        if !self.write_virt(va, &value.to_le_bytes()[..len]) { return Err(Fault); }
        Ok(old)
    }
    fn locked_fetch_add(&mut self, va: u64, width: u8, addend: u64)
        -> Result<u64, super::memory::AtomicMemoryError> {
        use super::memory::AtomicMemoryError::Fault;
        if !matches!(width, 1 | 2 | 4 | 8) { return Err(Fault); }
        // Vec-backed regions are owned without shared aliases. This exclusive
        // borrow excludes all observers for the complete read-modify-write.
        let mut bytes = [0; 8];
        let len = width as usize;
        if !self.probe_write(va, len) || self.read_virt(va, &mut bytes[..len]) != len { return Err(Fault); }
        let old = u64::from_le_bytes(bytes);
        if !self.write_virt(va, &old.wrapping_add(addend).to_le_bytes()[..len]) { return Err(Fault); }
        Ok(old)
    }
    fn locked_inc_dec(&mut self, va: u64, width: u8, decrement: bool)
        -> Result<u64, super::memory::AtomicMemoryError> {
        use super::memory::AtomicMemoryError::Fault;
        if !matches!(width, 1 | 2 | 4 | 8) { return Err(Fault); }
        // These mappings own Vec storage with no shared aliases. The exclusive
        // borrow covers read, arithmetic and write; no guest can interleave or
        // observe a partial update, including unaligned/cross-page operands.
        let mut bytes = [0; 8];
        let len = width as usize;
        if !self.probe_write(va, len) || self.read_virt(va, &mut bytes[..len]) != len { return Err(Fault); }
        let old = u64::from_le_bytes(bytes);
        let new = if decrement {old.wrapping_sub(1)} else {old.wrapping_add(1)};
        if !self.write_virt(va, &new.to_le_bytes()[..len]) { return Err(Fault); }
        Ok(old)
    }
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
    initialization: tls::InitialThread,
    threads: threads::State,
    stack_reserve: usize,
    terminal: Option<ImageRunResult>,
    pub win32: win32::Runtime,
    module_metadata: Vec<ModuleMetadata>,
    /// Exclusive application allocation boundary for the SP1 address model.
    /// WOW64 LARGEADDRESSAWARE is an executable property, not a DLL property.
    user_address_end: u64,
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
            self.finish_detach();
            if let Some(code) = self.all_threads_exited() {
                let result = ImageRunResult::Exited(code);
                self.terminal = Some(result);
                return result;
            }
            match self.schedule(services) {
                Ok(true) => {},
                Ok(false) => return ImageRunResult::Yielded,
                Err(reason) => { let result = ImageRunResult::Fault(reason); self.terminal = Some(result); return result; }
            }
            if let Some(startup) = &mut self.startup {
                if !startup.active() {
                    if let Err(reason) = startup.begin(&mut self.thread, &mut self.memory) {
                        let result = ImageRunResult::Fault(reason);
                        self.terminal = Some(result);
                        return result;
                    }
                    if !startup.active() {
                        self.startup = None;
                    }
                }
            }
            if self.startup.is_none() && self.detaching() { continue; }
            // A bounded interpreter slice provides preemption even when a
            // worker spins without calling an API.
            let slice = remaining.min(self.thread_slice());
            let outcome = self.thread.run(&mut self.memory, slice);
            remaining -= outcome.retired;
            self.account_thread_slice(outcome.retired);
            if let Some(startup) = &mut self.startup {
                match startup.complete(&mut self.thread, outcome.reason) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(reason) => {
                        let result = ImageRunResult::Fault(reason);
                        self.terminal = Some(result);
                        return result;
                    }
                }
            }
            match outcome.reason {
                StopReason::SystemCall { number } => {
                    let abi = if self.thread.context.mode == DecodeMode::X86 {
                        GuestAbi::X86_32
                    } else {
                        GuestAbi::X86_64
                    };
                    if number & 0xffff_0000 == win32::catalog::GATEWAY_BASE {
                        match win32::dispatch(self, number, services) {
                            Ok(win32::Outcome::Return(value)) => self.thread.context.set_reg(X86Reg::Rax, if abi == GuestAbi::X86_32 { value as u32 as u64 } else { value }),
                            Ok(win32::Outcome::Continue) => {},
                            Ok(win32::Outcome::Park(wait)) => self.park(wait),
                            Ok(win32::Outcome::ParkRetry(wait)) => {
                                self.thread.context.rip -= 2;
                                self.park(wait);
                            },
                            Ok(win32::Outcome::Retry) => {
                                // Retry the two-byte syscall/int 2e instruction
                                // with its original arguments after another thread
                                // or the host desktop has had a chance to run.
                                self.thread.context.rip -= 2;
                                self.yield_thread();
                                return ImageRunResult::Yielded;
                            },
                            Ok(win32::Outcome::ExitThread(code)) => self.exit_thread(code),
                            Ok(win32::Outcome::Exit(code)) => {
                                let result = ImageRunResult::Exited(code);
                                self.terminal = Some(result);
                                return result;
                            }
                            Err(reason) => {
                                let result = ImageRunResult::Fault(reason);
                                self.terminal = Some(result);
                                return result;
                            }
                        }
                        continue;
                    }
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
                        ServiceResult::ExitThread(status) => self.terminate_thread(status),
                        ServiceResult::ExitProcess(status) => {
                            let result = ImageRunResult::Exited(status);
                            self.terminal = Some(result);
                            return result;
                        }
                    }
                }
                StopReason::QuantumExpired => { if remaining == 0 { return ImageRunResult::Yielded; } },
                reason if self.returned_thread(reason) => self.exit_thread(self.thread.context.reg(X86Reg::Rax) as u32),
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
        // The installed mounted-volume image currently provisions this one
        // account (fs/system.rs), also used by CMD. Keep guest profile paths
        // consistent with that layout; never inherit the Linux host's user.
        let env = concat!(
            "SystemRoot=C:\\Windows\0WINDIR=C:\\Windows\0OS=Windows_NT\0USERNAME=Administrator\0",
            "USERPROFILE=C:\\Users\\Administrator\0HOMEDRIVE=C:\0HOMEPATH=\\Users\\Administrator\0",
            "APPDATA=C:\\Users\\Administrator\\AppData\\Roaming\0",
            "LOCALAPPDATA=C:\\Users\\Administrator\\AppData\\Local\0",
            "TEMP=C:\\Users\\Administrator\\AppData\\Local\\Temp\0",
            "TMP=C:\\Users\\Administrator\\AppData\\Local\\Temp\0ProgramData=C:\\ProgramData\0\0"
        );
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

    /// Load a same-ISA guest DLL transactionally. New libraries with DLL
    /// initialization or static TLS remain explicitly unsupported here.
    pub fn load_library(&mut self, name: &str, resolver: &mut dyn ImportResolver) -> Result<u64, LinkError> {
        link::load_library(self, name, resolver)
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
    /// Delay imports remain guest code's responsibility. Static TLS callbacks
    /// and DLL_PROCESS_ATTACH execute before the executable's entry point.
    pub fn load_linked(file: &[u8], resolver: &mut dyn ImportResolver) -> Result<Self, LinkError> {
        link::load(file, resolver)
    }

    fn from_mapped(mapped: MappedImage, libraries: Vec<MappedImage>) -> Result<Self, LoadError> {
        let bad = LoadError::InvalidImage;
        let user_address_end = mapped.user_address_end()?;
        if core::iter::once(&mapped).chain(libraries.iter()).any(|image|
            image.region.base.checked_add(image.region.bytes.len() as u64)
                .is_none_or(|end| end > user_address_end)) {
            return Err(bad);
        }
        let initialization = tls::InitialThread::plan(&mapped, &libraries)?;
        let mut module_metadata = Vec::new();
        module_metadata.try_reserve_exact(libraries.len() + 1).map_err(|_| LoadError::OutOfMemory)?;
        module_metadata.push(ModuleMetadata::from_image(&mapped));
        module_metadata.extend(libraries.iter().map(ModuleMetadata::from_image));
        let MappedImage {
            region: image,
            mode,
            entry,
            ..
        } = mapped;
        let base = image.base;
        let stack_reserve = if mode == DecodeMode::X86 {
            u32_at(&image.bytes, u32_at(&image.bytes, 0x3c)? as usize + 24 + 72)? as usize
        } else {
            usize::try_from(u64_at(&image.bytes, u32_at(&image.bytes, 0x3c)? as usize + 24 + 72)?).map_err(|_| bad)?
        };
        let stack_reserve = if stack_reserve == 0 { 0x10_0000 } else {
            stack_reserve.checked_add(0xffff).ok_or(bad)? / 0x10000 * 0x10000
        };
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
        regions.extend(libraries.into_iter().map(|image| image.region));
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
                (0x20, 1),
                (0x24, 1),
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
            for (offset, value) in [(8, stack_top), (0x10, stack_base), (0x30, teb), (0x40, 1), (0x48, 1), (0x60, peb)] {
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
        let startup = initialization.clone().install(&mut memory, &thread, teb)?;
        Ok(Self {
            thread,
            memory,
            image_base: base,
            peb,
            teb,
            startup,
            initialization,
            threads: threads::State::new(),
            stack_reserve,
            terminal: None,
            win32: win32::Runtime::new(base),
            module_metadata,
            user_address_end,
        })
    }
}

/// Immutable loader metadata: guest writes to mapped headers must not replace
/// the linker identity or directory bounds used by later dynamic loads.
#[derive(Clone, Copy)]
struct ModuleMetadata {
    base: u64,
    size: usize,
    mode: DecodeMode,
    entry: usize,
    directories: [(u32, u32); 16],
    /// Loader-owned state for DLL_THREAD_ATTACH/DLL_THREAD_DETACH policy.
    /// Initial process attachment and TLS initialization are unaffected.
    thread_notifications_disabled: bool,
}
impl ModuleMetadata {
    fn from_image(image: &MappedImage) -> Self {
        Self { base: image.region.base, size: image.region.bytes.len(), mode: image.mode,
            entry: image.entry, directories: image.directories,
            thread_notifications_disabled: false }
    }
    fn snapshot(&self, memory: &ImageMemory) -> Result<MappedImage, LoadError> {
        let source = memory.regions.iter().find(|r| r.base == self.base && r.bytes.len() == self.size)
            .ok_or(LoadError::InvalidImage)?;
        let mut bytes = zeroed(source.bytes.len())?;
        bytes.copy_from_slice(&source.bytes);
        let mut permissions = zeroed(source.permissions.len())?;
        permissions.copy_from_slice(&source.permissions);
        let mut protections = Vec::new();
        protections.try_reserve_exact(source.protections.len()).map_err(|_| LoadError::OutOfMemory)?;
        protections.extend_from_slice(&source.protections);
        Ok(MappedImage { region: Region { base: self.base, bytes, permissions,
            protections, allocation_protection: source.allocation_protection, image: source.image },
            mode: self.mode, entry: self.entry, directories: self.directories })
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
    // This is the exclusive maximum observed for the SP1 AMD64 process and
    // its two WOW64 address models, including the top reserved 64 KiB.
    fn address_end(mode: DecodeMode, large_address: bool) -> u64 {
        if mode == DecodeMode::X64 { 0x0000_07ff_ffff_0000 }
        else if large_address { 0xffff_0000 } else { 0x7fff_0000 }
    }

    fn user_address_end(&self) -> Result<u64, LoadError> {
        let characteristics = u16_at(&self.region.bytes,
            u32_at(&self.region.bytes, 0x3c)? as usize + 22)?;
        Ok(Self::address_end(self.mode, characteristics & 0x20 != 0))
    }

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
        // DLLs inherit the executable's narrower limit at link time. Their
        // own LAA bit must neither enlarge nor restrict that process limit.
        let limit = Self::address_end(mode, dll || characteristics & 0x20 != 0);
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
        image.image = true;
        image.allocation_protection = 0x80; // SEC_IMAGE: PAGE_EXECUTE_WRITECOPY
        image.protections.fill(0); // Unoccupied image pages are reserved.
        image.bytes[..header_size].copy_from_slice(&file[..header_size]);
        image.permissions[..header_size.div_ceil(PAGE)].fill(READ);
        image.protections[..header_size.div_ceil(PAGE)].fill(2);
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
            let protection = if permission & WRITE != 0 {
                if permission & EXECUTE != 0 { 0x80 } else { 8 }
            } else { win32::win_protection(permission) };
            image.protections[rva / PAGE..section_end.div_ceil(PAGE)].fill(protection);
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

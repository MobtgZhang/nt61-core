//! Static TLS and dependency-ordered DLL_PROCESS_ATTACH on the initial guest
//! thread. Dynamic loading and subsequent thread notifications remain separate.
use super::{
    u32_at, u64_at, zeroed, DecodeMode, ImageMemory, LoadError, MappedImage, Region, EXECUTE, PAGE,
    READ, WRITE,
};
use crate::arch::common::btl::{
    exec::GuestState,
    ir::X86Reg,
    memory::GuestMemory,
    runtime::{GuestThread, StopReason},
};
use alloc::vec::Vec;

const TLS_BASE: u64 = 0x7fc0_0000;
// A reserved non-executable page catches the callback's guest RET. It never
// contains a host address or native trampoline.
const RETURN_GATE: u64 = 0x7fbf_0000;
const DATA_OFFSET: usize = 8192;
const MAX_DATA: usize = 1024 * 1024;
const MAX_CALLBACKS: usize = 128;
const MAX_TOTAL_DATA: usize = 16 * 1024 * 1024;

pub(super) struct Plan {
    base: u64,
    data: Vec<u8>,
    index: u64,
    callbacks: Vec<u64>,
}
impl Plan {
    pub(super) fn parse(image: &MappedImage) -> Result<Option<Self>, LoadError> {
        let (rva, size) = image.directories[9];
        if (rva, size) == (0, 0) {
            return Ok(None);
        }
        let bad = LoadError::InvalidImage;
        let width = if image.mode == DecodeMode::X86 { 4 } else { 8 };
        let directory_size = width * 4 + 8;
        if rva == 0 || (size as usize) < directory_size {
            return Err(bad);
        }
        let read = |va, len| -> Result<&[u8], LoadError> {
            let range = image.region.range(va, len, READ).ok_or(bad)?;
            Ok(&image.region.bytes[range])
        };
        let directory = read(
            image.region.base.checked_add(rva as u64).ok_or(bad)?,
            directory_size,
        )?;
        let pointer = |offset| {
            if width == 4 {
                u32_at(directory, offset).map(u64::from)
            } else {
                u64_at(directory, offset)
            }
        };
        // TLS directory pointers are VAs, unlike ordinary data-directory RVAs.
        let start = pointer(0)?;
        let end = pointer(width)?;
        let index = pointer(width * 2)?;
        let callback_table = pointer(width * 3)?;
        let zero = u32_at(directory, width * 4)? as usize;
        let flags = u32_at(directory, width * 4 + 4)?;
        let alignment = (flags >> 20) & 15;
        if flags & !0x00f0_0000 != 0 || alignment == 15 || index == 0 {
            return Err(bad);
        }
        // Alignment encodings 1..14 request 1..8192 bytes. TLS_BASE and
        // DATA_OFFSET satisfy all of them, including the default alignment.
        let raw = usize::try_from(end.checked_sub(start).ok_or(bad)?).map_err(|_| bad)?;
        let length = raw.checked_add(zero).ok_or(bad)?;
        if length > MAX_DATA {
            return Err(LoadError::ImageTooLarge);
        }
        if (start == 0) != (end == 0) {
            return Err(bad);
        }
        read(index, 4)?;
        let mut data = zeroed(length.max(1))?;
        if raw != 0 {
            data[..raw].copy_from_slice(read(start, raw)?);
        }
        let mut callbacks = Vec::new();
        if callback_table != 0 {
            for i in 0..=MAX_CALLBACKS {
                let va = callback_table.checked_add((i * width) as u64).ok_or(bad)?;
                let slot = read(va, width)?;
                let address = if width == 4 {
                    u32_at(slot, 0)? as u64
                } else {
                    u64_at(slot, 0)?
                };
                if address == 0 {
                    break;
                }
                if i == MAX_CALLBACKS || image.region.range(address, 1, EXECUTE).is_none() {
                    return Err(bad);
                }
                callbacks.push(address);
            }
        }
        Ok(Some(Self {
            base: image.region.base,
            data,
            index,
            callbacks,
        }))
    }
}

struct Routine {
    address: u64,
    base: u64,
    dll_entry: bool,
}

pub(super) struct InitialThread {
    plans: Vec<Plan>,
    routines: Vec<Routine>,
}
impl InitialThread {
    pub(super) fn plan(
        executable: &MappedImage,
        libraries: &[MappedImage],
    ) -> Result<Self, LoadError> {
        let mut result = Self {
            plans: Vec::new(),
            routines: Vec::new(),
        };
        // Preserve executable TLS index zero; DLL indices follow the stable
        // dependency order. Allocate every module's TLS before any callback.
        let executable_tls = Plan::parse(executable)?;
        if let Some(plan) = executable_tls {
            result.plans.push(plan);
        }
        for library in libraries {
            if let Some(plan) = Plan::parse(library)? {
                for &address in &plan.callbacks {
                    result.routines.push(Routine {
                        address,
                        base: plan.base,
                        dll_entry: false,
                    });
                }
                result.plans.push(plan);
            }
            if library.entry != 0 {
                result.routines.push(Routine {
                    address: library.region.base + library.entry as u64,
                    base: library.region.base,
                    dll_entry: true,
                });
            }
        }
        if let Some(plan) = result
            .plans
            .first()
            .filter(|plan| plan.base == executable.region.base)
        {
            for &address in &plan.callbacks {
                result.routines.push(Routine {
                    address,
                    base: plan.base,
                    dll_entry: false,
                });
            }
        }
        Ok(result)
    }

    pub(super) fn install(
        self,
        memory: &mut ImageMemory,
        thread: &GuestThread,
        teb: u64,
    ) -> Result<Option<Startup>, LoadError> {
        let bad = LoadError::InvalidImage;
        let x86 = thread.context.mode == DecodeMode::X86;
        let width = if x86 { 4 } else { 8 };
        let mut cursor = DATA_OFFSET;
        let mut offsets = Vec::new();
        if self.plans.len() * width > DATA_OFFSET {
            return Err(LoadError::ImageTooLarge);
        }
        for plan in &self.plans {
            // All valid IMAGE_SCN_ALIGN_* requests (up to 8192) are satisfied
            // independently for each module, not just the first TLS template.
            cursor = cursor.div_ceil(DATA_OFFSET) * DATA_OFFSET;
            offsets.push(cursor);
            cursor = cursor
                .checked_add(plan.data.len())
                .ok_or(LoadError::ImageTooLarge)?;
            if cursor > MAX_TOTAL_DATA {
                return Err(LoadError::ImageTooLarge);
            }
        }
        let length = cursor.div_ceil(PAGE) * PAGE;
        for (base, size) in [
            (TLS_BASE, if self.plans.is_empty() { 0 } else { length }),
            (RETURN_GATE, if self.routines.is_empty() { 0 } else { PAGE }),
        ] {
            if size != 0
                && memory
                    .regions
                    .iter()
                    .any(|r| base < r.base + r.bytes.len() as u64 && r.base < base + size as u64)
            {
                return Err(bad);
            }
        }
        if !self.plans.is_empty() {
            let mut region = Region::new(TLS_BASE, length, READ | WRITE)?;
            for (slot, (plan, &offset)) in self.plans.iter().zip(&offsets).enumerate() {
                let pointer = TLS_BASE + offset as u64;
                region.bytes[slot * width..(slot + 1) * width]
                    .copy_from_slice(&pointer.to_le_bytes()[..width]);
                region.bytes[offset..offset + plan.data.len()].copy_from_slice(&plan.data);
                // Loader-only patch: final read-only image protections remain
                // enforced for all subsequent guest reads/writes.
                let owner = memory
                    .regions
                    .iter_mut()
                    .find(|r| r.base == plan.base && r.range(plan.index, 4, READ).is_some())
                    .ok_or(bad)?;
                let at = (plan.index - owner.base) as usize;
                owner.bytes[at..at + 4].copy_from_slice(&(slot as u32).to_le_bytes());
            }
            if !memory.write_virt(
                teb + if x86 { 0x2c } else { 0x58 },
                &TLS_BASE.to_le_bytes()[..width],
            ) {
                return Err(bad);
            }
            memory.regions.push(region);
        }
        if self.routines.is_empty() {
            return Ok(None);
        }
        memory.regions.push(Region::new(RETURN_GATE, PAGE, READ)?);
        Ok(Some(Startup {
            routines: self.routines,
            next: 0,
            saved: thread.context,
            return_sp: None,
        }))
    }
}

pub(super) struct Startup {
    routines: Vec<Routine>,
    next: usize,
    saved: GuestState,
    return_sp: Option<u64>,
}
impl Startup {
    pub(super) fn active(&self) -> bool {
        self.return_sp.is_some()
    }

    pub(super) fn begin(
        &mut self,
        thread: &mut GuestThread,
        memory: &mut ImageMemory,
    ) -> Result<(), StopReason> {
        if self.next == self.routines.len() {
            return Ok(());
        }
        let routine = &self.routines[self.next];
        let x86 = self.saved.mode == DecodeMode::X86;
        let sp = self.saved.reg(X86Reg::Rsp) - 0x100;
        let width = if x86 { 4 } else { 8 };
        let mut frame = [0; 40];
        frame[..width].copy_from_slice(&RETURN_GATE.to_le_bytes()[..width]);
        // DllMain's reserved pointer is non-NULL for static process startup.
        let reserved = u64::from(routine.dll_entry);
        if x86 {
            frame[4..8].copy_from_slice(&(routine.base as u32).to_le_bytes());
            frame[8..12].copy_from_slice(&1u32.to_le_bytes());
            frame[12..16].copy_from_slice(&(reserved as u32).to_le_bytes());
        }
        if !memory.write_virt(sp, &frame[..if x86 { 16 } else { 40 }]) {
            return Err(StopReason::DataFault { pc: self.saved.rip });
        }
        thread.context = self.saved;
        thread.context.set_reg(X86Reg::Rsp, sp);
        thread.context.set_reg(X86Reg::Rip, routine.address);
        if !x86 {
            thread.context.set_reg(X86Reg::Rcx, routine.base);
            thread.context.set_reg(X86Reg::Rdx, 1);
            thread.context.set_reg(X86Reg::R8, reserved);
        }
        self.return_sp = Some(sp + if x86 { 16 } else { 8 });
        self.next += 1;
        Ok(())
    }

    pub(super) fn complete(
        &mut self,
        thread: &mut GuestThread,
        reason: StopReason,
    ) -> Result<bool, StopReason> {
        if reason != (StopReason::InstructionFault { pc: RETURN_GATE })
            || self.return_sp != Some(thread.context.reg(X86Reg::Rsp))
        {
            return Ok(false);
        }
        let routine = &self.routines[self.next - 1];
        if routine.dll_entry && thread.context.reg(X86Reg::Rax) as u32 == 0 {
            return Err(StopReason::InitializationFailed {
                module_base: routine.base,
            });
        }
        thread.context = self.saved;
        self.return_sp = None;
        Ok(true)
    }
}

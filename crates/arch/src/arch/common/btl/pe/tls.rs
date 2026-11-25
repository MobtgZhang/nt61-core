//! Static TLS and loader notifications for guest process and thread lifetimes.
use super::{
    u32_at, u64_at, zeroed, DecodeMode, ImageMemory, LoadError, MappedImage, Region, EXECUTE,
    MAX_PROCESS_MEMORY, PAGE, READ, WRITE,
};
use crate::arch::common::btl::{
    exec::GuestState,
    ir::X86Reg,
    memory::GuestMemory,
    runtime::{GuestThread, StopReason},
};
use alloc::vec::Vec;

const TLS_BASE: u64 = 0x7fc0_0000;
const TLS_ALLOCATION_START: u64 = 0x3000_0000;
// A reserved non-executable page catches the callback's guest RET. It never
// contains a host address or native trampoline.
pub(super) const RETURN_GATE: u64 = 0x7fbf_0000;
const DATA_OFFSET: usize = 8192;
const MAX_DATA: usize = 1024 * 1024;
const MAX_CALLBACKS: usize = 128;
const MAX_TOTAL_DATA: usize = 16 * 1024 * 1024;
const MAX_SLOTS: usize = DATA_OFFSET / 8;

#[derive(Clone)]
pub(super) struct Plan {
    base: u64,
    data: Vec<u8>,
    index: u64,
    callbacks: Vec<u64>,
    slot: usize,
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
            slot: 0,
        }))
    }

    fn append_callbacks(&self, routines: &mut Vec<Routine>) -> Result<(), LoadError> {
        routines.try_reserve(self.callbacks.len()).map_err(|_| LoadError::OutOfMemory)?;
        routines.extend(self.callbacks.iter().map(|&address| Routine {
            address, base: self.base, dll_entry: false,
        }));
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Routine {
    address: u64,
    base: u64,
    dll_entry: bool,
}

impl Routine {
    pub(super) fn dll(base: u64, entry: usize) -> Self {
        Self { address: base + entry as u64, base, dll_entry: true }
    }
}

pub(super) struct InitialThread {
    plans: Vec<Plan>,
    routines: Vec<Routine>,
    blocks: Vec<DynamicBlock>,
}

struct DynamicBlock {
    module: u64,
    teb: u64,
    address: u64,
}

pub(super) struct DynamicTls {
    plans: Vec<Plan>,
    mappings: Vec<Region>,
    blocks: Vec<DynamicBlock>,
    threads: Vec<(u64, u64)>,
}

impl DynamicTls {
    pub(super) fn mapping_count(&self) -> usize { self.mappings.len() }
}

fn teb_vector(teb: u64, width: usize) -> u64 {
    teb + if width == 4 { 0x2c } else { 0x58 }
}

fn write_pointer(memory: &mut ImageMemory, region_base: u64, address: u64, value: u64, width: usize) {
    let region = memory.regions.iter_mut().find(|region| region.base == region_base)
        .expect("loader-owned TLS mapping");
    let offset = (address - region_base) as usize;
    region.bytes[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
}

fn storage_base<'a>(
    regions: impl Iterator<Item = &'a Region> + Clone, mut address: u64, size: usize, limit: u64,
) -> Result<u64, LoadError> {
    loop {
        let end = address + size as u64;
        if end > limit { return Err(LoadError::OutOfMemory); }
        let conflict = regions.clone().find(|region|
            address < region.base + region.bytes.len() as u64 && region.base < end);
        let Some(region) = conflict else { return Ok(address); };
        address = (region.base + region.bytes.len() as u64).div_ceil(DATA_OFFSET as u64) * DATA_OFFSET as u64;
    }
}
impl InitialThread {
    pub(super) fn plan(
        executable: &MappedImage,
        libraries: &[MappedImage],
    ) -> Result<Self, LoadError> {
        let mut result = Self {
            plans: Vec::new(),
            routines: Vec::new(),
            blocks: Vec::new(),
        };
        // Preserve executable TLS index zero; DLL indices follow the stable
        // dependency order. Allocate every module's TLS before any callback.
        let executable_tls = Plan::parse(executable)?;
        if let Some(plan) = executable_tls {
            result.plans.push(plan);
        }
        for library in libraries {
            if let Some(mut plan) = Plan::parse(library)? {
                plan.slot = result.plans.len();
                plan.append_callbacks(&mut result.routines)?;
                result.plans.push(plan);
            }
            if library.entry != 0 {
                result.routines.push(Routine::dll(library.region.base, library.entry));
            }
        }
        if let Some(plan) = result
            .plans
            .first()
            .filter(|plan| plan.base == executable.region.base)
        {
            plan.append_callbacks(&mut result.routines)?;
        }
        Ok(result)
    }

    pub(super) fn data_size(&self) -> usize {
        let mut cursor = DATA_OFFSET;
        for plan in &self.plans {
            cursor = cursor.div_ceil(DATA_OFFSET) * DATA_OFFSET + plan.data.len();
        }
        cursor.div_ceil(PAGE) * PAGE
    }

    pub(super) fn reserve_routines(&mut self, count: usize) -> Result<(), LoadError> {
        self.routines.try_reserve(count).map_err(|_| LoadError::OutOfMemory)
    }

    pub(super) fn register_routines(&mut self, startup: Startup) {
        self.routines.extend(startup.routines);
    }

    pub(super) fn remove_routines(&mut self, modules: &[u64]) {
        self.routines.retain(|routine| !modules.contains(&routine.base));
    }

    pub(super) fn stage_dynamic<'a>(
        &self,
        guest: &'a super::GuestImage,
        libraries: impl Iterator<Item = &'a MappedImage>,
        new_images: impl Iterator<Item = &'a Region> + Clone,
        routines: &mut Vec<Routine>,
    ) -> Result<DynamicTls, LoadError> {
        let mut pending = DynamicTls {
            plans: Vec::new(), mappings: Vec::new(), blocks: Vec::new(), threads: Vec::new(),
        };
        let mut used = [false; MAX_SLOTS];
        for plan in &self.plans { used[plan.slot] = true; }
        for library in libraries {
            if let Some(mut plan) = Plan::parse(library)? {
                plan.slot = used.iter().position(|occupied| !occupied).ok_or(LoadError::ImageTooLarge)?;
                used[plan.slot] = true;
                plan.append_callbacks(routines)?;
                pending.plans.try_reserve(1).map_err(|_| LoadError::OutOfMemory)?;
                pending.plans.push(plan);
            }
            if library.entry != 0 {
                routines.try_reserve(1).map_err(|_| LoadError::OutOfMemory)?;
                routines.push(Routine::dll(library.region.base, library.entry));
            }
        }
        if pending.plans.is_empty() { return Ok(pending); }
        if self.data_size() + pending.plans.iter().map(|plan| plan.data.len().div_ceil(DATA_OFFSET) * DATA_OFFSET).sum::<usize>() > MAX_TOTAL_DATA {
            return Err(LoadError::ImageTooLarge);
        }
        let threads = guest.threads.tls_threads(guest.teb);
        pending.threads.try_reserve(threads.clone().count()).map_err(|_| LoadError::OutOfMemory)?;
        pending.threads.extend(threads);
        let block_count = pending.threads.len() * pending.plans.len();
        pending.blocks.try_reserve_exact(block_count).map_err(|_| LoadError::OutOfMemory)?;
        pending.mappings.try_reserve_exact(block_count).map_err(|_| LoadError::OutOfMemory)?;
        let existing = guest.memory.regions.iter().chain(new_images);
        let mut total = existing.clone().map(|region| region.bytes.len()).sum::<usize>();
        let mut address = TLS_ALLOCATION_START;
        for plan in &pending.plans {
            let size = plan.data.len().div_ceil(PAGE) * PAGE;
            total = total.saturating_add(size * pending.threads.len());
            if total > MAX_PROCESS_MEMORY { return Err(LoadError::ImageTooLarge); }
            for &(teb, _) in &pending.threads {
                address = storage_base(existing.clone(), address, size, guest.user_address_end)?;
                let mut region = Region::new(address, size, READ | WRITE)?;
                region.bytes[..plan.data.len()].copy_from_slice(&plan.data);
                pending.mappings.push(region);
                pending.blocks.push(DynamicBlock { module: plan.base, teb, address });
                address += size.div_ceil(DATA_OFFSET) as u64 * DATA_OFFSET as u64;
            }
        }
        Ok(pending)
    }

    pub(super) fn reserve_dynamic(&mut self, pending: &DynamicTls) -> Result<(), LoadError> {
        self.plans.try_reserve_exact(pending.plans.len()).map_err(|_| LoadError::OutOfMemory)?;
        self.blocks.try_reserve_exact(pending.blocks.len()).map_err(|_| LoadError::OutOfMemory)
    }

    pub(super) fn install_dynamic(&mut self, memory: &mut ImageMemory, pending: DynamicTls, width: usize) {
        for (plan, blocks) in pending.plans.iter().zip(pending.blocks.chunks(pending.threads.len().max(1))) {
            write_pointer(memory, plan.base, plan.index, plan.slot as u64, 4);
            for (&(teb, vector), block) in pending.threads.iter().zip(blocks) {
                write_pointer(memory, vector, vector + (plan.slot * width) as u64, block.address, width);
                write_pointer(memory, teb, teb_vector(teb, width), vector, width);
            }
        }
        memory.regions.extend(pending.mappings);
        self.plans.extend(pending.plans);
        self.blocks.extend(pending.blocks);
    }

    pub(super) fn remove_thread(&mut self, memory: &mut ImageMemory, teb: u64) {
        self.blocks.retain(|block| {
            if block.teb != teb { return true; }
            memory.regions.retain(|region| region.base != block.address);
            false
        });
    }

    pub(super) fn remove_modules(
        &mut self, memory: &mut ImageMemory, modules: &[u64], vectors: impl Iterator<Item = u64> + Clone,
        width: usize,
    ) {
        self.plans.retain(|plan| {
            if !modules.contains(&plan.base) { return true; }
            for vector in vectors.clone() {
                write_pointer(memory, vector, vector + (plan.slot * width) as u64, 0, width);
            }
            false
        });
        self.blocks.retain(|block| {
            if !modules.contains(&block.module) { return true; }
            memory.regions.retain(|region| region.base != block.address);
            false
        });
    }

    pub(super) fn unload_routines(&self, modules: &[u64]) -> Result<Vec<Routine>, LoadError> {
        let mut routines = Vec::new();
        let selected = self.routines.iter().rev().filter(|routine| modules.contains(&routine.base));
        routines.try_reserve_exact(selected.clone().count()).map_err(|_| LoadError::OutOfMemory)?;
        routines.extend(selected.cloned());
        Ok(routines)
    }

    pub(super) fn notifications(&self, disabled: &[u64], detach: bool, thread: &GuestThread) -> Option<Startup> {
        let mut routines: Vec<_> = self.routines.iter()
            .filter(|routine| !disabled.contains(&routine.base)).cloned().collect();
        if detach { routines.reverse(); }
        if routines.is_empty() { return None; }
        Some(Startup { routines, next: 0, saved: thread.context, return_sp: None,
            reason: if detach { 3 } else { 2 }, static_load: false })
    }

    pub(super) fn install(
        &self,
        memory: &mut ImageMemory,
        thread: &GuestThread,
        teb: u64,
        limit: u64,
    ) -> Result<(Option<Startup>, u64), LoadError> {
        let size = self.data_size();
        if size > MAX_TOTAL_DATA { return Err(LoadError::ImageTooLarge); }
        let base = storage_base(memory.regions.iter(), TLS_BASE, size, limit)
            .or_else(|_| storage_base(memory.regions.iter(), TLS_ALLOCATION_START, size, TLS_BASE))?;
        let startup = self.install_at(memory, thread, teb, base, 1)?;
        Ok((startup, base))
    }

    pub(super) fn install_at(
        &self, memory: &mut ImageMemory, thread: &GuestThread, teb: u64,
        tls_base: u64, reason: u32,
    ) -> Result<Option<Startup>, LoadError> {
        let bad = LoadError::InvalidImage;
        let x86 = thread.context.mode == DecodeMode::X86;
        let width = if x86 { 4 } else { 8 };
        let mut cursor = DATA_OFFSET;
        let mut offsets = Vec::new();
        if self.plans.iter().any(|plan| plan.slot >= MAX_SLOTS) {
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
        let needs_return_gate = !memory.regions.iter().any(|region| region.base == RETURN_GATE);
        let additional = length + if needs_return_gate { PAGE } else { 0 };
        if memory.mapped_bytes().saturating_add(additional) > MAX_PROCESS_MEMORY {
            return Err(LoadError::ImageTooLarge);
        }
        for (base, size) in [
            (tls_base, length),
            (RETURN_GATE, PAGE),
        ] {
            if base == RETURN_GATE && reason != 1 && memory.regions.iter().any(|r| r.base == base && r.bytes.len() == PAGE) {
                continue;
            }
            if size != 0
                && memory
                    .regions
                    .iter()
                    .any(|r| base < r.base + r.bytes.len() as u64 && r.base < base + size as u64)
            {
                return Err(bad);
            }
        }
        let mut region = Region::new(tls_base, length, READ | WRITE)?;
        if !self.plans.is_empty() {
            for (plan, &offset) in self.plans.iter().zip(&offsets) {
                let pointer = tls_base + offset as u64;
                region.bytes[plan.slot * width..(plan.slot + 1) * width]
                    .copy_from_slice(&pointer.to_le_bytes()[..width]);
                region.bytes[offset..offset + plan.data.len()].copy_from_slice(&plan.data);
                // Loader-only patch: final read-only image protections remain
                // enforced for all subsequent guest reads/writes.
                if reason == 1 {
                    write_pointer(memory, plan.base, plan.index, plan.slot as u64, 4);
                }
            }
            if !memory.write_virt(
                teb_vector(teb, width),
                &tls_base.to_le_bytes()[..width],
            ) {
                return Err(bad);
            }
        }
        memory.regions.push(region);
        if !memory.regions.iter().any(|r| r.base == RETURN_GATE) {
            memory.regions.push(Region::new(RETURN_GATE, PAGE, READ)?);
        }
        if reason != 1 || self.routines.is_empty() {
            return Ok(None);
        }
        Ok(Some(Startup {
            routines: self.routines.clone(),
            next: 0,
            saved: thread.context,
            return_sp: None,
            reason,
            static_load: reason == 1,
        }))
    }
}

pub(super) struct Startup {
    routines: Vec<Routine>,
    next: usize,
    saved: GuestState,
    return_sp: Option<u64>,
    reason: u32,
    static_load: bool,
}
impl Startup {
    pub(super) fn dynamic(routines: Vec<Routine>, reason: u32, thread: &GuestThread) -> Self {
        Self { routines, next: 0, saved: thread.context, return_sp: None, reason, static_load: false }
    }

    pub(super) fn rollback(&mut self, thread: &mut GuestThread) {
        thread.context = self.saved;
        self.routines.truncate(self.next);
        self.routines.reverse();
        self.next = 0;
        self.return_sp = None;
        self.reason = 0;
    }

    pub(super) fn active(&self) -> bool {
        self.return_sp.is_some()
    }

    pub(super) fn active_in(&self, modules: &[u64]) -> bool {
        self.active() && modules.contains(&self.routines[self.next - 1].base)
    }

    pub(super) fn discard_queued(&mut self, modules: &[u64]) {
        let mut index = 0;
        self.routines.retain(|routine| {
            let keep = index < self.next || !modules.contains(&routine.base);
            index += 1;
            keep
        });
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
        let sp = self.saved.reg(X86Reg::Rsp).checked_sub(0x100)
            .ok_or(StopReason::DataFault { pc: self.saved.rip })?;
        let sp = if x86 { sp } else { (sp & !15) | 8 };
        let width = if x86 { 4 } else { 8 };
        let mut frame = [0; 40];
        frame[..width].copy_from_slice(&RETURN_GATE.to_le_bytes()[..width]);
        // DllMain's reserved pointer is non-NULL for static process startup.
        let reserved = u64::from(routine.dll_entry && self.static_load);
        if x86 {
            frame[4..8].copy_from_slice(&(routine.base as u32).to_le_bytes());
            frame[8..12].copy_from_slice(&self.reason.to_le_bytes());
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
            thread.context.set_reg(X86Reg::Rdx, u64::from(self.reason));
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
        if self.reason == 1 && routine.dll_entry && thread.context.reg(X86Reg::Rax) as u32 == 0 {
            return Err(StopReason::InitializationFailed {
                module_base: routine.base,
            });
        }
        thread.context = self.saved;
        self.return_sp = None;
        Ok(true)
    }
}

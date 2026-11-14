//! Static TLS for the executable's initial guest thread. DLL TLS, dynamic
//! loading and thread attach/detach require the process/thread loader lifecycle;
//! the DLL linker continues to reject those cases explicitly.
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

pub(super) struct Plan {
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
            data,
            index,
            callbacks,
        }))
    }

    pub(super) fn install(
        self,
        memory: &mut ImageMemory,
        thread: &GuestThread,
        teb: u64,
    ) -> Result<Startup, LoadError> {
        let length = (DATA_OFFSET + self.data.len()).div_ceil(PAGE) * PAGE;
        for (base, size) in [(TLS_BASE, length), (RETURN_GATE, PAGE)] {
            if memory
                .regions
                .iter()
                .any(|r| base < r.base + r.bytes.len() as u64 && r.base < base + size as u64)
            {
                return Err(LoadError::InvalidImage);
            }
        }
        let x86 = thread.context.mode == DecodeMode::X86;
        let width = if x86 { 4 } else { 8 };
        let mut region = Region::new(TLS_BASE, length, READ | WRITE)?;
        let gate = Region::new(RETURN_GATE, PAGE, READ)?;
        region.bytes[..width]
            .copy_from_slice(&(TLS_BASE + DATA_OFFSET as u64).to_le_bytes()[..width]);
        region.bytes[DATA_OFFSET..DATA_OFFSET + self.data.len()].copy_from_slice(&self.data);
        // The loader writes the executable's static TLS slot before applying
        // its final section protection, just as it patches a read-only IAT.
        let owner = memory
            .regions
            .iter_mut()
            .find(|r| r.range(self.index, 4, READ).is_some())
            .ok_or(LoadError::InvalidImage)?;
        let offset = (self.index - owner.base) as usize;
        owner.bytes[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes());
        if !memory.write_virt(
            teb + if x86 { 0x2c } else { 0x58 },
            &TLS_BASE.to_le_bytes()[..width],
        ) {
            return Err(LoadError::InvalidImage);
        }
        memory.regions.push(region);
        memory.regions.push(gate);
        Ok(Startup {
            callbacks: self.callbacks,
            next: 0,
            saved: thread.context,
            return_sp: None,
        })
    }
}

pub(super) struct Startup {
    callbacks: Vec<u64>,
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
        base: u64,
    ) -> Result<(), StopReason> {
        if self.next == self.callbacks.len() {
            return Ok(());
        }
        let x86 = self.saved.mode == DecodeMode::X86;
        let sp = self.saved.reg(X86Reg::Rsp) - 0x100;
        let width = if x86 { 4 } else { 8 };
        let mut frame = [0; 40];
        frame[..width].copy_from_slice(&RETURN_GATE.to_le_bytes()[..width]);
        if x86 {
            frame[4..8].copy_from_slice(&(base as u32).to_le_bytes());
            frame[8..12].copy_from_slice(&1u32.to_le_bytes()); // DLL_PROCESS_ATTACH
        }
        if !memory.write_virt(sp, &frame[..if x86 { 16 } else { 40 }]) {
            return Err(StopReason::DataFault { pc: self.saved.rip });
        }
        thread.context = self.saved;
        thread.context.set_reg(X86Reg::Rsp, sp);
        thread
            .context
            .set_reg(X86Reg::Rip, self.callbacks[self.next]);
        if !x86 {
            thread.context.set_reg(X86Reg::Rcx, base);
            thread.context.set_reg(X86Reg::Rdx, 1);
            thread.context.set_reg(X86Reg::R8, 0);
        }
        self.return_sp = Some(sp + if x86 { 16 } else { 8 });
        self.next += 1;
        Ok(())
    }

    pub(super) fn complete(&mut self, thread: &mut GuestThread, reason: StopReason) -> bool {
        if reason != (StopReason::InstructionFault { pc: RETURN_GATE })
            || self.return_sp != Some(thread.context.reg(X86Reg::Rsp))
        {
            return false;
        }
        thread.context = self.saved;
        self.return_sp = None;
        true
    }
}

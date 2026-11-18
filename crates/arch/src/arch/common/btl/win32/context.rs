//! SP1 RtlCaptureContext for the dedicated source-generated entry thunk.
//! Captures guest state only. Exception dispatch and unwind remain separate.
use super::{ApiError, DecodeMode, GuestImage, GuestMemory};
use crate::arch::common::btl::ir::X86Reg;

fn read(memory: &mut dyn GuestMemory, address: u64, width: usize) -> Result<u64, ApiError> {
    let mut bytes = [0; 8];
    if memory.read_virt(address, &mut bytes[..width]) != width {
        return Err(ApiError::MemoryFault);
    }
    Ok(u64::from_le_bytes(bytes))
}
fn write(
    memory: &mut dyn GuestMemory,
    base: u64,
    offset: u64,
    value: u64,
    width: usize,
) -> Result<(), ApiError> {
    let address = base.checked_add(offset).ok_or(ApiError::MemoryFault)?;
    if !memory.write_virt(address, &value.to_le_bytes()[..width]) {
        return Err(ApiError::MemoryFault);
    }
    Ok(())
}

pub(super) fn capture(image: &mut GuestImage, output: u64) -> Result<u64, ApiError> {
    let state = image.thread.context;
    let sp = state.reg(X86Reg::Rsp);
    let m = &mut image.memory;
    if state.mode == DecodeMode::X86 {
        // Thunk stack: saved EDX, ECX, EAX, return address, output pointer.
        let edx = read(m, sp, 4)?;
        let ecx = read(m, sp.checked_add(4).ok_or(ApiError::MemoryFault)?, 4)?;
        let eax = read(m, sp.checked_add(8).ok_or(ApiError::MemoryFault)?, 4)?;
        let frame = state.reg(X86Reg::Rbp);
        let ebp = read(m, frame, 4)?;
        let eip = read(m, frame.checked_add(4).ok_or(ApiError::MemoryFault)?, 4)?;
        let esp = (frame as u32).wrapping_add(8) as u64;
        write(m, output, 0, 0x10007, 4)?;
        for (offset, value) in [
            (0x8c, 0x2b),
            (0x90, 0x53),
            (0x94, 0x2b),
            (0x98, 0x2b),
            (0xbc, 0x23),
            (0xc8, 0x2b),
        ] {
            // SP1 writes only the segment WORD, preserving its high WORD.
            write(m, output, offset, value, 2)?;
        }
        for (offset, value) in [
            (0x9c, state.reg(X86Reg::Rdi)),
            (0xa0, state.reg(X86Reg::Rsi)),
            (0xa4, state.reg(X86Reg::Rbx)),
            (0xa8, edx),
            (0xac, ecx),
            (0xb0, eax),
            (0xb4, ebp),
            (0xb8, eip),
            (0xc0, state.rflags),
            (0xc4, esp),
        ] {
            write(m, output, offset, value, 4)?;
        }
        // Original x86 capture omits floating-point and extended state.
        Ok(esp)
    } else {
        if output & 15 != 0 {
            return Err(ApiError::MemoryFault);
        }
        // Thunk stack: saved R11, R10, RAX, original return address.
        let r11 = read(m, sp, 8)?;
        let r10 = read(m, sp.checked_add(8).ok_or(ApiError::MemoryFault)?, 8)?;
        let rax = read(m, sp.checked_add(16).ok_or(ApiError::MemoryFault)?, 8)?;
        let rip = read(m, sp.checked_add(24).ok_or(ApiError::MemoryFault)?, 8)?;
        let rsp = sp.checked_add(32).ok_or(ApiError::MemoryFault)?;
        write(m, output, 0x30, 0x10000f, 4)?;
        write(m, output, 0x34, u64::from(state.mxcsr), 4)?;
        for (offset, value) in [
            (0x38, 0x33),
            (0x3a, 0x2b),
            (0x3c, 0x2b),
            (0x3e, 0x53),
            (0x40, 0x2b),
            (0x42, 0x2b),
        ] {
            write(m, output, offset, value, 2)?;
        }
        write(m, output, 0x44, state.rflags, 4)?;
        let mut regs = state.regs;
        regs[X86Reg::Rax as usize] = rax;
        regs[X86Reg::Rcx as usize] = output;
        regs[X86Reg::Rsp as usize] = rsp;
        regs[X86Reg::R10 as usize] = r10;
        regs[X86Reg::R11 as usize] = r11;
        for (i, value) in regs.iter().enumerate() {
            write(m, output, 0x78 + i as u64 * 8, *value, 8)?;
        }
        write(m, output, 0xf8, rip, 8)?;

        // FXSAVE64's defined output through XMM15. The reference CPU zeros
        // reserved bytes and retains the caller's software area at 2a0+.
        // Our virtual Intel-compatible CPU exposes MXCSR_MASK=ffff and
        // cleared FOP/FDP (as on the native SP1 reference captures).
        let mut fx = [0; 0x1a0];
        fx[0..2].copy_from_slice(&state.x87_control.to_le_bytes());
        fx[2..4].copy_from_slice(&state.x87_status.to_le_bytes());
        fx[4] = !state.x87_empty;
        fx[8..16].copy_from_slice(&state.x87_instruction_pointer.to_le_bytes());
        fx[24..28].copy_from_slice(&state.mxcsr.to_le_bytes());
        fx[28..32].copy_from_slice(&0xffffu32.to_le_bytes());
        let top = usize::from((state.x87_status >> 11) & 7);
        for i in 0..8 {
            let value = state.x87_registers[(top + i) & 7];
            fx[32 + i * 16..40 + i * 16].copy_from_slice(&value.significand.to_le_bytes());
            fx[40 + i * 16..42 + i * 16].copy_from_slice(&value.sign_exponent.to_le_bytes());
        }
        for i in 0..16 {
            fx[0xa0 + i * 16..0xb0 + i * 16].copy_from_slice(&state.xmm[i]);
        }
        if !m.write_virt(output.checked_add(0x100).ok_or(ApiError::MemoryFault)?, &fx) {
            return Err(ApiError::MemoryFault);
        }
        Ok(state.rflags)
    }
}

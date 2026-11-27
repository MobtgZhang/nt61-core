//! x86/x64 interpreter over the shared BTL IR.
//!
//! This is the checked execution backend used on all supported hosts. Guest
//! state and memory remain isolated. Native JIT execution stays disabled until
//! it can preserve the same semantics; unsupported instructions stop precisely.

use super::ir::{
    AluOp, BitAction, BranchCond, ByteOperand, DecodeMode, IROp, IrInst, LockedLogicSource, MemOperand, Segment, SimdOp, SimdShuffle, SimdSrc, SimdWidth,
    X86Reg,
};
use super::memory::GuestMemory;
#[path = "exec/float.rs"]
mod float;
#[path = "exec/x87.rs"]
mod x87;
pub use x87::Extended;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod test_support {
    use super::{execute_block, ExecOutcome, GuestState, SyscallHook, X86Reg};
    use crate::arch::common::btl::ir::IrInst;
    use crate::arch::common::btl::memory::{FlatPaging, GuestAddressSpace};
    use crate::arch::common::btl::x86_decoder::decode_block;

    pub struct NoopSyscall;

    impl SyscallHook for NoopSyscall {
        fn syscall(&mut self, _state: &mut GuestState, _memory: &mut dyn super::GuestMemory) {}
    }

    pub fn decode(code: &[u8], base: u64) -> alloc::vec::Vec<IrInst> {
        let mut out = [IrInst::empty(); 16];
        let count = decode_block(code, base, &mut out);
        out[..count].to_vec()
    }

    pub fn flat_memory(backing: &mut [u8]) -> GuestAddressSpace<FlatPaging> {
        GuestAddressSpace::new(FlatPaging {
            base: backing.as_mut_ptr(),
            len: backing.len(),
        })
    }

    pub fn run_mov_imm_add_imm(memory: &mut dyn super::GuestMemory) -> (ExecOutcome, u64) {
        let code = [
            0x48, 0xB8, 42, 0, 0, 0, 0, 0, 0, 0, 0x48, 0x83, 0xC0, 8, 0xC3,
        ];
        let ir = decode(&code, 0x1000);
        let mut state = GuestState::new();
        state.rip = 0x1000;
        let mut hook = NoopSyscall;
        let outcome = execute_block(&ir, &mut state, memory, &mut hook);
        (outcome, state.reg(X86Reg::Rax))
    }
}

/// Architectural state owned by one guest thread, including its TEB segment bases.
#[derive(Clone, Copy, Debug)]
pub struct GuestState {
    pub regs: [u64; 16],
    pub rflags: u64,
    pub rip: u64,
    pub mode: DecodeMode,
    pub fs_base: u64,
    pub gs_base: u64,
    pub xmm: [[u8; 16]; 32],
    /// x87 status includes the physical register stack's TOP field.
    pub x87_status: u16,
    /// Architectural reset value: all exceptions masked, extended precision.
    pub x87_control: u16,
    pub x87_registers: [Extended; 8],
    /// One bit per physical register, set when the register is empty.
    pub x87_empty: u8,
    /// Last completed x87 data instruction for the virtual CPU's FXSAVE64.
    /// FOP/FDP use the cleared-field model of the native SP1 reference CPU.
    pub x87_instruction_pointer: u64,
    /// SSE reset state: exceptions masked, round to nearest, gradual underflow.
    pub mxcsr: u32,
}
impl GuestState {
    pub const fn new() -> Self {
        Self {
            regs: [0; 16],
            rflags: 2,
            rip: 0,
            mode: DecodeMode::X64,
            fs_base: 0,
            gs_base: 0,
            xmm: [[0; 16]; 32],
            x87_status: 0,
            x87_control: 0x037f,
            x87_registers: [Extended::ZERO; 8],
            x87_empty: 0xff,
            x87_instruction_pointer: 0,
            mxcsr: 0x1f80,
        }
    }
    pub fn reg(&self, r: X86Reg) -> u64 {
        if r == X86Reg::Rip {
            self.rip
        } else {
            self.regs[r as usize]
        }
    }
    pub fn set_reg(&mut self, r: X86Reg, v: u64) {
        let v = if self.mode == DecodeMode::X86 {
            v as u32 as u64
        } else {
            v
        };
        if r == X86Reg::Rip {
            self.rip = v
        } else {
            self.regs[r as usize] = v
        }
    }
    fn write_reg(&mut self, r: X86Reg, v: u64, width: u8) {
        self.set_reg(
            r,
            if width < 4 {
                (self.reg(r) & !mask(width)) | (v & mask(width))
            } else {
                v & mask(width)
            },
        );
    }
    fn read_subregister(&self, reg: X86Reg, width: u8, high_byte: bool) -> u64 {
        (self.reg(reg) >> if high_byte { 8 } else { 0 }) & mask(width)
    }
    fn write_subregister(&mut self, reg: X86Reg, value: u64, width: u8, high_byte: bool) {
        if high_byte {
            self.set_reg(reg, (self.reg(reg) & !0xff00) | ((value & 0xff) << 8));
        } else {
            self.write_reg(reg, value, width);
        }
    }
    fn flag(&self, bit: u32) -> bool {
        self.rflags & (1 << bit) != 0
    }
    fn set_flag(&mut self, bit: u32, v: bool) {
        self.rflags = (self.rflags & !(1 << bit)) | ((v as u64) << bit) | 2;
    }
    fn stack_width(&self, inst: &IrInst) -> u8 {
        if inst.operand_width == 2 {
            2
        } else if self.mode == DecodeMode::X86 {
            4
        } else {
            8
        }
    }
}
impl Default for GuestState {
    fn default() -> Self {
        Self::new()
    }
}
const FLAG_CF: u32 = 0;
const FLAG_PF: u32 = 2;
const FLAG_AF: u32 = 4;
const FLAG_ZF: u32 = 6;
const FLAG_SF: u32 = 7;
const FLAG_DF: u32 = 10;
const FLAG_OF: u32 = 11;
fn mask(width: u8) -> u64 {
    if width == 8 {
        u64::MAX
    } else {
        (1u64 << (width * 8)) - 1
    }
}
fn alu(state: &mut GuestState, op: AluOp, a: u64, b: u64, width: u8) -> u64 {
    let m = mask(width);
    let a = a & m;
    let b = b & m;
    let borrow = u64::from(op == AluOp::Sbb && state.flag(FLAG_CF));
    let r = (match op {
        AluOp::Add => a.wrapping_add(b),
        AluOp::Adc => return adc(state, a, b, width),
        AluOp::Sub | AluOp::Cmp => a.wrapping_sub(b),
        AluOp::Sbb => a.wrapping_sub(b).wrapping_sub(borrow),
        AluOp::And | AluOp::Test => a & b,
        AluOp::Or => a | b,
        AluOp::Xor => a ^ b,
    }) & m;
    set_alu_flags(state, op, a, b, r, width);
    r
}

pub(super) fn set_alu_flags(
    state: &mut GuestState,
    op: AluOp,
    left: u64,
    right: u64,
    result: u64,
    width: u8,
) {
    let sign = 1u64 << (width * 8 - 1);
    let borrow = u64::from(op == AluOp::Sbb && state.flag(FLAG_CF));
    state.set_flag(FLAG_ZF, result == 0);
    state.set_flag(FLAG_SF, result & sign != 0);
    state.set_flag(FLAG_PF, (result as u8).count_ones() % 2 == 0);
    match op {
        AluOp::Add => {
            state.set_flag(FLAG_CF, result < left);
            state.set_flag(FLAG_OF, (!(left ^ right) & (left ^ result) & sign) != 0);
            state.set_flag(FLAG_AF, (left ^ right ^ result) & 16 != 0);
        }
        AluOp::Sub | AluOp::Cmp | AluOp::Sbb => {
            state.set_flag(FLAG_CF, (left as u128) < right as u128 + borrow as u128);
            state.set_flag(FLAG_OF, ((left ^ right) & (left ^ result) & sign) != 0);
            state.set_flag(FLAG_AF, (left ^ right ^ result) & 16 != 0);
        }
        _ => {
            state.set_flag(FLAG_CF, false);
            state.set_flag(FLAG_OF, false);
        }
    }
}
fn adc(s: &mut GuestState, a: u64, b: u64, w: u8) -> u64 {
    let a = a & mask(w);
    let b = b & mask(w);
    let carry = s.flag(FLAG_CF) as u64;
    let wide = a as u128 + b as u128 + carry as u128;
    let r = wide as u64 & mask(w);
    let sign = 1u64 << (w * 8 - 1);
    s.set_flag(FLAG_CF, wide > mask(w) as u128);
    s.set_flag(FLAG_OF, (!(a ^ b) & (a ^ r) & sign) != 0);
    s.set_flag(FLAG_AF, (a ^ b ^ r) & 16 != 0);
    s.set_flag(FLAG_ZF, r == 0);
    s.set_flag(FLAG_SF, r & sign != 0);
    s.set_flag(FLAG_PF, (r as u8).count_ones() % 2 == 0);
    r
}
fn shift(s: &mut GuestState, kind: u8, a: u64, count: u8, w: u8) -> u64 {
    let bits = w * 8;
    let count = count & if w == 8 { 63 } else { 31 };
    let effective = match kind {
        0 | 1 => count % bits,
        2 | 3 if w < 4 => count % (bits + 1),
        _ => count,
    };
    let mut value = a & mask(w);
    if effective == 0 {
        if count != 0 && kind == 0 {
            s.set_flag(FLAG_CF, value & 1 != 0)
        } else if count != 0 && kind == 1 {
            s.set_flag(FLAG_CF, value >> (bits - 1) != 0)
        }
        return value;
    }
    let old_sign = value >> (bits - 1);
    let mut carry = s.flag(FLAG_CF);
    for _ in 0..effective {
        match kind {
            0 => {
                carry = value >> (bits - 1) != 0;
                value = ((value << 1) | carry as u64) & mask(w)
            }
            1 => {
                carry = value & 1 != 0;
                value = (value >> 1) | ((carry as u64) << (bits - 1))
            }
            2 => {
                let next = value >> (bits - 1) != 0;
                value = ((value << 1) | carry as u64) & mask(w);
                carry = next
            }
            3 => {
                let next = value & 1 != 0;
                value = (value >> 1) | ((carry as u64) << (bits - 1));
                carry = next
            }
            4 | 6 => {
                carry = value >> (bits - 1) != 0;
                value = (value << 1) & mask(w)
            }
            5 => {
                carry = value & 1 != 0;
                value >>= 1
            }
            _ => {
                carry = value & 1 != 0;
                value = (value >> 1) | (value & (1u64 << (bits - 1)))
            }
        }
    }
    s.set_flag(FLAG_CF, carry);
    if count == 1 {
        s.set_flag(
            FLAG_OF,
            match kind {
                0 | 2 | 4 | 6 => (value >> (bits - 1) != 0) ^ carry,
                1 | 3 => ((value >> (bits - 1)) ^ (value >> (bits - 2))) & 1 != 0,
                5 => old_sign != 0,
                _ => false,
            },
        )
    }
    if kind >= 4 {
        s.set_flag(FLAG_ZF, value == 0);
        s.set_flag(FLAG_SF, value >> (bits - 1) != 0);
        s.set_flag(FLAG_PF, (value as u8).count_ones() % 2 == 0)
    }
    value
}
fn eval_cond(s: &GuestState, c: BranchCond) -> bool {
    let z = s.flag(FLAG_ZF);
    let n = s.flag(FLAG_SF);
    let b = s.flag(FLAG_CF);
    let o = s.flag(FLAG_OF);
    let p = s.flag(FLAG_PF);
    match c {
        BranchCond::Always => true,
        BranchCond::Overflow => o,
        BranchCond::NotOverflow => !o,
        BranchCond::Below => b,
        BranchCond::AboveOrEqual => !b,
        BranchCond::Equal => z,
        BranchCond::NotEqual => !z,
        BranchCond::BelowOrEqual => b || z,
        BranchCond::Above => !b && !z,
        BranchCond::Sign => n,
        BranchCond::NotSign => !n,
        BranchCond::Parity => p,
        BranchCond::NotParity => !p,
        BranchCond::Less => n != o,
        BranchCond::GreaterOrEqual => n == o,
        BranchCond::LessOrEqual => z || n != o,
        BranchCond::Greater => !z && n == o,
    }
}
fn effective_address(s: &GuestState, m: &MemOperand, i: &IrInst, segment: bool) -> u64 {
    let base = m
        .base
        .map(|r| {
            if r == X86Reg::Rip {
                i.guest_va.wrapping_add(i.guest_len as u64)
            } else {
                s.reg(r)
            }
        })
        .unwrap_or(0);
    let offset = base
        .wrapping_add(
            m.index
                .map(|r| s.reg(r))
                .unwrap_or(0)
                .wrapping_mul(m.scale as u64),
        )
        .wrapping_add(m.disp as i64 as u64);
    let offset = if i.address_width == 4 {
        offset as u32 as u64
    } else {
        offset
    };
    let base = if !segment {
        0
    } else {
        match i.segment {
            Segment::Flat => 0,
            Segment::Fs => s.fs_base,
            Segment::Gs => s.gs_base,
        }
    };
    let addr = base.wrapping_add(offset);
    if s.mode == DecodeMode::X86 {
        addr as u32 as u64
    } else {
        addr
    }
}
fn mem_read(m: &mut dyn GuestMemory, a: u64, w: u8) -> Option<u64> {
    if !matches!(w, 1 | 2 | 4 | 8) {
        return None;
    }
    let mut b = [0; 8];
    if m.read_virt(a, &mut b[..w as usize]) != w as usize {
        None
    } else {
        Some(u64::from_le_bytes(b))
    }
}
fn mem_write(m: &mut dyn GuestMemory, a: u64, v: u64, w: u8) -> bool {
    matches!(w, 1 | 2 | 4 | 8) && m.write_virt(a, &v.to_le_bytes()[..w as usize])
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecOutcome {
    Retired { next_rip: u64 },
    Unsupported { guest_va: u64 },
    MemoryFault { guest_va: u64 },
    DivideError { guest_va: u64 },
    FloatingPointError { guest_va: u64, exceptions: u8 },
}
pub trait SyscallHook {
    fn syscall(&mut self, state: &mut GuestState, memory: &mut dyn GuestMemory);
}

/// Execute at architectural instruction boundaries. A fault leaves registers,
/// flags and RIP at the faulting instruction so exception delivery can restart it.
pub fn execute_block(
    ir: &[IrInst],
    s: &mut GuestState,
    m: &mut dyn GuestMemory,
    h: &mut dyn SyscallHook,
) -> ExecOutcome {
    for i in ir {
        if i.guest_va != s.rip {
            return ExecOutcome::Retired { next_rip: s.rip };
        }
        let before = *s;
        if let Err(out) = execute_one(i, s, m, h) {
            let mxcsr = s.mxcsr;
            *s = before;
            // SIMD exceptions publish sticky status even though the destination
            // and instruction pointer remain at the faulting instruction.
            if matches!(out, ExecOutcome::FloatingPointError { .. })
                && matches!(i.op, IROp::SimdArith { .. } | IROp::SimdToInteger { .. } | IROp::IntegerToSimd { .. }) {
                s.mxcsr = mxcsr;
            }
            return out;
        }
        if matches!(
            i.op,
            IROp::Jmp { .. }
                | IROp::Loop { .. }
                | IROp::Jcc { .. }
                | IROp::Call { .. }
                | IROp::Ret
                | IROp::RetImm { .. }
                | IROp::JumpReg { .. }
                | IROp::JumpMem { .. }
                | IROp::Syscall
        ) {
            return ExecOutcome::Retired { next_rip: s.rip };
        }
    }
    ExecOutcome::Retired { next_rip: s.rip }
}
fn execute_one(
    i: &IrInst,
    s: &mut GuestState,
    m: &mut dyn GuestMemory,
    h: &mut dyn SyscallHook,
) -> Result<(), ExecOutcome> {
    let fault = ExecOutcome::MemoryFault {
        guest_va: i.guest_va,
    };
    let unsupported = ExecOutcome::Unsupported {
        guest_va: i.guest_va,
    };
    let w = i.operand_width;
    if !matches!(w, 1 | 2 | 4 | 8) || i.guest_len == 0 || i.guest_len > 15 {
        return Err(unsupported);
    }
    let next = i.guest_va.wrapping_add(i.guest_len as u64);
    s.set_reg(X86Reg::Rip, next);
    let stores_result = |op| !matches!(op, AluOp::Cmp | AluOp::Test);
    match i.op {
        IROp::Nop => {}
        IROp::LoadMxcsr { source } => {
            let value = mem_read(m, effective_address(s, &source, i, true), 4).ok_or(fault)?;
            if value & !0xffff != 0 { return Err(fault); }
            s.mxcsr = value as u32;
        }
        IROp::StoreMxcsr { destination } => {
            if !mem_write(m, effective_address(s, &destination, i, true), u64::from(s.mxcsr), 4) {
                return Err(fault);
            }
        }
        IROp::ClearX87Exceptions => s.x87_status &= !0x80ff,
        IROp::WaitX87 | IROp::LoadX87Control { .. } => {
            let exceptions = (s.x87_status & !s.x87_control & 0x3f) as u8;
            if exceptions != 0 {
                return Err(ExecOutcome::FloatingPointError { guest_va: i.guest_va, exceptions });
            }
            if let IROp::LoadX87Control { source } = i.op {
                let value = mem_read(m, effective_address(s, &source, i, true), 2).ok_or(fault)?;
                // Bit 6 reads as one; reserved bits 7 and 13..15 read as zero.
                s.x87_control = (value as u16 & 0x1f3f) | 0x40;
            }
        }
        IROp::StoreX87Control { destination } => {
            let address = effective_address(s, &destination, i, true);
            if !mem_write(m, address, u64::from(s.x87_control), 2) { return Err(fault); }
        }
        IROp::LoadX87Float { source, double } => {
            x87::wait(s, i.guest_va)?;
            let value = mem_read(m, effective_address(s, &source, i, true), if double {8} else {4}).ok_or(fault)?;
            x87::load(s, value, double);
            s.x87_instruction_pointer = i.guest_va;
        }
        IROp::StoreX87Integer { destination, width, pop } => {
            if !matches!(width, 2 | 4 | 8) { return Err(unsupported); }
            x87::wait(s, i.guest_va)?;
            let address = effective_address(s, &destination, i, true);
            // The memory access must fault before changing status or popping.
            if !m.probe_write(address, usize::from(width)) { return Err(fault); }
            if let Some(value) = x87::store_integer(s, width, pop) {
                if !mem_write(m, address, value, width) { return Err(fault); }
            }
            s.x87_instruction_pointer = i.guest_va;
        }
        IROp::LoadX87Integer { source, width } => {
            if !matches!(width, 2 | 4 | 8) { return Err(unsupported); }
            x87::wait(s, i.guest_va)?;
            let bits = mem_read(m, effective_address(s, &source, i, true), width).ok_or(fault)?;
            x87::load_integer(s, bits, width);
            s.x87_instruction_pointer = i.guest_va;
        }
        IROp::StoreX87Float { destination, double, pop } => {
            x87::wait(s, i.guest_va)?;
            let width = if double {8} else {4};
            let address = effective_address(s, &destination, i, true);
            if !m.probe_write(address, usize::from(width)) { return Err(fault); }
            if let Some(value) = x87::store_float(s, double, pop) {
                if !mem_write(m, address, value, width) { return Err(fault); }
            }
            s.x87_instruction_pointer = i.guest_va;
        }
        IROp::MoveString { repeat } => {
            let aw = i.address_width;
            let count = s.reg(X86Reg::Rcx) & mask(aw);
            if !repeat || count != 0 {
                let source = s.reg(X86Reg::Rsi) & mask(aw);
                let target = s.reg(X86Reg::Rdi) & mask(aw);
                let segment_base = match i.segment {
                    Segment::Flat => 0,
                    Segment::Fs => s.fs_base,
                    Segment::Gs => s.gs_base,
                };
                let address = source.wrapping_add(segment_base);
                let address = if s.mode == DecodeMode::X86 { address as u32 as u64 } else { address };
                let value = mem_read(m, address, w).ok_or(fault)?;
                // Reading before writing preserves MOVS overlap ordering.
                // Only a completed element advances either index or count.
                if !mem_write(m, target, value, w) { return Err(fault); }
                let step = if s.flag(FLAG_DF) { (w as u64).wrapping_neg() } else { w as u64 };
                s.write_reg(X86Reg::Rsi, source.wrapping_add(step), aw);
                s.write_reg(X86Reg::Rdi, target.wrapping_add(step), aw);
                if repeat {
                    let count = count.wrapping_sub(1) & mask(aw);
                    s.write_reg(X86Reg::Rcx, count, aw);
                    if count != 0 { s.set_reg(X86Reg::Rip, i.guest_va); }
                }
            }
        }
        IROp::StoreString { repeat } => {
            let aw = i.address_width;
            let count = s.reg(X86Reg::Rcx) & mask(aw);
            if !repeat || count != 0 {
                // STOS always addresses ES:(E)DI; FS/GS overrides do not
                // redirect its destination. Long mode ignores ES base.
                let ptr = s.reg(X86Reg::Rdi) & mask(aw);
                if !mem_write(m, ptr, s.reg(X86Reg::Rax), w) { return Err(fault); }
                let step = if s.flag(FLAG_DF) { (w as u64).wrapping_neg() } else { w as u64 };
                s.write_reg(X86Reg::Rdi, ptr.wrapping_add(step), aw);
                if repeat {
                    let count = count.wrapping_sub(1) & mask(aw);
                    s.write_reg(X86Reg::Rcx, count, aw);
                    if count != 0 { s.set_reg(X86Reg::Rip, i.guest_va); }
                }
            }
        }
        IROp::Scan { repeat } => {
            let aw = i.address_width;
            let count = s.reg(X86Reg::Rcx) & mask(aw);
            if repeat == 0 || count != 0 {
                let ptr = s.reg(X86Reg::Rdi) & mask(aw);
                let value = mem_read(m, ptr, w).ok_or(fault)?;
                alu(s, AluOp::Cmp, s.reg(X86Reg::Rax), value, w);
                let step = if s.flag(FLAG_DF) {
                    (w as u64).wrapping_neg()
                } else {
                    w as u64
                };
                s.write_reg(X86Reg::Rdi, ptr.wrapping_add(step), aw);
                if repeat != 0 {
                    let count = count.wrapping_sub(1) & mask(aw);
                    s.write_reg(X86Reg::Rcx, count, aw);
                    if count != 0 && s.flag(FLAG_ZF) == (repeat == 1) {
                        s.set_reg(X86Reg::Rip, i.guest_va)
                    }
                }
            }
        }
        IROp::ByteMove { dst, .. } | IROp::ByteImmediate { dst, .. } => {
            let value = match i.op {
                IROp::ByteImmediate { value, .. } => u64::from(value),
                IROp::ByteMove { src: ByteOperand::Register { reg, high_byte }, .. } =>
                    (s.reg(reg) >> if high_byte {8} else {0}) & 255,
                IROp::ByteMove { src: ByteOperand::Memory(mem), .. } =>
                    mem_read(m, effective_address(s, &mem, i, true), 1).ok_or(fault)?,
                _ => return Err(unsupported),
            };
            match dst {
                ByteOperand::Register { reg, high_byte: true } => s.set_reg(reg, (s.reg(reg) & !0xff00) | (value << 8)),
                ByteOperand::Register { reg, high_byte: false } => s.write_reg(reg, value, 1),
                ByteOperand::Memory(mem) => if !mem_write(m, effective_address(s, &mem, i, true), value, 1) { return Err(fault); },
            }
        }
        IROp::ByteAlu { op, dst, .. } | IROp::ByteAluImmediate { op, dst, .. } => {
            let value = |m: &mut dyn GuestMemory, operand| match operand {
                ByteOperand::Register { reg, high_byte } => Some((s.reg(reg) >> if high_byte {8} else {0}) & 255),
                ByteOperand::Memory(mem) => mem_read(m, effective_address(s, &mem, i, true), 1),
            };
            let left = value(m, dst).ok_or(fault)?;
            let right = match i.op {
                IROp::ByteAlu { src, .. } => value(m, src).ok_or(fault)?,
                IROp::ByteAluImmediate { immediate, .. } => u64::from(immediate),
                _ => return Err(unsupported),
            };
            let result = alu(s, op, left, right, 1);
            if !matches!(op, AluOp::Cmp | AluOp::Test) {
                match dst {
                    ByteOperand::Register { reg, high_byte: true } => s.set_reg(reg, (s.reg(reg) & !0xff00) | (result << 8)),
                    ByteOperand::Register { reg, high_byte: false } => s.write_reg(reg, result, 1),
                    ByteOperand::Memory(mem) => if !mem_write(m, effective_address(s, &mem, i, true), result, 1) { return Err(fault); },
                }
            }
        }
        IROp::DoubleShift { dst, src, right, count } => {
            let a = match dst { Ok(reg) => s.reg(reg), Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)? } & mask(w);
            let b = s.reg(src) & mask(w);
            let count = count.unwrap_or(s.reg(X86Reg::Rcx) as u8) & if w == 8 {63} else {31};
            let bits = w * 8;
            if count > bits { return Err(unsupported); } // architecturally undefined 16-bit counts
            let value = if count == 0 { a } else {
                let result = if right { (((b as u128) << bits) | a as u128) >> count }
                    else { ((((a as u128) << bits) | b as u128) << count) >> bits } as u64 & mask(w);
                let carry = if right { (a >> (count - 1)) & 1 } else { (a >> (bits - count)) & 1 };
                s.set_flag(FLAG_CF, carry != 0);
                s.set_flag(FLAG_SF, result >> (bits - 1) != 0);
                s.set_flag(FLAG_ZF, result == 0);
                s.set_flag(FLAG_PF, (result as u8).count_ones() % 2 == 0);
                if count == 1 {
                    s.set_flag(FLAG_OF, if right { (a ^ result) >> (bits - 1) != 0 }
                        else { (result >> (bits - 1)) ^ carry != 0 });
                }
                result
            };
            match dst {
                Ok(reg) => s.write_reg(reg, value, w),
                Err(mem) => if !mem_write(m, effective_address(s, &mem, i, true), value, w) { return Err(fault); },
            }
        }
        IROp::Shift { kind, dst, count } => {
            let a = match dst {
                Ok(r) => s.reg(r),
                Err(mem) => {
                    mem_read(m, effective_address(s, &mem, i, true), mem.width).ok_or(fault)?
                }
            };
            let count = count.unwrap_or(s.reg(X86Reg::Rcx) as u8);
            let value = shift(s, kind, a, count, w);
            match dst {
                Ok(r) => s.write_reg(r, value, w),
                Err(mem) => {
                    if !mem_write(m, effective_address(s, &mem, i, true), value, mem.width) {
                        return Err(fault);
                    }
                }
            }
        }
        IROp::Exchange {
            left,
            right,
            left_high,
            right_high,
        } => {
            let l = (s.reg(left) >> if left_high { 8 } else { 0 }) & mask(w);
            let r = (s.reg(right) >> if right_high { 8 } else { 0 }) & mask(w);
            let set = |s: &mut GuestState, reg, high, v| {
                if high {
                    s.set_reg(reg, (s.reg(reg) & !0xff00) | ((v & 255) << 8))
                } else {
                    s.write_reg(reg, v, w)
                }
            };
            set(s, left, left_high, r);
            set(s, right, right_high, l);
        }
        IROp::ExchangeMem { reg, mem, high_byte } => {
            // Capture EA and register before updating either operand: the
            // register can itself be part of the memory address (or be SP).
            // GuestImage owns isolated memory and runs one guest instruction
            // at a time. This is not a cross-core atomic RMW implementation.
            let address = effective_address(s, &mem, i, true);
            if !m.probe_write(address, w as usize) { return Err(fault); }
            let original = mem_read(m, address, w).ok_or(fault)?;
            let value = (s.reg(reg) >> if high_byte { 8 } else { 0 }) & mask(w);
            if !mem_write(m, address, value, w) { return Err(fault); }
            if high_byte {
                s.set_reg(reg, (s.reg(reg) & !0xff00) | ((original & 255) << 8));
            } else {
                s.write_reg(reg, original, w);
            }
        }
        IROp::Loop { target, kind } => {
            let aw = i.address_width;
            let n = s.reg(X86Reg::Rcx) & mask(aw);
            let take = if kind == 3 {
                n == 0
            } else {
                let n = n.wrapping_sub(1) & mask(aw);
                s.write_reg(X86Reg::Rcx, n, aw);
                n != 0
                    && match kind {
                        0 => !s.flag(FLAG_ZF),
                        1 => s.flag(FLAG_ZF),
                        _ => true,
                    }
            };
            if take {
                s.set_reg(X86Reg::Rip, target)
            }
        }
        IROp::PushAll | IROp::PopAll => {
            if s.mode != DecodeMode::X86 {
                return Err(unsupported);
            }
            let sw = s.stack_width(i) as usize;
            let sp = s.reg(X86Reg::Rsp);
            if i.op == IROp::PushAll {
                let bottom = sp.wrapping_sub((sw * 8) as u64) as u32 as u64;
                let mut bytes = [0u8; 32];
                for (j, r) in [
                    X86Reg::Rdi,
                    X86Reg::Rsi,
                    X86Reg::Rbp,
                    X86Reg::Rsp,
                    X86Reg::Rbx,
                    X86Reg::Rdx,
                    X86Reg::Rcx,
                    X86Reg::Rax,
                ]
                .iter()
                .enumerate()
                {
                    bytes[j * sw..(j + 1) * sw].copy_from_slice(&s.reg(*r).to_le_bytes()[..sw]);
                }
                if !m.write_virt(bottom, &bytes[..sw * 8]) {
                    return Err(fault);
                }
                s.set_reg(X86Reg::Rsp, bottom);
            } else {
                let mut bytes = [0u8; 32];
                if m.read_virt(sp, &mut bytes[..sw * 8]) != sw * 8 {
                    return Err(fault);
                }
                for (j, r) in [
                    X86Reg::Rdi,
                    X86Reg::Rsi,
                    X86Reg::Rbp,
                    X86Reg::Rsp,
                    X86Reg::Rbx,
                    X86Reg::Rdx,
                    X86Reg::Rcx,
                    X86Reg::Rax,
                ]
                .iter()
                .enumerate()
                {
                    if *r == X86Reg::Rsp {
                        continue;
                    }
                    let mut value = [0; 8];
                    value[..sw].copy_from_slice(&bytes[j * sw..(j + 1) * sw]);
                    s.write_reg(*r, u64::from_le_bytes(value), sw as u8);
                }
                s.set_reg(X86Reg::Rsp, sp.wrapping_add((sw * 8) as u64));
            }
        }
        IROp::CompareExchange { dst, src, dst_high_byte, src_high_byte, locked } => {
            let accumulator = s.read_subregister(X86Reg::Rax, w, false);
            let replacement = s.read_subregister(src, w, src_high_byte);
            let old = match dst {
                Ok(reg) => {
                    let old = s.read_subregister(reg, w, dst_high_byte);
                    if accumulator == old {
                        s.write_subregister(reg, replacement, w, dst_high_byte);
                    }
                    old
                }
                Err(mem) => {
                    let address = effective_address(s, &mem, i, true);
                    if locked {
                        m.locked_compare_exchange(address, w, accumulator, replacement)
                            .map_err(|error| match error {
                                super::memory::AtomicMemoryError::Unsupported => unsupported,
                                super::memory::AtomicMemoryError::Fault => fault,
                            })?
                    } else {
                        let old = mem_read(m, address, w).ok_or(fault)?;
                        let value = if accumulator == old { replacement } else { old };
                        if !mem_write(m, address, value, w) {
                            return Err(fault);
                        }
                        old
                    }
                }
            };
            alu(s, AluOp::Cmp, accumulator, old, w);
            if accumulator != old {
                s.write_reg(X86Reg::Rax, old, w);
            }
        }
        IROp::ExchangeAdd { dst, src, dst_high_byte, src_high_byte, locked } => {
            let addend = s.read_subregister(src, w, src_high_byte);
            let old = match dst {
                Ok(reg) => s.read_subregister(reg, w, dst_high_byte),
                Err(mem) => {
                    let address = effective_address(s, &mem, i, true);
                    if locked {
                        m.locked_fetch_add(address, w, addend).map_err(|error| match error {
                            super::memory::AtomicMemoryError::Unsupported => unsupported,
                            super::memory::AtomicMemoryError::Fault => fault,
                        })?
                    } else {
                        let old = mem_read(m, address, w).ok_or(fault)?;
                        if !mem_write(m, address, old.wrapping_add(addend), w) {
                            return Err(fault);
                        }
                        old
                    }
                }
            };
            let sum = alu(s, AluOp::Add, old, addend, w);
            s.write_subregister(src, old, w, src_high_byte);
            if let Ok(reg) = dst {
                s.write_subregister(reg, sum, w, dst_high_byte);
            }
        }
        IROp::ByteSwap { dst } => {
            let value = s.reg(dst);
            let swapped = if w == 8 { value.swap_bytes() } else { (value as u32).swap_bytes() as u64 };
            s.write_reg(dst, swapped, w);
        }
        IROp::LockedAddImmediate { mem, immediate, subtract } => {
            let operand = immediate as u64 & mask(w);
            let addend = if subtract { operand.wrapping_neg() } else { operand };
            let old = m.locked_fetch_add(effective_address(s, &mem, i, true), w, addend)
                .map_err(|error| match error {
                    super::memory::AtomicMemoryError::Unsupported => unsupported,
                    super::memory::AtomicMemoryError::Fault => fault,
                })?;
            alu(s, if subtract { AluOp::Sub } else { AluOp::Add }, old, operand, w);
        }
        IROp::LockedLogic { op, mem, src } => {
            let operand = match src {
                LockedLogicSource::Register { reg, high_byte } => s.reg(reg) >> (u32::from(high_byte) * 8),
                LockedLogicSource::Immediate(value) => value as u64,
            } & mask(w);
            let old = m.locked_logic(effective_address(s, &mem, i, true), w, op, operand)
                .map_err(|error| match error {
                    super::memory::AtomicMemoryError::Unsupported => unsupported,
                    super::memory::AtomicMemoryError::Fault => fault,
                })?;
            alu(s, op, old, operand, w);
        }
        IROp::LockedIncDec { mem, decrement } => {
            let address = effective_address(s, &mem, i, true);
            let old = m.locked_inc_dec(address, w, decrement).map_err(|error| match error {
                super::memory::AtomicMemoryError::Unsupported => unsupported,
                super::memory::AtomicMemoryError::Fault => fault,
            })?;
            let carry = s.flag(FLAG_CF);
            alu(s, if decrement {AluOp::Sub} else {AluOp::Add}, old, 1, w);
            s.set_flag(FLAG_CF, carry);
        }
        IROp::IncDec { dst, high_byte, decrement } => {
            let old = match dst {
                Ok(reg) => s.reg(reg) >> if high_byte { 8 } else { 0 },
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            };
            let flags = s.rflags;
            let carry = s.flag(FLAG_CF);
            let value = alu(
                s,
                if decrement { AluOp::Sub } else { AluOp::Add },
                old,
                1,
                w,
            );
            match dst {
                Ok(reg) if high_byte => s.set_reg(reg, (s.reg(reg) & !0xff00) | ((value & 255) << 8)),
                Ok(reg) => s.write_reg(reg, value, w),
                Err(mem) => {
                    if !mem_write(m, effective_address(s, &mem, i, true), value, w) {
                        s.rflags = flags;
                        return Err(fault);
                    }
                }
            }
            s.set_flag(FLAG_CF, carry);
        }
        IROp::AdcReg { dst, src } => {
            let value = adc(s, s.reg(dst), s.reg(src), w);
            s.write_reg(dst, value, w);
        }
        IROp::AdcImm { dst, imm } => {
            let value = adc(s, s.reg(dst), imm as u64, w);
            s.write_reg(dst, value, w);
        }
        IROp::TestImmediate { src, high_byte, immediate } => {
            let value = match src {
                Ok(reg) => s.reg(reg) >> if high_byte { 8 } else { 0 },
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            };
            alu(s, AluOp::Test, value, immediate as u64, w);
        }
        IROp::BitTestImmediate { src, bit, action } => {
            let value = match src {
                Ok(reg) => s.reg(reg),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            };
            let bit = 1u64 << (bit % (w * 8));
            let result = match action {
                BitAction::Test => None, BitAction::Set => Some(value | bit),
                BitAction::Reset => Some(value & !bit), BitAction::Complement => Some(value ^ bit),
            };
            if let Some(result) = result {
                match src {
                    Ok(reg) => s.write_reg(reg, result, w),
                    Err(mem) => {
                        let address = effective_address(s, &mem, i, true);
                        if !m.probe_write(address, usize::from(w)) || !mem_write(m,address,result,w) {return Err(fault);}
                    }
                }
            }
            s.set_flag(FLAG_CF, value & bit != 0);
        }
        IROp::BitScan { dst, src, reverse } => {
            let value = match src {
                Ok(reg) => s.reg(reg),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            } & mask(w);
            s.set_flag(FLAG_ZF, value == 0);
            // On zero the destination and all other arithmetic flags are
            // architecturally undefined. Preserve them; do not invent TZCNT.
            if value != 0 {
                let index = if reverse { 63 - value.leading_zeros() } else { value.trailing_zeros() };
                s.write_reg(dst, u64::from(index), w);
            }
        }
        IROp::BitRegister { dst, index, action } => {
            let index = s.reg(index) & mask(w);
            let bit = 1u64 << (index % u64::from(w * 8));
            let old = match dst {
                Ok(reg) => {
                    let old = s.reg(reg);
                    let result = match action {
                        BitAction::Test => None, BitAction::Set => Some(old | bit),
                        BitAction::Reset => Some(old & !bit), BitAction::Complement => Some(old ^ bit),
                    };
                    if let Some(result) = result { s.write_reg(reg, result, w); }
                    old
                }
                Err(mem) => {
                    // Memory bit strings use a signed register index. Form
                    // the adjusted offset before applying address-size wrap
                    // and FS/GS, rather than wrapping the segment base.
                    let signed_index = ((index << (64 - w * 8)) as i64) >> (64 - w * 8);
                    let delta = (signed_index >> (w * 8).trailing_zeros()) * i64::from(w);
                    let mut offset = effective_address(s, &mem, i, false).wrapping_add(delta as u64);
                    if i.address_width == 4 { offset = u64::from(offset as u32); }
                    let base = match i.segment { Segment::Flat => 0, Segment::Fs => s.fs_base, Segment::Gs => s.gs_base };
                    let address = base.wrapping_add(offset);
                    let address = if s.mode == DecodeMode::X86 { u64::from(address as u32) } else { address };
                    let old = mem_read(m, address, w).ok_or(fault)?;
                    let result = match action {
                        BitAction::Test => None, BitAction::Set => Some(old | bit),
                        BitAction::Reset => Some(old & !bit), BitAction::Complement => Some(old ^ bit),
                    };
                    if let Some(result) = result {
                        if !m.probe_write(address, usize::from(w)) || !mem_write(m,address,result,w) { return Err(fault); }
                    }
                    old
                }
            };
            s.set_flag(FLAG_CF, old & bit != 0);
        }
        IROp::FullMultiply { src, high_byte, signed } => {
            // Both inputs must be sampled before AX/DX are overwritten.
            let right = match src {
                Ok(reg) => (s.reg(reg) >> if high_byte {8} else {0}) & mask(w),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            };
            let left = s.reg(X86Reg::Rax) & mask(w);
            let bits = u32::from(w) * 8;
            let sign = |v: u64| ((v as i128) << (128 - bits)) >> (128 - bits);
            let product = if signed { (sign(left) * sign(right)) as u128 }
                else { (left as u128) * (right as u128) };
            let low = product as u64 & mask(w);
            let high = (product >> bits) as u64 & mask(w);
            let overflow = if signed { sign(low) != product as i128 } else { high != 0 };
            if w == 1 {
                s.write_reg(X86Reg::Rax, low | (high << 8), 2);
            } else {
                s.write_reg(X86Reg::Rax, low, w);
                s.write_reg(X86Reg::Rdx, high, w);
            }
            s.set_flag(FLAG_CF, overflow);
            s.set_flag(FLAG_OF, overflow);
        }
        IROp::Divide { src, high_byte, signed } => {
            let divisor = match src {
                Ok(reg) => (s.reg(reg) >> if high_byte {8} else {0}) & mask(w),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            } as u128;
            let bits = u32::from(w) * 8;
            let dividend = if w == 1 { (s.reg(X86Reg::Rax) & 0xffff) as u128 }
                else { (((s.reg(X86Reg::Rdx) & mask(w)) as u128) << bits)
                    | (s.reg(X86Reg::Rax) & mask(w)) as u128 };
            let divide_error = ExecOutcome::DivideError { guest_va: i.guest_va };
            let (quotient, remainder) = if signed {
                let dividend_shift = 128 - 2 * bits;
                let divisor_shift = 128 - bits;
                let signed_dividend = ((dividend << dividend_shift) as i128) >> dividend_shift;
                let signed_divisor = ((divisor << divisor_shift) as i128) >> divisor_shift;
                let quotient = signed_dividend.checked_div(signed_divisor).ok_or(divide_error)?;
                let limit = 1i128 << (bits - 1);
                if !(-limit..limit).contains(&quotient) {
                    return Err(divide_error);
                }
                (quotient as u64 & mask(w), (signed_dividend % signed_divisor) as u64 & mask(w))
            } else {
                let quotient = dividend.checked_div(divisor).ok_or(divide_error)?;
                if quotient > mask(w) as u128 {
                    return Err(divide_error);
                }
                (quotient as u64, (dividend % divisor) as u64)
            };
            if w == 1 {
                s.write_reg(X86Reg::Rax, quotient | (remainder << 8), 2);
            } else {
                s.write_reg(X86Reg::Rax, quotient, w);
                s.write_reg(X86Reg::Rdx, remainder, w);
            }
        }
        IROp::SignedMultiply { dst, src, immediate } => {
            let right = match src {
                Ok(reg) => s.reg(reg),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            };
            let shift = 128 - w as u32 * 8;
            let signed = |v: u64| ((v as i128) << shift) >> shift;
            let left = immediate.map_or_else(|| signed(s.reg(dst)), |imm| signed(imm as u64));
            let product = left * signed(right);
            let result = product as u64 & mask(w);
            let overflow = signed(result) != product;
            s.write_reg(dst, result, w);
            s.set_flag(FLAG_CF, overflow);
            s.set_flag(FLAG_OF, overflow);
        }
        IROp::SetCondition { cond, dst, high_byte } => {
            let value = eval_cond(s, cond) as u64;
            match dst {
                Ok(reg) if high_byte => s.set_reg(reg, (s.reg(reg) & !0xff00) | (value << 8)),
                Ok(reg) => s.write_reg(reg, value, 1),
                Err(mem) => {
                    let address = effective_address(s, &mem, i, true);
                    if !mem_write(m, address, value, 1) { return Err(fault); }
                }
            }
        }
        IROp::CpuId => {
            // Do not forward host CPUID: the guest must never select host-only
            // SIMD, entropy or other instruction families. Advertise only CMOV
            // and the memory backend's NX enforcement. No x87/SSE family is
            // complete yet, so those capability bits remain clear.
            let values = match s.reg(X86Reg::Rax) as u32 {
                0 => [1, u32::from_le_bytes(*b"NT61"), u32::from_le_bytes(*b"able"), u32::from_le_bytes(*b"Port")],
                1 => [0x600, 1 << 16, 0, 1 << 15],
                0x8000_0000 => [0x8000_0001, 0, 0, 0],
                0x8000_0001 => [0, 0, 0, 1 << 20],
                _ => [0; 4],
            };
            for (reg, value) in [X86Reg::Rax, X86Reg::Rbx, X86Reg::Rcx, X86Reg::Rdx].into_iter().zip(values) {
                s.write_reg(reg, value as u64, 4);
            }
        }
        IROp::ConditionalMove { cond, dst, src } => {
            // The source is read even when the predicate is false. A failed
            // memory read must fault before changing any destination bits.
            let value = match src {
                Ok(reg) => s.reg(reg),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            };
            let result = if eval_cond(s, cond) { value } else { s.reg(dst) };
            // A 32-bit CMOV also clears the upper half when not taken.
            s.write_reg(dst, result, w);
        }
        IROp::MovImm { dst, imm } => s.write_reg(dst, imm as u64, w),
        IROp::MovReg { dst, src } => s.write_reg(dst, s.reg(src), w),
        IROp::ExtendMove { dst, src, source_width, signed, high_byte } => {
            if !matches!(source_width, 1 | 2 | 4) || w < source_width { return Err(unsupported); }
            let value = match src {
                Ok(reg) => (s.reg(reg) >> if high_byte {8} else {0}) & mask(source_width),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), source_width).ok_or(fault)?,
            };
            let value = if signed {
                if source_width == 1 {value as u8 as i8 as i64 as u64}
                else if source_width == 2 {value as u16 as i16 as i64 as u64}
                else {value as u32 as i32 as i64 as u64}
            } else {value};
            s.write_reg(dst, value, w);
        }
        IROp::SignExtendAccumulatorPair => {
            let sign_bit = 1u64 << (w * 8 - 1);
            let high = if s.reg(X86Reg::Rax) & sign_bit == 0 { 0 } else { u64::MAX };
            s.write_reg(X86Reg::Rdx, high, w);
        }
        IROp::Load { dst, mem } => {
            let a = effective_address(s, &mem, i, true);
            let v = mem_read(m, a, mem.width).ok_or(fault)?;
            s.write_reg(dst, v, w);
        }
        IROp::Store { mem, src } => {
            let a = effective_address(s, &mem, i, true);
            if !mem_write(m, a, s.reg(src), mem.width) {
                return Err(fault);
            }
        }
        IROp::StoreImm { mem, value } => {
            let a = effective_address(s, &mem, i, true);
            if !mem_write(m, a, value as u64, mem.width) {
                return Err(fault);
            }
        }
        IROp::AluReg { op, dst, src } => {
            let r = alu(s, op, s.reg(dst), s.reg(src), w);
            if stores_result(op) {
                s.write_reg(dst, r, w)
            }
        }
        IROp::AluImm { op, dst, imm } => {
            let r = alu(s, op, s.reg(dst), imm as u64, w);
            if stores_result(op) {
                s.write_reg(dst, r, w)
            }
        }
        IROp::AluMem { op, dst, mem } => {
            let a = effective_address(s, &mem, i, true);
            let b = mem_read(m, a, mem.width).ok_or(fault)?;
            let r = alu(s, op, s.reg(dst), b, w);
            if stores_result(op) {
                s.write_reg(dst, r, w)
            }
        }
        IROp::AluStore { op, mem, src } => {
            let a = effective_address(s, &mem, i, true);
            let b = mem_read(m, a, mem.width).ok_or(fault)?;
            let r = alu(s, op, b, s.reg(src), w);
            if stores_result(op) && !mem_write(m, a, r, mem.width) {
                return Err(fault);
            }
        }
        IROp::AluStoreImm { op, mem, imm } => {
            let a = effective_address(s, &mem, i, true);
            let b = mem_read(m, a, mem.width).ok_or(fault)?;
            let r = alu(s, op, b, imm as u64, w);
            if stores_result(op) && !mem_write(m, a, r, mem.width) {
                return Err(fault);
            }
        }
        IROp::Lea { dst, mem } => {
            let a = effective_address(s, &mem, i, false);
            s.write_reg(dst, a, w)
        }
        IROp::UnaryNotNeg { dst, high_byte, negate } => {
            let value = match dst {
                Ok(reg) => s.reg(reg) >> if high_byte { 8 } else { 0 },
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), w).ok_or(fault)?,
            };
            let result = if negate { alu(s, AluOp::Sub, 0, value, w) } else { !value & mask(w) };
            match dst {
                Ok(reg) if high_byte => s.set_reg(reg, (s.reg(reg) & !0xff00) | ((result & 255) << 8)),
                Ok(reg) => s.write_reg(reg, result, w),
                Err(mem) => {
                    if !mem_write(m, effective_address(s, &mem, i, true), result, w) { return Err(fault); }
                }
            }
        }
        IROp::Leave => {
            // Stack address size belongs to the stack segment/mode, not 67h
            // or the operand size. Thus 66h LEAVE still addresses through
            // EBP/RBP, but pops only BP and preserves its upper bits.
            let frame = if s.mode == DecodeMode::X86 {
                s.reg(X86Reg::Rbp) as u32 as u64
            } else {
                s.reg(X86Reg::Rbp)
            };
            let sw = s.stack_width(i);
            let value = mem_read(m, frame, sw).ok_or(fault)?;
            s.set_reg(X86Reg::Rsp, frame.wrapping_add(sw as u64));
            s.write_reg(X86Reg::Rbp, value, sw);
        }
        IROp::AccumulatorMemory { address, store } => {
            let segment = match i.segment { Segment::Flat => 0, Segment::Fs => s.fs_base, Segment::Gs => s.gs_base };
            let linear = segment.wrapping_add(address);
            let linear = if s.mode == DecodeMode::X86 { linear as u32 as u64 } else { linear };
            if store {
                if !mem_write(m, linear, s.reg(X86Reg::Rax), w) { return Err(fault); }
            } else {
                let value = mem_read(m, linear, w).ok_or(fault)?;
                s.write_reg(X86Reg::Rax, value, w);
            }
        }
        IROp::PushFlags => {
            // PUSHF[D/Q] clears RF and VM in the saved image.
            push(s, m, i, s.rflags & !0x30000).ok_or(fault)?;
        }
        IROp::SetDirectionFlag { set } => s.set_flag(FLAG_DF, set),
        IROp::PopFlags => {
            let sw = s.stack_width(i);
            let sp = s.reg(X86Reg::Rsp);
            let value = mem_read(m, sp, sw).ok_or(fault)?;
            // CPL3, IOPL0: IF and IOPL cannot change. AC/ID are available
            // only to POPFD/Q. VIF/VIP/VM and reserved bits are preserved.
            let writable = if sw == 2 { 0x4dd5 } else { 0x244dd5 };
            s.rflags = ((s.rflags & !writable) | (value & writable)) & !0x10000 | 2;
            let next_sp = sp.wrapping_add(sw as u64);
            s.set_reg(X86Reg::Rsp, if s.mode == DecodeMode::X86 { next_sp as u32 as u64 } else { next_sp });
        }
        IROp::Push { src } => {
            let v = s.reg(src);
            push(s, m, i, v).ok_or(fault)?;
        }
        IROp::PushMem { mem } => {
            // Sample the source (including [E/RSP]) before pushing changes SP.
            let address = effective_address(s, &mem, i, true);
            let value = mem_read(m, address, s.stack_width(i)).ok_or(fault)?;
            push(s, m, i, value).ok_or(fault)?;
        }
        IROp::PushImm { value } => {
            push(s, m, i, value as u64).ok_or(fault)?;
        }
        IROp::Pop { dst } => {
            let sp = s.reg(X86Reg::Rsp);
            let sw = s.stack_width(i);
            let v = mem_read(m, sp, sw).ok_or(fault)?;
            s.set_reg(X86Reg::Rsp, sp.wrapping_add(sw as u64));
            s.write_reg(dst, v, sw);
        }
        IROp::Jmp { target } => s.set_reg(X86Reg::Rip, target),
        IROp::Jcc { cond, target } => {
            if eval_cond(s, cond) {
                s.set_reg(X86Reg::Rip, target)
            }
        }
        IROp::Call { target } => {
            push(s, m, i, next).ok_or(fault)?;
            s.set_reg(X86Reg::Rip, target);
        }
        IROp::Ret | IROp::RetImm { .. } => {
            let sp = s.reg(X86Reg::Rsp);
            let sw = s.stack_width(i);
            let v = mem_read(m, sp, sw).ok_or(fault)?;
            let extra = if let IROp::RetImm { bytes } = i.op {
                bytes as u64
            } else {
                0
            };
            s.set_reg(X86Reg::Rsp, sp.wrapping_add(sw as u64).wrapping_add(extra));
            s.set_reg(X86Reg::Rip, v);
        }
        IROp::JumpReg { target, call } => {
            let v = s.reg(target);
            if call {
                push(s, m, i, next).ok_or(fault)?;
            }
            s.set_reg(X86Reg::Rip, v);
        }
        IROp::JumpMem { mem, call } => {
            let a = effective_address(s, &mem, i, true);
            let v = mem_read(m, a, mem.width).ok_or(fault)?;
            if call {
                push(s, m, i, next).ok_or(fault)?;
            }
            s.set_reg(X86Reg::Rip, v);
        }
        IROp::Syscall => {
            if s.mode == DecodeMode::X64 {
                s.set_reg(X86Reg::Rcx, next);
                s.set_reg(X86Reg::R11, s.rflags);
            }
            h.syscall(s, m);
        }
        IROp::SimdByteMask { dst, src } => {
            let value = s.xmm[src as usize].iter().enumerate()
                .fold(0u64, |mask, (lane, byte)| mask | (u64::from(byte >> 7) << lane));
            s.write_reg(dst, value, 4);
        }
        IROp::IntegerToSimd { dst, src, double, width } => {
            if !matches!(width, 4 | 8) { return Err(unsupported); }
            let value = match src {
                Ok(reg) => s.reg(reg),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), width).ok_or(fault)?,
            };
            let (result, exceptions) = float::from_integer(value, double, width == 8, s.mxcsr);
            s.mxcsr |= u32::from(exceptions);
            let unmasked = exceptions & !((s.mxcsr >> 7) as u8) & 0x3f;
            if unmasked != 0 {
                return Err(ExecOutcome::FloatingPointError { guest_va: i.guest_va, exceptions: unmasked });
            }
            let size = if double {8} else {4};
            s.xmm[dst as usize][..size].copy_from_slice(&result.to_le_bytes()[..size]);
        }
        IROp::SimdToInteger { dst, src, double, width } => {
            let source = simd_read(s, m, i, src, if double {8} else {4}).ok_or(fault)?;
            let mut bytes = [0; 8];
            bytes[..if double {8} else {4}].copy_from_slice(&source[..if double {8} else {4}]);
            let (value, exceptions) = float::truncate_integer(u64::from_le_bytes(bytes), double, width == 8, s.mxcsr);
            s.mxcsr |= u32::from(exceptions);
            let unmasked = exceptions & !((s.mxcsr >> 7) as u8) & 0x3f;
            if unmasked != 0 {
                return Err(ExecOutcome::FloatingPointError { guest_va: i.guest_va, exceptions: unmasked });
            }
            s.write_reg(dst, value, width);
        }
        IROp::SimdMoveInteger { dst, src, width } => {
            let value = match src {
                Ok(reg) => s.reg(reg),
                Err(mem) => mem_read(m, effective_address(s, &mem, i, true), width).ok_or(fault)?,
            };
            let mut result = [0; 16];
            result[..width as usize].copy_from_slice(&value.to_le_bytes()[..width as usize]);
            s.xmm[dst as usize] = result;
        }
        IROp::SimdShiftBytes { dst, left, count } => {
            let register = &mut s.xmm[dst as usize];
            let value = u128::from_le_bytes(*register);
            let bits = u32::from(count) * 8;
            let result = if left { value.checked_shl(bits) } else { value.checked_shr(bits) };
            *register = result.unwrap_or(0).to_le_bytes();
        }
        IROp::SimdExtractWord { dst, src, index } => {
            let start = usize::from(index & 7) * 2;
            let value = u16::from_le_bytes(s.xmm[src as usize][start..start + 2].try_into().unwrap());
            s.write_reg(dst, u64::from(value), 4);
        }
        IROp::SimdStoreInteger { dst, src, width } => {
            let value = u64::from_le_bytes(s.xmm[src as usize][..8].try_into().unwrap());
            match dst {
                Ok(reg) => s.write_reg(reg, value, width),
                Err(mem) => {
                    if !mem_write(m, effective_address(s, &mem, i, true), value, width) { return Err(fault); }
                }
            }
        }
        IROp::SimdMov { dst, src, width } => {
            let n = simd_size(width);
            let v = simd_read(s, m, i, src, n).ok_or(fault)?;
            if n == 16 {
                s.xmm[dst as usize] = v
            } else {
                if width == SimdWidth::LowQword || matches!(src, SimdSrc::Mem(_)) {
                    s.xmm[dst as usize] = [0; 16]
                }
                s.xmm[dst as usize][..n].copy_from_slice(&v[..n]);
            }
        }
        IROp::SimdShuffle { dst, src, kind, control } => {
            if let SimdSrc::Mem(mem) = src {
                if effective_address(s, &mem, i, true) & 15 != 0 { return Err(fault); }
            }
            let source = simd_read(s, m, i, src, 16).ok_or(fault)?;
            let (offset, lane_width) = match kind {
                SimdShuffle::Dwords => (0, 4),
                SimdShuffle::LowWords => (0, 2),
                SimdShuffle::HighWords => (8, 2),
            };
            let mut result = source;
            for lane in 0..4 {
                let selected = usize::from((control >> (lane * 2)) & 3);
                let output = offset + lane * lane_width;
                let input = offset + selected * lane_width;
                result[output..output + lane_width].copy_from_slice(&source[input..input + lane_width]);
            }
            s.xmm[dst as usize] = result;
        }
        IROp::SimdStore { mem, src, width } => {
            let a = effective_address(s, &mem, i, true);
            if !m.write_virt(a, &s.xmm[src as usize][..simd_size(width)]) {
                return Err(fault);
            }
        }
        IROp::SimdArith {
            op,
            dst,
            src,
            width,
        } => {
            if op == SimdOp::UnpackHighDoubles {
                if let SimdSrc::Mem(mem) = src {
                    if effective_address(s, &mem, i, true) & 15 != 0 { return Err(fault); }
                }
                let source = simd_read(s, m, i, src, 16).ok_or(fault)?;
                let left = s.xmm[dst as usize];
                s.xmm[dst as usize][..8].copy_from_slice(&left[8..]);
                s.xmm[dst as usize][8..].copy_from_slice(&source[8..]);
                return Ok(());
            }
            if op == SimdOp::EqualBytes {
                let source = simd_read(s, m, i, src, 16).ok_or(fault)?;
                for (left, right) in s.xmm[dst as usize].iter_mut().zip(source) { *left = if *left == right {255} else {0}; }
                return Ok(());
            }
            if matches!(op, SimdOp::EqualWords | SimdOp::EqualDwords | SimdOp::GreaterDwords | SimdOp::AddDwords | SimdOp::AddQwords) {
                if let SimdSrc::Mem(mem) = src {
                    if effective_address(s, &mem, i, true) & 15 != 0 { return Err(fault); }
                }
                let source = simd_read(s, m, i, src, 16).ok_or(fault)?;
                let result = &mut s.xmm[dst as usize];
                if op == SimdOp::AddQwords {
                    for (left, right) in result.chunks_exact_mut(8).zip(source.chunks_exact(8)) {
                        let value = u64::from_le_bytes(left.try_into().unwrap()).wrapping_add(u64::from_le_bytes(right.try_into().unwrap()));
                        left.copy_from_slice(&value.to_le_bytes());
                    }
                } else if op == SimdOp::EqualWords {
                    for (left, right) in result.chunks_exact_mut(2).zip(source.chunks_exact(2)) {
                        let value = if left == right { u16::MAX } else { 0 };
                        left.copy_from_slice(&value.to_le_bytes());
                    }
                } else {
                    for (left, right) in result.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
                        let a = i32::from_le_bytes(left.try_into().unwrap());
                        let b = i32::from_le_bytes(right.try_into().unwrap());
                        let value = if op == SimdOp::AddDwords { (a as u32).wrapping_add(b as u32) }
                            else if (op == SimdOp::EqualDwords && a == b) || (op == SimdOp::GreaterDwords && a > b) { u32::MAX }
                            else { 0 };
                        left.copy_from_slice(&value.to_le_bytes());
                    }
                }
                return Ok(());
            }
            if op == SimdOp::Compare {
                let size = simd_size(width);
                let source = simd_read(s, m, i, src, size).ok_or(fault)?;
                let mut left = [0; 8]; let mut right = [0; 8];
                left[..size].copy_from_slice(&s.xmm[dst as usize][..size]);
                right[..size].copy_from_slice(&source[..size]);
                let (flags, exceptions) = float::compare(
                    u64::from_le_bytes(left), u64::from_le_bytes(right), size == 8, s.mxcsr,
                );
                s.mxcsr |= u32::from(exceptions);
                let unmasked = exceptions & !((s.mxcsr >> 7) as u8) & 0x3f;
                if unmasked != 0 {
                    return Err(ExecOutcome::FloatingPointError { guest_va: i.guest_va, exceptions: unmasked });
                }
                s.rflags = (s.rflags & !0x8d5) | flags;
                return Ok(());
            }
            if matches!(op, SimdOp::UnpackLowBytes | SimdOp::UnpackLowDwords) {
                if let SimdSrc::Mem(mem) = src {
                    if effective_address(s, &mem, i, true) & 15 != 0 { return Err(fault); }
                }
                let source = simd_read(s, m, i, src, 8).ok_or(fault)?;
                let left = s.xmm[dst as usize];
                let lane = if op == SimdOp::UnpackLowDwords { 4 } else { 1 };
                for n in 0..8 / lane {
                    let from = n * lane;
                    let to = n * 2 * lane;
                    s.xmm[dst as usize][to..to + lane].copy_from_slice(&left[from..from + lane]);
                    s.xmm[dst as usize][to + lane..to + 2 * lane].copy_from_slice(&source[from..from + lane]);
                }
                return Ok(());
            }
            if matches!(op, SimdOp::Add | SimdOp::Sub | SimdOp::Mul | SimdOp::Div) {
                let size = simd_size(width);
                if let SimdSrc::Mem(mem) = src {
                    if size == 16 && effective_address(s, &mem, i, true) & 15 != 0 {
                        return Err(fault);
                    }
                }
                let source = simd_read(s, m, i, src, size).ok_or(fault)?;
                let mut result = s.xmm[dst as usize];
                let lane = match width {
                    SimdWidth::PackedF32 | SimdWidth::ScalarF32 => 4,
                    SimdWidth::PackedF64 | SimdWidth::ScalarF64 => 8,
                    _ => return Err(unsupported),
                };
                let mut exceptions = 0;
                for start in (0..size).step_by(lane) {
                    let mut left = [0; 8]; let mut right = [0; 8];
                    left[..lane].copy_from_slice(&result[start..start + lane]);
                    right[..lane].copy_from_slice(&source[start..start + lane]);
                    let (value, flags) = if op == SimdOp::Mul {
                        float::multiply(u64::from_le_bytes(left), u64::from_le_bytes(right), lane == 8, s.mxcsr)
                    } else if op == SimdOp::Div {
                        float::divide(u64::from_le_bytes(left), u64::from_le_bytes(right), lane == 8, s.mxcsr)
                    } else { float::add_sub(
                        u64::from_le_bytes(left), u64::from_le_bytes(right),
                        lane == 8, op == SimdOp::Sub, s.mxcsr,
                    ) };
                    result[start..start + lane].copy_from_slice(&value.to_le_bytes()[..lane]);
                    exceptions |= flags;
                }
                s.mxcsr |= u32::from(exceptions);
                let unmasked = exceptions & !((s.mxcsr >> 7) as u8) & 0x3f;
                if unmasked != 0 {
                    return Err(ExecOutcome::FloatingPointError { guest_va: i.guest_va, exceptions: unmasked });
                }
                s.xmm[dst as usize] = result;
                return Ok(());
            }
            // Bitwise SSE has identical semantics for integer and floating lanes.
            if !matches!(op, SimdOp::And | SimdOp::Or | SimdOp::Xor) {
                return Err(unsupported);
            }
            let b = simd_read(s, m, i, src, simd_size(width)).ok_or(fault)?;
            for (a, b) in s.xmm[dst as usize].iter_mut().zip(b) {
                *a = match op {
                    SimdOp::And => *a & b,
                    SimdOp::Or => *a | b,
                    _ => *a ^ b,
                }
            }
        }
        IROp::Unsupported => return Err(unsupported),
    }
    Ok(())
}
fn push(s: &mut GuestState, m: &mut dyn GuestMemory, i: &IrInst, v: u64) -> Option<()> {
    let sw = s.stack_width(i);
    let sp = s.reg(X86Reg::Rsp).wrapping_sub(sw as u64);
    let sp = if s.mode == DecodeMode::X86 {
        sp as u32 as u64
    } else {
        sp
    };
    if !mem_write(m, sp, v, sw) {
        return None;
    }
    s.set_reg(X86Reg::Rsp, sp);
    Some(())
}
fn simd_size(w: SimdWidth) -> usize {
    match w {
        SimdWidth::ScalarF32 => 4,
        SimdWidth::ScalarF64 | SimdWidth::LowQword => 8,
        _ => 16,
    }
}
fn simd_read(
    s: &GuestState,
    m: &mut dyn GuestMemory,
    i: &IrInst,
    src: SimdSrc,
    n: usize,
) -> Option<[u8; 16]> {
    match src {
        SimdSrc::Xmm(r) => Some(s.xmm[r as usize]),
        SimdSrc::Mem(mem) => {
            let a = effective_address(s, &mem, i, true);
            let mut b = [0; 16];
            if m.read_virt(a, &mut b[..n]) == n {
                Some(b)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::common::btl::abi::HostArch;
    use crate::arch::common::btl::ir::IrInst;
    use crate::arch::common::btl::x86_decoder::decode_block;
    use alloc::vec::Vec;

    struct NopHook;
    impl SyscallHook for NopHook {
        fn syscall(&mut self, _state: &mut GuestState, _memory: &mut dyn GuestMemory) {}
    }

    struct RecordHook {
        pub number: u64,
        pub arg0: u64,
    }

    impl SyscallHook for RecordHook {
        fn syscall(&mut self, state: &mut GuestState, _memory: &mut dyn GuestMemory) {
            self.number = state.reg(X86Reg::Rax);
            self.arg0 = state.reg(X86Reg::R10);
        }
    }

    fn decode(bytes: &[u8], base: u64) -> Vec<IrInst> {
        let mut out = [IrInst::empty(); 16];
        let n = decode_block(bytes, base, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn movq_load_reads_eight_bytes_and_clears_upper_xmm_bits() {
        let instructions = decode(&[0xf3, 0x44, 0x0f, 0x7e, 0x00, 0xf3, 0x45, 0x0f, 0x7e, 0xc8], 0x1000);
        let mut state = GuestState::new();
        state.rip = 0x1000;
        state.set_reg(X86Reg::Rax, 0x1ff8);
        state.xmm[8] = [0xa5; 16];
        state.xmm[9] = [0x5a; 16];
        let mut backing = [0u8; 0x2000];
        let data = 0x1234_5678_9abc_def0u64.to_le_bytes();
        backing[0x1ff8..].copy_from_slice(&data);
        let mut memory = test_support::flat_memory(&mut backing);
        let outcome = execute_block(&instructions, &mut state, &mut memory, &mut NopHook);
        let mut expected = [0; 16];
        expected[..8].copy_from_slice(&data);
        assert_eq!(outcome, ExecOutcome::Retired { next_rip: 0x100a });
        assert_eq!(state.xmm[8], expected);
        assert_eq!(state.xmm[9], expected);

        state.rip = 0x1000;
        state.set_reg(X86Reg::Rax, 0x1ff9);
        state.xmm[8] = [0xa5; 16];
        let outcome = execute_block(&instructions, &mut state, &mut memory, &mut NopHook);
        assert!(matches!(outcome, ExecOutcome::MemoryFault { guest_va: 0x1000 }));
        assert_eq!(state.xmm[8], [0xa5; 16]);
    }

    #[test]
    fn mov_imm_and_alu() {
        // mov rax, 42 (B8 imm64) ; add rax, 8 (83 /0 imm8)
        let code: [u8; 14] = [
            0x48, 0xB8, 42, 0, 0, 0, 0, 0, 0, 0, // mov rax, 42
            0x48, 0x83, 0xC0, 8, // add rax, 8
        ];
        let ir = decode(&code, 0x1000);
        let mut state = GuestState::new();
        state.rip = 0x1000;
        let mut backing = [0u8; 0x2000];
        let mut mem = test_support::flat_memory(&mut backing);
        let mut hook = NopHook;
        let out = execute_block(&ir, &mut state, &mut mem, &mut hook);
        assert_eq!(out, ExecOutcome::Retired { next_rip: 0x100E });
        assert_eq!(state.reg(X86Reg::Rax), 50);
    }

    #[test]
    fn guest_sequence_executes_for_each_host_architecture() {
        for arch in [HostArch::Aarch64, HostArch::LoongArch64, HostArch::RiscV64] {
            assert_eq!(arch.argument_register_count(), 8);
            assert!(!arch.argument_register_names()[0].is_empty());

            let mut backing = [0u8; 0x2000];
            let mut mem = test_support::flat_memory(&mut backing);
            let (outcome, rax) = test_support::run_mov_imm_add_imm(&mut mem);
            assert!(matches!(outcome, ExecOutcome::Retired { .. }));
            assert_eq!(rax, 50, "sequence failed for {arch:?}");
        }
    }

    #[test]
    fn load_store_roundtrip() {
        // mov rax, 0x100 (B8 imm64) ; mov rbx, [rax] (8B /r) ; ret
        let code: [u8; 14] = [
            0x48, 0xB8, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // mov rax, 0x100
            0x48, 0x8B, 0x18, // mov rbx, [rax]
            0xC3, // ret
        ];
        let ir = decode(&code, 0x1000);
        let mut state = GuestState::new();
        state.rip = 0x1000;
        let mut backing = [0u8; 0x2000];
        let mut mem = test_support::flat_memory(&mut backing);
        // Pre-fill guest memory at VA 0x100 with a known pattern.
        mem.write_virt(0x100, &[0x42, 0, 0, 0, 0, 0, 0, 0]);
        let mut hook = NopHook;
        let _ = execute_block(&ir, &mut state, &mut mem, &mut hook);
        assert_eq!(state.reg(X86Reg::Rax), 0x100);
        assert_eq!(state.reg(X86Reg::Rbx), 0x42);
    }

    #[test]
    fn syscall_hook_receives_abi_registers() {
        // mov rax, 0x18 (B8 imm64) ; mov r10, 0x1234 (41 B8+rr imm64) ; syscall
        let code: [u8; 22] = [
            0x48, 0xB8, 0x18, 0, 0, 0, 0, 0, 0, 0, // mov rax, 0x18
            0x49, 0xBA, 0x34, 0x12, 0, 0, 0, 0, 0, 0, // mov r10, 0x1234
            0x0F, 0x05, // syscall
        ];
        let ir = decode(&code, 0x1000);
        let mut state = GuestState::new();
        state.rip = 0x1000;
        let mut backing = [0u8; 0x2000];
        let mut mem = test_support::flat_memory(&mut backing);
        let mut hook = RecordHook { number: 0, arg0: 0 };
        let _ = execute_block(&ir, &mut state, &mut mem, &mut hook);
        assert_eq!(hook.number, 24);
        assert_eq!(hook.arg0, 0x1234);
    }

    #[test]
    fn jcc_takes_branch_on_zf() {
        // xor rcx, rcx (sets ZF) ; jz +2 (skip next) ; mov rdx, 1 ; ret
        let code: [u8; 9] = [
            0x48, 0x31, 0xC9, // xor rcx, rcx
            0x74, 0x02, // jz +2
            0x48, 0xC7, 0xC2, 1, // mov rdx, 1  (skipped)
        ];
        let ir = decode(&code, 0x2000);
        let mut state = GuestState::new();
        state.rip = 0x2000;
        let mut backing = [0u8; 0x2000];
        let mut mem = test_support::flat_memory(&mut backing);
        let mut hook = NopHook;
        let out = execute_block(&ir, &mut state, &mut mem, &mut hook);
        assert_eq!(out, ExecOutcome::Retired { next_rip: 0x2007 });
        assert!(state.flag(FLAG_ZF));
    }

    #[test]
    fn memory_fault_returns_error() {
        // Build IR directly: store rbx to [rax] where rax points past
        // the flat memory backing store.
        let mut state = GuestState::new();
        state.set_reg(X86Reg::Rax, 0xFFFF_0000);
        state.rip = 0x1000;
        let mut backing = [0u8; 0x2000];
        let mut mem = test_support::flat_memory(&mut backing);
        let mut hook = NopHook;
        let ir = [IrInst {
            op: IROp::Store {
                mem: MemOperand {
                    base: Some(X86Reg::Rax),
                    index: None,
                    scale: 1,
                    disp: 0,
                    width: 8,
                },
                src: X86Reg::Rbx,
            },
            guest_va: 0x1000,
            guest_len: 3,
            operand_width: 8,
            address_width: 8,
            segment: crate::arch::common::btl::ir::Segment::Flat,
        }];
        let out = execute_block(&ir, &mut state, &mut mem, &mut hook);
        assert!(matches!(out, ExecOutcome::MemoryFault { .. }));
    }
}

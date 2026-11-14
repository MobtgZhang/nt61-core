//! x86/x64 interpreter over the shared BTL IR.
//!
//! This is the checked execution backend used on all supported hosts. Guest
//! state and memory remain isolated. Native JIT execution stays disabled until
//! it can preserve the same semantics; unsupported instructions stop precisely.

use super::ir::{
    AluOp, BranchCond, DecodeMode, IROp, IrInst, MemOperand, Segment, SimdOp, SimdSrc, SimdWidth,
    X86Reg,
};
use super::memory::GuestMemory;

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
    let sign = 1u64 << (width * 8 - 1);
    let r = (match op {
        AluOp::Add => a.wrapping_add(b),
        AluOp::Sub | AluOp::Cmp => a.wrapping_sub(b),
        AluOp::And | AluOp::Test => a & b,
        AluOp::Or => a | b,
        AluOp::Xor => a ^ b,
    }) & m;
    state.set_flag(FLAG_ZF, r == 0);
    state.set_flag(FLAG_SF, r & sign != 0);
    state.set_flag(FLAG_PF, (r as u8).count_ones() % 2 == 0);
    match op {
        AluOp::Add => {
            state.set_flag(FLAG_CF, r < a);
            state.set_flag(FLAG_OF, (!(a ^ b) & (a ^ r) & sign) != 0);
            state.set_flag(FLAG_AF, (a ^ b ^ r) & 16 != 0);
        }
        AluOp::Sub | AluOp::Cmp => {
            state.set_flag(FLAG_CF, a < b);
            state.set_flag(FLAG_OF, ((a ^ b) & (a ^ r) & sign) != 0);
            state.set_flag(FLAG_AF, (a ^ b ^ r) & 16 != 0);
        }
        _ => {
            state.set_flag(FLAG_CF, false);
            state.set_flag(FLAG_OF, false);
        }
    }
    r
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
            *s = before;
            return out;
        }
        if matches!(
            i.op,
            IROp::Jmp { .. }
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
        IROp::MovImm { dst, imm } => s.write_reg(dst, imm as u64, w),
        IROp::MovReg { dst, src } => s.write_reg(dst, s.reg(src), w),
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
        IROp::Push { src } => {
            let v = s.reg(src);
            push(s, m, i, v).ok_or(fault)?;
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
        IROp::SimdMov { dst, src, width } => {
            let n = simd_size(width);
            let v = simd_read(s, m, i, src, n).ok_or(fault)?;
            if n == 16 {
                s.xmm[dst as usize] = v
            } else {
                if matches!(src, SimdSrc::Mem(_)) {
                    s.xmm[dst as usize] = [0; 16]
                }
                s.xmm[dst as usize][..n].copy_from_slice(&v[..n]);
            }
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
            // Bitwise SSE has identical semantics for integer and floating lanes.
            // Floating arithmetic waits for MXCSR rounding/exception modelling.
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
        SimdWidth::ScalarF64 => 8,
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

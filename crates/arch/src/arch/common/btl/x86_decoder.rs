//! Shared x86_64 instruction decoder producing IR.
//!
//! Decodes the hot subset of x86_64 instructions that PE startup code
//! and simple drivers use: MOV variants, PUSH/POP, CALL/RET, ALU ops
//! (ADD/SUB/AND/OR/XOR/CMP/TEST), JMP/Jcc, LEA, NOP, and SYSCALL.
//! Everything else yields IROp::Unsupported which the BTL runtime
//! handles by trapping back to the kernel.

use super::ir::{
    AluOp, BitAction, BranchCond, ByteOperand, DecodeMode, IROp, IrInst, LockedLogicSource, MemOperand, Segment, SimdOp, SimdShuffle, SimdSrc, SimdWidth,
    X86Reg, X86Xmm,
};

/// Maximum bytes per guest basic block.
pub const MAX_BLOCK_BYTES: usize = 1024;

/// Maximum decoded instructions per basic block.
pub const MAX_DECODED_INSTS: usize = 128;

/// REX prefix byte value (0x40..=0x4F).
const REX_PREFIX: u8 = 0x40;
/// REX.W bit - operand size 64-bit.
const REX_W: u8 = 0x08;
/// REX.R bit - ModR/M reg field high bit.
const REX_R: u8 = 0x04;
/// REX.X bit - SIB index field high bit.
const REX_X: u8 = 0x02;
/// REX.B bit - ModR/M r/m / SIB base high bit.
const REX_B: u8 = 0x01;

/// Legacy prefix: operand-size override (0x66).
const PREFIX_OPSIZE: u8 = 0x66;
/// Legacy prefix: address-size override (0x67).
const PREFIX_ADDRSIZE: u8 = 0x67;
/// Legacy segment override prefixes: ES, CS, SS, DS, FS, GS.
const PREFIX_SEGMENT_ES: u8 = 0x26;
const PREFIX_SEGMENT_CS: u8 = 0x2E;
const PREFIX_SEGMENT_SS: u8 = 0x36;
const PREFIX_SEGMENT_DS: u8 = 0x3E;
const PREFIX_SEGMENT_FS: u8 = 0x64;
const PREFIX_SEGMENT_GS: u8 = 0x65;

/// One-byte opcode map for ALU operations with ModR/M byte.
const OPCODE_ADD_RM_R: u8 = 0x01;
const OPCODE_ADD_R_RM: u8 = 0x03;
const OPCODE_SUB_RM_R: u8 = 0x29;
const OPCODE_SUB_R_RM: u8 = 0x2B;
const OPCODE_AND_RM_R: u8 = 0x21;
const OPCODE_AND_R_RM: u8 = 0x23;
const OPCODE_OR_RM_R: u8 = 0x09;
const OPCODE_OR_R_RM: u8 = 0x0B;
const OPCODE_XOR_RM_R: u8 = 0x31;
const OPCODE_XOR_R_RM: u8 = 0x33;
const OPCODE_CMP_RM_R: u8 = 0x39;
const OPCODE_CMP_R_RM: u8 = 0x3B;
const OPCODE_TEST_RM_R: u8 = 0x85;

/// ALU group opcode base (opcode byte = base + alu_index).
const OPCODE_ALU_IMM_BASE: u8 = 0x80;
/// ALU group: r/m, imm32 (0x81).
const OPCODE_ALU_IMM32: u8 = 0x81;
/// ALU group: r/m, imm8 sign-extended (0x83).
const OPCODE_ALU_IMM8: u8 = 0x83;

// ---- SIMD constants (Intel SDM Vol. 2A) ----
/// VEX 3-byte prefix opcode.
const VEX_PREFIX_3B: u8 = 0xC4;
/// VEX 2-byte prefix opcode.
const VEX_PREFIX_2B: u8 = 0xC5;
/// EVEX prefix opcode byte (0x62) for AVX-512 encoded instructions.
const EVEX_PREFIX: u8 = 0x62;
/// VEX 3-byte prefix: escape opcode byte (0x0F / 0x0F38 / 0x0F3A).
/// SSE: MOVAPS (aligned packed f32 move).
const SSE_MOVAPS: u8 = 0x28;
/// SSE: MOVAPS store.
const SSE_MOVAPS_STORE: u8 = 0x29;
/// SSE: MOVUPS (unaligned packed move, same as movaps with F3 prefix).
const SSE_MOVUPS: u8 = 0x10;
/// SSE: MOVUPS store.
const SSE_MOVUPS_STORE: u8 = 0x11;
/// SSE: MOVDQA (aligned packed integer move) with 66 prefix.
const SSE_MOVDQA: u8 = 0x6F;
/// SSE: MOVDQA store with 66 prefix.
const SSE_MOVDQA_STORE: u8 = 0x7F;
/// SSE: ADDPS / ADDSS / ADDPD / ADDSD.
const SSE_ADD: u8 = 0x58;
/// SSE: SUBPS / SUBSS / SUBPD / SUBSD.
const SSE_SUB: u8 = 0x5C;
/// SSE: MULPS / MULSS / MULPD / MULSD.
const SSE_MUL: u8 = 0x59;
/// SSE: DIVPS / DIVSS / DIVPD / DIVSD.
const SSE_DIV: u8 = 0x5E;
/// SSE: ANDPS.
const SSE_ANDPS: u8 = 0x54;
/// SSE: ORPS.
const SSE_ORPS: u8 = 0x56;
/// SSE: XORPS.
const SSE_XORPS: u8 = 0x57;
/// SSE: PAND (66 prefix).
const SSE_PAND: u8 = 0xDB;
/// SSE: POR (66 prefix).
const SSE_POR: u8 = 0xEB;
/// SSE: PXOR (66 prefix).
const SSE_PXOR: u8 = 0xEF;
/// ALU group: ADD r/m, imm8/32 (sign-extended).
const OPCODE_ALU_ADD_IMM: u8 = 0x00;
/// ALU group: SUB r/m, imm8/32 (sign-extended).
const OPCODE_ALU_SUB_IMM: u8 = 0x05;
/// ALU group: AND r/m, imm8/32 (sign-extended).
const OPCODE_ALU_AND_IMM: u8 = 0x04;
/// ALU group: OR r/m, imm8/32 (sign-extended).
const OPCODE_ALU_OR_IMM: u8 = 0x01;
/// ALU group: XOR r/m, imm8/32 (sign-extended).
const OPCODE_ALU_XOR_IMM: u8 = 0x06;
/// ALU group: CMP r/m, imm8/32 (sign-extended).
const OPCODE_ALU_CMP_IMM: u8 = 0x07;

/// Short-form ALU imm8 opcodes: ADD(0x04), SUB(0x2C), AND(0x24),
/// OR(0x0C), XOR(0x34), CMP(0x3C) - all on RAX.
const OPCODE_ADD_AL_IMM8: u8 = 0x04;
const OPCODE_ADD_AL_IMM32: u8 = 0x05;
const OPCODE_SUB_AL_IMM8: u8 = 0x2C;
const OPCODE_SUB_AL_IMM32: u8 = 0x2D;
const OPCODE_AND_AL_IMM8: u8 = 0x24;
const OPCODE_AND_AL_IMM32: u8 = 0x25;
const OPCODE_OR_AL_IMM8: u8 = 0x0C;
const OPCODE_OR_AL_IMM32: u8 = 0x0D;
const OPCODE_XOR_AL_IMM8: u8 = 0x34;
const OPCODE_XOR_AL_IMM32: u8 = 0x35;
const OPCODE_CMP_AL_IMM8: u8 = 0x3C;
const OPCODE_CMP_AL_IMM32: u8 = 0x3D;

/// MOV r64, imm64 (REX.W B8+rr opcode).
const OPCODE_MOV_R_IMM64: u8 = 0xB8;
/// MOV r64, r/m64 (REX.W 89 /r).
const OPCODE_MOV_RM_R: u8 = 0x89;
/// MOV r/m64, r64 (REX.W 8B /r).
const OPCODE_MOV_R_RM: u8 = 0x8B;
/// MOV moffs64, rax (REX.W A3).
const OPCODE_MOV_MOFFS_RAX: u8 = 0xA3;
/// MOV rax, moffs64 (REX.W A1).
const OPCODE_MOV_RAX_MOFFS: u8 = 0xA1;
/// MOV r/m64, imm32 (REX.W C7 /0).
const OPCODE_MOV_RM_IMM: u8 = 0xC7;
/// MOV r8, imm8 (B0+rr).
const OPCODE_MOV_R8_IMM8: u8 = 0xB0;
/// LEA r64, m (REX.W 8D /r).
const OPCODE_LEA: u8 = 0x8D;
/// PUSH r64 (REX.W 50+rr / 50..57).
const OPCODE_PUSH_R: u8 = 0x50;
/// POP r64 (REX.W 58+rr / 58..5F).
const OPCODE_POP_R: u8 = 0x58;
/// CALL rel32 (E8).
const OPCODE_CALL_REL32: u8 = 0xE8;
/// JMP rel32 (E9).
const OPCODE_JMP_REL32: u8 = 0xE9;
/// JMP rel8 (EB).
const OPCODE_JMP_REL8: u8 = 0xEB;
/// RET near (C3).
const OPCODE_RET_NEAR: u8 = 0xC3;
/// NOP (0x90).
const OPCODE_NOP: u8 = 0x90;
/// SYSCALL (0F 05).
const OPCODE_SYSCALL_HI: u8 = 0x0F;
const OPCODE_SYSCALL_LO: u8 = 0x05;
const OPCODE_JCC_REL32_BASE: u8 = 0x80;
/// Jcc rel8 (7x).
const OPCODE_JCC_REL8_BASE: u8 = 0x70;

/// Decoder state - consumes bytes from the guest code stream.
struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
    base_va: u64,
    has_rex: bool,
    rex_w: bool,
    rex_r: bool,
    rex_x: bool,
    rex_b: bool,
    op_size_64: bool,
    addr_size_64: bool,
    has_66: bool,
    has_f3: bool,
    has_f2: bool,
    has_lock: bool,
    /// EVEX R' extension: extends register index from XMM0-15 to XMM16-31.
    evex_reg_ext: bool,
    mode: DecodeMode,
    segment: Segment,
    byte_operand: bool,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8], base_va: u64, mode: DecodeMode) -> Self {
        Self {
            bytes,
            offset: 0,
            base_va,
            has_rex: false,
            rex_w: false,
            rex_r: false,
            rex_x: false,
            rex_b: false,
            op_size_64: false,
            addr_size_64: true,
            has_66: false,
            has_f3: false,
            has_f2: false,
            has_lock: false,
            evex_reg_ext: false,
            mode,
            segment: Segment::Flat,
            byte_operand: false,
        }
    }

    fn operand_width(&self) -> u8 {
        if self.byte_operand {
            1
        } else if self.rex_w {
            8
        } else if self.has_66 {
            2
        } else {
            4
        }
    }
    fn address_width(&self) -> u8 {
        if self.addr_size_64 {
            8
        } else {
            4
        }
    }
    fn fetch_imm(&mut self) -> Option<i64> {
        if self.operand_width() == 2 {
            Some(i16::from_le_bytes([self.fetch()?, self.fetch()?]) as i64)
        } else {
            Some(self.fetch_le32()? as i64)
        }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn fetch(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.offset).copied()?;
        self.offset += 1;
        Some(b)
    }

    fn fetch_le32(&mut self) -> Option<i32> {
        let b = [self.fetch()?, self.fetch()?, self.fetch()?, self.fetch()?];
        Some(i32::from_le_bytes(b))
    }

    fn fetch_le64(&mut self) -> Option<i64> {
        let mut b = [0u8; 8];
        for byte in b.iter_mut() {
            *byte = self.fetch()?;
        }
        Some(i64::from_le_bytes(b))
    }

    fn current_va(&self) -> u64 {
        self.base_va.wrapping_add(self.offset as u64)
    }

    fn current_va_at_start(&self, start: usize) -> u64 {
        self.base_va.wrapping_add(start as u64)
    }

    /// Consume legacy prefixes and REX prefix.
    fn consume_prefixes(&mut self) {
        self.has_rex = false;
        self.rex_w = false;
        self.rex_r = false;
        self.rex_x = false;
        self.rex_b = false;
        self.op_size_64 = false;
        self.segment = Segment::Flat;
        self.byte_operand = false;
        self.addr_size_64 = self.mode == DecodeMode::X64;
        self.has_66 = false;
        self.has_f3 = false;
        self.has_f2 = false;
        self.has_lock = false;
        loop {
            match self.peek() {
                Some(0xf0) => { self.has_lock = true; self.offset += 1; }
                Some(PREFIX_OPSIZE) => {
                    self.op_size_64 = false;
                    self.has_66 = true;
                    self.offset += 1;
                }
                Some(0xF3) => {
                    self.has_f3 = true;
                    self.offset += 1;
                }
                Some(0xF2) => {
                    self.has_f2 = true;
                    self.offset += 1;
                }
                Some(PREFIX_ADDRSIZE) => {
                    self.addr_size_64 = false;
                    self.offset += 1;
                }
                Some(PREFIX_SEGMENT_ES)
                | Some(PREFIX_SEGMENT_CS)
                | Some(PREFIX_SEGMENT_SS)
                | Some(PREFIX_SEGMENT_DS) => {
                    self.offset += 1;
                }
                Some(PREFIX_SEGMENT_FS) => {
                    self.segment = Segment::Fs;
                    self.offset += 1;
                }
                Some(PREFIX_SEGMENT_GS) => {
                    self.segment = Segment::Gs;
                    self.offset += 1;
                }
                Some(b) if self.mode == DecodeMode::X64 && (REX_PREFIX..=0x4F).contains(&b) => {
                    self.has_rex = true;
                    self.rex_w = b & REX_W != 0;
                    self.rex_r = b & REX_R != 0;
                    self.rex_x = b & REX_X != 0;
                    self.rex_b = b & REX_B != 0;
                    self.op_size_64 = self.rex_w;
                    self.offset += 1;
                }
                _ => break,
            }
        }
    }

    /// Decode the ModR/M byte, returning (reg_field, rm_info, consumed).
    fn decode_modrm(&mut self) -> Option<(X86Reg, Result<X86Reg, MemOperand>, u8)> {
        let modrm = self.fetch()?;
        let mode = (modrm >> 6) & 0x03;
        let reg_field = (modrm >> 3) & 0x07;
        let rm_field = modrm & 0x07;
        let reg = X86Reg::from_modrm(reg_field, self.rex_r);
        let width = self.operand_width();

        match mode {
            0x03 => {
                let rm = X86Reg::from_modrm(rm_field, self.rex_b);
                Some((reg, Ok(rm), 1))
            }
            _ => {
                let (base_reg, index_reg, scale, extra_disp) = if rm_field == 0x04 {
                    let sib = self.fetch()?;
                    let sib_scale_code = (sib >> 6) & 0x03;
                    let sib_index = (sib >> 3) & 0x07;
                    let sib_base = sib & 0x07;
                    let scale = 1u8 << sib_scale_code;
                    let idx = if sib_index == 0x04 && !self.rex_x {
                        None
                    } else {
                        Some(X86Reg::from_modrm(sib_index, self.rex_x))
                    };
                    if sib_base == 0x05 && mode == 0x00 {
                        (None, idx, scale, true)
                    } else {
                        (
                            Some(X86Reg::from_modrm(sib_base, self.rex_b)),
                            idx,
                            scale,
                            false,
                        )
                    }
                } else if rm_field == 0x05 && mode == 0x00 {
                    (
                        if self.addr_size_64 {
                            Some(X86Reg::Rip)
                        } else {
                            None
                        },
                        None,
                        1,
                        true,
                    )
                } else {
                    (
                        Some(X86Reg::from_modrm(rm_field, self.rex_b)),
                        None,
                        1,
                        false,
                    )
                };

                let disp = match mode {
                    0x00 => {
                        if extra_disp {
                            self.fetch_le32()?
                        } else {
                            0
                        }
                    }
                    0x01 => i32::from(self.fetch()? as i8),
                    0x02 => self.fetch_le32()?,
                    _ => 0,
                };

                Some((
                    reg,
                    Err(MemOperand {
                        base: base_reg,
                        index: index_reg,
                        scale,
                        disp,
                        width,
                    }),
                    1,
                ))
            }
        }
    }

    fn decode_alu_rm_r(&mut self, op: AluOp) -> Option<IROp> {
        let (src, dst, _) = self.decode_modrm()?;
        if self.has_lock {
            return Some(match (op, dst) {
                (AluOp::Or | AluOp::And | AluOp::Xor, Err(mem)) => IROp::LockedLogic {
                    op, mem, src: LockedLogicSource::Register { reg: src, high_byte: false },
                },
                _ => IROp::Unsupported,
            });
        }
        Some(match dst {
            Ok(dst) => IROp::AluReg { op, dst, src },
            Err(mem) => IROp::AluStore { op, mem, src },
        })
    }
    fn decode_alu_r_rm(&mut self, op: AluOp) -> Option<IROp> {
        let (dst, src, _) = self.decode_modrm()?;
        Some(match src {
            Ok(src) => IROp::AluReg { op, dst, src },
            Err(mem) => IROp::AluMem { op, dst, mem },
        })
    }

    /// Decode one instruction and return the IR op.
    fn decode_byte_operands(&mut self) -> Option<(ByteOperand, ByteOperand)> {
        let raw = self.peek()?;
        let (_, rm, _) = self.decode_modrm()?;
        let register = |field, extension| {
            let high_byte = !self.has_rex && field >= 4;
            ByteOperand::Register {
                reg: X86Reg::from_modrm(if high_byte {field - 4} else {field}, extension), high_byte,
            }
        };
        let reg = register((raw >> 3) & 7, self.rex_r);
        let rm = match rm { Ok(_) => register(raw & 7, self.rex_b), Err(mem) => ByteOperand::Memory(mem) };
        Some((reg, rm))
    }

    fn decode_one(&mut self) -> Option<IROp> {
        let start = self.offset;
        self.consume_prefixes();
        if self.offset - start >= 15 {
            return Some(IROp::Unsupported);
        }
        let op = self.fetch()?;
        // Only the implemented atomic RMW operations may consume LOCK.
        if self.has_lock && !matches!(op, 0x08 | 0x20 | 0x30 | OPCODE_OR_RM_R | OPCODE_AND_RM_R
            | OPCODE_XOR_RM_R | 0x0f | 0x80 | 0x81 | 0x83 | 0xfe | 0xff) {
            return Some(IROp::Unsupported);
        }
        // 16-bit addressing is deliberately rejected until its BX/SI forms exist.
        if self.mode == DecodeMode::X86 && self.bytes[start..self.offset].contains(&0x67)
            // LEAVE has no explicit address operand: 67h does not change its
            // stack segment address size, so no 16-bit ModRM support is needed.
            && op != 0xc9
        {
            return Some(IROp::Unsupported);
        }

        // REP/REPNZ are not interchangeable with ordinary integer semantics.
        // With MPX absent, F2 on near control transfers is an ignored BND
        // prefix. REPNE on ordinary integer operations stays unsupported.
        let bnd_control = self.has_f2 && !self.has_f3
            && matches!(op, 0x70..=0x7f | 0xc2 | 0xc3 | 0xe8 | 0xe9 | 0xeb | 0xff);
        if (self.has_f2 || self.has_f3) && !bnd_control && !matches!(op, 0x0f | 0x90 | 0xa4 | 0xa5 | 0xaa | 0xab | 0xae | 0xaf | OPCODE_RET_NEAR) {
            return Some(IROp::Unsupported);
        }
        if op == OPCODE_SYSCALL_HI {
            let op2 = self.fetch()?;
            if self.has_lock && !matches!(op2, 0xb0 | 0xb1 | 0xc0 | 0xc1) { return Some(IROp::Unsupported); }
            return match op2 {
                0xa4 | 0xa5 | 0xac | 0xad if !self.has_f2 && !self.has_f3 => {
                    let (src, dst, _) = self.decode_modrm()?;
                    let count = if op2 & 1 == 0 { Some(self.fetch()?) } else { None };
                    Some(IROp::DoubleShift { dst, src, right: op2 & 8 != 0, count })
                }
                0x74 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::EqualBytes),
                0x76 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::EqualDwords),
                0x75 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::EqualWords),
                0x66 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::GreaterDwords),
                0xd4 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::AddQwords),
                0xfe if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::AddDwords),
                0xc5 if self.has_66 && !self.has_f2 && !self.has_f3 => {
                    let raw = self.fetch()?;
                    if raw >> 6 != 3 { return Some(IROp::Unsupported); }
                    let index = self.fetch()?;
                    Some(IROp::SimdExtractWord { dst: X86Reg::from_modrm((raw >> 3) & 7, self.rex_r), src: X86Xmm::from_modrm(raw & 7, self.rex_b), index })
                }
                0x7e if self.has_66 && !self.has_f2 && !self.has_f3 => {
                    let raw = self.peek()?;
                    let (_, mut dst, _) = self.decode_modrm()?;
                    let width = if self.rex_w { 8 } else { 4 };
                    if let Err(ref mut mem) = dst { mem.width = width; }
                    Some(IROp::SimdStoreInteger { dst, src: X86Xmm::from_modrm((raw >> 3) & 7, self.rex_r), width })
                }
                0xd7 if self.has_66 && !self.has_f2 && !self.has_f3 => {
                    let raw = self.fetch()?;
                    if raw >> 6 != 3 { return Some(IROp::Unsupported); }
                    Some(IROp::SimdByteMask { dst: X86Reg::from_modrm((raw >> 3) & 7, self.rex_r), src: X86Xmm::from_modrm(raw & 7, self.rex_b) })
                }
                0x70 => {
                    let kind = match (self.has_66, self.has_f2, self.has_f3) {
                        (true, false, false) => SimdShuffle::Dwords,
                        (false, true, false) => SimdShuffle::LowWords,
                        (false, false, true) => SimdShuffle::HighWords,
                        _ => return Some(IROp::Unsupported),
                    };
                    let raw = self.peek()?;
                    let (_, operand, _) = self.decode_modrm()?;
                    let src = match operand {
                        Ok(_) => SimdSrc::Xmm(X86Xmm::from_modrm(raw & 7, self.rex_b)),
                        Err(mut mem) => { mem.width = 16; SimdSrc::Mem(mem) },
                    };
                    let control = self.fetch()?;
                    Some(IROp::SimdShuffle {
                        dst: X86Xmm::from_modrm((raw >> 3) & 7, self.rex_r), src, kind, control,
                    })
                }
                0x73 if self.has_66 && !self.has_f2 && !self.has_f3 => {
                    let raw = self.fetch()?;
                    let group = (raw >> 3) & 7;
                    if raw >> 6 != 3 || !matches!(group, 3 | 7) {
                        return Some(IROp::Unsupported);
                    }
                    Some(IROp::SimdShiftBytes {
                        dst: X86Xmm::from_modrm(raw & 7, self.rex_b),
                        left: group == 7,
                        count: self.fetch()?,
                    })
                }
                0x6e if self.has_66 && !self.has_f2 && !self.has_f3 => {
                    let raw = self.peek()?;
                    let (_, mut src, _) = self.decode_modrm()?;
                    let width = if self.rex_w { 8 } else { 4 };
                    if let Err(ref mut mem) = src { mem.width = width; }
                    Some(IROp::SimdMoveInteger { dst: X86Xmm::from_modrm((raw >> 3) & 7, self.rex_r), src, width })
                }
                0x60 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::UnpackLowBytes),
                0x62 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::UnpackLowDwords),
                0x15 if self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::UnpackHighDoubles),
                0x14 if !self.has_66 && !self.has_f2 && !self.has_f3 => self.decode_simd_arith(SimdOp::UnpackLowDwords),
                0x2e if !self.has_f2 && !self.has_f3 => {
                    let mut operation = self.decode_simd_arith(SimdOp::Compare)?;
                    if let IROp::SimdArith { ref mut width, .. } = operation {
                        *width = if self.has_66 {SimdWidth::ScalarF64} else {SimdWidth::ScalarF32};
                    }
                    Some(operation)
                }
                0xb0 | 0xb1 | 0xc0 | 0xc1 if !self.has_f2 && !self.has_f3 => {
                    self.byte_operand = op2 & 1 == 0;
                    let raw = self.peek()?;
                    let (mut src, mut dst, _) = self.decode_modrm()?;
                    let legacy_byte = self.byte_operand && !self.has_rex;
                    let src_high_byte = legacy_byte && (raw >> 3) & 7 >= 4;
                    let dst_high_byte = legacy_byte && dst.is_ok() && raw & 7 >= 4;
                    if src_high_byte {
                        src = X86Reg::from_modrm(((raw >> 3) & 7) - 4, false);
                    }
                    if dst_high_byte {
                        dst = Ok(X86Reg::from_modrm((raw & 7) - 4, false));
                    }
                    let locked = self.has_lock;
                    Some(if locked && dst.is_ok() {
                        IROp::Unsupported
                    } else if op2 < 0xc0 {
                        IROp::CompareExchange { dst, src, dst_high_byte, src_high_byte, locked }
                    } else {
                        IROp::ExchangeAdd { dst, src, dst_high_byte, src_high_byte, locked }
                    })
                }
                0x1f if !self.has_f2 && !self.has_f3 => {
                    if (self.peek()? >> 3) & 7 != 0 { return Some(IROp::Unsupported); }
                    // Multi-byte NOP consumes a ModRM/SIB/displacement but
                    // never evaluates the effective address or accesses it.
                    let _ = self.decode_modrm()?;
                    Some(IROp::Nop)
                }
                0xa2 if !self.has_f2 && !self.has_f3 => Some(IROp::CpuId),
                0xbc | 0xbd if !self.has_f2 => {
                    // F3 is a backwards-compatible hint on the virtual CPU:
                    // CPUID exposes neither BMI1 (TZCNT) nor ABM (LZCNT).
                    let (dst, src, _) = self.decode_modrm()?;
                    Some(IROp::BitScan { dst, src, reverse: op2 == 0xbd })
                }
                0xa3 | 0xab | 0xb3 | 0xbb if !self.has_f2 && !self.has_f3 => {
                    let (index, dst, _) = self.decode_modrm()?;
                    let action = match op2 {
                        0xa3 => BitAction::Test, 0xab => BitAction::Set,
                        0xb3 => BitAction::Reset, _ => BitAction::Complement,
                    };
                    Some(IROp::BitRegister { dst, index, action })
                }
                0x40..=0x4f if !self.has_f2 && !self.has_f3 => {
                    let (dst, src, _) = self.decode_modrm()?;
                    Some(IROp::ConditionalMove { cond: BranchCond::from_x86_cc(op2), dst, src })
                }
                0xc8..=0xcf if (!self.has_66 || self.rex_w) && !self.has_f2 && !self.has_f3 =>
                    Some(IROp::ByteSwap { dst: X86Reg::from_modrm(op2 & 7, self.rex_b) }),
                0xba if !self.has_f2 && !self.has_f3 => {
                    let raw = self.peek()?;
                    let action = match (raw >> 3) & 7 {
                        4 => BitAction::Test, 5 => BitAction::Set,
                        6 => BitAction::Reset, 7 => BitAction::Complement,
                        _ => return Some(IROp::Unsupported),
                    };
                    let (_, src, _) = self.decode_modrm()?;
                    let bit = self.fetch()?;
                    Some(IROp::BitTestImmediate { src, bit, action })
                }
                0xb6 | 0xb7 | 0xbe | 0xbf => {
                    let source_width = if op2 & 1 == 0 {1} else {2};
                    let raw = self.peek()?;
                    let (dst, mut src, _) = self.decode_modrm()?;
                    let high_byte = source_width == 1 && !self.has_rex && raw >> 6 == 3 && raw & 7 >= 4;
                    if high_byte { src = Ok(X86Reg::from_modrm((raw & 7) - 4, false)); }
                    if let Err(ref mut mem) = src { mem.width = source_width; }
                    Some(IROp::ExtendMove {dst,src,source_width,signed: op2 >= 0xbe,high_byte})
                }
                0xaf => {
                    let (dst, src, _) = self.decode_modrm()?;
                    Some(IROp::SignedMultiply { dst, src, immediate: None })
                }
                0x90..=0x9f => {
                    self.byte_operand = true;
                    let raw = self.peek()?;
                    let (_, mut dst, _) = self.decode_modrm()?;
                    let high_byte = !self.has_rex && raw >> 6 == 3 && raw & 7 >= 4;
                    if high_byte { dst = Ok(X86Reg::from_modrm((raw & 7) - 4, false)); }
                    Some(IROp::SetCondition {
                        cond: BranchCond::from_x86_cc(op2), dst, high_byte,
                    })
                }
                OPCODE_SYSCALL_LO if self.mode == DecodeMode::X64 => Some(IROp::Syscall),
                0x34 if self.mode == DecodeMode::X86 => Some(IROp::Syscall),
                // SSE data movement.
                0x2a if self.has_f2 != self.has_f3 && !self.has_66 => {
                    let raw = self.peek()?;
                    let (_, src, _) = self.decode_modrm()?;
                    Some(IROp::IntegerToSimd { dst: X86Xmm::from_modrm((raw >> 3) & 7, self.rex_r), src, double: self.has_f2, width: if self.rex_w {8} else {4} })
                }
                0x2c if self.has_f2 != self.has_f3 && !self.has_66 => {
                    let raw = self.peek()?;
                    let (dst, source, _) = self.decode_modrm()?;
                    let src = match source {
                        Ok(_) => SimdSrc::Xmm(X86Xmm::from_modrm(raw & 7, self.rex_b)),
                        Err(mem) => SimdSrc::Mem(mem),
                    };
                    Some(IROp::SimdToInteger { dst, src, double: self.has_f2, width: if self.rex_w {8} else {4} })
                }
                0x13 if !self.has_f2 && !self.has_f3 && self.peek()? >> 6 != 3 => {
                    self.decode_simd_store(SimdWidth::ScalarF64)
                }
                0xae => {
                    let group = (self.peek()? >> 3) & 7;
                    let (_, operand, _) = self.decode_modrm()?;
                    match (group, operand) {
                        (2, Err(source)) => Some(IROp::LoadMxcsr { source }),
                        (3, Err(destination)) => Some(IROp::StoreMxcsr { destination }),
                        _ => Some(IROp::Unsupported),
                    }
                }
                SSE_MOVAPS => self.decode_simd_mov(SimdWidth::PackedF32),
                SSE_MOVAPS_STORE => self.decode_simd_store(SimdWidth::PackedF32),
                SSE_MOVUPS => self.decode_simd_mov(self.simd_width()),
                SSE_MOVUPS_STORE => self.decode_simd_store(self.simd_width()),
                SSE_MOVDQA if self.has_66 || self.has_f3 => {
                    self.decode_simd_mov(SimdWidth::PackedI128)
                }
                SSE_MOVDQA_STORE if self.has_66 || self.has_f3 => {
                    self.decode_simd_store(SimdWidth::PackedI128)
                }
                // SSE arithmetic.
                SSE_ADD => self.decode_simd_arith(SimdOp::Add),
                SSE_SUB => self.decode_simd_arith(SimdOp::Sub),
                SSE_MUL => self.decode_simd_arith(SimdOp::Mul),
                SSE_DIV => self.decode_simd_arith(SimdOp::Div),
                SSE_ANDPS => self.decode_simd_arith(SimdOp::And),
                SSE_ORPS => self.decode_simd_arith(SimdOp::Or),
                SSE_XORPS => self.decode_simd_arith(SimdOp::Xor),
                SSE_PAND => self.decode_simd_arith(SimdOp::And),
                SSE_POR => self.decode_simd_arith(SimdOp::Or),
                SSE_PXOR => self.decode_simd_arith(SimdOp::Xor),
                // AVX requires distinct three-operand/upper-lane semantics.
                // Do not misinterpret extended 0F opcodes as VEX prefixes.
                OPCODE_JCC_REL32_BASE..=0x8F => {
                    let cc = op2 - OPCODE_JCC_REL32_BASE;
                    let rel = self.fetch_le32()?;
                    let len = (self.offset - start) as u64;
                    let target = self
                        .current_va_at_start(start + len as usize)
                        .wrapping_add(rel as i64 as u64);
                    Some(IROp::Jcc {
                        cond: BranchCond::from_x86_cc(cc),
                        target,
                    })
                }
                _ => Some(IROp::Unsupported),
            };
        }

        let result = match op {
            0x9b => Some(IROp::WaitX87),
            0xd9 => {
                let group = (self.peek()? >> 3) & 7;
                let (_, operand, _) = self.decode_modrm()?;
                match (group, operand) {
                    (0, Err(source)) => Some(IROp::LoadX87Float { source, double: false }),
                    (2 | 3, Err(destination)) => Some(IROp::StoreX87Float { destination, double: false, pop: group == 3 }),
                    (5, Err(source)) => Some(IROp::LoadX87Control { source }),
                    (7, Err(destination)) => Some(IROp::StoreX87Control { destination }),
                    _ => Some(IROp::Unsupported),
                }
            }
            0xdd => {
                let group = (self.peek()? >> 3) & 7;
                let (_, operand, _) = self.decode_modrm()?;
                match (group, operand) {
                    (0, Err(source)) => Some(IROp::LoadX87Float { source, double: true }),
                    (2 | 3, Err(destination)) => Some(IROp::StoreX87Float { destination, double: true, pop: group == 3 }),
                    _ => Some(IROp::Unsupported),
                }
            }
            0xdb | 0xdf if self.peek()? >> 6 != 3 => {
                let group = (self.peek()? >> 3) & 7;
                let (_, operand, _) = self.decode_modrm()?;
                match (group, operand) {
                    (0, Err(source)) => Some(IROp::LoadX87Integer { source, width: if op == 0xdb {4} else {2} }),
                    (5, Err(source)) if op == 0xdf => Some(IROp::LoadX87Integer { source, width: 8 }),
                    (2 | 3, Err(destination)) => Some(IROp::StoreX87Integer { destination, width: if op == 0xdb {4} else {2}, pop: group == 3 }),
                    (7, Err(destination)) if op == 0xdf => Some(IROp::StoreX87Integer { destination, width: 8, pop: true }),
                    _ => Some(IROp::Unsupported),
                }
            }
            0xdb if self.peek()? == 0xe2 => {
                self.fetch()?;
                Some(IROp::ClearX87Exceptions)
            }
            0xa8 | 0xa9 => {
                self.byte_operand = op == 0xa8;
                let immediate = if self.byte_operand { i64::from(self.fetch()?) } else { self.fetch_imm()? };
                Some(IROp::TestImmediate { src: Ok(X86Reg::Rax), high_byte: false, immediate })
            }
            0x69 | 0x6b => {
                let (dst, src, _) = self.decode_modrm()?;
                let imm = if op == 0x6b { self.fetch()? as i8 as i64 } else { self.fetch_imm()? };
                Some(IROp::SignedMultiply { dst, src, immediate: Some(imm) })
            }
            OPCODE_NOP => Some(IROp::Nop),
            0x9c => Some(IROp::PushFlags),
            0x9d => Some(IROp::PopFlags),
            0xfc | 0xfd => Some(IROp::SetDirectionFlag { set: op == 0xfd }),
            0xa0..=0xa3 => {
                self.byte_operand = op & 1 == 0;
                let mut address = 0u64;
                for byte in 0..if self.addr_size_64 { 8 } else { 4 } {
                    address |= u64::from(self.fetch()?) << (byte * 8);
                }
                Some(IROp::AccumulatorMemory { address, store: op & 2 != 0 })
            }
            0xa4 | 0xa5 => {
                self.byte_operand = op == 0xa4;
                Some(IROp::MoveString { repeat: self.has_f2 || self.has_f3 })
            }
            0xaa | 0xab => {
                self.byte_operand = op == 0xaa;
                Some(IROp::StoreString { repeat: self.has_f2 || self.has_f3 })
            }
            0xae | 0xaf => {
                self.byte_operand = op == 0xae;
                Some(IROp::Scan {
                    repeat: if self.has_f2 {
                        2
                    } else if self.has_f3 {
                        1
                    } else {
                        0
                    },
                })
            }
            0x00 | 0x02 | 0x08 | 0x0a | 0x18 | 0x1a | 0x20 | 0x22 | 0x28 | 0x2a | 0x30 | 0x32 | 0x38 | 0x3a
            | 0x84 => {
                self.byte_operand = true;
                let kind = match op {
                    0x00 | 0x02 => AluOp::Add,
                    0x08 | 0x0a => AluOp::Or,
                    0x18 | 0x1a => AluOp::Sbb,
                    0x20 | 0x22 => AluOp::And,
                    0x28 | 0x2a => AluOp::Sub,
                    0x30 | 0x32 => AluOp::Xor,
                    0x38 | 0x3a => AluOp::Cmp,
                    _ => AluOp::Test,
                };
                let (reg, rm) = self.decode_byte_operands()?;
                if self.has_lock {
                    return Some(match (reg, rm) {
                        (ByteOperand::Register { reg, high_byte }, ByteOperand::Memory(mem)) =>
                            IROp::LockedLogic {
                                op: kind, mem, src: LockedLogicSource::Register { reg, high_byte },
                            },
                        _ => IROp::Unsupported,
                    });
                }
                Some(if op == 0x84 || op & 2 != 0 { IROp::ByteAlu { op: kind, dst: reg, src: rm } }
                    else { IROp::ByteAlu { op: kind, dst: rm, src: reg } })
            }
            0x91..=0x97 => Some(IROp::Exchange {
                left: X86Reg::Rax,
                right: X86Reg::from_modrm(op & 7, self.rex_b),
                left_high: false,
                right_high: false,
            }),
            0x86 | 0x87 => {
                self.byte_operand = op == 0x86;
                let raw = self.peek()?;
                let (left, right, _) = self.decode_modrm()?;
                match right {
                    Ok(right) => {
                        let lh = self.byte_operand && !self.has_rex && ((raw >> 3) & 7) >= 4;
                        let rh = self.byte_operand && !self.has_rex && (raw & 7) >= 4;
                        Some(IROp::Exchange {
                            left: if lh {
                                X86Reg::from_modrm((raw >> 3) & 3, false)
                            } else {
                                left
                            },
                            right: if rh {
                                X86Reg::from_modrm(raw & 3, false)
                            } else {
                                right
                            },
                            left_high: lh,
                            right_high: rh,
                        })
                    }
                    Err(mem) => {
                        let high_byte = self.byte_operand && !self.has_rex && ((raw >> 3) & 7) >= 4;
                        Some(IROp::ExchangeMem {
                            reg: if high_byte { X86Reg::from_modrm((raw >> 3) & 3, false) } else { left },
                            mem,
                            high_byte,
                        })
                    }
                }
            }
            0xc0 | 0xc1 | 0xd0 | 0xd1 | 0xd2 | 0xd3 => {
                self.byte_operand = op & 1 == 0;
                let raw = self.peek()?;
                if self.byte_operand && !self.has_rex && raw >> 6 == 3 && raw & 7 >= 4 {
                    return Some(IROp::Unsupported);
                }
                let (_, dst, _) = self.decode_modrm()?;
                let count = match op {
                    0xc0 | 0xc1 => Some(self.fetch()?),
                    0xd0 | 0xd1 => Some(1),
                    _ => None,
                };
                Some(IROp::Shift {
                    kind: (raw >> 3) & 7,
                    dst,
                    count,
                })
            }
            0xe0..=0xe3 => {
                let rel = self.fetch()? as i8;
                Some(IROp::Loop {
                    target: self
                        .base_va
                        .wrapping_add(self.offset as u64)
                        .wrapping_add(rel as i64 as u64),
                    kind: op & 3,
                })
            }
            0x60 if self.mode == DecodeMode::X86 => Some(IROp::PushAll),
            0x61 if self.mode == DecodeMode::X86 => Some(IROp::PopAll),
            0xc9 => Some(IROp::Leave),
            0xf6 | 0xf7 => {
                self.byte_operand = op == 0xf6;
                let raw = self.peek()?;
                let group = (raw >> 3) & 7;
                let (_, mut dst, _) = self.decode_modrm()?;
                let high_byte = self.byte_operand && !self.has_rex && raw >> 6 == 3 && raw & 7 >= 4;
                if high_byte { dst = Ok(X86Reg::from_modrm((raw & 7) - 4, false)); }
                Some(if group == 0 {
                    let immediate = if self.byte_operand { i64::from(self.fetch()?) } else { self.fetch_imm()? };
                    IROp::TestImmediate { src: dst, high_byte, immediate }
                } else if group == 2 || group == 3 {
                    IROp::UnaryNotNeg { dst, high_byte, negate: group == 3 }
                } else if group == 4 || group == 5 {
                    IROp::FullMultiply { src: dst, high_byte, signed: group == 5 }
                } else if group == 6 || group == 7 {
                    IROp::Divide { src: dst, high_byte, signed: group == 7 }
                } else { IROp::Unsupported })
            }
            0x40..=0x4f if self.mode == DecodeMode::X86 => Some(IROp::IncDec {
                dst: Ok(X86Reg::from_modrm(op & 7, false)),
                high_byte: false,
                decrement: op >= 0x48,
            }),
            0x11 => self.decode_alu_rm_r(AluOp::Adc),
            OPCODE_RET_NEAR => Some(IROp::Ret),
            0xC2 => Some(IROp::RetImm {
                bytes: u16::from_le_bytes([self.fetch()?, self.fetch()?]),
            }),
            0x68 => Some(IROp::PushImm {
                value: self.fetch_imm()?,
            }),
            0x6A => Some(IROp::PushImm {
                value: self.fetch()? as i8 as i64,
            }),
            0xCD => Some(if self.fetch()? == 0x2e {
                IROp::Syscall
            } else {
                IROp::Unsupported
            }),
            0xFE | 0xFF => {
                self.byte_operand = op == 0xfe;
                let raw = self.peek()?;
                let group = (raw >> 3) & 7;
                if self.has_f2 && !matches!(group, 2 | 4) { return Some(IROp::Unsupported); }
                let (_, mut target, _) = self.decode_modrm()?;
                if self.has_lock {
                    return Some(match (group, target) {
                        (0 | 1, Err(mem)) => IROp::LockedIncDec { mem, decrement: group == 1 },
                        _ => IROp::Unsupported,
                    });
                }
                if group <= 1 {
                    let high_byte = self.byte_operand && !self.has_rex && raw >> 6 == 3 && raw & 7 >= 4;
                    if high_byte { target = Ok(X86Reg::from_modrm((raw & 7) - 4, false)); }
                    return Some(IROp::IncDec { dst: target, high_byte, decrement: group == 1 });
                }
                if op == 0xfe { return Some(IROp::Unsupported); }
                match (group, target) {
                    (6, Ok(src)) => Some(IROp::Push { src }),
                    (6, Err(mut mem)) => {
                        mem.width = if self.operand_width() == 2 { 2 }
                            else if self.mode == DecodeMode::X64 { 8 } else { 4 };
                        Some(IROp::PushMem { mem })
                    }
                    (2 | 4, Ok(target)) => Some(IROp::JumpReg {
                        target,
                        call: group == 2,
                    }),
                    (2 | 4, Err(mut mem)) => {
                        mem.width = if self.mode == DecodeMode::X64 { 8 } else { 4 };
                        Some(IROp::JumpMem {
                            mem,
                            call: group == 2,
                        })
                    }
                    _ => Some(IROp::Unsupported),
                }
            }

            OPCODE_MOV_R_IMM64..=0xBF if self.op_size_64 => {
                let reg = X86Reg::from_modrm(op & 0x07, self.rex_b);
                let imm = self.fetch_le64()?;
                Some(IROp::MovImm { dst: reg, imm })
            }

            OPCODE_MOV_R_IMM64..=0xBF => {
                let reg = X86Reg::from_modrm(op & 0x07, self.rex_b);
                let imm = self.fetch_imm()?;
                Some(IROp::MovImm {
                    dst: reg,
                    imm: imm as i64,
                })
            }

            OPCODE_MOV_R8_IMM8..=0xB7 => {
                self.byte_operand = true;
                let high_byte = !self.has_rex && (op & 7) >= 4;
                let reg = X86Reg::from_modrm(if high_byte {op & 3} else {op & 7}, self.rex_b);
                Some(IROp::ByteImmediate { dst: ByteOperand::Register { reg, high_byte }, value: self.fetch()? })
            }

            0x88 | 0x8a => {
                self.byte_operand = true;
                let (reg, rm) = self.decode_byte_operands()?;
                Some(if op == 0x88 { IROp::ByteMove { dst: rm, src: reg } }
                     else { IROp::ByteMove { dst: reg, src: rm } })
            }

            OPCODE_MOV_RM_R => {
                let (src, dst, _) = self.decode_modrm()?;
                match dst {
                    Ok(dst_reg) => Some(IROp::MovReg { dst: dst_reg, src }),
                    Err(mem) => Some(IROp::Store { mem, src }),
                }
            }

            OPCODE_MOV_R_RM => {
                let (dst, src, _) = self.decode_modrm()?;
                match src {
                    Ok(src_reg) => Some(IROp::MovReg { dst, src: src_reg }),
                    Err(mem) => Some(IROp::Load { dst, mem }),
                }
            }

            OPCODE_MOV_RM_IMM | 0xC6 => {
                self.byte_operand = op == 0xC6;
                if self.peek()? & 0x38 != 0 {
                    return Some(IROp::Unsupported);
                }
                if self.byte_operand {
                    let (_, dst) = self.decode_byte_operands()?;
                    return Some(IROp::ByteImmediate { dst, value: self.fetch()? });
                }
                let (_, dst, _) = self.decode_modrm()?;
                let value = if self.byte_operand {
                    self.fetch()? as i64
                } else {
                    self.fetch_imm()?
                };
                Some(match dst {
                    Ok(dst) => IROp::MovImm { dst, imm: value },
                    Err(mem) => IROp::StoreImm { mem, value },
                })
            }

            OPCODE_LEA => {
                let (dst, src, _) = self.decode_modrm()?;
                match src {
                    Err(mem) => Some(IROp::Lea { dst, mem }),
                    Ok(_) => Some(IROp::Unsupported),
                }
            }

            OPCODE_PUSH_R..=0x57 => {
                let reg = X86Reg::from_modrm(op & 0x07, self.rex_b);
                Some(IROp::Push { src: reg })
            }

            OPCODE_POP_R..=0x5F => {
                let reg = X86Reg::from_modrm(op & 0x07, self.rex_b);
                Some(IROp::Pop { dst: reg })
            }

            OPCODE_CALL_REL32 => {
                let rel = self.fetch_le32()?;
                let len = (self.offset - start) as u64;
                let target = self
                    .current_va_at_start(start + len as usize)
                    .wrapping_add(rel as i64 as u64);
                Some(IROp::Call { target })
            }

            OPCODE_JMP_REL32 => {
                let rel = self.fetch_le32()?;
                let len = (self.offset - start) as u64;
                let target = self
                    .current_va_at_start(start + len as usize)
                    .wrapping_add(rel as i64 as u64);
                Some(IROp::Jmp { target })
            }

            OPCODE_JMP_REL8 => {
                let rel = self.fetch()? as i8;
                let len = (self.offset - start) as u64;
                let target = self
                    .current_va_at_start(start + len as usize)
                    .wrapping_add(rel as i64 as u64);
                Some(IROp::Jmp { target })
            }

            0x98 => Some(IROp::ExtendMove {
                dst: X86Reg::Rax, src: Ok(X86Reg::Rax),
                source_width: self.operand_width() / 2, signed: true, high_byte: false,
            }),
            0x99 => Some(IROp::SignExtendAccumulatorPair),
            0x63 if self.mode == DecodeMode::X64 => {
                let (dst, mut src, _) = self.decode_modrm()?;
                let source_width = if self.operand_width() == 8 { 4 } else { self.operand_width() };
                if let Err(ref mut mem) = src { mem.width = source_width; }
                Some(IROp::ExtendMove { dst, src, source_width, signed: true, high_byte: false })
            }
            OPCODE_JCC_REL8_BASE..=0x7F => {
                let cc = op - OPCODE_JCC_REL8_BASE;
                let rel = self.fetch()? as i8;
                let len = (self.offset - start) as u64;
                let target = self
                    .current_va_at_start(start + len as usize)
                    .wrapping_add(rel as i64 as u64);
                Some(IROp::Jcc {
                    cond: BranchCond::from_x86_cc(cc),
                    target,
                })
            }

            OPCODE_TEST_RM_R => {
                let (dst, src, _) = self.decode_modrm()?;
                match src {
                    Ok(src_reg) => Some(IROp::AluReg {
                        op: AluOp::Test,
                        dst,
                        src: src_reg,
                    }),
                    Err(mem) => Some(IROp::AluMem {
                        op: AluOp::Test,
                        dst,
                        mem,
                    }),
                }
            }

            OPCODE_ADD_RM_R => self.decode_alu_rm_r(AluOp::Add),
            OPCODE_OR_RM_R => self.decode_alu_rm_r(AluOp::Or),
            OPCODE_AND_RM_R => self.decode_alu_rm_r(AluOp::And),
            OPCODE_SUB_RM_R => self.decode_alu_rm_r(AluOp::Sub),
            0x19 => self.decode_alu_rm_r(AluOp::Sbb),
            OPCODE_XOR_RM_R => self.decode_alu_rm_r(AluOp::Xor),
            OPCODE_CMP_RM_R => self.decode_alu_rm_r(AluOp::Cmp),

            OPCODE_ADD_R_RM => self.decode_alu_r_rm(AluOp::Add),
            0x13 => self.decode_alu_r_rm(AluOp::Adc),
            OPCODE_OR_R_RM => self.decode_alu_r_rm(AluOp::Or),
            OPCODE_AND_R_RM => self.decode_alu_r_rm(AluOp::And),
            OPCODE_SUB_R_RM => self.decode_alu_r_rm(AluOp::Sub),
            0x1b => self.decode_alu_r_rm(AluOp::Sbb),
            OPCODE_XOR_R_RM => self.decode_alu_r_rm(AluOp::Xor),
            OPCODE_CMP_R_RM => self.decode_alu_r_rm(AluOp::Cmp),

            OPCODE_ALU_IMM_BASE | OPCODE_ALU_IMM32 | OPCODE_ALU_IMM8 => {
                // Peek the raw ModR/M byte so we can extract the ALU sub-op
                // from the reg field before decode_modrm consumes it.
                let modrm_byte = self.bytes.get(self.offset).copied().unwrap_or(0);
                self.byte_operand = op == OPCODE_ALU_IMM_BASE;
                if self.byte_operand && !self.has_lock {
                    let (_, dst) = self.decode_byte_operands()?;
                    let operation = match (modrm_byte >> 3) & 7 {
                        0 => AluOp::Add, 1 => AluOp::Or, 2 => AluOp::Adc, 3 => AluOp::Sbb,
                        4 => AluOp::And, 5 => AluOp::Sub, 6 => AluOp::Xor, _ => AluOp::Cmp,
                    };
                    return Some(IROp::ByteAluImmediate { op: operation, dst, immediate: self.fetch()? });
                }
                let (_, dst, _) = self.decode_modrm()?;
                // The ALU sub-operation is encoded in the ModR/M reg field.
                let alu_idx = (modrm_byte >> 3) & 0x07;
                if self.has_lock {
                    let immediate = if op == OPCODE_ALU_IMM32 { self.fetch_imm()? }
                        else { self.fetch()? as i8 as i64 };
                    return Some(match (alu_idx, dst) {
                        (0 | 5, Err(mem)) => IROp::LockedAddImmediate { mem, immediate, subtract: alu_idx == 5 },
                        (1 | 4 | 6, Err(mem)) => IROp::LockedLogic {
                            op: match alu_idx { 1 => AluOp::Or, 4 => AluOp::And, _ => AluOp::Xor },
                            mem,
                            src: LockedLogicSource::Immediate(immediate),
                        },
                        _ => IROp::Unsupported,
                    });
                }
                if alu_idx == 2 {
                    let imm = if op == OPCODE_ALU_IMM32 {
                        self.fetch_imm()?
                    } else {
                        self.fetch()? as i8 as i64
                    };
                    return Some(match dst {
                        Ok(dst) => IROp::AdcImm { dst, imm },
                        Err(mem) => IROp::AluStoreImm { op: AluOp::Adc, mem, imm },
                    });
                }
                let alu_op = match alu_idx {
                    OPCODE_ALU_ADD_IMM => AluOp::Add,
                    OPCODE_ALU_OR_IMM => AluOp::Or,
                    OPCODE_ALU_AND_IMM => AluOp::And,
                    OPCODE_ALU_SUB_IMM => AluOp::Sub,
                    3 => AluOp::Sbb,
                    OPCODE_ALU_XOR_IMM => AluOp::Xor,
                    OPCODE_ALU_CMP_IMM => AluOp::Cmp,
                    _ => return Some(IROp::Unsupported),
                };
                // 0x80/0x83 use a sign-extended imm8; 0x81 uses a full imm32.
                let imm = if op == OPCODE_ALU_IMM32 {
                    self.fetch_imm()?
                } else {
                    self.fetch()? as i8 as i64
                };
                Some(match dst {
                    Ok(dst) => IROp::AluImm {
                        op: alu_op,
                        dst,
                        imm,
                    },
                    Err(mem) => IROp::AluStoreImm {
                        op: alu_op,
                        mem,
                        imm,
                    },
                })
            }

            0x1c => {
                self.byte_operand = true;
                Some(IROp::AluImm { op: AluOp::Sbb, dst: X86Reg::Rax, imm: self.fetch()? as i8 as i64 })
            }
            0x1d => Some(IROp::AluImm { op: AluOp::Sbb, dst: X86Reg::Rax, imm: self.fetch_imm()? }),
            OPCODE_ADD_AL_IMM8 => {
                self.byte_operand = true;
                Some(IROp::AluImm {
                    op: AluOp::Add,
                    dst: X86Reg::Rax,
                    imm: self.fetch()? as i8 as i64,
                })
            }
            OPCODE_ADD_AL_IMM32 => Some(IROp::AluImm {
                op: AluOp::Add,
                dst: X86Reg::Rax,
                imm: self.fetch_imm()?,
            }),
            OPCODE_SUB_AL_IMM8 => {
                self.byte_operand = true;
                Some(IROp::AluImm {
                    op: AluOp::Sub,
                    dst: X86Reg::Rax,
                    imm: self.fetch()? as i8 as i64,
                })
            }
            OPCODE_SUB_AL_IMM32 => Some(IROp::AluImm {
                op: AluOp::Sub,
                dst: X86Reg::Rax,
                imm: self.fetch_imm()?,
            }),
            OPCODE_AND_AL_IMM8 => {
                self.byte_operand = true;
                Some(IROp::AluImm {
                    op: AluOp::And,
                    dst: X86Reg::Rax,
                    imm: self.fetch()? as i8 as i64,
                })
            }
            OPCODE_AND_AL_IMM32 => Some(IROp::AluImm {
                op: AluOp::And,
                dst: X86Reg::Rax,
                imm: self.fetch_imm()?,
            }),
            OPCODE_OR_AL_IMM8 => {
                self.byte_operand = true;
                Some(IROp::AluImm {
                    op: AluOp::Or,
                    dst: X86Reg::Rax,
                    imm: self.fetch()? as i8 as i64,
                })
            }
            OPCODE_OR_AL_IMM32 => Some(IROp::AluImm {
                op: AluOp::Or,
                dst: X86Reg::Rax,
                imm: self.fetch_imm()?,
            }),
            OPCODE_XOR_AL_IMM8 => {
                self.byte_operand = true;
                Some(IROp::AluImm {
                    op: AluOp::Xor,
                    dst: X86Reg::Rax,
                    imm: self.fetch()? as i8 as i64,
                })
            }
            OPCODE_XOR_AL_IMM32 => Some(IROp::AluImm {
                op: AluOp::Xor,
                dst: X86Reg::Rax,
                imm: self.fetch_imm()?,
            }),
            OPCODE_CMP_AL_IMM8 => {
                self.byte_operand = true;
                Some(IROp::AluImm {
                    op: AluOp::Cmp,
                    dst: X86Reg::Rax,
                    imm: self.fetch()? as i8 as i64,
                })
            }
            OPCODE_CMP_AL_IMM32 => Some(IROp::AluImm {
                op: AluOp::Cmp,
                dst: X86Reg::Rax,
                imm: self.fetch_imm()?,
            }),

            _ => Some(IROp::Unsupported),
        };

        result
    }

    // ---- SIMD decoding ----

    /// Determine SIMD width from the current prefix state.
    fn simd_width(&self) -> SimdWidth {
        if self.has_f3 {
            SimdWidth::ScalarF32
        } else if self.has_f2 {
            SimdWidth::ScalarF64
        } else if self.has_66 {
            SimdWidth::PackedF64
        } else {
            SimdWidth::PackedF32
        }
    }

    /// Decode a SIMD move (dst_xmm ← src).
    fn decode_simd_mov(&mut self, width: SimdWidth) -> Option<IROp> {
        let modrm = self.fetch()?;
        let reg_idx = (modrm >> 3) & 0x07;
        let mode = (modrm >> 6) & 0x03;
        let rm_field = modrm & 0x07;
        let dst = X86Xmm::from_modrm(reg_idx, self.rex_r);
        if mode == 3 {
            let src = X86Xmm::from_modrm(rm_field, self.rex_b);
            Some(IROp::SimdMov {
                dst,
                src: SimdSrc::Xmm(src),
                width,
            })
        } else {
            let mem = self.decode_simd_mem_operand(rm_field, mode)?;
            Some(IROp::SimdMov {
                dst,
                src: SimdSrc::Mem(mem),
                width,
            })
        }
    }

    /// Decode a SIMD store (mem ← src_xmm).
    fn decode_simd_store(&mut self, width: SimdWidth) -> Option<IROp> {
        let modrm = self.fetch()?;
        let reg_idx = (modrm >> 3) & 0x07;
        let mode = (modrm >> 6) & 0x03;
        let rm_field = modrm & 0x07;
        let src = X86Xmm::from_modrm(reg_idx, self.rex_r);
        if mode == 3 {
            let dst = X86Xmm::from_modrm(rm_field, self.rex_b);
            Some(IROp::SimdMov {
                dst,
                src: SimdSrc::Xmm(src),
                width,
            })
        } else {
            let mem = self.decode_simd_mem_operand(rm_field, mode)?;
            Some(IROp::SimdStore { mem, src, width })
        }
    }

    /// Decode a SIMD arithmetic op (dst_xmm = dst_xmm <op> src).
    fn decode_simd_arith(&mut self, op: SimdOp) -> Option<IROp> {
        let width = self.simd_width();
        let modrm = self.fetch()?;
        let reg_idx = (modrm >> 3) & 0x07;
        let mode = (modrm >> 6) & 0x03;
        let rm_field = modrm & 0x07;
        let dst = X86Xmm::from_modrm(reg_idx, self.rex_r);
        if mode == 3 {
            let src = X86Xmm::from_modrm(rm_field, self.rex_b);
            Some(IROp::SimdArith {
                op,
                dst,
                src: SimdSrc::Xmm(src),
                width,
            })
        } else {
            let mem = self.decode_simd_mem_operand(rm_field, mode)?;
            Some(IROp::SimdArith {
                op,
                dst,
                src: SimdSrc::Mem(mem),
                width,
            })
        }
    }

    /// Decode a memory operand for SIMD (32-bit displacement always,
    /// no SIB simplification — the BTL runtime handles the general case).
    fn decode_simd_mem_operand(&mut self, _rm_field: u8, _mode: u8) -> Option<MemOperand> {
        self.offset -= 1;
        let (_, operand, _) = self.decode_modrm()?;
        match operand {
            Err(mut mem) => {
                mem.width = 16;
                Some(mem)
            }
            _ => None,
        }
    }

}

/// Decode a basic block from guest code bytes, producing IR.
pub fn decode_block(bytes: &[u8], base_va: u64, out: &mut [IrInst]) -> usize {
    decode_block_mode(bytes, base_va, DecodeMode::X64, out)
}

pub fn decode_block_mode(
    bytes: &[u8],
    base_va: u64,
    mode: DecodeMode,
    out: &mut [IrInst],
) -> usize {
    let mut dec = Decoder::new(bytes, base_va, mode);
    let mut written = 0;
    while written < out.len() {
        let va = dec.current_va();
        let start = dec.offset;
        let op = match dec.decode_one() {
            Some(op) => op,
            None => break,
        };
        let len = (dec.offset - start) as u8;
        if len == 0 || len > 15 {
            break;
        }
        out[written] = IrInst {
            op,
            guest_va: va,
            guest_len: len,
            operand_width: dec.operand_width(),
            address_width: dec.address_width(),
            segment: dec.segment,
        };
        written += 1;
        if matches!(
            op,
            IROp::Ret
                | IROp::Loop { .. }
                | IROp::RetImm { .. }
                | IROp::Call { .. }
                | IROp::JumpReg { .. }
                | IROp::JumpMem { .. }
                | IROp::Jcc { .. }
                | IROp::Jmp { .. }
                | IROp::Syscall
                | IROp::Unsupported
        ) {
            break;
        }
    }
    written
}

/// Decode a single instruction. Returns (IrInst, bytes_consumed).
pub fn decode_one(bytes: &[u8], base_va: u64) -> Option<(IrInst, usize)> {
    decode_one_mode(bytes, base_va, DecodeMode::X64)
}

pub fn decode_one_mode(bytes: &[u8], base_va: u64, mode: DecodeMode) -> Option<(IrInst, usize)> {
    let mut dec = Decoder::new(bytes, base_va, mode);
    let va = dec.current_va();
    let start = dec.offset;
    let op = dec.decode_one()?;
    let len = dec.offset - start;
    if len == 0 || len > 15 {
        return None;
    }
    Some((
        IrInst {
            op,
            guest_va: va,
            guest_len: len as u8,
            operand_width: dec.operand_width(),
            address_width: dec.address_width(),
            segment: dec.segment,
        },
        len,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_nop() {
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&[0x90], 0x1000, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].op, IROp::Nop);
        assert_eq!(out[0].guest_len, 1);
    }

    #[test]
    fn decode_ret() {
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&[0xC3], 0x1000, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].op, IROp::Ret);
    }

    #[test]
    fn decode_mov_r64_imm64() {
        let bytes = [0x48, 0xB8, 0xF0, 0xDE, 0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::MovImm { dst, imm } => {
                assert_eq!(dst, X86Reg::Rax);
                assert_eq!(imm, 0x1234_5678_9ABC_DEF0u64 as i64);
            }
            _ => panic!("expected MovImm"),
        }
        assert_eq!(out[0].guest_len, 10);
    }

    #[test]
    fn decode_alu_imm() {
        let bytes = [0x48, 0x83, 0xC0, 0x2A];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::AluImm { op, dst, imm } => {
                assert_eq!(op, AluOp::Add);
                assert_eq!(dst, X86Reg::Rax);
                assert_eq!(imm, 42);
            }
            _ => panic!("expected AluImm"),
        }
    }

    #[test]
    fn decode_jmp_rel8() {
        let bytes = [0xEB, 0x05];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::Jmp { target } => {
                assert_eq!(target, 0x1007);
            }
            _ => panic!("expected Jmp"),
        }
    }

    #[test]
    fn decode_call_rel32() {
        let bytes = [0xE8, 0xFC, 0xFF, 0xFF, 0xFF];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::Call { target } => {
                // next_rip = 0x1000 + 5 = 0x1005; rel32 = -4; target = 0x1005 - 4 = 0x1001
                assert_eq!(target, 0x1001);
            }
            _ => panic!("expected Call"),
        }
    }

    #[test]
    fn decode_jcc_rel8() {
        let bytes = [0x74, 0x04];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x2000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::Jcc { cond, target } => {
                assert_eq!(cond, BranchCond::Equal);
                assert_eq!(target, 0x2006);
            }
            _ => panic!("expected Jcc"),
        }
    }

    #[test]
    fn decode_push_pop() {
        let bytes = [0x50, 0x58];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 2);
        assert_eq!(out[0].op, IROp::Push { src: X86Reg::Rax });
        assert_eq!(out[1].op, IROp::Pop { dst: X86Reg::Rax });
    }

    #[test]
    fn decode_test_rm_r() {
        let bytes = [0x48, 0x85, 0xC0];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::AluReg { op, dst, src } => {
                assert_eq!(op, AluOp::Test);
                assert_eq!(dst, X86Reg::Rax);
                assert_eq!(src, X86Reg::Rax);
            }
            _ => panic!("expected AluReg Test"),
        }
    }

    #[test]
    fn decode_lea() {
        let bytes = [0x48, 0x8D, 0x43, 0x08];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::Lea { dst, mem } => {
                assert_eq!(dst, X86Reg::Rax);
                assert_eq!(mem.base, Some(X86Reg::Rbx));
                assert_eq!(mem.disp, 8);
                assert_eq!(mem.width, 8);
            }
            _ => panic!("expected Lea"),
        }
    }

    #[test]
    fn decode_store_reg_to_mem() {
        let bytes = [0x48, 0x89, 0x18];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::Store { mem, src } => {
                assert_eq!(src, X86Reg::Rbx);
                assert_eq!(mem.base, Some(X86Reg::Rax));
                assert_eq!(mem.width, 8);
            }
            _ => panic!("expected Store"),
        }
    }

    #[test]
    fn decode_load_mem_to_reg() {
        let bytes = [0x48, 0x8B, 0x01];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::Load { dst, mem } => {
                assert_eq!(dst, X86Reg::Rax);
                assert_eq!(mem.base, Some(X86Reg::Rcx));
                assert_eq!(mem.width, 8);
            }
            _ => panic!("expected Load"),
        }
    }

    #[test]
    fn decode_mov_reg_reg() {
        let bytes = [0x48, 0x89, 0xD8];
        let mut out = [IrInst::empty(); 4];
        let n = decode_block(&bytes, 0x1000, &mut out);
        assert_eq!(n, 1);
        match out[0].op {
            IROp::MovReg { dst, src } => {
                assert_eq!(dst, X86Reg::Rax);
                assert_eq!(src, X86Reg::Rbx);
            }
            _ => panic!("expected MovReg"),
        }
    }
}

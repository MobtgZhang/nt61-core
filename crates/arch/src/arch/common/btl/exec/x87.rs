//! The x87 load/integer-store subset, using explicit 80-bit register values.
//! Arithmetic and other unimplemented x87 instructions still stop precisely.
use super::{ExecOutcome, GuestState};

/// An x87 extended value with its explicit integer bit (no host FP dependency).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extended {
    pub significand: u64,
    pub sign_exponent: u16,
}
impl Extended {
    pub const ZERO: Self = Self {
        significand: 0,
        sign_exponent: 0,
    };
    const INDEFINITE: Self = Self {
        significand: 0xc000_0000_0000_0000,
        sign_exponent: 0xffff,
    };
}

pub(super) fn wait(s: &GuestState, pc: u64) -> Result<(), ExecOutcome> {
    let exceptions = (s.x87_status & !s.x87_control & 0x3f) as u8;
    if exceptions == 0 {
        Ok(())
    } else {
        Err(ExecOutcome::FloatingPointError {
            guest_va: pc,
            exceptions,
        })
    }
}

fn raise(s: &mut GuestState, exceptions: u16) -> bool {
    s.x87_status |= exceptions;
    let unmasked = exceptions & !s.x87_control & 0x3f != 0;
    if unmasked {
        s.x87_status |= 0x8080;
    }
    unmasked
}

fn from_ieee(bits: u64, double: bool) -> (Extended, u16) {
    let (fraction, exponent_bits, bias) = if double { (52, 11, 1023) } else { (23, 8, 127) };
    let maximum = (1u64 << exponent_bits) - 1;
    let exponent = (bits >> fraction) & maximum;
    let negative = bits >> (fraction + exponent_bits) & 1 != 0;
    let payload = bits & ((1u64 << fraction) - 1);
    let sign = if negative { 0x8000 } else { 0 };
    let mut flags = 0;
    let (significand, exponent) = if exponent == maximum {
        if payload != 0 && payload & (1 << (fraction - 1)) == 0 {
            flags = 1;
        }
        (
            (1u64 << 63) | (payload << (63 - fraction)) | if payload != 0 { 1u64 << 62 } else { 0 },
            0x7fff,
        )
    } else if exponent == 0 {
        if payload == 0 {
            (0, 0)
        } else {
            flags = 2;
            let shift = payload.leading_zeros();
            (
                payload << shift,
                (16383 + 1 - bias + (63 - shift) as i32 - fraction as i32) as u16,
            )
        }
    } else {
        (
            ((1u64 << fraction) | payload) << (63 - fraction),
            (exponent as i32 + 16383 - bias) as u16,
        )
    };
    (
        Extended {
            significand,
            sign_exponent: sign | exponent,
        },
        flags,
    )
}

pub(super) fn load(s: &mut GuestState, bits: u64, double: bool) {
    let (value, flags) = from_ieee(bits, double);
    push(s, value, flags);
}

pub(super) fn load_integer(s: &mut GuestState, bits: u64, width: u8) {
    let value = match width {
        2 => bits as i16 as i64,
        4 => bits as i32 as i64,
        _ => bits as i64,
    };
    let magnitude = value.unsigned_abs();
    let extended = if magnitude == 0 {
        Extended::ZERO
    } else {
        let shift = magnitude.leading_zeros();
        Extended {
            significand: magnitude << shift,
            sign_exponent: (16383 + 63 - shift) as u16 | if value < 0 { 0x8000 } else { 0 },
        }
    };
    push(s, extended, 0);
}

fn push(s: &mut GuestState, value: Extended, flags: u16) {
    let next = ((s.x87_status >> 11).wrapping_sub(1) & 7) as usize;
    s.x87_status &= !0x200; // C1 is clear unless the stack overflows.
    let (value, flags) = if s.x87_empty & (1 << next) == 0 {
        s.x87_status |= 0x200;
        (Extended::INDEFINITE, 0x41) // invalid + stack fault
    } else {
        (value, flags)
    };
    // FLD completes its push for an unmasked denormal exception; invalid
    // operands and stack overflow suppress the push when invalid is unmasked.
    if raise(s, flags) && flags & 1 != 0 {
        return;
    }
    s.x87_registers[next] = value;
    s.x87_empty &= !(1 << next);
    s.x87_status = (s.x87_status & !0x3800) | ((next as u16) << 11);
}

fn to_ieee(value: Extended, double: bool, control: u16) -> (u64, u16, bool) {
    let fraction = if double { 52 } else { 23 };
    let max_exponent = if double { 2047 } else { 255 };
    let sign = u64::from(value.sign_exponent >> 15) << (fraction + if double { 11 } else { 8 });
    let exponent = value.sign_exponent & 0x7fff;
    let quiet = 1u64 << (fraction - 1);
    let infinity = max_exponent << fraction;
    let indefinite = (1u64 << (fraction + if double {11} else {8})) | infinity | quiet;
    if exponent != 0 && value.significand >> 63 == 0 {
        return (indefinite, 1, false);
    }
    if exponent == 0x7fff {
        let payload = value.significand & !(1u64 << 63);
        if payload == 0 {
            return (sign | infinity, 0, false);
        }
        return (
            sign | infinity | (payload >> (63 - fraction)) | quiet,
            u16::from(payload & (1u64 << 62) == 0),
            false,
        );
    }
    if value.significand == 0 {
        return (sign, 0, false);
    }
    let shift = 63 - fraction - 3;
    let significand = u128::from(value.significand >> shift)
        | u128::from(value.significand & ((1u64 << shift) - 1) != 0);
    // FST ignores precision control; its memory width determines precision.
    // Use gradual underflow (no SSE DAZ/FTZ), mapping only RC and masks.
    let mxcsr = (u32::from(control & 0x3f) << 7) | (u32::from((control >> 10) & 3) << 13);
    let (result, flags) = super::float::round_pack(
        significand,
        i32::from(exponent.max(1)) - 16383 + if double { 1023 } else { 127 },
        sign != 0,
        fraction,
        max_exponent,
        mxcsr,
        0,
    );
    let (rounded, _) = from_ieee(result, double);
    let mut flags = u16::from(flags);
    // Unmasked x87 underflow includes an exactly representable subnormal,
    // but a tiny value rounded up to the minimum normal does not underflow.
    // Range exceptions suppress the store
    // and the precision exception that a masked rounded result would raise.
    if control & 16 == 0 && result & infinity == 0 {
        flags |= 16;
    }
    if flags & !control & (8 | 16) != 0 { return (result, flags & !32, false); }
    let increment =
        (rounded.sign_exponent & 0x7fff, rounded.significand) > (exponent.max(1), value.significand);
    (result, flags, increment)
}

pub(super) fn store_float(s: &mut GuestState, double: bool, pop: bool) -> Option<u64> {
    let top = ((s.x87_status >> 11) & 7) as usize;
    s.x87_status &= !0x200;
    let (result, flags, increment) = if s.x87_empty & (1 << top) != 0 {
        let (value, _, _) = to_ieee(Extended::INDEFINITE, double, s.x87_control);
        (value, 0x41, false)
    } else {
        to_ieee(s.x87_registers[top], double, s.x87_control)
    };
    if increment {
        s.x87_status |= 0x200;
    }
    raise(s, flags);
    if flags & !s.x87_control & (1 | 8 | 16) != 0 {
        return None;
    }
    if pop {
        s.x87_empty |= 1 << top;
        s.x87_status = (s.x87_status & !0x3800) | ((((top + 1) & 7) as u16) << 11);
    }
    Some(result)
}

fn to_integer(value: Extended, width: u8, rounding: u16) -> (u64, u16, bool) {
    let exponent = value.sign_exponent & 0x7fff;
    let negative = value.sign_exponent & 0x8000 != 0;
    let indefinite = 1u64 << (width * 8 - 1);
    if exponent == 0x7fff || (exponent != 0 && value.significand >> 63 == 0) {
        return (indefinite, 1, false);
    }
    if value.significand == 0 {
        return (0, 0, false);
    }
    let shift = 16383 + 63 - i32::from(exponent.max(1));
    // Numbers of magnitude >= 2^64 cannot fit any supported signed integer.
    if shift < 0 {
        return (indefinite, 1, false);
    }
    let significand = u128::from(value.significand);
    let (mut magnitude, tail, half) = if shift >= 128 {
        (0, significand, u128::MAX)
    } else if shift == 0 {
        (significand, 0, 0)
    } else {
        (
            significand >> shift,
            significand & ((1u128 << shift) - 1),
            1u128 << (shift - 1),
        )
    };
    let increment = match rounding {
        0 => tail > half || (tail == half && tail != 0 && magnitude & 1 != 0),
        1 => negative && tail != 0,
        2 => !negative && tail != 0,
        _ => false,
    };
    magnitude += u128::from(increment);
    if magnitude > u128::from(indefinite) - u128::from(!negative) {
        return (indefinite, 1, false);
    }
    let result = if negative {
        (magnitude as u64).wrapping_neg()
    } else {
        magnitude as u64
    };
    (result, if tail != 0 { 32 } else { 0 }, increment)
}

pub(super) fn store_integer(s: &mut GuestState, width: u8, pop: bool) -> Option<u64> {
    let top = ((s.x87_status >> 11) & 7) as usize;
    s.x87_status &= !0x200;
    let (result, flags, increment) = if s.x87_empty & (1 << top) != 0 {
        (1u64 << (width * 8 - 1), 0x41, false)
    } else {
        to_integer(s.x87_registers[top], width, (s.x87_control >> 10) & 3)
    };
    if increment {
        s.x87_status |= 0x200;
    }
    // Unlike invalid conversion, an unmasked precision exception still stores
    // the rounded integer and completes FISTP's pop before deferred delivery.
    if raise(s, flags) && flags & 1 != 0 {
        return None;
    }
    if pop {
        s.x87_empty |= 1 << top;
        s.x87_status = (s.x87_status & !0x3800) | ((((top + 1) & 7) as u16) << 11);
    }
    Some(result)
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    #[repr(align(16))]
    struct Saved([u8; 512]);

    fn hardware_float_store(value: Extended, double: bool, control: u16) -> (u64, u16) {
        let mut saved = Saved([0; 512]);
        let mut output = 0xa5a5_a5a5_a5a5_a5a5u64;
        let mut status = 0u16;
        let mut input = [0u8; 10];
        input[..8].copy_from_slice(&value.significand.to_le_bytes());
        input[8..].copy_from_slice(&value.sign_exponent.to_le_bytes());
        macro_rules! run {($store:literal)=>{
            // SAFETY: 80-bit input, aligned save area and live output buffers;
            // restore the complete host FP state before returning to Rust.
            unsafe {core::arch::asm!(
                "fxsave64 [{saved}]", "fninit", "fldcw word ptr [{control}]",
                "fld tbyte ptr [{input}]", $store, "fnstsw word ptr [{status}]", "fxrstor64 [{saved}]",
                saved=in(reg)saved.0.as_mut_ptr(),control=in(reg)&control,input=in(reg)input.as_ptr(),
                output=in(reg)&mut output,status=in(reg)&mut status,options(nostack),
            );}
        }}
        if double {
            run!("fstp qword ptr [{output}]");
        } else {
            run!("fstp dword ptr [{output}]");
        }
        (output, status)
    }

    #[test]
    fn x87_extended_stores_match_hardware_specials_rounding_and_exception_priority() {
        let mut seed = 0x44a61e3897u64;
        let mut values = alloc::vec![Extended::ZERO, Extended::INDEFINITE];
        for exponent in [
            0, 1, 0x3bfc, 0x3bff, 0x3c00, 0x3f80, 0x3fff, 0x4000, 0x43ff, 0x7ffe, 0x7fff,
        ] {
            for significand in [
                0,
                1,
                0x7fff_ffff_ffff_ffff,
                0x8000_0000_0000_0000,
                0x8000_0000_0000_0001,
                u64::MAX,
            ] {
                for sign in [0, 0x8000] {
                    values.push(Extended {
                        significand,
                        sign_exponent: exponent | sign,
                    });
                }
            }
        }
        for _ in 0..256 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            values.push(Extended {
                significand: seed,
                sign_exponent: (seed >> 32) as u16,
            });
        }
        for double in [false, true] {
            for rounding in 0..4 {
                for unmask in [0, 1, 8, 16, 32] {
                    let control = (0x37f | rounding << 10) & !unmask;
                    for &value in &values {
                        let hardware = hardware_float_store(value, double, control);
                        let mut s = GuestState::new();
                        s.x87_control = control;
                        s.x87_status = 0x3800;
                        s.x87_empty &= !(1 << 7);
                        s.x87_registers[7] = value;
                        let result = store_float(&mut s, double, true);
                        let mut expected = 0xa5a5_a5a5_a5a5_a5a5u64.to_le_bytes();
                        if let Some(v) = result {
                            let size = if double { 8 } else { 4 };
                            expected[..size].copy_from_slice(&v.to_le_bytes()[..size]);
                        }
                        assert_eq!(
                            (u64::from_le_bytes(expected), s.x87_status),
                            hardware,
                            "value={value:?} double={double} control={control:#x}"
                        );
                    }
                }
            }
        }
    }

    fn hardware(bits: u64, double: bool, width: u8, control: u16) -> (u64, u16) {
        let mut saved = Saved([0; 512]);
        let mut output = 0xa5a5_a5a5_a5a5_a5a5u64;
        let mut status = 0u16;
        macro_rules! run {
            ($load:literal, $store:literal) => {
                // SAFETY: all operands point to live, correctly sized buffers.
                // Save/restore the full host FP environment within one asm
                // block; inspect pending exceptions before a waiting FISTP.
                unsafe { core::arch::asm!(
                    "fxsave64 [{saved}]", "fninit", "fldcw word ptr [{control}]",
                    $load, "fnstsw ax", "test ax, 0x80", "jnz 2f", $store,
                    "2:", "fnstsw word ptr [{status}]", "fxrstor64 [{saved}]",
                    saved=in(reg) saved.0.as_mut_ptr(), control=in(reg) &control,
                    input=in(reg) &bits, output=in(reg) &mut output, status=in(reg) &mut status,
                    out("ax") _, options(nostack),
                ); }
            }
        }
        match (double, width) {
            (false, 2) => run!("fld dword ptr [{input}]", "fistp word ptr [{output}]"),
            (false, 4) => run!("fld dword ptr [{input}]", "fistp dword ptr [{output}]"),
            (false, 8) => run!("fld dword ptr [{input}]", "fistp qword ptr [{output}]"),
            (true, 2) => run!("fld qword ptr [{input}]", "fistp word ptr [{output}]"),
            (true, 4) => run!("fld qword ptr [{input}]", "fistp dword ptr [{output}]"),
            (true, 8) => run!("fld qword ptr [{input}]", "fistp qword ptr [{output}]"),
            _ => unreachable!(),
        }
        (output, status)
    }

    #[test]
    fn x87_load_and_integer_conversion_match_hardware_rounding_exceptions_and_stack() {
        let mut random = 0x9133_4234_37cd_fa12u64;
        for double in [false, true] {
            let mut values = alloc::vec![
                0,
                1,
                0x8000_0000,
                0x8000_0000_0000_0000,
                0x7f80_0000,
                0xff80_0000,
                0x7fc0_0000,
                0x7f80_0001,
                0x7ff0_0000_0000_0000,
                0xfff0_0000_0000_0000,
                0x7ff8_0000_0000_0000,
                0x7ff0_0000_0000_0001
            ];
            for value in [
                0.5f64,
                -0.5,
                1.5,
                -1.5,
                5.75,
                -5.75,
                32767.5,
                -32768.5,
                2147483647.5,
                -2147483648.5,
                9223372036854775808.0,
                -9223372036854775808.0,
            ] {
                values.push(if double {
                    value.to_bits()
                } else {
                    (value as f32).to_bits() as u64
                });
            }
            for _ in 0..512 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                values.push(random);
            }
            for width in [2, 4, 8] {
                for rounding in 0..4 {
                    for unmask in [0, 1, 2, 32] {
                        let control = (0x37f | (rounding << 10)) & !unmask;
                        for &bits in &values {
                            let (output, status) = hardware(bits, double, width, control);
                            let mut state = GuestState::new();
                            state.x87_control = control;
                            load(&mut state, bits, double);
                            let converted = if wait(&state, 0).is_ok() {
                                store_integer(&mut state, width, true)
                            } else {
                                None
                            };
                            let mut expected = 0xa5a5_a5a5_a5a5_a5a5u64.to_le_bytes();
                            if let Some(value) = converted {
                                expected[..width as usize]
                                    .copy_from_slice(&value.to_le_bytes()[..width as usize]);
                            }
                            assert_eq!(
                                (u64::from_le_bytes(expected), state.x87_status),
                                (output, status),
                                "bits={bits:#x} double={double} width={width} control={control:#x}"
                            );
                        }
                    }
                }
            }
        }
    }
}

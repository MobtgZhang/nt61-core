//! Integer-only IEEE-754 arithmetic and comparisons for SSE lanes. Three guard bits
//! retain rounding information; right shifts jam discarded bits into sticky.
//! No result depends on the host floating-point environment or architecture.
fn jam(value: u128, shift: u32) -> u128 {
    if shift == 0 {
        value
    } else if shift >= 128 {
        u128::from(value != 0)
    } else {
        (value >> shift) | u128::from(value & ((1u128 << shift) - 1) != 0)
    }
}

pub(super) fn from_integer(bits: u64, double: bool, wide: bool, mxcsr: u32) -> (u64, u8) {
    let signed = if wide {
        bits as i64
    } else {
        bits as i32 as i64
    };
    let magnitude = signed.unsigned_abs();
    if magnitude == 0 {
        return (0, 0);
    }
    let highest = 63 - magnitude.leading_zeros();
    let fraction = if double { 52 } else { 23 };
    let shift = highest as i32 - (fraction + 3) as i32;
    let significand = if shift > 0 {
        jam(u128::from(magnitude), shift as u32)
    } else {
        u128::from(magnitude) << -shift
    };
    round_pack(
        significand,
        highest as i32 + if double { 1023 } else { 127 },
        signed < 0,
        fraction,
        if double { 2047 } else { 255 },
        mxcsr,
        0,
    )
}

pub(super) fn add_sub(
    mut a: u64,
    mut b: u64,
    double: bool,
    subtract: bool,
    mxcsr: u32,
) -> (u64, u8) {
    let fraction = if double { 52 } else { 23 };
    let exponent_bits = if double { 11 } else { 8 };
    let sign = 1u64 << (fraction + exponent_bits);
    let fraction_mask = (1u64 << fraction) - 1;
    let max_exponent = (1u64 << exponent_bits) - 1;
    let infinity = max_exponent << fraction;
    let quiet = 1u64 << (fraction - 1);
    let exp = |v: u64| (v >> fraction) & max_exponent;
    let nan = |v: u64| exp(v) == max_exponent && v & fraction_mask != 0;
    let signaling = |v: u64| nan(v) && v & quiet == 0;
    if nan(a) || nan(b) {
        let invalid = u8::from(signaling(a) || signaling(b));
        return ((if nan(a) { a } else { b }) | quiet, invalid);
    }
    let mut flags = 0;
    for value in [&mut a, &mut b] {
        if exp(*value) == 0 && *value & fraction_mask != 0 {
            if mxcsr & (1 << 6) != 0 {
                *value &= sign;
            } else {
                flags |= 2;
            }
        }
    }
    if subtract {
        b ^= sign;
    }
    if exp(a) == max_exponent || exp(b) == max_exponent {
        if exp(a) == max_exponent && exp(b) == max_exponent && (a ^ b) & sign != 0 {
            return (sign | infinity | quiet, 1);
        }
        return (if exp(a) == max_exponent { a } else { b }, flags);
    }
    // Put the larger magnitude first, so opposite signs always subtract a
    // smaller significand. Exponent 0 uses exponent 1 without a hidden bit.
    if a & !sign < b & !sign {
        core::mem::swap(&mut a, &mut b);
    }
    let mut exponent = exp(a).max(1) as u32;
    let other_exponent = exp(b).max(1) as u32;
    let significand = |v: u64| -> u128 {
        u128::from((v & fraction_mask) | if exp(v) != 0 { 1 << fraction } else { 0 }) << 3
    };
    let left = significand(a);
    let right = jam(significand(b), exponent - other_exponent);
    let negative = a & sign != 0;
    let same_sign = (a ^ b) & sign == 0;
    let mut value = if same_sign {
        left + right
    } else {
        left - right
    };
    let rounding = (mxcsr >> 13) & 3;
    if value == 0 {
        let negative_zero = if same_sign { negative } else { rounding == 1 };
        return (if negative_zero { sign } else { 0 }, flags);
    }
    let hidden = 1u128 << (fraction + 3);
    if value >= hidden << 1 {
        value = jam(value, 1);
        exponent += 1;
    } else {
        while value < hidden && exponent > 1 {
            value <<= 1;
            exponent -= 1;
        }
    }
    round_pack(
        value,
        exponent as i32,
        negative,
        fraction,
        max_exponent,
        mxcsr,
        flags,
    )
}

pub(super) fn round_pack(
    mut value: u128,
    mut exponent: i32,
    negative: bool,
    fraction: u32,
    max_exponent: u64,
    mxcsr: u32,
    mut flags: u8,
) -> (u64, u8) {
    let sign = 1u64 << (fraction + if fraction == 52 { 11 } else { 8 });
    let infinity = max_exponent << fraction;
    let fraction_mask = (1u64 << fraction) - 1;
    let rounding = (mxcsr >> 13) & 3;
    if exponent <= 0 {
        value = jam(value, (1 - exponent) as u32);
        exponent = 1;
    }
    let tail = value & 7;
    let mut rounded = value >> 3;
    let increment = match rounding {
        0 => tail > 4 || (tail == 4 && rounded & 1 != 0),
        1 => negative && tail != 0,
        2 => !negative && tail != 0,
        _ => false,
    };
    rounded += u128::from(increment);
    if rounded >= 1u128 << (fraction + 1) {
        rounded >>= 1;
        exponent += 1;
    }
    let sign_bit = if negative { sign } else { 0 };
    if exponent as u64 >= max_exponent {
        let to_infinity =
            rounding == 0 || (rounding == 1 && negative) || (rounding == 2 && !negative);
        return (
            sign_bit | if to_infinity { infinity } else { infinity - 1 },
            flags | 8 | 32,
        );
    }
    let subnormal = exponent == 1 && rounded < 1u128 << fraction;
    if tail != 0 {
        flags |= 32;
        if subnormal {
            flags |= 16;
        }
    }
    // With underflow masked, FTZ flushes even an exactly representable tiny
    // nonzero result and raises both underflow and precision exceptions.
    if subnormal && mxcsr & (1 << 15) != 0 && mxcsr & (1 << 11) != 0 {
        return (sign_bit, flags | 16 | 32);
    }
    let exponent_field = if subnormal {
        0
    } else {
        (exponent as u64) << fraction
    };
    (
        sign_bit | exponent_field | (rounded as u64 & fraction_mask),
        flags,
    )
}

/// IEEE-754 division, retaining quotient remainder as a sticky rounding bit.
pub(super) fn divide(mut a: u64, mut b: u64, double: bool, mxcsr: u32) -> (u64, u8) {
    let fraction = if double { 52 } else { 23 };
    let max_exponent = if double { 2047 } else { 255 };
    let bias = max_exponent / 2;
    let sign = 1u64 << (fraction + if double { 11 } else { 8 });
    let fraction_mask = (1u64 << fraction) - 1;
    let infinity = max_exponent << fraction;
    let quiet = 1u64 << (fraction - 1);
    let exp = |v: u64| (v >> fraction) & max_exponent;
    let nan = |v: u64| exp(v) == max_exponent && v & fraction_mask != 0;
    if nan(a) || nan(b) {
        let invalid = (nan(a) && a & quiet == 0) || (nan(b) && b & quiet == 0);
        return ((if nan(a) { a } else { b }) | quiet, u8::from(invalid));
    }
    let mut flags = 0;
    for value in [&mut a, &mut b] {
        if exp(*value) == 0 && *value & fraction_mask != 0 {
            if mxcsr & 0x40 != 0 {
                *value &= sign;
            } else {
                flags |= 2;
            }
        }
    }
    let sign_bit = (a ^ b) & sign;
    let zero_a = a & !sign == 0;
    let zero_b = b & !sign == 0;
    let inf_a = exp(a) == max_exponent;
    let inf_b = exp(b) == max_exponent;
    if (zero_a && zero_b) || (inf_a && inf_b) {
        return (sign | infinity | quiet, 1);
    }
    if inf_a {
        return (sign_bit | infinity, flags);
    }
    if inf_b || zero_a {
        return (sign_bit, flags);
    }
    // Divide-by-zero takes precedence over the denormal-operand condition.
    if zero_b {
        return (sign_bit | infinity, 4);
    }
    let normalize = |v: u64| {
        let mut exponent = exp(v).max(1) as i32;
        let mut mantissa = (v & fraction_mask) | if exp(v) != 0 { 1u64 << fraction } else { 0 };
        while mantissa < 1u64 << fraction {
            mantissa <<= 1;
            exponent -= 1;
        }
        (u128::from(mantissa), exponent)
    };
    let (mut left, left_exp) = normalize(a);
    let (right, right_exp) = normalize(b);
    let mut exponent = left_exp - right_exp + bias as i32;
    if left < right {
        left <<= 1;
        exponent -= 1;
    }
    let numerator = left << (fraction + 3);
    let quotient = numerator / right | u128::from(numerator % right != 0);
    round_pack(
        quotient,
        exponent,
        sign_bit != 0,
        fraction,
        max_exponent,
        mxcsr,
        flags,
    )
}

/// IEEE-754 multiplication with an exact (at most 106-bit) product.
pub(super) fn multiply(mut a: u64, mut b: u64, double: bool, mxcsr: u32) -> (u64, u8) {
    let fraction = if double { 52 } else { 23 };
    let max_exponent = if double { 2047 } else { 255 };
    let bias = max_exponent / 2;
    let sign = 1u64 << (fraction + if double { 11 } else { 8 });
    let fraction_mask = (1u64 << fraction) - 1;
    let infinity = max_exponent << fraction;
    let quiet = 1u64 << (fraction - 1);
    let exp = |v: u64| (v >> fraction) & max_exponent;
    let nan = |v: u64| exp(v) == max_exponent && v & fraction_mask != 0;
    if nan(a) || nan(b) {
        let invalid = (nan(a) && a & quiet == 0) || (nan(b) && b & quiet == 0);
        return ((if nan(a) { a } else { b }) | quiet, u8::from(invalid));
    }
    let mut flags = 0;
    for value in [&mut a, &mut b] {
        if exp(*value) == 0 && *value & fraction_mask != 0 {
            if mxcsr & 0x40 != 0 {
                *value &= sign;
            } else {
                flags |= 2;
            }
        }
    }
    let sign_bit = (a ^ b) & sign;
    let zero_a = a & !sign == 0;
    let zero_b = b & !sign == 0;
    let inf_a = exp(a) == max_exponent;
    let inf_b = exp(b) == max_exponent;
    if (zero_a && inf_b) || (inf_a && zero_b) {
        return (sign | infinity | quiet, 1);
    }
    if inf_a || inf_b {
        return (sign_bit | infinity, flags);
    }
    if zero_a || zero_b {
        return (sign_bit, flags);
    }
    let normalize = |v: u64| {
        let mut exponent = exp(v).max(1) as i32;
        let mut mantissa = (v & fraction_mask) | if exp(v) != 0 { 1u64 << fraction } else { 0 };
        while mantissa < 1u64 << fraction {
            mantissa <<= 1;
            exponent -= 1;
        }
        (u128::from(mantissa), exponent)
    };
    let (left, left_exp) = normalize(a);
    let (right, right_exp) = normalize(b);
    let mut exponent = left_exp + right_exp - bias as i32;
    let product = left * right;
    let mut shift = fraction - 3;
    if product >= 1u128 << (fraction * 2 + 1) {
        shift += 1;
        exponent += 1;
    }
    round_pack(
        jam(product, shift),
        exponent,
        sign_bit != 0,
        fraction,
        max_exponent,
        mxcsr,
        flags,
    )
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;

    fn hardware(a: u64, b: u64, double: bool, operation: u8, control: u32) -> (u64, u8) {
        let mut saved = 0u32;
        let mut status = 0u32;
        let mut output = 0u64;
        let mut flags = 0u64;
        macro_rules! run {
            ($instruction:literal) => {
                // SAFETY: all pointers refer to live, correctly sized locals.
                // Every exception is masked. Restore host MXCSR in this same
                // assembly block before any Rust code can observe a change.
                // PUSHFQ/POP balances the stack and reads only a local flag word.
                unsafe { core::arch::asm!(
                    "stmxcsr [{saved}]", "ldmxcsr [{control}]",
                    "movq xmm0, [{a}]", "movq xmm1, [{b}]", $instruction,
                    "movq [{output}], xmm0", "pushfq", "pop {flags}",
                    "stmxcsr [{status}]", "ldmxcsr [{saved}]",
                    saved = in(reg) &mut saved, control = in(reg) &control,
                    a = in(reg) &a, b = in(reg) &b, output = in(reg) &mut output,
                    status = in(reg) &mut status, flags = out(reg) flags,
                    out("xmm0") _, out("xmm1") _,
                ); }
            }
        }
        match (double, operation) {
            (true, 4) => run!("ucomisd xmm0, xmm1"),
            (false, 4) => run!("ucomiss xmm0, xmm1"),
            (true, 3) => run!("mulsd xmm0, xmm1"),
            (false, 3) => run!("mulss xmm0, xmm1"),
            (true, 2) => run!("divsd xmm0, xmm1"),
            (false, 2) => run!("divss xmm0, xmm1"),
            (true, 1) => run!("subsd xmm0, xmm1"),
            (true, _) => run!("addsd xmm0, xmm1"),
            (false, 1) => run!("subss xmm0, xmm1"),
            (false, _) => run!("addss xmm0, xmm1"),
        }
        (
            if operation == 4 {
                flags & 0x8d5
            } else if double {
                output
            } else {
                output as u32 as u64
            },
            status as u8 & 0x3f,
        )
    }

    #[test]
    fn integer_sse_arithmetic_matches_hardware_rounding_denormals_and_special_values() {
        for double in [false, true] {
            let values: &[u64] = if double {
                &[
                    0,
                    1,
                    0x000fffffffffffff,
                    0x0010000000000000,
                    0x3ff0000000000000,
                    0x3ca0000000000000,
                    0x7fefffffffffffff,
                    0x7ff0000000000000,
                    0x7ff0000000000001,
                    0x7ff8000000000002,
                    0xfff0000000000003,
                    0xfff8000000000004,
                    0x8000000000000000,
                    0xbff0000000000000,
                ]
            } else {
                &[
                    0, 1, 0x007fffff, 0x00800000, 0x3f800000, 0x33800000, 0x7f7fffff, 0x7f800000,
                    0x7f800001, 0x7fc00002, 0xff800003, 0xffc00004, 0x80000000, 0xbf800000,
                ]
            };
            let mut seed = 0x9e3779b97f4a7c15u64;
            for rounding in 0..4 {
                for policy in [0, 0x40, 0x8000, 0x8040] {
                    let control = 0x1f80 | rounding << 13 | policy;
                    for operation in 0..5 {
                        let subtract = operation == 1;
                        let software = |a, b| {
                            if operation == 4 {
                                compare(a, b, double, control)
                            } else if operation == 3 {
                                multiply(a, b, double, control)
                            } else if operation == 2 {
                                divide(a, b, double, control)
                            } else {
                                add_sub(a, b, double, subtract, control)
                            }
                        };
                        for &a in values {
                            for &b in values {
                                assert_eq!(software(a,b), hardware(a,b,double,operation,control),
                                "a={a:x} b={b:x} double={double} op={operation} mxcsr={control:x}");
                            }
                        }
                        for _ in 0..4096 {
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            let a = if double { seed } else { seed as u32 as u64 };
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            let b = if double { seed } else { seed as u32 as u64 };
                            assert_eq!(
                                software(a, b),
                                hardware(a, b, double, operation, control),
                                "a={a:x} b={b:x} double={double} op={operation} mxcsr={control:x}"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// UCOMISS/UCOMISD comparison as integer sign/magnitude, with quiet-NaN rules.
pub(super) fn compare(mut a: u64, mut b: u64, double: bool, mxcsr: u32) -> (u64, u8) {
    let fraction = if double { 52 } else { 23 };
    let sign = 1u64 << (fraction + if double { 11 } else { 8 });
    let infinity = if double {
        0x7ff0000000000000
    } else {
        0x7f800000
    };
    let quiet = 1u64 << (fraction - 1);
    let mag = |v: u64| v & !sign;
    if mag(a) > infinity || mag(b) > infinity {
        let invalid =
            (mag(a) > infinity && a & quiet == 0) || (mag(b) > infinity && b & quiet == 0);
        return (0x45, u8::from(invalid));
    }
    let mut flags = 0;
    for value in [&mut a, &mut b] {
        if mag(*value) != 0 && mag(*value) < 1u64 << fraction {
            if mxcsr & 0x40 != 0 {
                *value &= sign;
            } else {
                flags |= 2;
            }
        }
    }
    let relation = if a == b || (mag(a) == 0 && mag(b) == 0) {
        0x40
    } else if if (a ^ b) & sign != 0 {
        a & sign != 0
    } else if a & sign != 0 {
        mag(a) > mag(b)
    } else {
        mag(a) < mag(b)
    } {
        1
    } else {
        0
    };
    (relation, flags)
}

/// CVTTSS2SI/CVTTSD2SI truncate independently of MXCSR's rounding mode.
/// These conversions signal invalid/precision, never denormal-operand.
pub(super) fn truncate_integer(value: u64, double: bool, wide: bool, mxcsr: u32) -> (u64, u8) {
    let fraction = if double { 52 } else { 23 };
    let exponent_mask = if double { 2047 } else { 255 };
    let exponent = (value >> fraction) & exponent_mask;
    let mantissa = value & ((1u64 << fraction) - 1);
    let negative = value & (1u64 << (fraction + if double { 11 } else { 8 })) != 0;
    let limit = 1u64 << if wide { 63 } else { 31 };
    if exponent == exponent_mask {
        return (limit, 1);
    }
    if exponent == 0 && (mantissa == 0 || mxcsr & 0x40 != 0) {
        return (0, 0);
    }
    let binary_exponent = exponent.max(1) as i32 - (exponent_mask / 2) as i32;
    if binary_exponent < 0 {
        return (0, 32);
    }
    if binary_exponent > if wide { 63 } else { 31 } {
        return (limit, 1);
    }
    let significant = u128::from(mantissa | if exponent != 0 { 1u64 << fraction } else { 0 });
    let shift = binary_exponent - fraction as i32;
    let (integer, inexact) = if shift >= 0 {
        (significant << shift, false)
    } else {
        let n = (-shift) as u32;
        (significant >> n, significant & ((1u128 << n) - 1) != 0)
    };
    if integer > u128::from(limit) || (!negative && integer == u128::from(limit)) {
        return (limit, 1);
    }
    let integer = integer as u64;
    let result = if negative {
        integer.wrapping_neg()
    } else {
        integer
    };
    (
        if wide { result } else { result as u32 as u64 },
        if inexact { 32 } else { 0 },
    )
}

#[cfg(all(test, target_arch = "x86_64"))]
mod conversion_tests {
    use super::*;
    fn hardware_from_integer(value: u64, double: bool, wide: bool, control: u32) -> (u64, u8) {
        let mut saved = 0u32;
        let mut status = 0u32;
        let result: u64;
        macro_rules! run {($instruction:literal)=>{
            // SAFETY: live local operands; restore host MXCSR within the same
            // block, declare xmm0 clobbered, and mask all hardware exceptions.
            unsafe {core::arch::asm!(
                "stmxcsr [{saved}]", "ldmxcsr [{control}]", "pxor xmm0,xmm0",
                $instruction, "movq {result},xmm0", "stmxcsr [{status}]", "ldmxcsr [{saved}]",
                saved=in(reg)&mut saved, control=in(reg)&control, status=in(reg)&mut status,
                value=in(reg)value, result=out(reg)result, out("xmm0")_, options(nostack)
            );}
        }}
        match (double, wide) {
            (true, true) => run!("cvtsi2sd xmm0,{value}"),
            (true, false) => run!("cvtsi2sd xmm0,{value:e}"),
            (false, true) => run!("cvtsi2ss xmm0,{value}"),
            (false, false) => run!("cvtsi2ss xmm0,{value:e}"),
        }
        (result, status as u8 & 0x3f)
    }
    #[test]
    fn signed_integer_to_sse_matches_hardware_rounding_and_precision_flags() {
        let mut seed = 0xa9fac2486123u64;
        for double in [false, true] {
            for wide in [false, true] {
                for rounding in 0..4 {
                    let control = 0x1f80 | rounding << 13;
                    let mut values = alloc::vec![
                        0,
                        1,
                        u64::MAX,
                        0x8000_0000,
                        0x7fff_ffff,
                        0x8000_0000_0000_0000,
                        0x7fff_ffff_ffff_ffff,
                        0x2000_0000_0000_01,
                        0x100_0001,
                        0x100_0003,
                        0xffff_ffff_ff00_0001
                    ];
                    for _ in 0..2048 {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        values.push(seed);
                    }
                    for value in values {
                        assert_eq!(
                            from_integer(value, double, wide, control),
                            hardware_from_integer(value, double, wide, control),
                            "value={value:#x} double={double} wide={wide} control={control:#x}"
                        );
                    }
                }
            }
        }
    }
    fn hardware(value: u64, double: bool, wide: bool, control: u32) -> (u64, u8) {
        let mut saved = 0u32;
        let mut status = 0u32;
        let result: u64;
        macro_rules! run {($instruction:literal)=>{
            // SAFETY: valid local pointers, all SIMD exceptions masked; restore
            // host MXCSR before Rust code resumes. Only xmm0 is modified.
            unsafe {core::arch::asm!(
                "stmxcsr [{saved}]","ldmxcsr [{control}]","movq xmm0,[{value}]",
                $instruction,"stmxcsr [{status}]","ldmxcsr [{saved}]",
                saved=in(reg)&mut saved,control=in(reg)&control,value=in(reg)&value,
                status=in(reg)&mut status,result=out(reg)result,out("xmm0")_,options(nostack)
            );}
        }}
        match (double, wide) {
            (true, true) => run!("cvttsd2si {result},xmm0"),
            (true, false) => run!("cvttsd2si {result:e},xmm0"),
            (false, true) => run!("cvttss2si {result},xmm0"),
            (false, false) => run!("cvttss2si {result:e},xmm0"),
        }
        (result, status as u8 & 0x3f)
    }
    #[test]
    fn truncating_sse_conversions_match_hardware_integer_limits_rounding_and_daz() {
        let mut seed = 0x42abc19f7171u64;
        for double in [false, true] {
            for wide in [false, true] {
                for rounding in 0..4 {
                    for daz in [0, 0x40] {
                        let control = 0x1f80 | rounding << 13 | daz;
                        let values: &[u64] = if double {
                            &[
                                0,
                                1,
                                0x8000000000000000,
                                0x3ff0000000000000,
                                0xbff8000000000000,
                                0x41e0000000000000,
                                0xc1e0000000000000,
                                0x43e0000000000000,
                                0xc3e0000000000000,
                                0x7ff0000000000000,
                                0x7ff0000000000001,
                                0x7ff8000000000001,
                            ]
                        } else {
                            &[
                                0, 1, 0x80000000, 0x3f800000, 0xbfc00000, 0x4f000000, 0xcf000000,
                                0x5f000000, 0xdf000000, 0x7f800000, 0x7f800001, 0x7fc00001,
                            ]
                        };
                        for &value in values {
                            assert_eq!(
                                truncate_integer(value, double, wide, control),
                                hardware(value, double, wide, control),
                                "value={value:x} double={double} wide={wide} control={control:x}"
                            );
                        }
                        for _ in 0..4096 {
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            let value = if double { seed } else { seed as u32 as u64 };
                            assert_eq!(
                                truncate_integer(value, double, wide, control),
                                hardware(value, double, wide, control),
                                "value={value:x} double={double} wide={wide} control={control:x}"
                            );
                        }
                    }
                }
            }
        }
    }
}

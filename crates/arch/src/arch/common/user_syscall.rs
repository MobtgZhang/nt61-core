//! User-mode native system-call trampoline.

#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::user_syscall::*;

#[cfg(not(target_arch = "x86_64"))]
fn padded_arguments(args: &[u64]) -> Option<[u64; 17]> {
    if args.len() > 17 {
        return None;
    }
    let mut padded = [0; 17];
    padded[..args.len()].copy_from_slice(args);
    Some(padded)
}

/// Issue a native call with up to 17 arguments, retaining stack arguments.
///
/// # Safety
/// The caller must satisfy the selected service's pointer and lifetime contract.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub unsafe fn syscall_varargs(num: u32, args: &[u64]) -> i64 {
    let Some(arguments) = padded_arguments(args) else {
        return 0xC000_000Du32 as i32 as i64;
    };
    let result: i64;
    // SAFETY: the padded input is valid for all loads, the outgoing stack
    // stays aligned to 16 bytes, and SP is restored before Rust resumes.
    core::arch::asm!(
        "sub sp, sp, #80",
        "ldr x9, [x15, #64]",
        "str x9, [sp, #0]",
        "ldr x9, [x15, #72]",
        "str x9, [sp, #8]",
        "ldr x9, [x15, #80]",
        "str x9, [sp, #16]",
        "ldr x9, [x15, #88]",
        "str x9, [sp, #24]",
        "ldr x9, [x15, #96]",
        "str x9, [sp, #32]",
        "ldr x9, [x15, #104]",
        "str x9, [sp, #40]",
        "ldr x9, [x15, #112]",
        "str x9, [sp, #48]",
        "ldr x9, [x15, #120]",
        "str x9, [sp, #56]",
        "ldr x9, [x15, #128]",
        "str x9, [sp, #64]",
        "svc #0",
        "add sp, sp, #80",
        in("x16") num as u64,
        inlateout("x0") arguments[0] => result,
        in("x1") arguments[1],
        in("x2") arguments[2],
        in("x3") arguments[3],
        in("x4") arguments[4],
        in("x5") arguments[5],
        in("x6") arguments[6],
        in("x7") arguments[7],
        in("x15") arguments.as_ptr(),
        out("x9") _,
    );
    result
}

/// Issue a native call with up to 17 arguments, retaining stack arguments.
///
/// # Safety
/// The caller must satisfy the selected service's pointer and lifetime contract.
#[cfg(target_arch = "riscv64")]
#[inline(always)]
pub unsafe fn syscall_varargs(num: u32, args: &[u64]) -> i64 {
    let Some(arguments) = padded_arguments(args) else {
        return 0xC000_000Du32 as i32 as i64;
    };
    let result: i64;
    // SAFETY: the padded input is valid for all loads, the outgoing stack
    // stays aligned to 16 bytes, and SP is restored before Rust resumes.
    core::arch::asm!(
        "addi sp, sp, -80",
        "ld t1, 56(t0)",
        "sd t1, 0(sp)",
        "ld t1, 64(t0)",
        "sd t1, 8(sp)",
        "ld t1, 72(t0)",
        "sd t1, 16(sp)",
        "ld t1, 80(t0)",
        "sd t1, 24(sp)",
        "ld t1, 88(t0)",
        "sd t1, 32(sp)",
        "ld t1, 96(t0)",
        "sd t1, 40(sp)",
        "ld t1, 104(t0)",
        "sd t1, 48(sp)",
        "ld t1, 112(t0)",
        "sd t1, 56(sp)",
        "ld t1, 120(t0)",
        "sd t1, 64(sp)",
        "ld t1, 128(t0)",
        "sd t1, 72(sp)",
        "ecall",
        "addi sp, sp, 80",
        in("a7") num as u64,
        inlateout("a0") arguments[0] => result,
        in("a1") arguments[1],
        in("a2") arguments[2],
        in("a3") arguments[3],
        in("a4") arguments[4],
        in("a5") arguments[5],
        in("a6") arguments[6],
        in("t0") arguments.as_ptr(),
        out("t1") _,
    );
    result
}

/// Issue a native call with up to 17 arguments, retaining stack arguments.
///
/// # Safety
/// The caller must satisfy the selected service's pointer and lifetime contract.
#[cfg(target_arch = "loongarch64")]
#[inline(always)]
pub unsafe fn syscall_varargs(num: u32, args: &[u64]) -> i64 {
    let Some(arguments) = padded_arguments(args) else {
        return 0xC000_000Du32 as i32 as i64;
    };
    let result: i64;
    // SAFETY: the padded input is valid for all loads, the outgoing stack
    // stays aligned to 16 bytes, and SP is restored before Rust resumes.
    core::arch::asm!(
        "addi.d $sp, $sp, -80",
        "ld.d $r13, $r12, 56",
        "st.d $r13, $sp, 0",
        "ld.d $r13, $r12, 64",
        "st.d $r13, $sp, 8",
        "ld.d $r13, $r12, 72",
        "st.d $r13, $sp, 16",
        "ld.d $r13, $r12, 80",
        "st.d $r13, $sp, 24",
        "ld.d $r13, $r12, 88",
        "st.d $r13, $sp, 32",
        "ld.d $r13, $r12, 96",
        "st.d $r13, $sp, 40",
        "ld.d $r13, $r12, 104",
        "st.d $r13, $sp, 48",
        "ld.d $r13, $r12, 112",
        "st.d $r13, $sp, 56",
        "ld.d $r13, $r12, 120",
        "st.d $r13, $sp, 64",
        "ld.d $r13, $r12, 128",
        "st.d $r13, $sp, 72",
        "syscall 0",
        "addi.d $sp, $sp, 80",
        in("$r11") num as u64,
        inlateout("$r4") arguments[0] => result,
        in("$r5") arguments[1],
        in("$r6") arguments[2],
        in("$r7") arguments[3],
        in("$r8") arguments[4],
        in("$r9") arguments[5],
        in("$r10") arguments[6],
        in("$r12") arguments.as_ptr(),
        out("$r13") _,
    );
    result
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn syscall0(num: u32) -> i64 {
    syscall_varargs(num, &[])
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn syscall1(num: u32, arg0: u64) -> i64 {
    syscall_varargs(num, &[arg0])
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn syscall2(num: u32, arg0: u64, arg1: u64) -> i64 {
    syscall_varargs(num, &[arg0, arg1])
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn syscall3(num: u32, arg0: u64, arg1: u64, arg2: u64) -> i64 {
    syscall_varargs(num, &[arg0, arg1, arg2])
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn syscall4(num: u32, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    syscall_varargs(num, &[arg0, arg1, arg2, arg3])
}

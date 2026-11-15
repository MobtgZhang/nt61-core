//! ntdll.dll — syscall macros.
//!
//! Provides the inline-asm `syscall` instruction following the
//! Microsoft x64 calling convention: arguments in RCX, RDX, R8, R9,
//! with stack arguments at RSP + 0x28 ...
//!
//! **This module is x86_64 only.** The actual `syscall` instruction
//! and Microsoft x64 ABI are architecture-specific; on LoongArch64 /
//! ARM64 / RISC-V64 the ntdll user-mode syscall stubs are provided by
//! per-architecture code under `userspace/<arch>/ntdll/`.

use core::arch::asm;

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Issue a syscall with one return value. The arguments follow the
/// Microsoft x64 ABI: r10, rdx, r8, r9 (then stack).
#[inline(always)]
pub unsafe fn syscall0(num: u32) -> i64 {
    let r: i64;
    asm!(
        "syscall",
        in("rax") num as u64,
        in("r10") 0u64,
        in("rdx") 0u64,
        in("r8")  0u64,
        in("r9")  0u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") r,
        options(nostack),
    );
    r
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn syscall1(num: u32, a0: u64) -> i64 {
    let r: i64;
    asm!(
        "syscall",
        in("rax") num as u64,
        in("r10") a0,
        in("rdx") 0u64,
        in("r8")  0u64,
        in("r9")  0u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") r,
        options(nostack),
    );
    r
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn syscall2(num: u32, a0: u64, a1: u64) -> i64 {
    let r: i64;
    asm!(
        "syscall",
        in("rax") num as u64,
        in("r10") a0,
        in("rdx") a1,
        in("r8")  0u64,
        in("r9")  0u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") r,
        options(nostack),
    );
    r
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn syscall3(num: u32, a0: u64, a1: u64, a2: u64) -> i64 {
    let r: i64;
    asm!(
        "syscall",
        in("rax") num as u64,
        in("r10") a0,
        in("rdx") a1,
        in("r8")  a2,
        in("r9")  0u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") r,
        options(nostack),
    );
    r
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
#[inline(always)]
pub unsafe fn syscall4(num: u32, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let r: i64;
    asm!(
        "syscall",
        in("rax") num as u64,
        in("r10") a0,
        in("rdx") a1,
        in("r8")  a2,
        in("r9")  a3,
        out("rcx") _,
        out("r11") _,
        lateout("rax") r,
        options(nostack),
    );
    r
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Variable-arg syscall used by APIs with five or more parameters.
#[inline(always)]
pub unsafe fn syscall_varargs(num: u32, args: &[u64]) -> i64 {
    if args.len() > crate::arch::common::syscall_frame::SYSCALL_ARGUMENT_COUNT {
        return 0xC000_000Du32 as i32 as i64;
    }
    let tail = args.get(4..).unwrap_or(&[]);
    // Native Win7 stubs retain a return address and four home slots. The
    // project's private services retain their historical one-slot prefix.
    let stack_offset = if num < 0x200 || (0x1000..0x2000).contains(&num) {
        40usize
    } else {
        8
    };
    let result: i64;
    // SAFETY: reserve an outgoing frame below the SysV red zone, copy all
    // trailing arguments, and restore RSP before returning to Rust. The
    // bounded 13-slot tail cannot overlap compiler-owned red-zone locals.
    asm!(
        "sub rsp, 384",
        "add rdi, rsp",
        "rep movsq",
        "syscall",
        "add rsp, 384",
        inlateout("rax") num as u64 => result,
        in("r10") args.first().copied().unwrap_or(0),
        in("rdx") args.get(1).copied().unwrap_or(0),
        in("r8") args.get(2).copied().unwrap_or(0),
        in("r9") args.get(3).copied().unwrap_or(0),
        inout("rsi") tail.as_ptr() => _,
        inout("rdi") stack_offset => _,
        inout("rcx") tail.len() => _,
        out("r11") _,
    );
    result
}

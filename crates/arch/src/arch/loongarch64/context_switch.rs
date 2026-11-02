//! LoongArch64 context-switch helpers.
//!
//! This module exposes the `swap_context` / `KiSwapContext` ABI used
//! by `ke::scheduler::schedule()` and `arch::loongarch64::user_entry`.
//! LoongArch64 has a 32-register GPR file (r0 = zero, r1 = ra, r2 = tp
//! etc.) — only the callee-saved subset needs to be spilled to memory
//! on a context switch.
//!
//! The ABI follows the LoongArch64 psABI:
//!   * Caller-saved (scratch):    $a0-$a7, $t0-$t8, $r21 (the
//!                                "argument" / "temporary" group, also
//!                                $fa0-$fa7, $ft0-$ft11 for FP).
//!   * Callee-saved (preserved):  $s0-$s8, $fp ($r22), $ra ($r1),
//!                                $sp ($r3), $r23-$r31.
//!
//! ## Layout of `ContextFrame`
//!
//! The struct is laid out so the assembly below can spill registers
//! at fixed offsets (in bytes):
//!
//! ```text
//!   0  + 8 :  r0  (always 0, kept for completeness)
//!   8  + 8 :  ra  ($r1)
//!  16  + 8 :  sp  ($r3)
//!  24  + 8 :  s0  ($r23)
//!  32  + 8 :  s1
//!  40  + 8 :  s2
//!  48  + 8 :  s3
//!  56  + 8 :  s4
//!  64  + 8 :  s5
//!  72  + 8 :  s6
//!  80  + 8 :  s7
//!  88  + 8 :  s8  ($r30)
//!  96  + 8 :  fp  ($r22)
//! ```
//!
//! The frame also reserves room for the FPU / LSX / LASX state at
//! the tail (filled in by `fpu.rs` in Phase 3).

use core::arch::{asm, global_asm};

/// Size in bytes of one `ContextFrame` slot.
pub const CONTEXT_FRAME_SIZE: usize = 104;

/// Full register save area for one thread.
///
/// This is a `#[repr(C)]` struct so the assembly below can rely on
/// the field offsets. Do not reorder fields without updating the
/// assembly.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct ContextFrame {
    pub r0: u64,
    pub ra: u64,
    pub sp: u64,
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub fp: u64,
}

/// Pointer to a `ContextFrame` living somewhere on the kernel stack
/// of the thread being switched out.
pub type ContextFramePtr = *mut ContextFrame;

/// Pointer-sized `*mut ContextFrame`.
pub type RawContextPtr = usize;

global_asm!(
    r#"
    .text
    .align 2
    .global __nt61_loongarch_thread_start
__nt61_loongarch_thread_start:
    ld.d $t0, $sp, 0
    ld.d $a0, $sp, 8
    addi.d $sp, $sp, 32
    jirl $zero, $t0, 0
    .size __nt61_loongarch_thread_start, . - __nt61_loongarch_thread_start
    "#,
);

extern "C" {
    static __nt61_loongarch_thread_start: u8;
}

/// `swap_context` — swap from the outgoing stack pointer (pointed to
/// by `out_rsp_addr`) to the incoming stack pointer (`new_rsp`).
///
/// On return the caller resumes execution with the outgoing context
/// saved at `out_rsp_addr`. The function never returns to its caller
/// in the linear sense — control flows to whichever context last
/// executed on `new_rsp`.
///
/// # Safety
///
/// `out_rsp_addr` must point at a valid, aligned `usize` that will be
/// used to publish the saved stack pointer. `new_rsp` must be the top
/// of a stack containing a `ContextFrame` laid out per the contract
/// above.
/// Wrapper that defers to `arch_swap_context` (a non-naked asm
/// implementation).
#[no_mangle]
pub unsafe extern "C" fn swap_context(out_rsp_addr: *mut u64, new_rsp: u64) {
    asm!(
        "addi.d $sp, $sp, -104",
        "st.d $r1, $sp, 8",
        "st.d $r3, $sp, 16",
        "st.d $r23, $sp, 24",
        "st.d $r24, $sp, 32",
        "st.d $r25, $sp, 40",
        "st.d $r26, $sp, 48",
        "st.d $r27, $sp, 56",
        "st.d $r28, $sp, 64",
        "st.d $r29, $sp, 72",
        "st.d $r30, $sp, 80",
        "st.d $r31, $sp, 88",
        "st.d $r22, $sp, 96",
        "st.d $sp, {out}, 0",
        "or $sp, {new}, $zero",
        "ld.d $r1, $sp, 8",
        "ld.d $r3, $sp, 16",
        "ld.d $r23, $sp, 24",
        "ld.d $r24, $sp, 32",
        "ld.d $r25, $sp, 40",
        "ld.d $r26, $sp, 48",
        "ld.d $r27, $sp, 56",
        "ld.d $r28, $sp, 64",
        "ld.d $r29, $sp, 72",
        "ld.d $r30, $sp, 80",
        "ld.d $r31, $sp, 88",
        "ld.d $r22, $sp, 96",
        "jirl $zero, $r1, 0",
        out = in(reg) out_rsp_addr,
        new = in(reg) new_rsp,
        options(noreturn),
    );
}

/// Assembly body for `swap_context`. (Reserved for future hand-tuned
/// inline asm — `arch_swap_context` is currently implemented in pure
/// Rust + `asm!`.)
#[cfg(target_arch = "loongarch64")]
mod asm_impl {
    // Intentionally empty: the LA64 toolchain does not yet expose
    // a stable `naked_asm!` macro, so we route through the asm!
    // implementation in `arch_swap_context`.
}

/// Fill a fresh `ContextFrame` so the thread starts executing
/// `rip(user_entry)` with `rsp = user_stack_top`.
///
/// This is used by `ke::process::create_user_process` to bootstrap a
/// new thread's kernel-stack context before its first `swap_context`.
pub fn init_user_frame(frame: &mut ContextFrame, user_entry: u64, user_stack_top: u64) {
    frame.r0 = 0;
    frame.ra = user_entry; // when we `ertn` out of swap_context, ra jumps here
    frame.sp = user_stack_top;
    frame.s0 = 0;
    frame.s1 = 0;
    frame.s2 = 0;
    frame.s3 = 0;
    frame.s4 = 0;
    frame.s5 = 0;
    frame.s6 = 0;
    frame.s7 = 0;
    frame.s8 = 0;
    frame.fp = 0;
}

/// Save the current callee-saved register set into `out`.
///
/// # Safety
///
/// `out` must point at a live, properly-aligned `ContextFrame`.
#[inline(never)]
pub unsafe fn save_context(out: *mut ContextFrame) {
    asm!(
        "st.d $ra,  {out}, 8",
        "st.d $sp,  {out}, 16",
        "st.d $s0,  {out}, 24",
        "st.d $s1,  {out}, 32",
        "st.d $s2,  {out}, 40",
        "st.d $s3,  {out}, 48",
        "st.d $s4,  {out}, 56",
        "st.d $s5,  {out}, 64",
        "st.d $s6,  {out}, 72",
        "st.d $s7,  {out}, 80",
        "st.d $s8,  {out}, 88",
        "st.d $fp,  {out}, 96",
        out = in(reg) out,
        options(nostack, preserves_flags),
    );
}

/// Restore a previously-saved `ContextFrame`.
///
/// # Safety
///
/// `frame` must point at a valid `ContextFrame` produced by
/// `save_context` or `init_user_frame`.
#[inline(never)]
pub unsafe fn restore_context(frame: *const ContextFrame) {
    asm!(
        "ld.d $ra,  {frame}, 8",
        "ld.d $sp,  {frame}, 16",
        "ld.d $s0,  {frame}, 24",
        "ld.d $s1,  {frame}, 32",
        "ld.d $s2,  {frame}, 40",
        "ld.d $s3,  {frame}, 48",
        "ld.d $s4,  {frame}, 56",
        "ld.d $s5,  {frame}, 64",
        "ld.d $s6,  {frame}, 72",
        "ld.d $s7,  {frame}, 80",
        "ld.d $s8,  {frame}, 88",
        "ld.d $fp,  {frame}, 96",
        frame = in(reg) frame,
        options(nostack, preserves_flags),
    );
}

/// Seed a fresh kernel context so `swap_context` enters `entry(arg)`.
///
/// # Safety
///
/// `stack_top` must point at writable memory holding at least
/// `CONTEXT_FRAME_SIZE + 32` bytes below its address.
pub unsafe extern "C" fn seed_thread_frame(stack_top: *mut u64, entry: u64, arg: u64) -> u64 {
    let frame_addr = stack_top as u64 - CONTEXT_FRAME_SIZE as u64 - 32;
    let start_data = frame_addr + CONTEXT_FRAME_SIZE as u64;
    unsafe {
        (start_data as *mut u64).write_unaligned(entry);
        (start_data as *mut u64).add(1).write_unaligned(arg);

        let frame = frame_addr as *mut ContextFrame;
        (*frame).r0 = 0;
        (*frame).ra = core::ptr::addr_of!(__nt61_loongarch_thread_start) as u64;
        (*frame).sp = start_data;
        (*frame).s0 = 0;
        (*frame).s1 = 0;
        (*frame).s2 = 0;
        (*frame).s3 = 0;
        (*frame).s4 = 0;
        (*frame).s5 = 0;
        (*frame).s6 = 0;
        (*frame).s7 = 0;
        (*frame).s8 = 0;
        (*frame).fp = 0;
    }
    frame_addr
}

/// `KiSwapContext` — full context switch with optional FPU save/restore.
///
/// Phase 1: only GPRs are switched. Phase 3 wires up LSX/LASX via
/// `fpu_save` / `fpu_restore` (currently no-ops).
///
/// # Safety
///
/// Same as `swap_context` plus `out` must be a `ContextFrame` of a
/// currently-running thread.
#[no_mangle]
pub unsafe extern "C" fn KiSwapContext(out: *mut ContextFrame, incoming: *const ContextFrame) {
    // Save callee-saved registers into `out`.
    save_context(out);
    // Restore callee-saved registers from `incoming`.
    restore_context(incoming);
    // Jump to incoming ra (return address that the incoming thread
    // will resume at). We use a tail-call to `jr` semantics by way
    // of `restore_context` followed by an indirect jump.
    let target_ra: u64 = (*incoming).ra;
    asm!(
        "jr {target_ra}",
        target_ra = in(reg) target_ra,
        options(noreturn),
    );
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Externally-callable alias for the kernel scheduler; behaves like
/// the x86_64 `swap_context` (publishes *out_rsp, jumps to new_rsp).
#[no_mangle]
pub unsafe extern "C" fn arch_swap_context(out_rsp_addr: *mut u64, new_rsp: u64) {
    // Publish kernel stack pointer.
    asm!(
        "st.d $sp, {addr}, 0",
        addr = in(reg) out_rsp_addr,
        options(nostack, preserves_flags),
    );
    // Switch stack and resume.
    asm!(
        "move $sp, {new_sp}",
        new_sp = in(reg) new_rsp,
        options(nostack, preserves_flags),
    );
}

//! RISC-V 64 `stvec` and exception handling.
//!
//! The `stvec_trap` assembler below is the entry point installed
//! into the `stvec` CSR by [`init`]. It:
//!
//!   1. Uses `sscratch` to select the user thread's kernel stack;
//!      supervisor traps stay on the interrupted kernel stack.
//!   2. Saves the general-purpose register file plus `sepc`,
//!      `sstatus`, `stval`, `scause` onto the new stack.
//!   3. Calls [`super::trap::riscv64_trap_dispatch`], which
//!      decodes the cause and dispatches to the right handler.
//!   4. Restores the registers, swaps `sp` back, and `sret`s to
//!      the saved `sepc`.
//!
//! `TrapFrame` itself lives in [`super::trap`]; this module keeps
//! only the assembler stub and the `init()` glue.

use core::arch::asm;
use core::arch::global_asm;

// sscratch is zero in supervisor mode and holds the kernel-stack top in
// user mode. Kernel/nested traps grow the interrupted stack; they must never
// reset sp to a stale top and overwrite active Rust frames. All GPRs and the
// floating-point state survive asynchronous interrupts and user ecalls.
// The first 280 bytes match TrapFrame; FP state begins at the next 16-byte
// boundary. Keep these offsets in sync with the assertions in trap.rs.
global_asm!(
    ".align 4",
    ".global stvec_trap",
    "stvec_trap:",
    "  csrrw sp, sscratch, sp",
    "  bnez sp, 2f",
    "  csrr sp, sscratch",
    "2:",
    "  addi sp, sp, -560",
    "  sd t0, 8(sp)",
    "  csrr t0, sscratch",
    "  sd t0, 240(sp)",
    "  csrw sscratch, zero",
    "  sd ra, 0(sp)",
    "  sd t1, 16(sp)",
    "  sd t2, 24(sp)",
    "  sd s0, 32(sp)",
    "  sd s1, 40(sp)",
    "  sd a0, 48(sp)",
    "  sd a1, 56(sp)",
    "  sd a2, 64(sp)",
    "  sd a3, 72(sp)",
    "  sd a4, 80(sp)",
    "  sd a5, 88(sp)",
    "  sd a6, 96(sp)",
    "  sd a7, 104(sp)",
    "  sd s2, 112(sp)",
    "  sd s3, 120(sp)",
    "  sd s4, 128(sp)",
    "  sd s5, 136(sp)",
    "  sd s6, 144(sp)",
    "  sd s7, 152(sp)",
    "  sd s8, 160(sp)",
    "  sd s9, 168(sp)",
    "  sd s10, 176(sp)",
    "  sd s11, 184(sp)",
    "  sd gp, 192(sp)",
    "  sd tp, 200(sp)",
    "  sd t3, 248(sp)",
    "  sd t4, 256(sp)",
    "  sd t5, 264(sp)",
    "  sd t6, 272(sp)",
    "  csrr t0, sepc",
    "  sd t0, 208(sp)",
    "  csrr t0, sstatus",
    "  sd t0, 216(sp)",
    "  csrr t0, stval",
    "  sd t0, 224(sp)",
    "  csrr t0, scause",
    "  sd t0, 232(sp)",
    "  ld t0, 216(sp)",
    "  li t1, 0x6000",
    "  and t0, t0, t1",
    "  csrs sstatus, t1",
    "  beqz t0, 3f",
    "  fsd f0, 288(sp)",
    "  fsd f1, 296(sp)",
    "  fsd f2, 304(sp)",
    "  fsd f3, 312(sp)",
    "  fsd f4, 320(sp)",
    "  fsd f5, 328(sp)",
    "  fsd f6, 336(sp)",
    "  fsd f7, 344(sp)",
    "  fsd f8, 352(sp)",
    "  fsd f9, 360(sp)",
    "  fsd f10, 368(sp)",
    "  fsd f11, 376(sp)",
    "  fsd f12, 384(sp)",
    "  fsd f13, 392(sp)",
    "  fsd f14, 400(sp)",
    "  fsd f15, 408(sp)",
    "  fsd f16, 416(sp)",
    "  fsd f17, 424(sp)",
    "  fsd f18, 432(sp)",
    "  fsd f19, 440(sp)",
    "  fsd f20, 448(sp)",
    "  fsd f21, 456(sp)",
    "  fsd f22, 464(sp)",
    "  fsd f23, 472(sp)",
    "  fsd f24, 480(sp)",
    "  fsd f25, 488(sp)",
    "  fsd f26, 496(sp)",
    "  fsd f27, 504(sp)",
    "  fsd f28, 512(sp)",
    "  fsd f29, 520(sp)",
    "  fsd f30, 528(sp)",
    "  fsd f31, 536(sp)",
    "  frcsr t0",
    "  sd t0, 544(sp)",
    "3:",
    "  mv a0, sp",
    "  call riscv64_trap_dispatch",
    // A syscall may enable interrupts while waiting or lowering IRQL. Close
    // that window before restoring sepc/FP state and publishing sscratch for
    // U-mode. Otherwise a timer between csrw sscratch and csrw sstatus treats
    // this supervisor return as a user trap and overwrites the live frame.
    "  csrci sstatus, 2",
    "  ld t0, 216(sp)",
    "  li t1, 0x6000",
    "  and t0, t0, t1",
    "  beqz t0, 4f",
    "  fld f0, 288(sp)",
    "  fld f1, 296(sp)",
    "  fld f2, 304(sp)",
    "  fld f3, 312(sp)",
    "  fld f4, 320(sp)",
    "  fld f5, 328(sp)",
    "  fld f6, 336(sp)",
    "  fld f7, 344(sp)",
    "  fld f8, 352(sp)",
    "  fld f9, 360(sp)",
    "  fld f10, 368(sp)",
    "  fld f11, 376(sp)",
    "  fld f12, 384(sp)",
    "  fld f13, 392(sp)",
    "  fld f14, 400(sp)",
    "  fld f15, 408(sp)",
    "  fld f16, 416(sp)",
    "  fld f17, 424(sp)",
    "  fld f18, 432(sp)",
    "  fld f19, 440(sp)",
    "  fld f20, 448(sp)",
    "  fld f21, 456(sp)",
    "  fld f22, 464(sp)",
    "  fld f23, 472(sp)",
    "  fld f24, 480(sp)",
    "  fld f25, 488(sp)",
    "  fld f26, 496(sp)",
    "  fld f27, 504(sp)",
    "  fld f28, 512(sp)",
    "  fld f29, 520(sp)",
    "  fld f30, 528(sp)",
    "  fld f31, 536(sp)",
    "  ld t0, 544(sp)",
    "  fscsr t0",
    "4:",
    "  ld t0, 208(sp)",
    "  csrw sepc, t0",
    "  ld t0, 216(sp)",
    "  andi t1, t0, 0x100",
    "  bnez t1, 5f",
    "  addi t1, sp, 560",
    "  csrw sscratch, t1",
    "5:",
    "  csrw sstatus, t0",
    "  ld ra, 0(sp)",
    "  ld t1, 16(sp)",
    "  ld t2, 24(sp)",
    "  ld s0, 32(sp)",
    "  ld s1, 40(sp)",
    "  ld a0, 48(sp)",
    "  ld a1, 56(sp)",
    "  ld a2, 64(sp)",
    "  ld a3, 72(sp)",
    "  ld a4, 80(sp)",
    "  ld a5, 88(sp)",
    "  ld a6, 96(sp)",
    "  ld a7, 104(sp)",
    "  ld s2, 112(sp)",
    "  ld s3, 120(sp)",
    "  ld s4, 128(sp)",
    "  ld s5, 136(sp)",
    "  ld s6, 144(sp)",
    "  ld s7, 152(sp)",
    "  ld s8, 160(sp)",
    "  ld s9, 168(sp)",
    "  ld s10, 176(sp)",
    "  ld s11, 184(sp)",
    "  ld gp, 192(sp)",
    "  ld tp, 200(sp)",
    "  ld t3, 248(sp)",
    "  ld t4, 256(sp)",
    "  ld t5, 264(sp)",
    "  ld t6, 272(sp)",
    "  ld t0, 8(sp)",
    "  ld sp, 240(sp)",
    "  sret",
);

extern "C" {
    fn stvec_trap();
}

/// Runtime address of the trap entry, for low-level diagnostics.
pub fn trap_entry_addr() -> u64 {
    stvec_trap as *const () as u64
}

/// Install the `stvec` handler.
pub fn init() {
    unsafe {
        let v = stvec_trap as *const () as u64;
        asm!("csrw stvec, {}", in(reg) v, options(nostack));
        asm!("csrw sscratch, zero", options(nostack));
    }
}

/// `TrapFrame` is defined in [`super::trap`]. Re-export here for
/// callers that historically imported it from this module.
pub use super::trap::TrapFrame;

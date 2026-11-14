//! LoongArch 64 EENTRY exception handler

use core::arch::asm;
use core::arch::global_asm;

// LA64 CSR numbers (must fit in 14-bit immediate).
// 0x0   = CRMD
// 0x1   = PRMD
// 0x6   = ERA
// 0x7   = BADV
// 0x5   = ESTAT (exception cause)
// 0xc   = EENTRY
// 0x30-0x35 = KS0..KS5 (KScratch — scratch registers for the stub)

global_asm!(
    ".align 12",
    ".global loongarch64_exception",
    "loongarch64_exception:",
    // EENTRY+0x000 — general exception/interrupt entry. With
    // ECFG.VS=0, QEMU routes every non-refill exception and every
    // external interrupt to EENTRY+0x000. The only thing this stub
    // may do is branch to the common save/restore prologue.
    "  b loongarch64_exception_common",
    // Reserve the rest of the first 1 KiB so the common handler has
    // stable, well-known spacing inside the vector page.
    "  .space 0x3FC",
    // EENTRY+0x400 — TLB refill exception entry. This is the hardware
    // page-table walker: refill exceptions vector through TLBRENTRY
    // (CSR 0x88) here with ISTLBR=1, so all addressing below is
    // physical and no general-purpose register may be clobbered
    // without saving it. The sequence mirrors Linux's
    // `handle_tlb_refill` (arch/loongarch/mm/tlbex.S): read the active
    // root from CSR_PGD, walk the four directory levels with LDDIR,
    // fetch the even/odd leaf PTE pair with LDPTE, and commit the
    // pair into the TLB with TLBFILL.
    "loongarch64_tlb_refill:",
    "  csrwr $t0, 0x8b",   // save t0 in TLBRSAVE
    "  csrrd $t0, 0x19",   // PGDL: active page-table root (PA)
    "  lddir $t0, $t0, 4", // root index VA[56:48] -> L1 table PA
    "  lddir $t0, $t0, 3", // L1 index VA[47:39]  -> L2 table PA
    "  lddir $t0, $t0, 2", // L2 index VA[38:30]  -> L3 table PA
    "  lddir $t0, $t0, 1", // L3 index VA[29:21]  -> leaf table PA
    "  ldpte $t0, 0",      // even PTE (VA[20:12]) -> TLBRELO0
    "  ldpte $t0, 1",      // odd  PTE (VA[20:12]) -> TLBRELO1
    "  tlbfill",           // commit the PTE pair into the TLB
    "  csrrd $t0, 0x8b",   // restore t0 from TLBRSAVE
    "  ertn",
    // 11 instructions (44 bytes); pad to EENTRY+0x480 so the common
    // handler starts at a predictable offset. QEMU also sends debug
    // breakpoints directly to EENTRY+0x480.
    "  .space 0x54",
    "loongarch64_exception_common:",
    // KS0/KS5 are scratch only until the frame is saved. Nested kernel
    // exceptions grow the current stack instead of resetting its top.
    "  csrwr $t0, 0x30",
    "  move $t0, $sp",
    "  csrwr $t0, 0x35",
    "  csrrd $t0, 0x1",
    "  andi $t0, $t0, 3",
    "  beqz $t0, 2f",
    "  la.local $t0, _exception_stack_top",
    "  ld.d $sp, $t0, 0",
    "2:",
    "  addi.d $sp, $sp, -560",
    "  st.d $ra, $sp, 0",
    "  st.d $tp, $sp, 8",
    "  st.d $s0, $sp, 16",
    "  st.d $s1, $sp, 24",
    "  st.d $s2, $sp, 32",
    "  st.d $s3, $sp, 40",
    "  st.d $s4, $sp, 48",
    "  st.d $s5, $sp, 56",
    "  st.d $s6, $sp, 64",
    "  st.d $s7, $sp, 72",
    "  st.d $s8, $sp, 80",
    "  st.d $fp, $sp, 88",
    "  st.d $a0, $sp, 96",
    "  st.d $a1, $sp, 104",
    "  st.d $a2, $sp, 112",
    "  st.d $a3, $sp, 120",
    "  st.d $a4, $sp, 128",
    "  st.d $a5, $sp, 136",
    "  st.d $a6, $sp, 144",
    "  st.d $a7, $sp, 152",
    "  st.d $t1, $sp, 168",
    "  st.d $t2, $sp, 176",
    "  st.d $t3, $sp, 184",
    "  st.d $t4, $sp, 192",
    "  st.d $t5, $sp, 200",
    "  st.d $t6, $sp, 208",
    "  st.d $t7, $sp, 216",
    "  st.d $t8, $sp, 232",
    "  st.d $r21, $sp, 240",
    "  csrrd $t0, 0x30",
    "  st.d $t0, $sp, 160",
    "  csrrd $t0, 0x35",
    "  st.d $t0, $sp, 224",
    "  csrrd $t0, 0x6",
    "  st.d $t0, $sp, 248",
    "  csrrd $t0, 0x1",
    "  st.d $t0, $sp, 256",
    "  csrrd $t0, 0x2",
    "  st.d $t0, $sp, 264",
    "  csrrd $t0, 0x7",
    "  st.d $t0, $sp, 272",
    "  csrrd $t0, 0x5",
    "  st.d $t0, $sp, 280",
    // Rust handlers may use floating point, including the animation renderer.
    "  ld.d $t0, $sp, 264",
    "  ori $t1, $t0, 1",
    "  csrwr $t1, 0x2",
    "  andi $t0, $t0, 1",
    "  beqz $t0, 3f",
    "  fst.d $f0, $sp, 288",
    "  fst.d $f1, $sp, 296",
    "  fst.d $f2, $sp, 304",
    "  fst.d $f3, $sp, 312",
    "  fst.d $f4, $sp, 320",
    "  fst.d $f5, $sp, 328",
    "  fst.d $f6, $sp, 336",
    "  fst.d $f7, $sp, 344",
    "  fst.d $f8, $sp, 352",
    "  fst.d $f9, $sp, 360",
    "  fst.d $f10, $sp, 368",
    "  fst.d $f11, $sp, 376",
    "  fst.d $f12, $sp, 384",
    "  fst.d $f13, $sp, 392",
    "  fst.d $f14, $sp, 400",
    "  fst.d $f15, $sp, 408",
    "  fst.d $f16, $sp, 416",
    "  fst.d $f17, $sp, 424",
    "  fst.d $f18, $sp, 432",
    "  fst.d $f19, $sp, 440",
    "  fst.d $f20, $sp, 448",
    "  fst.d $f21, $sp, 456",
    "  fst.d $f22, $sp, 464",
    "  fst.d $f23, $sp, 472",
    "  fst.d $f24, $sp, 480",
    "  fst.d $f25, $sp, 488",
    "  fst.d $f26, $sp, 496",
    "  fst.d $f27, $sp, 504",
    "  fst.d $f28, $sp, 512",
    "  fst.d $f29, $sp, 520",
    "  fst.d $f30, $sp, 528",
    "  fst.d $f31, $sp, 536",
    "  movfcsr2gr $t0, $fcsr0",
    "  st.d $t0, $sp, 544",
    "  movcf2gr $t0, $fcc0",
    "  st.b $t0, $sp, 552",
    "  movcf2gr $t0, $fcc1",
    "  st.b $t0, $sp, 553",
    "  movcf2gr $t0, $fcc2",
    "  st.b $t0, $sp, 554",
    "  movcf2gr $t0, $fcc3",
    "  st.b $t0, $sp, 555",
    "  movcf2gr $t0, $fcc4",
    "  st.b $t0, $sp, 556",
    "  movcf2gr $t0, $fcc5",
    "  st.b $t0, $sp, 557",
    "  movcf2gr $t0, $fcc6",
    "  st.b $t0, $sp, 558",
    "  movcf2gr $t0, $fcc7",
    "  st.b $t0, $sp, 559",
    "3:",
    "  move $a0, $sp",
    "  la.local $t0, handle_trap",
    "  jirl $ra, $t0, 0",
    "  ld.d $t0, $sp, 264",
    "  andi $t0, $t0, 1",
    "  beqz $t0, 4f",
    "  fld.d $f0, $sp, 288",
    "  fld.d $f1, $sp, 296",
    "  fld.d $f2, $sp, 304",
    "  fld.d $f3, $sp, 312",
    "  fld.d $f4, $sp, 320",
    "  fld.d $f5, $sp, 328",
    "  fld.d $f6, $sp, 336",
    "  fld.d $f7, $sp, 344",
    "  fld.d $f8, $sp, 352",
    "  fld.d $f9, $sp, 360",
    "  fld.d $f10, $sp, 368",
    "  fld.d $f11, $sp, 376",
    "  fld.d $f12, $sp, 384",
    "  fld.d $f13, $sp, 392",
    "  fld.d $f14, $sp, 400",
    "  fld.d $f15, $sp, 408",
    "  fld.d $f16, $sp, 416",
    "  fld.d $f17, $sp, 424",
    "  fld.d $f18, $sp, 432",
    "  fld.d $f19, $sp, 440",
    "  fld.d $f20, $sp, 448",
    "  fld.d $f21, $sp, 456",
    "  fld.d $f22, $sp, 464",
    "  fld.d $f23, $sp, 472",
    "  fld.d $f24, $sp, 480",
    "  fld.d $f25, $sp, 488",
    "  fld.d $f26, $sp, 496",
    "  fld.d $f27, $sp, 504",
    "  fld.d $f28, $sp, 512",
    "  fld.d $f29, $sp, 520",
    "  fld.d $f30, $sp, 528",
    "  fld.d $f31, $sp, 536",
    "  ld.d $t0, $sp, 544",
    "  movgr2fcsr $fcsr0, $t0",
    "  ld.bu $t0, $sp, 552",
    "  movgr2cf $fcc0, $t0",
    "  ld.bu $t0, $sp, 553",
    "  movgr2cf $fcc1, $t0",
    "  ld.bu $t0, $sp, 554",
    "  movgr2cf $fcc2, $t0",
    "  ld.bu $t0, $sp, 555",
    "  movgr2cf $fcc3, $t0",
    "  ld.bu $t0, $sp, 556",
    "  movgr2cf $fcc4, $t0",
    "  ld.bu $t0, $sp, 557",
    "  movgr2cf $fcc5, $t0",
    "  ld.bu $t0, $sp, 558",
    "  movgr2cf $fcc6, $t0",
    "  ld.bu $t0, $sp, 559",
    "  movgr2cf $fcc7, $t0",
    "4:",
    // Restore the frame, including the possibly advanced syscall ERA.
    // Only ERTN restores CRMD.IE; enabling interrupts earlier races return.
    "  ld.d $t0, $sp, 264",
    "  csrwr $t0, 0x2",
    "  ld.d $t0, $sp, 272",
    "  csrwr $t0, 0x7",
    "  ld.d $t0, $sp, 248",
    "  csrwr $t0, 0x6",
    "  ld.d $t0, $sp, 256",
    "  csrwr $t0, 0x1",
    "  ld.d $ra, $sp, 0",
    "  ld.d $tp, $sp, 8",
    "  ld.d $s0, $sp, 16",
    "  ld.d $s1, $sp, 24",
    "  ld.d $s2, $sp, 32",
    "  ld.d $s3, $sp, 40",
    "  ld.d $s4, $sp, 48",
    "  ld.d $s5, $sp, 56",
    "  ld.d $s6, $sp, 64",
    "  ld.d $s7, $sp, 72",
    "  ld.d $s8, $sp, 80",
    "  ld.d $fp, $sp, 88",
    "  ld.d $a0, $sp, 96",
    "  ld.d $a1, $sp, 104",
    "  ld.d $a2, $sp, 112",
    "  ld.d $a3, $sp, 120",
    "  ld.d $a4, $sp, 128",
    "  ld.d $a5, $sp, 136",
    "  ld.d $a6, $sp, 144",
    "  ld.d $a7, $sp, 152",
    "  ld.d $t1, $sp, 168",
    "  ld.d $t2, $sp, 176",
    "  ld.d $t3, $sp, 184",
    "  ld.d $t4, $sp, 192",
    "  ld.d $t5, $sp, 200",
    "  ld.d $t6, $sp, 208",
    "  ld.d $t7, $sp, 216",
    "  ld.d $t8, $sp, 232",
    "  ld.d $r21, $sp, 240",
    "  ld.d $t0, $sp, 160",
    "  ld.d $sp, $sp, 224",
    "  ertn",
);

extern "C" {
    fn loongarch64_exception();
    fn loongarch64_tlb_refill();
}

/// Base VA of the exception vector page (the page that must be
/// executable at EENTRY). The page-table root serving exceptions has
/// to map this page or the first exception re-faults on the vector
/// itself (PIF at ERA=EENTRY) and the handler can never run.
pub fn vector_page_va() -> u64 {
    let v = loongarch64_exception as *const () as u64;
    v & !0xFFF
}

/// Install the exception vector.
pub fn init() {
    // The vector loads this pointer on every trap, including early faults.
    crate::arch::loongarch64::exception_stack::init();
    unsafe {
        let v = loongarch64_exception as *const () as u64;
        // EENTRY (0xc): with ECFG.VS=0, QEMU routes every normal
        // exception and every unmasked interrupt to EENTRY+0x000, where
        // the single-branch stub dispatches to the common prologue.
        // TLB refill exceptions are vectored separately via TLBRENTRY.
        asm!("csrwr {}, 0xc", inout(reg) v => _, options(nostack));
        // TLB refill exceptions enter the exact address in TLBRENTRY
        // (CSR 0x88); there is no implicit EENTRY+0x400 offset. Point
        // it directly at the physical-addressing refill stub.
        let v2 = loongarch64_tlb_refill as *const () as u64;
        asm!("csrwr {}, 0x88", inout(reg) v2 => _, options(nostack));
        // QEMU resets ECFG.LIE to zero. LIE occupies bits [14:0] and
        // is aligned with ESTAT.IS; unmask the external (IS[2]) and
        // constant-timer (IS[11]) interrupt lines. VS occupies
        // bits [18:16]; keep it at zero so QEMU sends all normal
        // exceptions and interrupts to EENTRY+0x000.
        let mut ecfg: u64;
        asm!("csrrd {}, 0x4", out(reg) ecfg, options(nostack));
        ecfg &= !(0x3FFF);
        ecfg |= 1 << 2;
        ecfg |= 1 << 11;
        ecfg &= !(0x7 << 16);
        asm!("csrwr {}, 0x4", in(reg) ecfg, options(nostack));

        // The vector page must be executable in the page-table root
        // that is active when the first exception arrives. Re-check
        // and re-map right before interrupts can be taken; allocator
        // reuse has been observed to wipe the bootstrap mapping.
        let vector_va = vector_page_va();
        let mapped = crate::arch::loongarch64::paging::page_is_mapped_in_root(
            crate::arch::loongarch64::paging::read_page_root_pfn(),
            vector_va,
        );
        if !mapped {
            let _ = crate::arch::loongarch64::paging::map_page(
                vector_va,
                vector_va,
                crate::mm::pte::PTE_P | crate::mm::pte::PTE_RW,
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn handle_trap(frame: u64) {
    // Dispatch to the high-level trap handler (trap.rs) which
    // handles interrupts (clearing TICLR for timer ticks), syscalls,
    // page faults, and panics on unexpected exceptions. Without
    // this call the timer interrupt is never acknowledged and the
    // CPU enters an interrupt storm that looks like a boot hang.
    crate::arch::loongarch64::trap::trap_dispatch(
        frame as *mut crate::arch::loongarch64::trap::TrapFrame,
    );
}

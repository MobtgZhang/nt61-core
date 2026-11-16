//! Whole-machine code publication primitives. The kernel executes these in
//! two acknowledged phases: clean every CPU's data, then synchronize fetch on
//! every CPU. No caller virtual address is dereferenced by these operations.

/// Make preceding code writes visible to instruction-cache maintenance.
/// Called with interrupts disabled so cache selection and CPU identity remain
/// stable. Cleaning the whole machine also covers writes made through aliases.
pub fn publish_code_stores() {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: EL1 cache maintenance, with interrupts disabled by the caller.
    unsafe {
        use core::arch::asm;
        let clidr: u64;
        let previous: u64;
        let features: u64;
        asm!("dsb sy", "mrs {0}, clidr_el1", "mrs {1}, csselr_el1",
            "mrs {2}, id_aa64mmfr2_el1", out(reg) clidr, out(reg) previous,
            out(reg) features, options(nostack));
        let extended = (features >> 20) & 15 != 0; // FEAT_CCIDX
        for level in 0..((clidr >> 24) & 7) {
            let kind = (clidr >> (level * 3)) & 7;
            if !(2..=4).contains(&kind) { continue; }
            let selector = level << 1;
            let geometry: u64;
            asm!("msr csselr_el1, {0}", "isb", "mrs {1}, ccsidr_el1",
                in(reg) selector, out(reg) geometry, options(nostack));
            let line_shift = (geometry & 7) + 4;
            let last_way = if extended { (geometry >> 3) & 0x1fffff }
                else { (geometry >> 3) & 0x3ff };
            let last_set = if extended { (geometry >> 32) & 0xffffff }
                else { (geometry >> 13) & 0x7fff };
            let way_shift = (last_way as u32).leading_zeros();
            for set in (0..=last_set).rev() {
                for way in (0..=last_way).rev() {
                    // A direct-mapped cache has no way bits (shift would be 32).
                    let way_bits = if last_way == 0 { 0 } else { way << way_shift };
                    let operand = selector | (set << line_shift) | way_bits;
                    asm!("dc csw, {0}", in(reg) operand, options(nostack));
                }
            }
            asm!("dsb sy", options(nostack));
        }
        asm!("msr csselr_el1, {0}", "isb", in(reg) previous, options(nostack));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// Discard stale instructions and serialize this CPU's instruction stream.
/// Invoked only after every CPU acknowledged the preceding store-clean phase.
pub fn synchronize_code_fetch() {
    #[cfg(target_arch = "x86_64")]
    { let _ = core::arch::x86_64::__cpuid(0); }
    #[cfg(target_arch = "aarch64")]
    // SAFETY: EL1 local instruction-cache maintenance; no memory operand.
    unsafe { core::arch::asm!("ic iallu", "dsb sy", "isb", options(nostack)); }
    #[cfg(target_arch = "riscv64")]
    // SAFETY: supervisor instruction synchronization; no memory operand.
    unsafe { core::arch::asm!("fence rw, rw", "fence.i", options(nostack)); }
    #[cfg(target_arch = "loongarch64")]
    // SAFETY: LoongArch hardware maintains I/D coherency. IBAR removes the
    // local instruction hazard after DBAR publishes previous data writes.
    unsafe { core::arch::asm!("dbar 0", "ibar 0", options(nostack)); }
}

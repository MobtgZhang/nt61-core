//! RISC-V64 TLB (Translation Lookaside buffer) Management
//!
//! Comprehensive TLB operations using sfence.vma:
//! - Single page invalidation
//! - Range invalidation
//! - ASID-based invalidation
//! - Global TLB flush

use core::arch::asm;

/// Address Space ID type
pub type Asid = u16;

/// Invalidate entire TLB
#[inline]
pub fn flush_all() {
    unsafe {
        asm!("sfence.vma zero, zero", options(nostack));
    }
}

/// Invalidate TLB for specific ASID
#[inline]
pub fn flush_asid(asid: Asid) {
    unsafe {
        asm!("sfence.vma zero, {asid}", asid = in(reg) asid as usize, options(nostack));
    }
}

/// Invalidate single page at virtual address
#[inline]
pub fn flush_page(vaddr: u64) {
    unsafe {
        asm!("sfence.vma {addr}, zero", addr = in(reg) vaddr, options(nostack));
    }
}

/// Invalidate single page with specific ASID
#[inline]
pub fn flush_page_asid(vaddr: u64, asid: Asid) {
    unsafe {
        asm!(
            "sfence.vma {addr}, {asid}",
            addr = in(reg) vaddr,
            asid = in(reg) asid as usize,
            options(nostack)
        );
    }
}

/// Invalidate TLB for a range of pages
#[inline]
pub fn flush_range(start_vaddr: u64, end_vaddr: u64) {
    const PAGE_SIZE: u64 = 4096;
    let mut addr = start_vaddr & !(PAGE_SIZE - 1);
    let end = (end_vaddr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    // For small ranges, invalidate individual pages
    // For large ranges, flush everything
    let num_pages = (end - addr) / PAGE_SIZE;

    if num_pages > 64 {
        flush_all();
    } else {
        unsafe {
            while addr < end {
                asm!("sfence.vma {addr}, zero", addr = in(reg) addr, options(nostack));
                addr += PAGE_SIZE;
            }
        }
    }
}

/// Invalidate TLB for range with specific ASID
#[inline]
pub fn flush_range_asid(start_vaddr: u64, end_vaddr: u64, asid: Asid) {
    const PAGE_SIZE: u64 = 4096;
    let mut addr = start_vaddr & !(PAGE_SIZE - 1);
    let end = (end_vaddr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let num_pages = (end - addr) / PAGE_SIZE;

    if num_pages > 64 {
        flush_asid(asid);
    } else {
        unsafe {
            while addr < end {
                asm!(
                    "sfence.vma {addr}, {asid}",
                    addr = in(reg) addr,
                    asid = in(reg) asid as usize,
                    options(nostack)
                );
                addr += PAGE_SIZE;
            }
        }
    }
}

/// read current ASID from satp
#[inline]
pub fn read_current_asid() -> Asid {
    let satp: u64;
    unsafe {
        asm!("csrr {}, satp", out(reg) satp, options(nostack));
    }
    ((satp >> 44) & 0xFFFF) as Asid
}

/// Set ASID in satp
#[inline]
pub fn set_asid(asid: Asid) {
    unsafe {
        let mut satp: u64;
        asm!("csrr {}, satp", out(reg) satp, options(nostack));

        // Clear old ASID (bits 59:44)
        satp &= !(0xFFFF_u64 << 44);
        // Set new ASID
        satp |= (asid as u64) << 44;

        asm!("csrw satp, {}", in(reg) satp, options(nostack));
        // Fence to ensure TLB sees new ASID
        asm!("sfence.vma zero, zero", options(nostack));
    }
}

/// Invalidate TLB for all global mappings
#[inline]
pub fn flush_global() {
    flush_all();
}

/// Remote TLB shootdown (for SMP)
/// Send IPI to other harts to flush their TLBs
#[inline]
pub fn flush_remote(cpu_mask: u64) {
    let current = crate::arch::riscv64::smp::__smp_get_current_cpu_id();
    for cpu in 0..crate::arch::riscv64::smp::cpu_count().min(64) {
        if cpu == current || cpu_mask & (1u64 << cpu) == 0 { continue; }
        let hart = crate::arch::riscv64::smp::hardware_cpu_id(cpu)
            .expect("online CPU has no firmware hart ID");
        // The kernel runs in S-mode. OpenSBI owns CLINT MSIP under PMP;
        // a direct MMIO write faults even when its VA is identity mapped.
        // SBI delivers SSIP, rather than an M-mode software interrupt.
        let result = unsafe { crate::arch::riscv64::sbi::ipi_send_range(1, hart) };
        assert_eq!(result.error, 0, "firmware failed to deliver shootdown IPI");
    }
}

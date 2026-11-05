//! Page fault handler (`MmAccessFault`)
//
//! Called from the architecture-specific page-fault trap
//! (`KiTrap0E` on x86_64, the data-abort handler on aarch64, the
//! load/store/access-fault handler on riscv64, and the TLB refill +
//! exception handler on loongarch64).
//
//! The CR2 / FAR / equivalent registers have already been captured
//! into the `va` parameter, and the architecture has already saved
//! the general-purpose registers into the per-CPU `TrapFrame`.
//
//! The handler:
//
//! 1. Walks the page table via the recursive self-map.
//! 2. Decides which of the five PTE types is at the faulting VA:
//!    demand-zero / page-file / transition / prototype / hardware.
//! 3. Resolves the fault (allocates a zeroed page, pops a transition
//!    page, resolves a prototype PTE, etc.) and updates the PTE.
//! 4. Returns to the trap handler which `iret`s / `eret`s.
//
//!    If the fault is unresolvable, returns the appropriate NTSTATUS.
//!    The trap handler then turns that into a status code in the
//!    saved `rax` and dispatches to the user-mode exception dispatcher.

use crate::ke::scheduler;
use crate::mm::cow;
use crate::mm::pagefile;
use crate::mm::pfn;
use crate::mm::pte::{pfn_to_phys, MMPTE};
use crate::mm::vad::VadEntry;
use crate::mm::vas::USER_BASE;
use crate::mm::zeropage;

/// Page-fault access flags — packed from the architecture's error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user: bool,
    pub reserved_bit: bool,
    pub instruction_fetch: bool,
}

impl AccessFlags {
    pub const fn default() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            user: false,
            reserved_bit: false,
            instruction_fetch: false,
        }
    }
}

/// Outcome of a page fault handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultStatus {
    /// Fault was resolved, retry the instruction.
    Handled,
    /// Page was reserved or otherwise not committable; dispatch to
    /// VAD validation.
    CheckVad,
    /// Unrecoverable — return as access violation.
    AccessViolation,
    /// Unrecoverable — out of memory.
    OutOfMemory,
}

/// MmAccessFault — top-level entry. Returns the status to the trap
/// handler, which will either retry the instruction or surface the
/// exception.
///
/// This function is called from the page fault handler with interrupts
/// disabled. It walks the page table to resolve the fault.
pub fn handle(va: u64, access: AccessFlags) -> FaultStatus {
    // Canonicalise: anything above 0x0000_7FFF_FFFF_FFFF and below
    // 0xFFFF_8000_0000_0000 is a non-canonical hole. The CPU should
    // not get here in the first place, but check defensively.
    if !is_canonical(va) {
        return FaultStatus::AccessViolation;
    }

    // Walk the page table using direct physical access for both
    // user and kernel VAs. The self-map-based `pte_address_of`
    // returns a pointer into the wrong page table level (see the
    // comments in `vas.rs`), so we always go through the full
    // page-table walk.
    let pte_ptr = match walk_user_page_table(va) {
        Some(ptr) => ptr,
        None => return FaultStatus::AccessViolation,
    };

    if pte_ptr.is_null() {
        return FaultStatus::AccessViolation;
    }
    let pte = unsafe { *pte_ptr };
    resolve_pte(pte, va, pte_ptr, access)
}

/// Walk the page table for a user VA using direct physical access.
/// Uses the PML4 at current_root() (physical address).
/// Returns a pointer to the leaf PTE, or None if the walk fails.
///
/// If intermediate page table entries are missing, this function will
/// automatically allocate them (demand-zero allocation).
///
/// For 1 GiB / 2 MiB large pages, this function returns a pointer to
/// the large PTE (the PDPT or PD entry with the H bit set). The caller
/// can then mark the A/D bit on the large PTE and return Handled.
/// This is the proper way to service an "A bit" #PF on a large
/// page: the page is already present, we just need to record the
/// access.
fn walk_user_page_table(va: u64) -> Option<*mut MMPTE> {
    use crate::mm::pfn;
    use crate::mm::pte::MMPTE;
    use crate::mm::vas::current_root;

    let pml4_phys = current_root();
    if pml4_phys == 0 {
        return None;
    }

    // PML4 index
    let pml4_idx = ((va >> 39) & 0x1FF) as usize;
    let pml4e = unsafe { *((pml4_phys as *const MMPTE).add(pml4_idx)) };

    // NT 6.1: intermediate entries for kernel VAs must be
    // supervisor-only. Only user VAs (below 0x0000_8000_0000_0000)
    // get the U bit. Without this check kernel PML4Es get US=1,
    // weakening kernel-mode isolation.
    let user_flags = if va < 0x0000_8000_0000_0000 { 0x4 } else { 0 };

    // PDPT - allocate if missing
    let pdpt_phys = if !pml4e.is_hardware() {
        // Allocate a new PDPT page
        let pdpt_pfn = pfn::allocate_pfn()?;
        let phys = crate::mm::pte::pfn_to_phys(pdpt_pfn);
        // Zero the new PDPT
        unsafe {
            core::ptr::write_bytes(phys as *mut u8, 0, 4096);
        }
        // Install in PML4
        let pml4e_ptr = unsafe { (pml4_phys as *mut MMPTE).add(pml4_idx) };
        unsafe {
            (*pml4e_ptr).set_hardware(phys, 0x3 | user_flags); // P=1, R/W=1, U=user?
        }
        phys
    } else {
        pml4e.hardware_page_frame()
    };

    // PDPT index
    let pdpt_idx = ((va >> 30) & 0x1FF) as usize;
    let pdpte = unsafe { *((pdpt_phys + pdpt_idx as u64 * 8) as *mut MMPTE) };
    // 1 GiB large page: the PDPT entry is the leaf. Return a
    // pointer to the PDPT entry so the caller can update the A/D
    // bits. The H bit (0x80) is set; the page frame is in the
    // upper bits.
    //
    // NOTE on A/D bit updates: the kernel walks the page table
    // using physical addresses as virtual pointers. After we
    // switched CR3 to the per-process PML4, the user PML4's
    // identity maps may not have W=1 permissions on every page,
    // so the A/D bit update can itself fault the kernel.
    // Skip it for now: the CPU marks PML4/PDPT/PD entries as
    // accessed via its own caches, and we can do a deferred A/D
    // update later if a more robust identity map is wired in.
    if pdpte.large() {
        return Some((pdpt_phys + pdpt_idx as u64 * 8) as *mut MMPTE);
    }
    let pd_phys = if !pdpte.is_hardware() {
        // NT 6.1: a committed-but-unmapped region must allocate the
        // missing PD page rather than returning ACCESS_VIOLATION.
        let pd_pfn = pfn::allocate_pfn()?;
        let pd_phys_new = crate::mm::pte::pfn_to_phys(pd_pfn);
        unsafe {
            core::ptr::write_bytes(pd_phys_new as *mut u8, 0, 4096);
        }
        let pdpte_ptr = (pdpt_phys + pdpt_idx as u64 * 8) as *mut MMPTE;
        unsafe {
            (*pdpte_ptr).set_hardware(pd_phys_new, 0x3 | user_flags);
        }
        pd_phys_new
    } else {
        pdpte.hardware_page_frame()
    };

    // PD - the leaf for 2 MiB pages, or intermediate for 4 KiB pages
    let pd_idx = ((va >> 21) & 0x1FF) as usize;
    let pde = unsafe { *((pd_phys + pd_idx as u64 * 8) as *mut MMPTE) };
    // 2 MiB large page: the PD entry is the leaf.
    //
    // Skip the A/D update: kernel page-table walks via
    // physical-as-virtual pointers can themselves fault when
    // the page tables' identity-map pages are mapped read-only
    // in the current CR3 (e.g. for OVMF-supplied identity
    // mappings). The CPU retains its own A/D caches and a
    // future fix can wire a robust phys-map and re-enable the
    // update path.
    if pde.large() {
        return Some((pd_phys + pd_idx as u64 * 8) as *mut MMPTE);
    }
    if !pde.is_hardware() {
        return None;
    }

    // PT - the leaf
    let pt_phys = pde.hardware_page_frame();
    let pt_idx = ((va >> 12) & 0x1FF) as usize;
    let pte_ptr = (pt_phys + pt_idx as u64 * 8) as *mut MMPTE;
    Some(pte_ptr)
}

/// Decide what to do based on the PTE type.
fn resolve_pte(pte: MMPTE, va: u64, pte_ptr: *mut MMPTE, access: AccessFlags) -> FaultStatus {
    if pte.is_hardware() {
        return resolve_hardware(pte, va, pte_ptr, access);
    }
    if pte.is_transition() {
        return resolve_transition(pte, va, pte_ptr, access);
    }
    if pte.is_prototype() {
        return resolve_prototype(pte, va, pte_ptr, access);
    }
    // Pure zero / demand zero / page file
    if pte.raw() == 0 {
        return resolve_demand_zero(va, pte_ptr, access);
    }
    // Has some bits set: page-file PTE
    resolve_page_file(va, pte_ptr, access)
}

fn resolve_hardware(pte: MMPTE, va: u64, pte_ptr: *mut MMPTE, access: AccessFlags) -> FaultStatus {
    if access.write && !pte.writable() {
        if pte.is_copy_on_write() {
            // COW path
            return match cow::perform_cow(va) {
                Ok(()) => FaultStatus::Handled,
                Err(_) => FaultStatus::AccessViolation,
            };
        }
        return FaultStatus::AccessViolation;
    }
    if access.execute && pte.no_execute() {
        return FaultStatus::AccessViolation;
    }
    // NT 6.1: a user-mode fault on a kernel page is an ACCESS_VIOLATION,
    // not "handled" — without this check the faulting instruction retries
    // forever because the PTE is present but supervisor-only.
    if access.user && (pte.u_long & 0x4) == 0 {
        return FaultStatus::AccessViolation;
    }
    // Present and permissions look right; the A/D bit must have been
    // missing — set them. (The CPU only sets A/D for some access
    // types; we set them all on the first fault.)
    unsafe {
        (*pte_ptr).set_accessed(true);
        if access.write {
            (*pte_ptr).set_dirty(true);
        }
    }
    // CRITICAL: Flush ALL TLB entries by writing CR3 back to itself.
    // invlpg only invalidates a single VA but the CPU may have
    // cached parent table entries (e.g. a stale 2MB PDE in PD[296]
    // that the loader replaced with a 4KB PDE). A full CR3 reload
    // flushes everything.
    //
    // x86_64 only — on aarch64 / riscv64 / loongarch64 TLB flush
    // is handled by each arch's `tlb_flush_all` helper.
    crate::arch::flush_tlb();
    // Flush the TLB entry for this VA. Without this, the CPU can keep
    // using a stale cached translation (e.g. "page not present" from
    // a prior probe) and re-fault on iretq.
    crate::arch::invalidate_tlb(va);
    FaultStatus::Handled
}

fn resolve_transition(
    pte: MMPTE,
    _va: u64,
    pte_ptr: *mut MMPTE,
    access: AccessFlags,
) -> FaultStatus {
    let pfn_no = pte.transition_pfn();
    // Transition -> hardware: pop the PFN off its current list
    // (standby/modified), promote the PTE.
    let mut db = pfn::PFN_DB.lock();
    if let Some(entry) = db.entry(pfn_no) {
        unsafe {
            db.unlink_entry(entry, pfn_no);
            // NT 6.1 (MiTransitionFault): after unlinking from
            // standby/modified, the PFN state must become Active so
            // it is no longer on any eviction list. Without this the
            // page remains on the standby list while the user PTE
            // claims it, letting the pager hand the same frame to
            // another process.
            (*entry).set_state(crate::mm::pfn::MMPFNSTATE::Active);
        }
    }
    drop(db);
    let pa = pfn_to_phys(pfn_no);
    let pte_flags = pte.transition_protection();
    // Translate the 5-bit MM protection into a 12-bit hardware PTE
    // bitmask. The transition PTE's protection field is the same
    // encoding as the software PTE's protection field, so we
    // delegate to the same routine.
    let new_bits = crate::mm::pte::mm_protect_to_pte_bits(pte_flags, true);
    // Clear the transition bit (bit 11) so this is now a hardware PTE.
    // NT 6.1: only set the D bit on write faults; a read fault on a
    // transition page must not dirty it.
    let mut new_bits = (new_bits & !(1u64 << 11)) | 0x20; // A
    if access.write {
        new_bits |= 0x40; // D
    }
    unsafe {
        (*pte_ptr).set_hardware(pa, new_bits);
    }
    FaultStatus::Handled
}

fn resolve_prototype(pte: MMPTE, va: u64, pte_ptr: *mut MMPTE, access: AccessFlags) -> FaultStatus {
    // Follow the prototype PTE chain. The target prototype PTE
    // describes the underlying page; we recurse through
    // `resolve_pte` to handle hardware/transition/software
    // sub-cases uniformly.
    let proto_addr = pte.prototype_address();
    if proto_addr.is_null() {
        // The prototype pointer was zero — check the VAD to
        // see if the address is even a valid region. If not,
        // access violation; if so, fall through to demand zero
        // (treating it as an unbacked demand-zero range).
        if let Some(vad) = vad_for_user_va(va) {
            // VAD allows the range; treat as demand-zero.
            return resolve_demand_zero_inner(va, pte_ptr, access, Some(vad));
        }
        return FaultStatus::AccessViolation;
    }
    let proto = unsafe { *proto_addr };
    // NT 6.1 (MiResolveProtoPteFault): after resolving through the
    // prototype PTE, the resolution must be *backfilled* into the
    // user PTE that actually faulted, so subsequent accesses to the
    // same address don't re-fault through the prototype chain every
    // time.  We resolve through the prototype chain first, then copy
    // the resulting hardware PTE into the user PTE slot.
    let status = resolve_pte(proto, va, proto_addr as *mut MMPTE, access);
    if status == FaultStatus::Handled && !pte_ptr.is_null() {
        // The prototype PTE has been converted to a hardware PTE by
        // the recursive call; copy the resolved mapping into the
        // user's PTE so the next access doesn't fault again.
        let resolved = unsafe { *proto_addr };
        if resolved.is_hardware() {
            unsafe {
                *pte_ptr = resolved;
            }
            // Flush the TLB entry for the faulting VA so the retry
            // sees the newly installed PTE.
            crate::arch::invalidate_tlb(va);
        }
    }
    status
}

fn resolve_demand_zero(va: u64, pte_ptr: *mut MMPTE, access: AccessFlags) -> FaultStatus {
    //     // // // kprintln!("[DEMAND_ZERO] va=0x{:x} USER_BASE=0x{:x}", va, USER_BASE);
    let vad = if va >= USER_BASE {
        vad_for_user_va(va)
    } else {
        None
    };
    //     // // // kprintln!("[DEMAND_ZERO] vad.is_some()={}", vad.is_some());
    resolve_demand_zero_inner(va, pte_ptr, access, vad)
}

fn resolve_demand_zero_inner(
    va: u64,
    pte_ptr: *mut MMPTE,
    access: AccessFlags,
    vad: Option<*mut VadEntry>,
) -> FaultStatus {
    //     // // // kprintln!("[DEMAND_ZERO_INNER] va=0x{:x} vad.is_some()={}", va, vad.is_some());
    if !access.read && !access.write && !access.execute {
        //         // // // kprintln!("[DEMAND_ZERO_INNER] no access flags");
        return FaultStatus::AccessViolation;
    }
    if va >= USER_BASE && vad.is_none() {
        // User VA, no VAD cover: not committable.
        //         // // // kprintln!("[DEMAND_ZERO_INNER] user VA without VAD");
        return FaultStatus::AccessViolation;
    }
    // If the VAD is a guard page, the access is supposed to
    // trigger the guard-page exception path. We surface that as
    // a "check VAD" so the upper layers can run the guard-page
    // exception dispatcher.
    if let Some(v) = vad {
        unsafe {
            if (*v).flags.is_guard() {
                //                 // // // kprintln!("[DEMAND_ZERO_INNER] guard page at VA=0x{:016x}", va);
                return FaultStatus::CheckVad;
            }
        }
    }
    //     // // // kprintln!("[DEMAND_ZERO_INNER] about to call zeropage::get_zeroed_page");
    let pfn_no = match zeropage::get_zeroed_page() {
        Some(p) => p,
        None => {
            //             // // // kprintln!("[DEMAND_ZERO_INNER] get_zeroed_page returned None");
            return FaultStatus::OutOfMemory;
        }
    };
    //     // // // kprintln!("[DEMAND_ZERO_INNER] got pfn={}", pfn_no);
    let pa = pfn_to_phys(pfn_no);
    //     // // // kprintln!("[DEMAND_ZERO_INNER] pte_ptr=0x{:x} pa=0x{:x}", pte_ptr as u64, pa);
    let mut pte_bits: u64 = 1; // P=1
                               // NT 6.1: the hardware R/W bit must honor the VAD's software
                               // protection, not just the fault type. A read-only VAD page
                               // must not become writable simply because a write fault occurred.
    let vad_protection = if let Some(v) = vad {
        u64::from(unsafe { (*v).protection.bits() })
    } else {
        0
    };
    let is_software_writable = vad_protection & 0x4 != 0;
    if is_software_writable && access.write {
        pte_bits |= 0x2;
    }
    if access.user {
        pte_bits |= 0x4;
    }
    pte_bits |= 0x20; // A
    pte_bits |= 0x40; // D
    if !access.execute {
        pte_bits |= 1u64 << 63;
    } // NX
      //     // // // kprintln!("[DEMAND_ZERO_INNER] pte_bits=0x{:x}", pte_bits);
      // read current PTE value before write
    let _pte_before = unsafe { (*pte_ptr).raw() };
    //     // // // kprintln!("[DEMAND_ZERO_INNER] pte_before=0x{:016x}", _pte_before);
    unsafe {
        (*pte_ptr).set_hardware(pa, pte_bits);
    }
    // read PTE value after write
    let pte_after = unsafe { (*pte_ptr).raw() };
    //     // // // kprintln!("[DEMAND_ZERO_INNER] pte_after=0x{:016x}", pte_after);
    let _ = va;
    if (pte_after & 1) == 0 {
        //         // // // kprintln!("[DEMAND_ZERO_INNER] FAIL: PTE not present after write!");
    }
    //     // // // kprintln!("[DEMAND_ZERO_INNER] success");
    FaultStatus::Handled
}

/// Look up the VAD for a user VA, if the current process has a
/// VAD tree set up. Returns `None` for kernel VAs and for user
/// VAs outside any VAD.
fn vad_for_user_va(va: u64) -> Option<*mut VadEntry> {
    //     // // // kprintln!("[VAD_LOOKUP] va=0x{:x}", va);
    // The current thread's ETHREAD points at the current EPROCESS,
    // which carries the process's VAD tree.
    scheduler::with_current_thread(|ethread| {
        let eprocess = ethread.kthread.process;
        if eprocess.is_null() {
            None
        } else {
            unsafe { (*eprocess).vad_root.find(va) }
        }
    })
    .flatten()
}

fn resolve_page_file(va: u64, pte_ptr: *mut MMPTE, access: AccessFlags) -> FaultStatus {
    let pte = unsafe { *pte_ptr };
    let page_file = pte.software_page_file();
    let offset = pte.software_offset();
    let prot = pte.software_protection();
    let pfn_no = match pagefile::read_page(page_file, offset) {
        Some(p) => p,
        None => return FaultStatus::OutOfMemory,
    };
    let pa = pfn_to_phys(pfn_no);
    let mut pte_bits: u64 = 1;
    // NT 6.1: the hardware R/W bit must come from the *software*
    // protection bits (VAD), not from the fault type. A read-only
    // committed page stays read-only after page-in; only if the
    // software PTE says writable (bit 2 = MM_PAGE_WRITABLE) do we
    // set the hardware write-enable bit. The faulting access itself
    // must also be a write for the D bit to be meaningful.
    let is_software_writable = prot & 0x4 != 0;
    if is_software_writable {
        pte_bits |= 0x2;
    }
    pte_bits |= 0x4; // user
    pte_bits |= 0x20; // A
    if access.write {
        pte_bits |= 0x40; // D (only on write faults)
    }
    // NT 6.1: NX is only forced for non-executable software pages.
    // Code pages paged back in must remain executable.
    if prot & 0x10 != 0 {
        pte_bits |= 1u64 << 63; // NX (bit 4 = non-executable)
    }
    unsafe {
        (*pte_ptr).set_hardware(pa, pte_bits);
    }
    let _ = va;
    FaultStatus::Handled
}

/// Check whether `va` is canonical (sign-extended 48-bit address).
fn is_canonical(va: u64) -> bool {
    let top = va >> 47;
    top == 0 || top == 0x1FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_check() {
        assert!(is_canonical(0x0000_1234_5678u64));
        assert!(is_canonical(0xFFFF_8000_0000_0000u64));
        assert!(!is_canonical(0x0001_8000_0000_0000u64));
    }
}

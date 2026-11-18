//! Hyperspace
//
//! Hyperspace is a per-process temporary mapping window used to map
//! page tables (and other kernel data structures) of *another*
//! address space into the current one. NT 6.1 implements this as a
//! temporary window; our bootstrap mapper reserves slots until released.
//
//! In our layout hyperspace lives at `0xFFFFF700_0000_0000`. The
//! underlying PTE is the index `[0]` in the corresponding PDE/PPE
//! chain. To map a physical page there, we set the PTE at
//! `HYPERSPACE_BASE + (reserved_slot * 4096)` and then access the
//! address. Nested mappings must retain distinct slots until unmapping.

#[cfg(target_arch = "x86_64")]
use crate::ke::sync::Spinlock;
use core::ptr;

#[cfg(target_arch = "x86_64")]
use crate::mm::constants::MI_HYPERSPACE_INDEX;
#[cfg(target_arch = "x86_64")]
use crate::mm::pte::MMPTE;

pub use crate::mm::constants::{HYPERSPACE_BASE, HYPERSPACE_END, HYPERSPACE_ENTRIES};

/// Kernel address of the leaf PTE for hyperspace slot `slot` in
/// the current address space, obtained by walking the page tables
/// from the current root by physical address.
///
/// The recursive self-map installed at boot is a single-page
/// recursion (PML4[0x1ED] = PML4), so the linear-address formula
/// behind `vas::pte_address_of` does NOT reach the hyperspace PTEs
/// (PML4[0x1EE] chain): every computed address collapsed onto the
/// slot-0 PTE, leaving all other slots unusable and corrupting
/// unrelated entries. The physical walk matches what the hardware
/// does and is layout-independent.
#[cfg(target_arch = "x86_64")]
fn hyperspace_pte(slot: u64) -> *mut MMPTE {
    const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    const PTE_P: u64 = 0x1;
    const PTE_PS: u64 = 1 << 7;
    // The window belongs to the address space executing this mapping.
    // Bootstrap attach/detach bookkeeping is global and can name a different
    // root after direct user entry or an assembly context switch.
    let root = crate::arch::read_current_page_root();
    if root == 0 || slot >= HYPERSPACE_ENTRIES {
        return core::ptr::null_mut();
    }
    // The permanent bank map is inherited by every process. Low identity
    // pointers cease to name physical tables once a user image replaces them.
    let table_address = |physical| {
        crate::mm::kernel_physical_alias(physical, 4096).unwrap_or(physical)
    };
    // SAFETY: the current root and its present non-large descendants remain
    // allocated while the caller holds the hyperspace slot lock.
    let pml4e = unsafe { core::ptr::read_volatile((table_address(root) as *const u64).add(MI_HYPERSPACE_INDEX)) };
    if pml4e & PTE_P == 0 {
        return core::ptr::null_mut();
    }
    let pdpte = unsafe { core::ptr::read_volatile(table_address(pml4e & PTE_ADDR_MASK) as *const u64) };
    if pdpte & PTE_P == 0 || pdpte & PTE_PS != 0 {
        return core::ptr::null_mut();
    }
    let pde = unsafe { core::ptr::read_volatile(table_address(pdpte & PTE_ADDR_MASK) as *const u64) };
    if pde & PTE_P == 0 || pde & PTE_PS != 0 {
        return core::ptr::null_mut();
    }
    let pt = table_address(pde & PTE_ADDR_MASK) as *mut MMPTE;
    unsafe { pt.add(slot as usize) }
}

// CPUs executing one process share its temporary-mapping chain. Reserve a slot for
// the complete lifetime of each mapping, including nested guards on one CPU.
// Global bookkeeping conservatively prevents reuse across different roots too.
// A CPU-indexed single slot lets the second guard silently replace the first
// (notably the system/process page tables in refresh_kernel_mappings).
#[cfg(target_arch = "x86_64")]
static HYPERSPACE_SLOTS: Spinlock<[bool; HYPERSPACE_ENTRIES as usize]> =
    Spinlock::new([false; HYPERSPACE_ENTRIES as usize]);

/// Reserve and map a contiguous physical span in the temporary window.
/// Exhaustion or a missing window fails without replacing an active mapping.
#[cfg(target_arch = "x86_64")]
pub fn map_hyperspace_pages(pfn: u64, pages: u64) -> u64 {
    if pages == 0 || pages > HYPERSPACE_ENTRIES ||
        pfn.checked_add(pages - 1).filter(|&last| last <= (0x000f_ffff_ffff_f000 >> 12)).is_none() {
        return 0;
    }
    let mut slots = HYPERSPACE_SLOTS.lock();
    let count = pages as usize;
    let Some(start) = slots.windows(count).position(|span| span.iter().all(|&used| !used)) else {
        return 0;
    };
    // Preflight the whole range before changing the shared page tables.
    for offset in 0..count {
        if hyperspace_pte((start + offset) as u64).is_null() { return 0; }
    }
    for offset in 0..count {
        let slot = start + offset;
        let va = HYPERSPACE_BASE + slot as u64 * 4096;
        unsafe { (*hyperspace_pte(slot as u64)).set_hardware((pfn + offset as u64) << 12, 0x63); }
        crate::mm::vas::invalidate_tlb(va);
        slots[slot] = true;
    }
    HYPERSPACE_BASE + start as u64 * 4096
}

pub fn map_hyperspace(pfn: u64) -> u64 {
    map_hyperspace_pages(pfn, 1)
}

/// Non-x86 targets expose RAM through a boot-time physical identity map.
#[cfg(not(target_arch = "x86_64"))]
pub fn map_hyperspace_pages(pfn: u64, pages: u64) -> u64 {
    if pages == 0 || pages > HYPERSPACE_ENTRIES { return 0; }
    let Some(end) = pfn.checked_add(pages) else { return 0; };
    for frame in pfn..end {
        let pa = crate::mm::pte::pfn_to_phys(frame);
        if !crate::arch::map_page(pa, pa, crate::mm::pte::PTE_RW) { return 0; }
    }
    crate::mm::pte::pfn_to_phys(pfn)
}

#[cfg(target_arch = "x86_64")]
pub fn unmap_hyperspace(va: u64) {
    if !(HYPERSPACE_BASE..HYPERSPACE_END).contains(&va) || va & 0xfff != 0 {
        return;
    }
    let slot = (va - HYPERSPACE_BASE) / 4096;
    let mut slots = HYPERSPACE_SLOTS.lock();
    let pte_ptr = hyperspace_pte(slot);
    if pte_ptr.is_null() || !slots[slot as usize] { return; }
    unsafe { (*pte_ptr).clear(); }
    crate::mm::vas::invalidate_tlb(va);
    slots[slot as usize] = false;
}

#[cfg(not(target_arch = "x86_64"))]
pub fn unmap_hyperspace(_va: u64) {}

/// Translate a VA in the current address space to the hyperspace
/// window — used by debug helpers.
pub fn hyperspace_pointer(va: u64) -> *mut u8 {
    if (HYPERSPACE_BASE..HYPERSPACE_END).contains(&va) {
        va as *mut u8
    } else {
        ptr::null_mut()
    }
}

#[cfg(target_arch = "x86_64")]
pub fn init() {}

#[cfg(not(target_arch = "x86_64"))]
pub fn init() {}

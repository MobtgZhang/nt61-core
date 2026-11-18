//! A deliberately narrow x64 private-image demand-zero path. The loader has
//! already allocated the page tables and installed a READWRITE software PTE.
//! No missing table or arbitrary image hole is populated by this handler.
use super::{AccessFlags, FaultStatus};
use crate::mm::vad::{VadEntry, VadFlags, VadProtection, VadType};
use core::sync::atomic::{AtomicU64, Ordering};

const ZERO_RW: u64 = 4 << 5;
const NX: u64 = 1 << 63;
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;

/// Return None only for a present leaf, whose protection fault belongs to the
/// regular handler. Every non-present image leaf is checked here, including
/// unknown software owners; none falls through to anonymous zero allocation.
pub fn resolve(root: u64, va: u64, access: AccessFlags, vad: &VadEntry) -> Option<FaultStatus> {
    if !access.user || access.reserved_bit || va < 4096 || va >= 1 << 47
        || root == 0 || root & 4095 != 0 || !vad.contains(va)
        || vad.vad_type != VadType::VadImage
    {
        return Some(FaultStatus::AccessViolation);
    }
    let mut table = root;
    for shift in [39, 30, 21] {
        let guard = match crate::mm::vm::direct_map_guard(table, 4096) {
            Ok(g) => g,
            Err(_) => return Some(FaultStatus::AccessViolation),
        };
        let index = ((va >> shift) & 511) as usize;
        let raw = unsafe { (&*((guard.va() as *const AtomicU64).add(index))).load(Ordering::Acquire) };
        // A block mapping is already present; regular protection handling.
        if shift != 39 && raw & 0x81 == 0x81 { return None; }
        if raw & 7 != 7 || raw & ADDRESS == 0 {
            return Some(FaultStatus::AccessViolation);
        }
        table = raw & ADDRESS;
    }
    let guard = match crate::mm::vm::direct_map_guard(table, 4096) {
        Ok(g) => g,
        Err(_) => return Some(FaultStatus::AccessViolation),
    };
    let leaf = unsafe { &*((guard.va() as *const AtomicU64).add(((va >> 12) & 511) as usize)) };
    let raw = leaf.load(Ordering::Acquire);
    if raw & 1 != 0 { return None; }
    if raw != ZERO_RW || !vad.flags.contains(VadFlags::SEC_COMMIT)
        || vad.protection != VadProtection::READWRITE
        || access.execute || access.instruction_fetch || !(access.read || access.write)
    {
        return Some(FaultStatus::AccessViolation);
    }
    if vad.flags.is_guard() { return Some(FaultStatus::CheckVad); }
    let Some(pfn) = crate::mm::pfn::allocate_pfn() else { return Some(FaultStatus::OutOfMemory); };
    let pa = pfn << 12;
    let zeroed = match crate::mm::vm::direct_map_guard(pa, 4096) {
        Ok(page) => {
            // SAFETY: the frame is private to this fault until the atomic PTE
            // installation. A hyperspace alias avoids any user identity map.
            unsafe { core::ptr::write_bytes(page.va() as *mut u8, 0, 4096); }
            true
        }
        Err(_) => false,
    };
    if !zeroed {
        crate::mm::pfn::free_pfn(pfn);
        return Some(FaultStatus::OutOfMemory);
    }
    // Permissions are determined by the software PTE, never by fault type.
    // A first read must retain write permission, and every page remains NX.
    let value = pa | NX | 7 | 0x20 | if access.write { 0x40 } else { 0 };
    match leaf.compare_exchange(ZERO_RW, value, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => {
            crate::mm::vas::invalidate_tlb(va & !4095);
            Some(FaultStatus::Handled)
        }
        Err(winner) => {
            crate::mm::pfn::free_pfn(pfn);
            // Another CPU may have serviced this same not-present fault. Only
            // a completed RW+US+NX mapping is acceptable; don't lose its frame.
            if winner & (NX | 7) == (NX | 7) && winner & ADDRESS != 0 {
                crate::mm::vas::invalidate_tlb(va & !4095);
                Some(FaultStatus::Handled)
            } else { Some(FaultStatus::AccessViolation) }
        }
    }
}

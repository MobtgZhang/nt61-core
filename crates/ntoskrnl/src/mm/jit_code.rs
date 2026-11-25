//! Supervisor-only executable pages for the bounded guest translator.

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::arch::common::btl::runtime::jit::{Entry, Executable, Publisher};

#[cfg(target_arch = "riscv64")]
const BASE: u64 = 0xffff_ffd4_0000_0000;
#[cfg(not(target_arch = "riscv64"))]
const BASE: u64 = 0xffff_b900_0000_0000;
const PAGE: u64 = 4096;
const RW_NX: u64 = 0x8000_0000_0000_0003;
const RX: u64 = 1;
static READY: AtomicBool = AtomicBool::new(false);
static SLOTS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

pub(super) fn init(seed_frame: u64) -> bool {
    if crate::arch::translate_virt(BASE).is_some()
        || !crate::arch::map_page(BASE, seed_frame, RW_NX)
    {
        return false;
    }
    assert_eq!(crate::arch::unmap_page(BASE), Some(seed_frame));
    READY.store(true, Ordering::Release);
    true
}

#[cfg(not(target_arch = "x86_64"))]
pub(super) fn shared_branch(levels: u8, index: usize, prefix: u64) -> bool {
    #[cfg(target_arch = "aarch64")]
    const LEVEL: u8 = 4;
    #[cfg(target_arch = "riscv64")]
    const LEVEL: u8 = 3;
    #[cfg(target_arch = "loongarch64")]
    const LEVEL: u8 = 4;

    if !READY.load(Ordering::Acquire) || levels != LEVEL {
        return false;
    }
    let shift = 12 + 9 * (LEVEL - 1);
    let root_bits = if cfg!(target_arch = "loongarch64") {
        57
    } else {
        shift + 9
    };
    let prefix_mask = ((1u64 << root_bits) - 1) & !((1u64 << shift) - 1);
    (prefix | ((index as u64) << shift)) == BASE & prefix_mask
}

struct Slot(usize);

impl Slot {
    fn acquire() -> Option<Self> {
        for (index, word) in SLOTS.iter().enumerate() {
            let reserve =
                |bits: u64| (bits != u64::MAX).then(|| bits | (1 << (!bits).trailing_zeros()));
            if let Ok(previous) = word.try_update(Ordering::AcqRel, Ordering::Relaxed, reserve) {
                return Some(Self(index * 64 + (!previous).trailing_zeros() as usize));
            }
        }
        None
    }

    fn address(&self) -> u64 {
        BASE + self.0 as u64 * PAGE
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        SLOTS[self.0 / 64].fetch_and(!(1 << (self.0 % 64)), Ordering::Release);
    }
}

struct CodePage {
    slot: Slot,
    physical: u64,
}

impl Drop for CodePage {
    fn drop(&mut self) {
        let address = self.slot.address();
        assert_eq!(crate::arch::unmap_page(address), Some(self.physical));
        crate::ke::tlb_shootdown::invalidate_va_all_cpus_complete(address);
        super::frame::free_frame(self.physical);
    }
}

unsafe impl Executable for CodePage {
    fn entry(&self) -> Entry {
        unsafe { core::mem::transmute::<usize, Entry>(self.slot.address() as usize) }
    }
}

pub struct KernelPublisher;

unsafe impl Publisher for KernelPublisher {
    fn publish(&mut self, code: &[u8]) -> Option<Box<dyn Executable>> {
        if !READY.load(Ordering::Acquire) || code.is_empty() || code.len() > PAGE as usize {
            return None;
        }
        let slot = Slot::acquire()?;
        let address = slot.address();
        if crate::arch::translate_virt(address).is_some() {
            return None;
        }
        let physical = super::frame::allocate_frame()?;
        if !crate::arch::map_page(address, physical, RW_NX) {
            super::frame::free_frame(physical);
            return None;
        }
        #[cfg(target_arch = "loongarch64")]
        crate::ke::tlb_shootdown::invalidate_va_all_cpus_complete(address);
        unsafe {
            let destination = address as *mut u8;
            destination.write_bytes(0, PAGE as usize);
            core::ptr::copy_nonoverlapping(code.as_ptr(), destination, code.len());
        }
        assert_eq!(crate::arch::unmap_page(address), Some(physical));
        crate::ke::tlb_shootdown::invalidate_va_all_cpus_complete(address);
        crate::ke::instruction_shootdown::flush_all();
        if !crate::arch::map_page(address, physical, RX) {
            super::frame::free_frame(physical);
            return None;
        }
        crate::ke::tlb_shootdown::invalidate_va_all_cpus_complete(address);
        Some(Box::new(CodePage { slot, physical }))
    }
}

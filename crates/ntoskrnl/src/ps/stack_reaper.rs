//! Retired system stacks remain live until their CPU has switched away.
use core::sync::atomic::{AtomicU64, Ordering};

static RETIRED: [AtomicU64; 256] = [const { AtomicU64::new(0) }; 256];

/// The caller has disabled local interrupts and is exiting on this stack.
/// Only this CPU accesses its slot. Any previous occupant has switched away.
pub fn retire(cpu: usize, stack: u64) {
    let previous = RETIRED[cpu].swap(stack, Ordering::Relaxed);
    if previous != 0 {
        crate::mm::pool::free_aligned(previous as *mut u8);
    }
}

/// Called with local interrupts disabled before selecting another thread.
/// The first dispatch still uses the retiring stack and must leave it alone.
pub fn reap(cpu: usize, active_stack: u64) {
    let stack = RETIRED[cpu].load(Ordering::Relaxed);
    if stack != 0 && stack != active_stack {
        RETIRED[cpu].store(0, Ordering::Relaxed);
        crate::mm::pool::free_aligned(stack as *mut u8);
    }
}

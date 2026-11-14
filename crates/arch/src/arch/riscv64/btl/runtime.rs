//! Experimental RV64 emitter entry points. Executable dispatch is gated until
//! native code preserves guest flags, exceptions and the thread ABI. Use the
//! shared `GuestThread` / `GuestImage` resumable correctness backend instead.

#![cfg(feature = "btl")]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::decoder::DecMode;
use super::syscall_glue::{btl_syscall_dispatch, BtlSyscallResult};

/// Maximum cached guest length translated per call. The first
/// pass over a chunk of code may translate up to this many bytes
/// before yielding back to the kernel scheduler.
pub const TRANSLATION_QUANTUM: usize = 64;

/// Number of translated blocks produced since boot (Phase 5 stat).
static TRANSLATED_COUNT: AtomicU64 = AtomicU64::new(0);

/// Number of cache hits (lookup succeeded) since boot.
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);

/// Number of cache misses (lookup failed, fell through to
/// translation) since boot.
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

/// Number of guest syscalls routed through BTL since boot.
static SYSCALLS_ROUTED: AtomicU64 = AtomicU64::new(0);

/// Bitfield of active BTL runtime states (loaded by inline asm
/// from the per-CPU area; kept here so we can sanity-test the
/// tables in unit tests).
static ENABLED_MASK: AtomicU32 = AtomicU32::new(0);

/// Create resumable state for one guest thread. The caller retains it across
/// scheduler quanta; guest instructions must not be handed directly to SRET.
pub fn start_thread(guest_pc: u64, guest_sp: u64, mode: DecMode) -> super::GuestThread {
    use crate::arch::common::btl::ir::DecodeMode;
    super::GuestThread::new(match mode { DecMode::Mode32 => DecodeMode::X86,
        DecMode::Mode64 => DecodeMode::X64 }, guest_pc, guest_sp)
}

/// Native entry is unavailable; never hand an experimental code pointer to SRET.
pub fn translate_and_dispatch(_guest_va: u64) -> u64 { 0 }

/// Router for the BTL guest's `syscall` instruction. Mirrors
/// [`crate::arch::riscv64::syscall::dispatch_syscall`] but is
/// called with the guest-side argument capture (the `rdi..`
/// registers were lowered to `a0..a5`).
pub fn route_syscall(num: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    SYSCALLS_ROUTED.fetch_add(1, Ordering::Relaxed);
    match btl_syscall_dispatch(num, a0, a1, a2, a3, a4) {
        BtlSyscallResult::Handled { result } => result,
        BtlSyscallResult::NotHandled => 0xC000_000Cu64,
    }
}

/// Runtime statistics.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeStats {
    pub translated: u64,
    pub hits: u64,
    pub misses: u64,
    pub syscalls: u64,
}

pub fn stats() -> RuntimeStats {
    RuntimeStats {
        translated: TRANSLATED_COUNT.load(Ordering::Relaxed),
        hits: CACHE_HITS.load(Ordering::Relaxed),
        misses: CACHE_MISSES.load(Ordering::Relaxed),
        syscalls: SYSCALLS_ROUTED.load(Ordering::Relaxed),
    }
}

pub fn init() {
    ENABLED_MASK.store(0, Ordering::Relaxed);
}

/// Self-check: enqueue a translation, route a syscall, read stats.
pub fn smoke_test() -> bool {
    start_thread(0x1000, 0x7FFF_FFF0_000, DecMode::Mode64);
    translate_and_dispatch(0x1000);
    let _ = route_syscall(0, 0, 0, 0, 0, 0);
    let s = stats();
    s.translated == 0 && s.syscalls >= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_fails_closed_without_guest_mapping() {
        let before = stats();
        assert_eq!(translate_and_dispatch(0x7F00_1000), 0);
        let after = stats();
        assert_eq!(after.translated, before.translated);
        assert_eq!(after.misses, before.misses);
    }

    #[test]
    fn route_syscall_counts_bridged_calls() {
        let before = stats().syscalls;
        let _ = route_syscall(0xFFFF, 0, 0, 0, 0, 0);
        assert_eq!(stats().syscalls, before + 1);
    }
}

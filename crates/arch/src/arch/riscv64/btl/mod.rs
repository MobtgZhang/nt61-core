//! RISC-V support for portable x86/x64 guest execution.
//!
//! GuestImage/GuestThread use the shared checked PE interpreter and typed
//! NT 6.1 services. Native RV64 emitters/caches remain experimental and their
//! execution entry is disabled. This is neither ELF support nor a port of
//! Microsoft's ARM emulation libraries.

#![cfg(any(feature = "btl", test))]

pub mod cache;
pub mod codegen;
pub mod decoder;
pub mod exception;
pub mod mem;
pub mod perf;
pub mod runtime;
pub mod syscall_glue;
pub mod translator;

pub use crate::arch::common::btl::abi::{normalize_syscall_args, GuestAbi};
pub use crate::arch::common::btl::exec::{execute_block, GuestState, SyscallHook};
pub use crate::arch::common::btl::memory::{GuestAddressSpace, GuestMemory};

/// Kernel-mode entry point. Brings the BTL's subsystems online
/// in dependency order. Called by [`crate::arch::riscv64::init`].
pub fn init() {
    mem::init();
    cache::init();
    translator::init();
    codegen::init();
    decoder::init();
    runtime::init();
    syscall_glue::init();
    perf::init();
    exception::init();
}

/// Smoke test — Phase 4 ships a no-op validator that returns
/// `true` for every platform.
pub fn smoke_test() -> bool {
    true
}

/// Portable execution path; each scheduled guest owns this state.
pub use crate::arch::common::btl::runtime::{GuestThread, RunResult, StopReason};
pub use crate::arch::common::btl::pe::{GuestImage, ImageRunResult};

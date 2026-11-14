//! LoongArch64 support for portable x86/x64 guest execution.
//!
//! GuestImage/GuestThread use the shared checked PE interpreter and NT 6.1
//! services. Native decoder, emitter and cache modules are experimental;
//! no native translation dispatcher or Ring 3 integration is provided here.

#![cfg(any(target_arch = "loongarch64", test))]

pub mod cache;
pub mod common;
pub mod cpu;
pub mod decoder;
pub mod emit;
pub mod syscall;
pub mod translate;

/// Boot-time initialisation hook for the BTL. Wires up the syscall
/// bridge and translation-cache allocator.
pub fn init() {
    common::init();
    cache::init();
    syscall::init();
    // decoder / translate / emit are pure code modules; their
    // tables are `const`, so no extra runtime wiring is required.
}

pub use cache::{TranslationCache, TranslationCacheEntry};
pub use cpu::{GuestFlags, GuestRegs};
pub use translate::{translate_block, translate_instruction, TranslateError};

pub use crate::arch::common::btl::abi::{normalize_syscall_args, GuestAbi};
pub use crate::arch::common::btl::exec::{execute_block, GuestState, SyscallHook};
pub use crate::arch::common::btl::memory::{GuestAddressSpace, GuestMemory};

/// Portable execution path; each scheduled guest owns this state.
pub use crate::arch::common::btl::runtime::{GuestThread, RunResult, StopReason};
pub use crate::arch::common::btl::pe::{GuestImage, ImageRunResult};

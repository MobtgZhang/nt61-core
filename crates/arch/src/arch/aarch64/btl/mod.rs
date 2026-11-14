//! AArch64 support for portable x86/x64 guest execution.
//!
//! GuestImage/GuestThread use the shared checked PE interpreter and NT 6.1
//! services. The optional native emitters are experimental: translate/resolve
//! remain disabled until they satisfy the interpreter's semantics. Portable
//! console invocation is available independently of the native `btl` feature.

pub mod code_cache;
pub mod translation_manager;

pub use crate::arch::common::btl::abi::{normalize_syscall_args, GuestAbi};
pub use crate::arch::common::btl::exec::{execute_block, GuestState, SyscallHook};
pub use crate::arch::common::btl::memory::{GuestAddressSpace, GuestMemory};

#[cfg(feature = "btl")]
pub mod codegen;
#[cfg(feature = "btl")]
pub mod decoder;
#[cfg(feature = "btl")]
pub mod ir;
#[cfg(feature = "btl")]
pub mod state;
#[cfg(any(feature = "btl", test))]
pub mod syscall_bridge;

pub use code_cache::{CodeCache, TranslationUnit};
pub use translation_manager::{TranslationManager, TranslationRequest};

/// Initialise the BTL subsystem. Currently a no-op; the code cache
/// and translation manager are self-contained statics.
pub fn init() {}

/// Feature bits reported by the BTL interface.
pub const BTL_FEATURE_X86_64_TRANSLATION: u32 = 1 << 0;
pub const BTL_FEATURE_SYSCALL_BRIDGE: u32 = 1 << 1;
pub const BTL_FEATURE_ARM32_TRANSLATION: u32 = 1 << 2;

/// BTL feature flag shared with `Cargo.toml`.
///
/// Describes the shared portable x64 execution/service interface, not JIT
/// readiness or general Win32 compatibility. ARM32 execution is unavailable.
pub const BTL_FEATURES: u32 = BTL_FEATURE_X86_64_TRANSLATION | BTL_FEATURE_SYSCALL_BRIDGE;

/// Compile-time gate for experimental native decoder/codegen modules.
/// This does not disable the shared portable PE interpreter.
pub const ENABLED: bool = cfg!(feature = "btl");

/// BTL errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtlError {
    /// `feature = "btl"` is off; the call is a no-op.
    Disabled = -1,
    /// Architecture not yet supported (e.g. ARM32).
    UnsupportedArch = -2,
    /// Decoder ran past the end of the source buffer.
    OutOfBounds = -3,
    /// Invalid instruction encoding.
    InvalidInstruction = -4,
    /// Code cache full; cannot allocate a new translation unit.
    CacheFull = -5,
    /// Disassembly produced an instruction we cannot translate.
    Untranslatable = -6,
    /// Resource exhaustion (memory pool exhausted).
    OutOfMemory = -7,
}

pub type BtlResult<T> = Result<T, BtlError>;

/// Smoke test: verify the BTL is wired up and reachable.
pub fn smoke_test() -> bool {
    code_cache::smoke_test() && translation_manager::smoke_test()
}

/// Portable execution path; each scheduled guest owns this state.
pub use crate::arch::common::btl::runtime::{GuestThread, RunResult, StopReason};
pub use crate::arch::common::btl::pe::{GuestImage, ImageRunResult};

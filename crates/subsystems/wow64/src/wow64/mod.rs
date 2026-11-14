//! WOW64 ABI conversion and x86 process-environment support.
//!
//! Native x86 compatibility-mode execution and portable instruction emulation
//! are separate backends. These modules do not imply complete Win32 support;
//! non-x86 guests use the checked common BTL service boundary.
//!
//! References:
//!   * MSDN Library "Windows 7" — wow64.dll
//!   * geoffchappell.com — WoW64 architecture
//!   * ReactOS 0.3.x `wow64` thunk layer
//!   * Wine 1.7.x wow64 stub
//
//!     Module layout:
//!   * `klog`     — wow64-local lightweight log macro
//!     (avoids the `kprintln!` memcpy path).
//!   * `handle`   — HANDLE32 → kernel object resolvers.
//!   * `types`    — 32↔64 conversion helpers, PEB32/TEB32 structures
//!   * `wow64vas` — 32-bit virtual address space manager
//!   * `thunk`    — wow64_prepare_for_exception, wow64_apc_routine
//!   * `ssd`      — system service dispatch table
//!   * `syscall_thunk` — 32-bit syscall dispatch
//!   * `mem_thunk`    — memory API thunks
//!   * `ps_thunk`     — process/thread info thunks
//!   * `apc_exc_thunk`— APC and exception thunks

extern crate alloc;

use crate::ke::sync::Spinlock;
use alloc::vec::Vec;

pub mod apc_exc_thunk;
pub mod cpu;
pub mod handle;
pub mod klog;
pub mod mem_thunk;
pub mod ps_thunk;
pub mod ssd;
pub mod syscall_thunk;
pub mod thunk;
pub mod types;
pub mod wow64vas;

/// WoW64 service export discovery table.
pub static WOW64_SERVICES: Spinlock<Vec<(&'static str, u64)>> = Spinlock::new(Vec::new());

/// Initialise the WOW64 ABI modules.
pub fn init() {
    crate::wow64_klog!("init ABI modules");
    // Defer to the per-subsystem init so that the SSD and
    // thunk layers also get a chance to publish themselves.
    ssd::init_service_table();
    thunk::register_services();
    crate::wow64_klog!(
        "ready ({} services registered)",
        WOW64_SERVICES.lock().len()
    );
}

pub mod smoke;
pub use smoke::smoke_test as run_smoke;

pub mod syscall;
pub mod tests;
pub mod win32k_gdi32;
pub mod win32k_thunk;
pub mod win32k_user32;

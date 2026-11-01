#![cfg_attr(not(test), no_std)]
//! NT6.1 public facade re-exporting the migrated kernel core.

pub use nt61_boot_types::boot_types;
pub use nt61_sync::{atomic, sync};
pub use ntoskrnl::{
    arch, crypto, drivers, fs, hal, io, ke, kernel_main, kernel_main_safe, loader, lpc, mm,
    netstack, ob, ps, registry, rtl, se, userspace,
};
pub use ntoskrnl::servers::server;

/// Compatibility shim: the pre-migration facade exposed the subsystem
/// libraries under `nt61::libs::*`, and the Ring3 API crates still
/// re-export from those paths.
pub mod libs {
    pub use ntoskrnl::libs::{win32k, wow64, wow64win};
}

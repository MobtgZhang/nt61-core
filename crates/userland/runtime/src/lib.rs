//! Ring 3 user-mode runtime and process stubs.

#![no_std]
#![allow(dead_code)]

extern crate alloc;
extern crate ntoskrnl;

pub use nt61_boot_types::boot_types;
pub use ntoskrnl::{
    arch, boot_debug_println, boot_milestone, boot_print, boot_println, drivers, fs, hal, io, ke,
    kernel_main, kprintln, kprintln_debug, kprintln_error, kprintln_info, kprintln_warn, libs,
    loader, lpc, mm, netstack, ob, ps, registry, rtl, se, servers,
};

#[cfg(not(test))]
pub mod userspace;

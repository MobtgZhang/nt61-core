//! Kernel file system support.

#![no_std]
#![allow(dead_code)]

extern crate alloc;
extern crate ntoskrnl;

pub use nt61_boot_types::boot_types;
pub use ntoskrnl::{
    arch, boot_debug_println, boot_milestone, boot_print, boot_println, crypto, drivers, hal, io,
    ke, kprintln_debug, kprintln_error, kprintln_info, kprintln_warn, libs, loader, lpc, mm,
    netstack, ob, ps, registry, rtl, se, servers, userspace,
};

pub mod fs;

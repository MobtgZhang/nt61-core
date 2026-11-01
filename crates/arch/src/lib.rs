//! Architecture-specific kernel support.

#![no_std]
#![allow(dead_code)]

extern crate alloc;
extern crate ntoskrnl;

pub use nt61_boot_types::boot_types;
pub use ntoskrnl::{
    boot_debug_println, boot_ok, boot_print, boot_println, drivers, fs, hal, io, ke, kernel_main,
    libs, loader, lpc, mm, netstack, ob, ps, registry, rtl, se, servers, userspace,
};

pub mod arch;

//! WOW64 graphics/user interface support.

#![no_std]
#![allow(dead_code)]

extern crate alloc;
extern crate ntoskrnl;

pub use ntoskrnl::{
    arch, boot_debug_println, boot_println, drivers, fs, hal, io, ke, kernel_main, kprintln,
    kprintln_debug, kprintln_error, kprintln_info, libs, loader, lpc, mm, netstack, ob, ps,
    registry, rtl, se, servers, userspace,
};

pub use ntoskrnl::wow64_klog;

pub mod wow64win;

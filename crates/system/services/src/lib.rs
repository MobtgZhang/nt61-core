//! Ring 3 system process: services

#![no_std]
#![allow(dead_code)]

extern crate alloc;
extern crate ntoskrnl;

pub use ntoskrnl::{
    arch, boot_debug_println, boot_println, drivers, fs, hal, io, ke, kernel_main, kprintln,
    kprintln_debug, kprintln_error, kprintln_info, kprintln_warn, libs, loader, lpc, mm, netstack,
    ob, ps, registry, rtl, se, servers, userspace,
};

pub mod services;

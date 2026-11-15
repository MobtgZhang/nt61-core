//! Win32k kernel graphics/user subsystem.

#![no_std]
#![allow(dead_code)]

extern crate alloc;
extern crate ntoskrnl;

pub use ntoskrnl::{
    arch, boot_debug_println, boot_println, drivers, fs, hal, io, ke, kernel_main, kprintln,
    kprintln_debug, kprintln_error, kprintln_info, loader, lpc, mm, netstack, ob, ps,
    registry, rtl, se, servers, userspace,
};

pub mod win32k;

// This crate also builds the subsystem independently for host tests. Resolve
// internal win32k imports to this instance, rather than ntoskrnl's embedded copy.
pub mod libs {
    pub use ntoskrnl::libs::*;
    pub use crate::win32k;
}

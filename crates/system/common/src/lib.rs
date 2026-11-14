//! Shared Ring 3 system-process support.

#![allow(unexpected_cfgs)]
#![allow(dead_code)]

extern crate alloc;

#[cfg(feature = "standalone")]
extern crate ntoskrnl;
#[cfg(feature = "standalone")]
extern crate self as servers;

#[cfg(not(feature = "standalone"))]
pub use crate::{
    arch, boot_debug_println, boot_milestone, boot_print, boot_println, crypto, drivers, fs, hal,
    io, ke, kernel_main, kprintln, kprintln_debug, kprintln_error, kprintln_info, kprintln_warn,
    libs, loader, lpc, mm, netstack, ob, ps, registry, rtl, se, userspace,
};
pub use nt61_boot_types::boot_types;
#[cfg(feature = "standalone")]
pub use ntoskrnl::{
    arch, boot_debug_println, boot_milestone, boot_print, boot_println, crypto, drivers, fs, hal,
    io, ke, kernel_main, kprintln, kprintln_debug, kprintln_error, kprintln_info, kprintln_warn,
    libs, loader, lpc, mm, netstack, ob, ps, registry, rtl, se, userspace,
};

pub mod cmd;
pub mod cmd_extensions;
pub mod boot_config;
pub mod command_tools;
pub mod guest_program;
#[path = "../../csrss/src/csrss.rs"]
pub mod csrss;
pub mod libs_smoke;
pub mod pid1;
pub mod pid2;
pub mod server;
#[path = "../../services/src/services.rs"]
pub mod services;
#[path = "../../smss/src/smss.rs"]
pub mod smss;
#[path = "../../sshd/src/sshd/mod.rs"]
pub mod sshd;
#[path = "../../wininit/src/wininit.rs"]
pub mod wininit;
#[path = "../../winlogon/src/winlogon.rs"]
pub mod winlogon;

/// Initialise the NT 6.1 system-server set.
pub fn init() {
    smss::init();
    csrss::init();
    wininit::init();
    winlogon::init();
    pid1::init();
    pid2::init();
    services::init();
}
pub mod smoke;
pub mod test_csrss_services;

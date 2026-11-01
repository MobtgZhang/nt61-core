//! Windows NT 6.1 kernel executive implementation.

#![no_std]
#![allow(dead_code)]

extern crate alloc;
extern crate self as ntoskrnl;

pub use nt61_boot_types::boot_types;
pub use libs::{msvcrt, ntdll};

#[path = "../../arch/src/arch/mod.rs"]
pub mod arch;

#[path = "../../drivers/src/drivers/mod.rs"]
pub mod drivers;

#[path = "../../fs/src/fs/mod.rs"]
pub mod fs;

#[path = "../../hal/src/hal/mod.rs"]
pub mod hal;

#[path = "../../loader/src/loader/mod.rs"]
pub mod loader;

#[path = "../../netstack/src/netstack/mod.rs"]
pub mod netstack;

#[path = "../../registry/src/registry/mod.rs"]
pub mod registry;

#[path = "../../crypto/src/crypto/mod.rs"]
pub mod crypto;

pub mod io;
pub mod ke;
pub mod lpc;
pub mod mm;
pub mod ob;
pub mod ps;
pub mod rtl;
pub mod se;

#[path = "kernel_main.rs"]
pub mod kernel_main;

#[path = "kernel_main_safe.rs"]
pub mod kernel_main_safe;

#[path = "../../libs/mod.rs"]
pub mod libs;

#[path = "../../system/common/src/lib.rs"]
pub mod servers;

#[path = "../../userland/runtime/src/userspace/mod.rs"]
pub mod userspace;

//! NT 6.1 desktop shell model and resource bindings.
//!
//! The desktop is deliberately kept independent from any one boot
//! transport. This crate owns the Win7 desktop shape, the resource
//! manifest, and the architecture/filesystem compatibility table.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod arch;
pub mod boot;
pub mod fs;
#[cfg(any(test, feature = "resource-gen"))]
pub mod resource_generator;
pub mod resources;
pub mod shell;
pub mod theme;

pub const NT_VERSION: (u16, u16, u16) = (6, 1, 7601);

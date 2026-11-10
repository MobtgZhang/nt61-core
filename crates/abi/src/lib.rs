//! Windows 7 SP1 x64 Win32 ABI — shared handle and primitive types.
//!
//! This crate is the *only* place where Win32 ABI definitions live.
//! The kernel (`win32k`), the Ring-3 runtime (`user32.dll`,
//! `gdi32.dll`, `uxtheme.dll`, …) and the build tools all depend on it
//! so that the ABI cannot drift between the two sides of the syscall
//! boundary.
//!
//! # Rules
//!
//! * **No logic.** This crate contains only `#[repr(C)]` definitions,
//!   type aliases and constants. Anything that computes belongs
//!   elsewhere.
//! * **Exact Windows 7 x64 layout.** `tests/abi_layout.rs` asserts the
//!   size and alignment of every structure against the official Win7
//!   x64 values.
//! * **No `std`.** The crate is `no_std` so the kernel can use it.

#![no_std]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

pub mod constants;
pub mod error;
pub mod gdi;
pub mod structs;
pub mod syscalls;
pub mod types;
pub mod winuser;

pub use constants::*;
pub use error::*;
pub use structs::*;
pub use types::*;

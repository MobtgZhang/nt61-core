//! Kernel cryptographic services.

#![no_std]
#![allow(dead_code)]

extern crate ntoskrnl;

pub use ntoskrnl::{arch, hal, ke, mm};

pub mod crypto;

//! NT 6.1 desktop shell model and resource bindings.
//!
//! The desktop is deliberately kept independent from any one boot
//! transport. This crate owns the Win7 desktop shape, the resource
//! manifest, and the architecture/filesystem compatibility table.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod applications;
pub mod arch;
pub mod bmp;
pub mod boot;
pub mod compositor;
pub mod controls;
pub mod desktop_files;
pub mod display_settings;
pub mod file;
pub mod fs;
pub mod graphics;
pub mod login;
pub mod platform;
// Filesystem-based resource generation belongs to the host tool. Keep std out
// of the desktop library even when Cargo unifies the tool's feature flags.
#[cfg(test)]
pub mod resource_generator;
pub mod resources;
pub mod shell;
pub mod shell_resources;
pub mod theme;
pub mod user_account_pictures;
pub mod user_allocator;
pub mod widgets;
pub mod win32;
pub mod window;

pub const NT_VERSION: (u16, u16, u16) = (6, 1, 7601);

#[cfg(test)]
#[path = "ico_test.rs"]
mod ico_tests;

pub mod session;

pub mod power_menu;
pub mod logon_power;

pub mod accounts;

pub mod control_panel;

pub mod sound_settings;
pub mod network_settings;

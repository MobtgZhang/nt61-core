//! Boot handoff ABI shared by the bootloader, OS loader, and kernel.

#![no_std]
#![allow(dead_code)]

pub mod boot_types;
pub mod boot_image;
pub mod image_cache;

pub mod boot_options;

/// Private loader payload contract; deliberately separate from WDM DRIVER_OBJECT.
pub mod boot_driver;

/// ABI-reviewed native PE exports shared with the image builder.
pub mod native_exports;

pub mod memory_diagnostic;

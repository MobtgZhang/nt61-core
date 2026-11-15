#![no_std]
#![no_main]

use nt61_driver::{
    dispatch_unsupported, register_driver, unload_driver, BootDriverObject, BootDriverRegistryPath,
};

#[unsafe(no_mangle)]
pub unsafe extern "efiapi" fn DriverEntry(
    driver_object: *mut BootDriverObject,
    _registry_path: *const BootDriverRegistryPath,
) -> i32 {
    // SAFETY: the loader lends its initialized private ABI object for this call.
    unsafe {
        register_driver(
            driver_object,
            nt61_drivers::drivers::video::vga::init,
            dispatch_unsupported,
            unload_driver,
        )
    }
}

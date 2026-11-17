#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[cfg_attr(not(test), global_allocator)]
#[cfg_attr(test, allow(dead_code))]
static ALLOCATOR: ntoskrnl::mm::heap::KernelHeapAllocator = ntoskrnl::mm::heap::KernelHeapAllocator;

use nt61_boot_types::boot_driver::{BootDriverDispatch, BootDriverUnload, BOOT_DRIVER_MARKER};
pub use nt61_boot_types::boot_driver::{BootDriverObject, BootDriverRegistryPath};
// Payload entry points use the shared adapter for both their private ABI and
// subsystem initialization, keeping the driver implementation dependency here.
pub use nt61_drivers::drivers;

pub const STATUS_SUCCESS: i32 = 0;
pub const STATUS_INVALID_PARAMETER: i32 = 0xC000_000Du32 as i32;
pub const STATUS_NOT_SUPPORTED: i32 = 0xC000_00BBu32 as i32;

// Keep the format marker in each linked payload, even with LTO and section GC.
#[used]
static PAYLOAD_MARKER: [u8; 16] = BOOT_DRIVER_MARKER;

// Retain a real image-relative pointer even for otherwise position-independent
// tiny payloads. The ELF relocation becomes a DIR64 fixup in the PE, so the
// loader can rebase every private driver using the same checked contract.
#[used]
static PAYLOAD_MARKER_REF: &[u8; 16] = &PAYLOAD_MARKER;

/// Initialize a private boot payload and return its temporary callback evidence.
///
/// # Safety
/// `driver_object` must point to an initialized, uniquely borrowed BootDriverObject
/// for this call. It is not a WDM DRIVER_OBJECT. Payloads may not retain it.
pub unsafe fn register_driver(
    driver_object: *mut BootDriverObject,
    init: fn(),
    dispatch: BootDriverDispatch,
    unload: BootDriverUnload,
) -> i32 {
    if driver_object.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    // SAFETY: the caller lends a valid object for this entry invocation.
    let object = unsafe { &mut *driver_object };
    // SAFETY: this immutable static is always valid; the volatile read retains
    // the on-disk format marker which the loader checks before invoking entry.
    let marker_ptr = unsafe { core::ptr::read_volatile(&PAYLOAD_MARKER_REF) };
    let marker = unsafe { core::ptr::read_volatile(marker_ptr) };
    if !object.has_supported_abi() || object.magic != marker {
        return STATUS_INVALID_PARAMETER;
    }
    init();
    object.dispatch = Some(dispatch);
    object.unload = Some(unload);
    STATUS_SUCCESS
}

/// These initialization payloads do not implement IRP processing.
pub unsafe extern "efiapi" fn dispatch_unsupported(_device_object: *mut u8, _irp: *mut u8) -> i32 {
    STATUS_NOT_SUPPORTED
}

pub unsafe extern "efiapi" fn unload_driver(_driver_object: *mut BootDriverObject) {}

#[unsafe(no_mangle)]
pub unsafe extern "efiapi" fn efi_main(
    driver_object: *mut BootDriverObject,
    registry_path: *const BootDriverRegistryPath,
) -> i32 {
    unsafe extern "efiapi" {
        fn DriverEntry(
            driver_object: *mut BootDriverObject,
            registry_path: *const BootDriverRegistryPath,
        ) -> i32;
    }
    // SAFETY: the PE entry follows the same private ABI and caller obligations.
    unsafe { DriverEntry(driver_object, registry_path) }
}

//! Versioned initialization contract for this project's boot payloads.
//!
//! This is not the Windows WDM DRIVER_OBJECT or a native driver loading ABI.
//! The loader lends this object and registry path only for the duration of entry;
//! callbacks are initialization evidence, never installed in the kernel I/O manager.

/// Identifies a project payload in a PE image before the loader calls its entry.
/// This is a format discriminator, not an authenticity or signature check.
pub const BOOT_DRIVER_MARKER: [u8; 16] = *b"NT61BOOTDRV1\0\0\0\0";
pub const BOOT_DRIVER_ABI_VERSION: u32 = 1;
pub type BootDriverDispatch = unsafe extern "efiapi" fn(*mut u8, *mut u8) -> i32;
pub type BootDriverUnload = unsafe extern "efiapi" fn(*mut BootDriverObject);
pub type BootDriverEntry =
    unsafe extern "efiapi" fn(*mut BootDriverObject, *const BootDriverRegistryPath) -> i32;

/// Borrowed counted UTF-16 path; lengths are bytes, as for an NT UNICODE_STRING.
#[repr(C)]
pub struct BootDriverRegistryPath {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *const u16,
}

/// Scratch initialization result. The loader retains no live references to it.
#[repr(C)]
pub struct BootDriverObject {
    pub magic: [u8; 16],
    pub version: u32,
    pub size: u32,
    pub dispatch: Option<BootDriverDispatch>,
    pub unload: Option<BootDriverUnload>,
}

impl BootDriverObject {
    pub const fn new() -> Self {
        Self {
            magic: BOOT_DRIVER_MARKER,
            version: BOOT_DRIVER_ABI_VERSION,
            size: core::mem::size_of::<Self>() as u32,
            dispatch: None,
            unload: None,
        }
    }

    pub fn has_supported_abi(&self) -> bool {
        self.magic == BOOT_DRIVER_MARKER
            && self.version == BOOT_DRIVER_ABI_VERSION
            && self.size as usize == core::mem::size_of::<Self>()
    }
}

impl Default for BootDriverObject {
    fn default() -> Self {
        Self::new()
    }
}

// All supported architectures use 64-bit pointers. Guard cross-build ABI drift.
const _: () = {
    assert!(core::mem::size_of::<BootDriverObject>() == 40);
    assert!(core::mem::offset_of!(BootDriverObject, dispatch) == 24);
    assert!(core::mem::offset_of!(BootDriverObject, unload) == 32);
    assert!(core::mem::size_of::<BootDriverRegistryPath>() == 16);
    assert!(core::mem::offset_of!(BootDriverRegistryPath, buffer) == 8);
};

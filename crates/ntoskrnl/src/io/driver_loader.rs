//! Lifecycle registry for caller-owned, statically mapped private drivers.
//!
//! This is not a native WDM loader or a disk-backed PE/import resolver.

use alloc::vec::Vec;
use core::ptr::{null_mut, NonNull};
use core::sync::atomic::{AtomicU32, Ordering};

use super::DriverObject;
use crate::ke::sync::Spinlock;
use crate::libs::ntdll::status::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ServiceStartType {
    BootStart = 0,
    SystemStart = 1,
    AutoStart = 2,
    DemandStart = 3,
    Disabled = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverLoadState {
    NotLoaded,
    Loading,
    Loaded,
    Failed,
    Unloading,
}

pub struct DriverRegistryEntry {
    pub name: [u8; 64],
    pub driver_object: *mut DriverObject,
    pub start_type: ServiceStartType,
    pub load_state: DriverLoadState,
    pub image_base: u64,
    pub image_size: u64,
    pub ref_count: AtomicU32,
}

impl Clone for DriverRegistryEntry {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            driver_object: self.driver_object,
            start_type: self.start_type,
            load_state: self.load_state,
            image_base: self.image_base,
            image_size: self.image_size,
            ref_count: AtomicU32::new(self.ref_count.load(Ordering::Relaxed)),
        }
    }
}

impl DriverRegistryEntry {
    pub fn new(name: &[u8], driver: *mut DriverObject, start_type: ServiceStartType) -> Self {
        let mut name_buf = [0u8; 64];
        let len = name.len().min(64);
        name_buf[..len].copy_from_slice(&name[..len]);

        Self {
            name: name_buf,
            driver_object: driver,
            start_type,
            load_state: DriverLoadState::NotLoaded,
            image_base: 0,
            image_size: 0,
            ref_count: AtomicU32::new(1),
        }
    }
}

const MAX_LOADED_DRIVERS: usize = 64;

static DRIVER_REGISTRY: Spinlock<Vec<Option<DriverRegistryEntry>>> = Spinlock::new(Vec::new());

/// An owning reference to a loaded private driver. The registry's caller-owned
/// storage contract remains in force; this guard prevents unload from entering
/// the driver's callback while code or device pointers are being used.
#[must_use]
pub struct DriverReference(NonNull<DriverObject>);

impl DriverReference {
    /// Resolve the name and retain the loaded record in one registry transaction.
    /// A borrowed find_driver result followed by reference_driver is not atomic.
    pub fn find(name: &[u8]) -> Result<Self, i32> {
        let mut registry = DRIVER_REGISTRY.lock();
        let entry = registry
            .iter_mut()
            .flatten()
            .find(|entry| {
                let length = entry.name.iter().position(|&b| b == 0).unwrap_or(64);
                entry.load_state == DriverLoadState::Loaded && &entry.name[..length] == name
            })
            .ok_or(STATUS_OBJECT_NAME_NOT_FOUND)?;
        let pointer = NonNull::new(entry.driver_object).ok_or(STATUS_NO_SUCH_DEVICE)?;
        let count = entry.ref_count.load(Ordering::Relaxed);
        let next = count.checked_add(1).ok_or(STATUS_INSUFFICIENT_RESOURCES)?;
        entry.ref_count.store(next, Ordering::Relaxed);
        Ok(Self(pointer))
    }

    pub fn as_ptr(&self) -> *mut DriverObject {
        self.0.as_ptr()
    }

    pub fn try_clone(&self) -> Result<Self, i32> {
        if reference_driver(self.as_ptr()) {
            Ok(Self(self.0))
        } else {
            Err(STATUS_INSUFFICIENT_RESOURCES)
        }
    }
}

impl Drop for DriverReference {
    fn drop(&mut self) {
        let released = dereference_driver(self.as_ptr());
        debug_assert!(released, "owned driver reference disappeared before Drop");
    }
}

pub fn init() {
    let mut registry = DRIVER_REGISTRY.lock();
    if registry.is_empty() {
        registry.resize_with(MAX_LOADED_DRIVERS, || None);
    }
}

/// Make sure the registry has slots. `load_driver` and the registration
/// helpers used to fail unconditionally when `init` had never been
/// called, because the registry `Vec` was empty and had no free slots.
fn ensure_registry(registry: &mut Vec<Option<DriverRegistryEntry>>) {
    if registry.is_empty() {
        registry.resize_with(MAX_LOADED_DRIVERS, || None);
    }
}

/// Register a caller-owned private driver object. Objects with no initializer
/// are already initialized (the bootstrap drivers use this form). This layout
/// and its `extern "C"` callbacks are not the native Windows WDM ABI.
///
/// # Safety
/// The object and its counted UTF-16 name must remain valid until unregistered.
/// The caller must serialize mutation of the object and its device list.
pub unsafe fn register_start_type_driver(
    driver: *mut DriverObject,
    start_type: ServiceStartType,
) -> bool {
    if driver.is_null() {
        return false;
    }
    let name = &(*driver).driver_name;
    let units = usize::from(name.length) / 2;
    if name.length % 2 != 0
        || name.length > name.maximum_length
        || name.buffer.is_null()
        || units == 0
        || units > 64
    {
        return false;
    }
    let mut name_buf = [0u8; 64];
    for (j, byte) in name_buf.iter_mut().enumerate().take(units) {
        let ch = *name.buffer.add(j);
        // The registry's key is ASCII; do not silently alias/truncate UTF-16.
        if ch == 0 || ch > 0x7f {
            return false;
        }
        *byte = ch as u8;
    }
    let mut registry = DRIVER_REGISTRY.lock();
    ensure_registry(&mut registry);
    if registry
        .iter()
        .flatten()
        .any(|e| e.driver_object == driver || e.name == name_buf)
    {
        return false;
    }
    let mut entry = DriverRegistryEntry::new(&name_buf, driver, start_type);
    entry.image_base = (*driver).driver_start;
    entry.image_size = (*driver).driver_size;
    if (*driver).driver_init.is_none() && start_type != ServiceStartType::Disabled {
        entry.load_state = DriverLoadState::Loaded;
    }
    match registry.iter_mut().find(|e| e.is_none()) {
        Some(slot) => {
            *slot = Some(entry);
            true
        }
        None => false,
    }
}

/// Disk-backed PE driver loading is not implemented by this private registry.
/// In particular an empty DriverObject is not an initialized image. Callers
/// supplying an already mapped private driver use registration + initialization.
pub fn load_driver(
    _driver_name: &[u8],
    _registry_path: &[u8],
    _start_type: ServiceStartType,
) -> *mut DriverObject {
    null_mut()
}

/// Run an already registered private initializer outside the registry lock.
/// The optional private registry path is NUL-terminated UTF-16, not a WDM
/// UNICODE_STRING. A failed initializer rolls back newly created devices when
/// they are idle; any busy objects retain their caller-owned storage.
///
/// # Safety
/// The caller owns the live driver object and serializes device-list changes.
pub unsafe fn initialize_registered_driver(
    driver: *mut DriverObject,
    registry_path: &[u16],
) -> i32 {
    if driver.is_null() || (!registry_path.is_empty() && registry_path.last() != Some(&0)) {
        return STATUS_INVALID_PARAMETER;
    }
    let init = {
        let mut registry = DRIVER_REGISTRY.lock();
        let Some(entry) = registry
            .iter_mut()
            .flatten()
            .find(|e| e.driver_object == driver)
        else {
            return STATUS_INVALID_PARAMETER;
        };
        if entry.load_state != DriverLoadState::NotLoaded
            || entry.start_type == ServiceStartType::Disabled
        {
            return STATUS_INVALID_DEVICE_REQUEST;
        }
        let Some(init) = (*driver).driver_init else {
            return STATUS_INVALID_DEVICE_REQUEST;
        };
        entry.load_state = DriverLoadState::Loading;
        init
    };
    initialize_reserved_driver(driver, registry_path, init)
}

// The Loading state pins the caller-owned object until this finishes.
unsafe fn initialize_reserved_driver(
    driver: *mut DriverObject,
    registry_path: &[u16],
    init: unsafe extern "C" fn(*mut DriverObject, *mut u16) -> u32,
) -> i32 {
    let original_devices = (*driver).device_object;
    // A mutable copy accommodates this project's historical private callback
    // signature without casting immutable byte storage to a UTF-16 pointer.
    let mut path = registry_path.to_vec();
    let path_ptr = if path.is_empty() {
        null_mut()
    } else {
        path.as_mut_ptr()
    };
    let status = init(driver, path_ptr) as i32;
    if status < 0 {
        // DriverUnload is not called for a failed DriverEntry. Only new, idle
        // devices are ours to roll back; never free an attached/in-use object.
        let mut device = (*driver).device_object;
        for _ in 0..super::MAX_DEVICES {
            if device.is_null() || device == original_devices {
                break;
            }
            let next = (*device).next_device;
            let _ = super::delete_device(device);
            device = next;
        }
    }
    let mut registry = DRIVER_REGISTRY.lock();
    if let Some(entry) = registry
        .iter_mut()
        .flatten()
        .find(|e| e.driver_object == driver)
    {
        entry.load_state = if status >= 0 {
            DriverLoadState::Loaded
        } else {
            DriverLoadState::Failed
        };
    }
    status
}

/// Unregister a caller-owned private object after its unload callback deletes
/// its devices. No callback runs with the registry lock held. Image/object
/// storage remains owned by the registering caller, including on failure.
/// Failed initialization can be unregistered once its devices are gone.
///
/// # Safety
/// The caller owns the live driver and serializes device-list mutation.
pub unsafe fn unload_driver(driver: *mut DriverObject) -> bool {
    if driver.is_null() {
        return false;
    }
    let unload = {
        let mut registry = DRIVER_REGISTRY.lock();
        let Some(entry) = registry
            .iter_mut()
            .flatten()
            .find(|e| e.driver_object == driver)
        else {
            return false;
        };
        if entry.ref_count.load(Ordering::Relaxed) != 1 {
            return false;
        }
        let unload = match entry.load_state {
            DriverLoadState::Loaded => {
                let Some(callback) = (*driver).driver_unload else {
                    return false;
                };
                Some(callback)
            }
            DriverLoadState::Failed | DriverLoadState::NotLoaded => {
                if !(*driver).device_object.is_null() {
                    return false;
                }
                None
            }
            _ => return false,
        };
        entry.load_state = DriverLoadState::Unloading;
        unload
    };
    if let Some(unload) = unload {
        unload(driver);
    }
    let unregistered = (*driver).device_object.is_null() && super::unregister_driver(driver);
    let mut registry = DRIVER_REGISTRY.lock();
    let Some(index) = registry
        .iter()
        .position(|e| e.as_ref().is_some_and(|e| e.driver_object == driver))
    else {
        return false;
    };
    if !unregistered {
        // Keep an inert record rather than expose partially unloaded callbacks.
        registry[index].as_mut().unwrap().load_state = DriverLoadState::Failed;
        return false;
    }
    registry[index] = None;
    true
}

pub fn reference_driver(driver: *mut DriverObject) -> bool {
    let mut registry = DRIVER_REGISTRY.lock();
    let Some(entry) = registry
        .iter_mut()
        .flatten()
        .find(|e| e.driver_object == driver)
    else {
        return false;
    };
    let refs = entry.ref_count.load(Ordering::Relaxed);
    if entry.load_state != DriverLoadState::Loaded || refs == u32::MAX {
        return false;
    }
    entry.ref_count.store(refs + 1, Ordering::Relaxed);
    true
}

pub fn dereference_driver(driver: *mut DriverObject) -> bool {
    let mut registry = DRIVER_REGISTRY.lock();
    let Some(entry) = registry
        .iter_mut()
        .flatten()
        .find(|e| e.driver_object == driver)
    else {
        return false;
    };
    let refs = entry.ref_count.load(Ordering::Relaxed);
    // The registry owns the last reference; only unload can release it.
    if refs <= 1 {
        return false;
    }
    entry.ref_count.store(refs - 1, Ordering::Relaxed);
    true
}

/// Return a borrowed pointer; a user retaining it must acquire a reference.
pub fn find_driver(name: &[u8]) -> *mut DriverObject {
    let registry = DRIVER_REGISTRY.lock();
    registry
        .iter()
        .flatten()
        .find(|entry| {
            let length = entry.name.iter().position(|&b| b == 0).unwrap_or(64);
            entry.load_state == DriverLoadState::Loaded && &entry.name[..length] == name
        })
        .map_or(null_mut(), |entry| entry.driver_object)
}

fn load_drivers_with_start_type(wanted: ServiceStartType) -> usize {
    // Reserve all candidates under the registry lock so unload cannot remove
    // an object between snapshotting its pointer and entering its callback.
    let drivers: Vec<_> = {
        let mut registry = DRIVER_REGISTRY.lock();
        registry
            .iter_mut()
            .flatten()
            .filter_map(|entry| {
                if entry.start_type != wanted
                    || entry.load_state != DriverLoadState::NotLoaded
                    || wanted == ServiceStartType::Disabled
                {
                    return None;
                }
                let init = unsafe { (*entry.driver_object).driver_init }?;
                entry.load_state = DriverLoadState::Loading;
                Some((entry.driver_object, init))
            })
            .collect()
    };
    drivers
        .into_iter()
        .filter(|&(driver, init)| unsafe { initialize_reserved_driver(driver, &[], init) >= 0 })
        .count()
}

pub fn load_boot_start_drivers() -> usize {
    load_drivers_with_start_type(ServiceStartType::BootStart)
}
pub fn load_system_start_drivers() -> usize {
    load_drivers_with_start_type(ServiceStartType::SystemStart)
}
pub fn load_auto_start_drivers() -> usize {
    load_drivers_with_start_type(ServiceStartType::AutoStart)
}
pub fn loaded_driver_count() -> usize {
    DRIVER_REGISTRY
        .lock()
        .iter()
        .flatten()
        .filter(|e| e.load_state == DriverLoadState::Loaded)
        .count()
}

/// Callbacks receive detached metadata snapshots and may reenter the registry.
/// The snapshot's raw pointer is diagnostic only; it does not own a reference.
pub fn enumerate_drivers<F: FnMut(&DriverRegistryEntry) -> bool>(mut callback: F) {
    let entries: Vec<DriverRegistryEntry> =
        DRIVER_REGISTRY.lock().iter().flatten().cloned().collect();
    for entry in &entries {
        if !callback(entry) {
            break;
        }
    }
}

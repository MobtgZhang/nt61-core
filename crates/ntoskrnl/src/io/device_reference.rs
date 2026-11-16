//! Temporary references to private I/O devices. The allocation owner retains
//! its initial reference until delete_device; references never free storage.
use super::{device_lifecycle::DEVICE_GRAPH_LOCK, DeviceObject, MAX_DEVICES};
use crate::libs::ntdll::status::{STATUS_INSUFFICIENT_RESOURCES, STATUS_NO_SUCH_DEVICE};
use alloc::vec::Vec;
use core::{ptr::NonNull, sync::atomic::Ordering};

#[must_use]
pub struct DeviceReference(NonNull<DeviceObject>);

impl DeviceReference {
    /// Retain a driver's current first device under the same graph lock used
    /// for publication and deletion. The driver guard prevents unload callbacks
    /// from changing the object/image while the pointer is being resolved.
    pub fn for_driver(driver: &super::driver_loader::DriverReference) -> Result<Self, i32> {
        let _graph = DEVICE_GRAPH_LOCK.lock();
        // SAFETY: the driver guard retains the registered owner, while the graph
        // lock protects this list link and the allocation's initial reference.
        unsafe { Self::acquire_locked((*driver.as_ptr()).device_object) }
    }

    /// # Safety
    /// `device` must be null or a live device protected by an existing owner
    /// reference. A stale raw pointer cannot be made safe by taking this lock.
    pub unsafe fn acquire(device: *mut DeviceObject) -> Result<Self, i32> {
        let _graph = DEVICE_GRAPH_LOCK.lock();
        // SAFETY: the caller retains the initial live reference.
        unsafe { Self::acquire_locked(device) }
    }

    unsafe fn acquire_locked(device: *mut DeviceObject) -> Result<Self, i32> {
        let pointer = NonNull::new(device).ok_or(STATUS_NO_SUCH_DEVICE)?;
        // SAFETY: the caller owns a live reference and holds the graph lock.
        unsafe { &(*device).ref_count }
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count != 0).then(|| count.checked_add(1)).flatten()
            })
            .map_err(|_| STATUS_INSUFFICIENT_RESOURCES)?;
        Ok(Self(pointer))
    }

    pub fn as_ptr(&self) -> *mut DeviceObject {
        self.0.as_ptr()
    }

    pub fn try_clone(&self) -> Result<Self, i32> {
        // SAFETY: self keeps the allocation alive throughout acquisition.
        unsafe { Self::acquire(self.as_ptr()) }
    }

    /// Transfer one reference to a driver-owned DEVICE_RELATIONS result.
    pub fn into_raw(self) -> *mut DeviceObject {
        let pointer = self.as_ptr();
        core::mem::forget(self);
        pointer
    }

    /// # Safety
    /// `device` must carry one live reference previously transferred with
    /// into_raw. This adopts that reference instead of incrementing it.
    pub unsafe fn from_transferred(device: *mut DeviceObject) -> Option<Self> {
        NonNull::new(device).map(Self)
    }

    /// Retain every device in the current stack under the graph lock. Driver
    /// completion can then retain the original allocations across a detach.
    /// Callers still serialize raw-link reads with attachment mutation.
    pub fn reference_stack(&self) -> Result<ReferencedDeviceStack, i32> {
        let _graph = DEVICE_GRAPH_LOCK.lock();
        let mut current = self.as_ptr();
        for step in 0..MAX_DEVICES {
            // SAFETY: self and locked attachment links protect each device.
            match unsafe { (*current).attached_to } {
                None => break,
                Some(above) if !above.is_null() && step + 1 < MAX_DEVICES => current = above,
                _ => return Err(STATUS_NO_SUCH_DEVICE),
            }
        }
        let mut devices = Vec::new();
        devices
            .try_reserve_exact(MAX_DEVICES)
            .map_err(|_| STATUS_INSUFFICIENT_RESOURCES)?;
        while !current.is_null() {
            if devices.len() == MAX_DEVICES
                || devices
                    .iter()
                    .any(|device: &DeviceReference| device.as_ptr() == current)
            {
                return Err(STATUS_NO_SUCH_DEVICE);
            }
            // SAFETY: attachment mutation and deletion use the same graph lock.
            devices.push(unsafe { Self::acquire_locked(current)? });
            // SAFETY: the newly acquired reference retains current.
            current = unsafe { (*current).attached_device };
        }
        Ok(ReferencedDeviceStack { devices })
    }
}

impl Drop for DeviceReference {
    fn drop(&mut self) {
        // SAFETY: the allocation owner cannot delete with this reference live.
        let previous = unsafe { &(*self.as_ptr()).ref_count }.fetch_sub(1, Ordering::Release);
        debug_assert!(previous > 1);
    }
}

pub struct ReferencedDeviceStack {
    devices: Vec<DeviceReference>,
}

impl ReferencedDeviceStack {
    pub fn top(&self) -> *mut DeviceObject {
        self.devices[0].as_ptr()
    }

    pub fn depth(&self) -> u8 {
        self.devices.len() as u8
    }
}

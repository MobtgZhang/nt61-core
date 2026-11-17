//! An immutable, owned route for a private-source IRP. Topology edits affect
//! future requests; pending requests keep their original devices and drivers.
//! This does not provide native WDM layout or general work-item ownership.
use super::{device_reference::DeviceReference, driver_loader::DriverReference, DeviceObject};
use crate::libs::ntdll::status::*;
use alloc::vec::Vec;

pub(super) struct DispatchReference {
    // Release the allocation reference before permitting its driver to unload.
    device: DeviceReference,
    driver: DriverReference,
}

pub struct RequestRoute {
    hops: Vec<DispatchReference>,
}

impl DispatchReference {
    pub(super) fn driver(&self) -> *mut super::DriverObject {
        self.driver.as_ptr()
    }
}

impl RequestRoute {
    /// Snapshot the graph first, then retain every owning driver. No registry
    /// lock is taken under the device graph lock. If unload has already begun,
    /// fail before dispatch, releasing all partial references.
    pub fn capture(device: &DeviceReference) -> Result<Self, i32> {
        let stack = device.reference_stack()?;
        let mut hops = Vec::new();
        hops.try_reserve_exact(stack.depth() as usize)
            .map_err(|_| STATUS_INSUFFICIENT_RESOURCES)?;
        for device in stack.into_devices() {
            // SAFETY: the retained allocation keeps this immutable owner link
            // live. acquire compares identity under the registry lock before
            // the caller can read or execute any driver fields.
            let driver = DriverReference::acquire(unsafe { (*device.as_ptr()).driver_object })?;
            hops.push(DispatchReference { device, driver });
        }
        Ok(Self { hops })
    }

    pub fn top(&self) -> *mut DeviceObject {
        self.hops[0].device.as_ptr()
    }

    pub fn depth(&self) -> u8 {
        self.hops.len() as u8
    }

    pub fn lower(&self, device: *mut DeviceObject) -> Result<*mut DeviceObject, i32> {
        let index = self
            .hops
            .iter()
            .position(|hop| hop.device.as_ptr() == device)
            .ok_or(STATUS_NO_SUCH_DEVICE)?;
        self.hops
            .get(index + 1)
            .map(|hop| hop.device.as_ptr())
            .ok_or(STATUS_INVALID_DEVICE_REQUEST)
    }

    /// A dispatch callback can complete and free its IRP before returning.
    /// Keep both the callback's device and its driver alive through its return.
    pub(super) fn retain_dispatch(
        &self,
        device: *mut DeviceObject,
    ) -> Result<DispatchReference, i32> {
        let hop = self
            .hops
            .iter()
            .find(|hop| hop.device.as_ptr() == device)
            .ok_or(STATUS_NO_SUCH_DEVICE)?;
        Ok(DispatchReference {
            device: hop.device.try_clone()?,
            driver: hop.driver.try_clone()?,
        })
    }
}

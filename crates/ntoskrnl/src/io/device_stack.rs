use super::{device_lifecycle, DeviceObject, MAX_DEVICES};
use alloc::vec::Vec;
use core::ptr::null_mut;

/// Device stack structure for managing filter device attachments.
/// The device stack represents the chain of devices attached to each other,
/// from the top (closest to user) to the bottom (closest to hardware).
#[derive(Debug)]
pub struct DeviceStack {
    /// Top device in the stack (filter drivers attach here)
    pub top_device: *mut DeviceObject,
    /// Bottom device in the stack (base device/hardware)
    pub bottom_device: *mut DeviceObject,
    /// List of attached filter devices
    pub attached_devices: Option<Vec<*mut DeviceObject>>,
}

impl DeviceStack {
    /// create a new device stack with a base device.
    pub fn new(base_device: *mut DeviceObject) -> Option<Self> {
        if base_device.is_null() {
            return None;
        }
        Some(Self {
            top_device: base_device,
            bottom_device: base_device,
            attached_devices: Some(Vec::new()),
        })
    }

    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    /// Attach a filter device above another device in the stack.
    /// The filter device will receive IRPs before the target device.
    pub unsafe fn attach_filter_device(&mut self, filter: *mut DeviceObject) -> bool {
        if filter.is_null() {
            return false;
        }

        unsafe {
            let _graph = device_lifecycle::DEVICE_GRAPH_LOCK.lock();
            if self.top_device.is_null()
                || filter == self.top_device
                || !(*filter).attached_device.is_null()
                || (*filter).attached_to.is_some()
                || (*self.top_device).attached_to.is_some()
            {
                return false;
            }
            let mut member = self.top_device;
            for step in 0..MAX_DEVICES {
                if member.is_null() {
                    break;
                }
                if member == filter || step + 1 == MAX_DEVICES {
                    return false;
                }
                member = (*member).attached_device;
            }
            // Reserve before changing either link: allocation failure must
            // leave the existing graph and tracking list unchanged.
            let Some(list) = self.attached_devices.as_mut() else {
                return false;
            };
            if list.try_reserve(1).is_err() {
                return false;
            }
            // Link filter above target
            (*filter).attached_device = self.top_device;

            // Update target's attached_to field
            (*self.top_device).attached_to = Some(filter);

            // Add to our tracking list
            if let Some(ref mut list) = self.attached_devices {
                list.push(filter);
            }

            // Update top to the filter
            self.top_device = filter;
        }
        true
    }

    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    /// Detach a filter device from the stack.
    pub unsafe fn detach_filter_device(&mut self, filter: *mut DeviceObject) -> bool {
        if filter.is_null() {
            return false;
        }

        unsafe {
            let _graph = device_lifecycle::DEVICE_GRAPH_LOCK.lock();
            if !self
                .attached_devices
                .as_ref()
                .is_some_and(|devices| devices.contains(&filter))
            {
                return false;
            }
            let below = (*filter).attached_device;
            let above = (*filter).attached_to;

            // Re-link the chain
            if let Some(above_dev) = above {
                (*above_dev).attached_device = below;
            }
            if !below.is_null() {
                (*below).attached_to = above;
            }
            (*filter).attached_device = null_mut();
            (*filter).attached_to = None;

            // Update top/bottom if needed
            if self.top_device == filter {
                self.top_device = below;
            }
            if self.bottom_device == filter {
                self.bottom_device = below;
            }

            // Remove from tracking list
            if let Some(ref mut list) = self.attached_devices {
                list.retain(|&d| d != filter);
            }
        }
        true
    }

    /// Get the number of devices in the stack.
    pub fn depth(&self) -> usize {
        self.attached_devices.as_ref().map_or(0, |v| v.len()) + 1
    }
}

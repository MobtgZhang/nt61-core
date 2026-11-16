//! Lifetime teardown for this project's private device objects.
use super::{DeviceObject, DevicePnPState, MAX_DEVICES};
use crate::mm::pool;
use core::sync::atomic::Ordering;

pub(super) static DEVICE_GRAPH_LOCK: crate::ke::sync::Spinlock<()> = crate::ke::sync::Spinlock::new(());

/// Publish a fully initialized caller-owned device in its driver's list.
///
/// # Safety
/// Both allocations must be live and owned by the caller; driver unload and
/// registration are serialized by the caller or an owning driver reference.
pub(super) unsafe fn publish_device(device: *mut DeviceObject) -> bool {
    publish_device_with(device, || {})
}

/// Run final namespace initialization after capacity validation but before the
/// graph exposes the allocation. The callback must not reenter the graph lock.
///
/// # Safety
/// Same allocation/owner contract as publish_device. The callback initializes
/// only this unpublished allocation and must not free it or change list links.
pub(super) unsafe fn publish_device_with(
    device: *mut DeviceObject,
    initialize: impl FnOnce(),
) -> bool {
    if device.is_null() { return false; }
    let _graph = DEVICE_GRAPH_LOCK.lock();
    let driver = (*device).driver_object;
    if driver.is_null() || (*device).ref_count.load(Ordering::Acquire) != 1 {
        return false;
    }
    let mut current = (*driver).device_object;
    let mut count = 0;
    while !current.is_null() {
        count += 1;
        if current == device || count >= MAX_DEVICES { return false; }
        current = (*current).next_device;
    }
    initialize();
    (*device).next_device = (*driver).device_object;
    (*driver).device_object = device;
    true
}

/// Delete an idle private device allocated by create_device. This refuses
/// attached devices, outstanding device/OB references, VPBs and active IRPs.
/// The driver owns extensions and must release them before deleting a device.
///
/// # Safety
/// The caller serializes device-list access and owns this live allocation.
pub unsafe fn delete_device(device: *mut DeviceObject) -> bool {
    if device.is_null() {
        return false;
    }
    let _graph = DEVICE_GRAPH_LOCK.lock();
    let object = &*device;
    let driver = object.driver_object;
    if driver.is_null()
        || object.ref_count.load(Ordering::Acquire) != 1
        || !object.current_irp.is_null()
        || !object.attached_device.is_null()
        || object.attached_to.is_some()
        || !object.vpb.is_null()
        || !object.device_extension.is_null()
        || !object.device_queue.is_empty()
        || !object.driver_queues.is_empty()
    {
        return false;
    }
    let mut link = core::ptr::addr_of_mut!((*driver).device_object);
    let mut found = false;
    for _ in 0..MAX_DEVICES {
        if *link == device {
            found = true;
            break;
        }
        if (*link).is_null() {
            break;
        }
        link = core::ptr::addr_of_mut!((**link).next_device);
    }
    if !found {
        return false;
    }
    if object.namespace_handle != 0
        && !crate::ob::remove_external_io_object(object.namespace_handle, device.cast())
    {
        return false;
    }
    *link = object.next_device;
    (*device).ref_count.store(0, Ordering::Release);
    (*device).set_pnp_state(DevicePnPState::Deleted);
    let _ = pool::free(device.cast());
    true
}

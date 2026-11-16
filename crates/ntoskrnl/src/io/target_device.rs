//! Synchronous TargetDeviceRelation requests on the private source-level I/O
//! ABI. The request numbers and ownership contract follow NT; these layouts
//! are not binary-compatible WDM structures.
use super::{device_reference::DeviceReference, DeviceObject, FileObject, Irp};
use crate::libs::ntdll::status::*;
use core::{
    ptr,
    sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
};

pub const TARGET_DEVICE_RELATION: u32 = 4;

/// Variable-size result. Drivers allocate it with allocate_relations and
/// transfer every contained device reference to the request's recipient.
#[repr(C)]
pub struct DeviceRelations {
    pub count: u32,
    pub objects: [*mut DeviceObject; 1],
}

pub fn allocate_relations(devices: &[&DeviceReference]) -> Result<*mut DeviceRelations, i32> {
    if devices.len() > super::MAX_DEVICES {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let size = core::mem::size_of::<DeviceRelations>()
        + devices.len().saturating_sub(1) * core::mem::size_of::<*mut DeviceObject>();
    let raw = crate::mm::pool::allocate(crate::mm::pool::PoolType::NonPaged, size)
        as *mut DeviceRelations;
    if raw.is_null() {
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    // SAFETY: raw is an aligned allocation large enough for the header and
    // all entries. count tracks only references already transferred to it.
    unsafe {
        (*raw).count = 0;
        (*raw).objects[0] = ptr::null_mut();
        let entries = ptr::addr_of_mut!((*raw).objects).cast::<*mut DeviceObject>();
        for device in devices {
            let reference = match device.try_clone() {
                Ok(reference) => reference,
                Err(status) => {
                    release_relations(raw);
                    return Err(status);
                }
            };
            entries
                .add((*raw).count as usize)
                .write(reference.into_raw());
            (*raw).count += 1;
        }
    }
    Ok(raw)
}

/// # Safety
/// relations must be null or an owned allocation with count valid entries,
/// each holding one reference transferred with DeviceReference::into_raw.
pub unsafe fn release_relations(relations: *mut DeviceRelations) {
    if relations.is_null() {
        return;
    }
    // SAFETY: the caller transfers the result allocation and its references.
    unsafe {
        let entries = ptr::addr_of!((*relations).objects).cast::<*mut DeviceObject>();
        for index in 0..(*relations).count as usize {
            drop(DeviceReference::from_transferred(entries.add(index).read()));
        }
        let _ = crate::mm::pool::free(relations.cast());
    }
}

struct Completion {
    done: AtomicBool,
    status: AtomicI32,
    information: AtomicUsize,
}

unsafe extern "C" fn capture(_: *mut DeviceObject, irp: *mut Irp, context: *mut ()) -> i32 {
    // SAFETY: query_target_device retains context and IRP until completion;
    // no access to either is made after the release publication below.
    unsafe {
        let completion = &*(context as *const Completion);
        completion
            .status
            .store((*irp).io_status.status as i32, Ordering::Relaxed);
        completion
            .information
            .store((*irp).io_status.information, Ordering::Relaxed);
        completion.done.store(true, Ordering::Release);
    }
    STATUS_MORE_PROCESSING_REQUIRED
}

/// Resolve the hardware target by issuing the actual PnP relation request.
/// Driver errors, resource exhaustion and pending completion are preserved.
///
/// # Safety
/// file must remain live until this call returns. The caller and drivers must
/// serialize attachment topology changes with raw-link forwarding through the
/// request's completion; retained references protect storage, not graph reads.
/// Drivers must follow the
/// private I/O ownership contract: complete a nonpending request before
/// returning, or return STATUS_PENDING and eventually complete it. A successful
/// Information value is null or a valid owned DeviceRelations allocation.
pub unsafe fn query_target_device(
    device: &DeviceReference,
    file: *mut FileObject,
) -> Result<DeviceReference, i32> {
    let stack = device.reference_stack()?;
    // One extra slot belongs to the issuer, above every driver's stack slot.
    let irp = super::allocate_irp(stack.depth() + 1);
    if irp.is_null() {
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    let completion = Completion {
        done: AtomicBool::new(false),
        status: AtomicI32::new(STATUS_NOT_SUPPORTED),
        information: AtomicUsize::new(0),
    };
    // SAFETY: allocation includes the issuer slot and one slot per retained
    // device; context lives through synchronous or asynchronous completion.
    let dispatched = unsafe {
        let issuer = (*irp).current_stack;
        (*issuer).completion_routine = Some(capture);
        (*issuer).completion_context = ptr::addr_of!(completion) as *mut ();
        (*issuer).control = super::irp_stack::SL_INVOKE_ON_SUCCESS
            | super::irp_stack::SL_INVOKE_ON_ERROR
            | super::irp_stack::SL_INVOKE_ON_CANCEL;
        super::irp_stack::skip_current_stack_location(irp);
        let request = (*irp).current_stack;
        (*request).major_function = super::major::IRP_MJ_PNP;
        (*request).minor_function = super::pnp::IRP_MN_QUERY_DEVICE_RELATIONS;
        (*request).parameters.as_u64 = TARGET_DEVICE_RELATION as u64;
        (*request).file_object = file;
        (*irp).io_status.status = STATUS_NOT_SUPPORTED as u32;
        super::dispatch::dispatch_irp(stack.top(), irp)
    };
    if dispatched != STATUS_PENDING && !completion.done.load(Ordering::Acquire) {
        // A driver returning synchronously without completing violates the
        // contract. Reclaim the still-owned IRP and report the failure rather
        // than inventing a relation or leaving an issuer pointer dangling.
        // SAFETY: no pending ownership was transferred by this return value.
        unsafe {
            (*irp).io_status.status = if dispatched < 0 {
                dispatched
            } else {
                STATUS_INVALID_DEVICE_REQUEST
            } as u32;
            super::io_complete_request(irp, 0);
        }
    }
    while !completion.done.load(Ordering::Acquire) {
        crate::ke::scheduler::yield_();
    }
    let status = completion.status.load(Ordering::Relaxed);
    let result = completion.information.load(Ordering::Relaxed) as *mut DeviceRelations;
    // SAFETY: capture took ownership with MORE_PROCESSING_REQUIRED; resuming
    // completion after its slot was removed performs normal cleanup exactly once.
    unsafe {
        super::io_complete_request(irp, 0);
    }
    if status < 0 {
        return Err(status);
    }
    if result.is_null() {
        return Err(STATUS_NO_SUCH_DEVICE);
    }
    // SAFETY: a successful driver transfers a valid relation allocation.
    unsafe {
        if (*result).count != 1 {
            release_relations(result);
            return Err(STATUS_NO_SUCH_DEVICE);
        }
        let target = DeviceReference::from_transferred((*result).objects[0]);
        let _ = crate::mm::pool::free(result.cast());
        target.ok_or(STATUS_NO_SUCH_DEVICE)
    }
}

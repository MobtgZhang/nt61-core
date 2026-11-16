//! Completion ownership for private synchronous file requests.
//! Keeps the issuer's buffers, file and stack capture alive until completion,
//! including a driver returning PENDING. This is not user asynchronous I/O.
use super::{DeviceObject, Irp};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

struct Completion {
    done: AtomicBool,
    status: AtomicI32,
    information: AtomicUsize,
}

unsafe extern "C" fn capture(_device: *mut DeviceObject, irp: *mut Irp, context: *mut ()) -> i32 {
    // SAFETY: submit_and_wait retains the capture until the release/acquire
    // handshake. The driver completes once and owns the IRP until this point.
    let completion = unsafe { &*(context as *const Completion) };
    unsafe {
        completion
            .status
            .store((*irp).io_status.status as i32, Ordering::Relaxed);
        completion
            .information
            .store((*irp).io_status.information, Ordering::Relaxed);
    }
    completion.done.store(true, Ordering::Release);
    // Transfer IRP storage to the waiting issuer; completion must not free it.
    crate::libs::ntdll::status::STATUS_MORE_PROCESSING_REQUIRED
}

/// Submit a newly allocated private IRP and reclaim it after its completion.
///
/// # Safety
/// The device, IRP, its file and all buffers must remain alive. No completion
/// routine may already be installed at the current (highest) stack location. The driver
/// must eventually complete exactly once, including immediate dispatch errors.
/// Call in a schedulable context without locks required by completion/drivers.
pub unsafe fn submit_and_wait(device: *mut DeviceObject, irp: *mut Irp) -> (i32, usize) {
    let completion = Completion {
        done: AtomicBool::new(false),
        status: AtomicI32::new(0),
        information: AtomicUsize::new(0),
    };
    unsafe {
        let stack = (*irp).current_stack;
        (*stack).completion_routine = Some(capture);
        (*stack).completion_context = core::ptr::addr_of!(completion) as *mut ();
        (*stack).control = super::irp_stack::SL_INVOKE_ON_SUCCESS
            | super::irp_stack::SL_INVOKE_ON_ERROR
            | super::irp_stack::SL_INVOKE_ON_CANCEL;
        // A non-PENDING dispatch result is not proof of completion either.
        // Only the completion callback can release the issuer's lifetimes.
        let _ = super::io_call_driver(device, irp);
        while !completion.done.load(Ordering::Acquire) {
            crate::ke::scheduler::yield_();
        }
        let result = (
            completion.status.load(Ordering::Relaxed),
            completion.information.load(Ordering::Relaxed),
        );
        // The capture routine was consumed on its first invocation. Finish the
        // ordinary statistics/notification/free path now that we own the IRP.
        super::io_complete_request(irp, 0);
        result
    }
}

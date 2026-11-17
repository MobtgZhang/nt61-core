//! Private FILE_OBJECT cleanup/close requests. The handle table determines
//! when to send each request and retains the payload through its completion.
use super::{DeviceObject, FileObject, Irp};
use crate::libs::ntdll::status::{STATUS_INVALID_DEVICE_REQUEST, STATUS_NO_MEMORY};
use core::ptr;

/// Resources acquired before CREATE establishes any driver-owned open state.
/// A failed CREATE releases both unused requests; a successful open consumes
/// them independently at last handle and last reference, without allocating.
pub(crate) struct Requests {
    cleanup: Option<Request>,
    close: Option<Request>,
}

impl Requests {
    pub const fn new() -> Self {
        Self { cleanup: None, close: None }
    }

    pub fn reserve() -> Result<Self, i32> {
        // Locals own each allocation so a partially acquired pair rolls back.
        let cleanup = Request::allocate(super::major::IRP_MJ_CLEANUP)?;
        let close = Request::allocate(super::major::IRP_MJ_CLOSE)?;
        Ok(Self { cleanup: Some(cleanup), close: Some(close) })
    }

    /// Transfer just one request, ending the FILE_OBJECT field borrow before
    /// entering the driver, which can complete other requests on the same file.
    pub fn take(&mut self, major: u8) -> Result<Request, i32> {
        match major {
            super::major::IRP_MJ_CLEANUP => self.cleanup.take(),
            super::major::IRP_MJ_CLOSE => self.close.take(),
            _ => None,
        }.ok_or(STATUS_INVALID_DEVICE_REQUEST)
    }
}

/// An undispatched teardown IRP. Dropping it releases its unused allocation.
pub(crate) struct Request {
    irp: *mut Irp,
    major: u8,
}

impl Request {
    fn allocate(major: u8) -> Result<Self, i32> {
        let irp = super::allocate_irp(1);
        if irp.is_null() { return Err(STATUS_NO_MEMORY); }
        Ok(Self { irp, major })
    }

    /// Send CLEANUP or CLOSE using its reserved IRP and await completion.
    ///
    /// # Safety
    /// `device`, its driver and `file` must stay alive until return. Call
    /// without a lock needed by the driver or pending I/O: cleanup may cancel
    /// and complete those requests itself.
    pub unsafe fn submit(mut self, device: *mut DeviceObject, file: *mut FileObject) -> i32 {
        if device.is_null() { return STATUS_INVALID_DEVICE_REQUEST; }
        let irp = core::mem::replace(&mut self.irp, ptr::null_mut());
        // SAFETY: this request owns one unused stack slot; the caller retains
        // its targets. submit_and_wait takes ownership and frees after completion.
        unsafe {
            let stack = (*irp).current_stack;
            (*stack).major_function = self.major;
            (*stack).device_object = device;
            (*stack).file_object = file;
            super::synchronous_request::submit_and_wait(device, irp).0
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        if !self.irp.is_null() {
            // This request has never been submitted; no driver can own it.
            super::free_irp(self.irp);
        }
    }
}

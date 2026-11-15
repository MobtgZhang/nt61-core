//! Trusted wait helpers share the native dispatcher object state and queues.
//! User APC delivery still requires a user-return trampoline.

use super::types::{HANDLE, NTSTATUS};
use core::ptr;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KWaitBlock {
    pub next_wait_block: *mut KWaitBlock,
    pub thread: u64, // ETHREAD pointer (or TID)
    pub object: u64, // Object pointer
    pub wait_key: u16,
    pub wait_type: u16,
    pub wait_list_entry: *mut KWaitBlock,
}

impl KWaitBlock {
    pub const fn new() -> Self {
        Self {
            next_wait_block: ptr::null_mut(),
            thread: 0,
            object: 0,
            wait_key: 0,
            wait_type: 0,
            wait_list_entry: ptr::null_mut(),
        }
    }
}

impl Default for KWaitBlock {
    fn default() -> Self {
        Self::new()
    }
}

pub const WAIT_TYPE_ANY: u16 = 1; // WaitAny - return when any object is signaled
pub const WAIT_TYPE_ALL: u16 = 0; // WaitAll - return when all objects are signaled

pub const MAXIMUM_WAIT_OBJECTS: usize = 64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    NotWaiting = 0,
    Waiting = 1,
    Satisfied = 2,
    TimedOut = 3,
    Abandoned = 4,
    Alerted = 5,
}

#[derive(Clone, Copy)]
pub struct ThreadWaitContext {
    pub state: WaitState,
    pub wait_blocks: [KWaitBlock; MAXIMUM_WAIT_OBJECTS],
    pub wait_count: usize,
    pub wait_type: u16,
    pub timeout_ms: u64,
    pub signaled_index: usize,
}

impl ThreadWaitContext {
    pub const fn new() -> Self {
        const EMPTY_BLOCK: KWaitBlock = KWaitBlock::new();
        Self {
            state: WaitState::NotWaiting,
            wait_blocks: [EMPTY_BLOCK; MAXIMUM_WAIT_OBJECTS],
            wait_count: 0,
            wait_type: WAIT_TYPE_ANY,
            timeout_ms: 0,
            signaled_index: 0,
        }
    }
}

impl Default for ThreadWaitContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait using a captured timeout rather than a user pointer.
pub fn nt_wait_for_single_object_kernel(
    handle: HANDLE,
    alertable: u8,
    timeout: Option<i64>,
) -> NTSTATUS {
    super::sync::nt_wait_for_single_object_kernel(handle, alertable, timeout)
}

/// WaitAll=0 and WaitAny=1, matching WAIT_TYPE in the native API.
pub fn nt_wait_for_multiple_objects_kernel(
    count: u32,
    handles: &[HANDLE],
    wait_type: u32,
    alertable: u8,
    timeout: Option<i64>,
) -> NTSTATUS {
    super::sync::nt_wait_for_multiple_objects_kernel(count, handles, wait_type, alertable, timeout)
}

pub fn signal_event(handle: HANDLE) -> bool {
    // SAFETY: no user output is requested; the handle is checked by the dispatcher.
    unsafe { super::sync::nt_set_event(handle, ptr::null_mut()) >= 0 }
}

pub fn release_mutex(handle: HANDLE) -> bool {
    // SAFETY: no user output is requested; ownership is checked by the dispatcher.
    unsafe { super::sync::nt_release_mutant(handle, ptr::null_mut()) >= 0 }
}

pub fn release_semaphore(handle: HANDLE, release_count: i32) -> bool {
    // SAFETY: no user output is requested; count and handle are checked natively.
    unsafe { super::sync::nt_release_semaphore(handle, release_count as u32, ptr::null_mut()) >= 0 }
}

/// APC alert testing and delivery are not implemented by this compatibility helper.
pub fn test_alert() -> bool {
    false
}
pub fn deliver_apcs() {}

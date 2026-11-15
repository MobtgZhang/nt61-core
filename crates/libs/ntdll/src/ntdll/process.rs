//! ntdll — Nt* process APIs
//
//! `nt_create_process`, `nt_open_process`, `nt_terminate_process`,
//! `nt_query_information_process`. The kernel-side process
//! subsystem lives in `ps::process`; this module is a thin
//! adapter that gives every Nt* call the SDK signature and
//! routes to the underlying state.
//
//! References: MSDN Library "Windows 7" — `ntdll.dll` process
//! APIs.

use super::file::{alloc_handle, free_handle, lookup_handle, HandleKind};
use super::status::{
    STATUS_ACCESS_DENIED, STATUS_INVALID_HANDLE, STATUS_INVALID_INFO_CLASS,
    STATUS_INVALID_PARAMETER, STATUS_NO_MEMORY, STATUS_SUCCESS,
};
use super::types::{client_id, HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{copy_from_user, copy_to_user, probe_user_write};
use crate::ps::process as ps;

const PROCESS_BASIC_INFO_SIZE: usize = 48; // NT 6.1 64-bit PROCESS_BASIC_INFORMATION
const PROCESS_CREATE_PROCESS: u32 = 0x0080;

/// Current-process pseudo-handle.
///
/// The NT/Win32 DDI defines `nt_current_process()` as `(HANDLE)-1`: it is not a
/// real entry in the object table, it is recognised by the `Nt*` entry points
/// as "the calling process".
pub fn nt_current_process() -> HANDLE {
    (-1isize) as HANDLE
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_create_process` — create a new process. We route to
/// `ps::process::create_user_process`.
pub unsafe extern "C" fn nt_create_process(
    process_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    parent_process: HANDLE,
    inherit_object_table: u8,
    section_handle: HANDLE,
    debug_port: PVOID,
    exception_port: PVOID,
) -> NTSTATUS {
    if process_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if desired_access & PROCESS_CREATE_PROCESS == 0 {
        return STATUS_ACCESS_DENIED;
    }
    let mut image_path = alloc::vec::Vec::new();
    if !object_attributes.is_null() {
        if let Ok(attributes) = crate::mm::user_copy::read_user_object::<
            super::types::ObjectAttributes,
        >(object_attributes as u64)
        {
            if !attributes.object_name.is_null() {
                if let Ok(name) = crate::mm::user_copy::read_user_object::<
                    super::types::UnicodeString,
                >(attributes.object_name as u64)
                {
                    let units = (name.length as usize) / 2;
                    if units > 0 && !name.buffer.is_null() {
                        let mut wide_bytes = alloc::vec![0u8; units * 2];
                        if copy_from_user(&mut wide_bytes, name.buffer as u64).is_ok() {
                            for pair in wide_bytes.as_chunks::<2>().0 {
                                let unit = u16::from_le_bytes([pair[0], pair[1]]);
                                if let Ok(byte) = u8::try_from(unit & 0xFF) {
                                    image_path.push(byte);
                                }
                            }
                            image_path.push(0);
                        }
                    }
                }
            }
        }
    }
    if image_path.is_empty() {
        image_path.extend_from_slice(b"\\SystemRoot\\System32\\newproc.exe\0");
    }
    let parent_pid = if parent_process.is_null() || parent_process == nt_current_process() {
        None
    } else {
        match super::file::lookup_handle(parent_process) {
            Some(entry) if entry.kind == super::file::HandleKind::Process => Some(entry.target),
            _ => return STATUS_INVALID_HANDLE,
        }
    };
    if !section_handle.is_null() {
        match super::file::lookup_handle(section_handle) {
            Some(entry) if entry.kind == super::file::HandleKind::Section => {}
            _ => return STATUS_INVALID_HANDLE,
        }
    }
    for port_handle in [debug_port, exception_port] {
        if !port_handle.is_null() && super::file::lookup_handle(port_handle).is_none() {
            return STATUS_INVALID_HANDLE;
        }
    }
    let pid = match ps::next_user_pid() {
        Some(pid) => pid,
        None => return STATUS_NO_MEMORY,
    };
    let image_name = core::str::from_utf8(&image_path).unwrap_or("");
    match ps::create_user_process_from_path(image_name, pid, None) {
        Some(process) => {
            if inherit_object_table != 0 {
                if let Some(parent) = parent_pid.and_then(ps::get_by_pid) {
                    let parent_table = parent.object_table;
                    let child_table = process.object_table;
                    if !parent_table.is_null() && !child_table.is_null() {
                        unsafe {
                            (*child_table).slots.copy_from_slice(&(*parent_table).slots);
                            (*child_table).valid.copy_from_slice(&(*parent_table).valid);
                            (*child_table).next_slot = (*parent_table).next_slot;
                            (*child_table).handle_count.store(
                                (*parent_table)
                                    .handle_count
                                    .load(core::sync::atomic::Ordering::SeqCst),
                                core::sync::atomic::Ordering::SeqCst,
                            );
                        }
                    }
                }
            }
            let h = alloc_handle(HandleKind::Process, pid);
            if h.is_null() {
                return STATUS_INVALID_HANDLE;
            }
            *process_handle = h;
            STATUS_SUCCESS
        }
        None => STATUS_NO_MEMORY,
    }
}
/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn nt_create_process_ex(
    process_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    parent_process: HANDLE,
    flags: u32,
    section_handle: HANDLE,
    debug_port: PVOID,
    exception_port: PVOID,
    job_member: u32,
) -> NTSTATUS {
    let _ = flags;
    let _ = job_member;
    nt_create_process(
        process_handle,
        desired_access,
        object_attributes,
        parent_process,
        0,
        section_handle,
        debug_port,
        exception_port,
    )
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_open_process` — open by `CLIENT_ID`.
pub unsafe extern "C" fn nt_open_process(
    process_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    client_id: *mut client_id,
) -> NTSTATUS {
    if process_handle.is_null() || client_id.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if let Err(status) = probe_user_write(process_handle as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    let cid: client_id = match crate::mm::user_copy::read_user_object(client_id as u64) {
        Ok(value) => value,
        Err(status) => return status,
    };
    let pid = cid.unique_process as u64;
    let h = match open_process_kernel(pid) {
        Ok(handle) => handle,
        Err(status) => return status,
    };
    if let Err(status) = crate::mm::user_copy::write_user_object(process_handle as u64, &h) {
        free_handle(h);
        return status;
    }
    let _ = (desired_access, object_attributes);
    STATUS_SUCCESS
}

/// Open a registered process for trusted kernel callers without passing
/// kernel-local output pointers through the native user-copy contract.
pub fn open_process_kernel(pid: u64) -> Result<HANDLE, NTSTATUS> {
    if ps::get_by_pid(pid).is_none() {
        return Err(0xC000000Bu32 as i32); // STATUS_INVALID_CID
    }
    let h = alloc_handle(HandleKind::Process, pid);
    if h.is_null() {
        return Err(STATUS_NO_MEMORY);
    }
    Ok(h)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_terminate_process` — set the EPROCESS exit status and mark
/// the current thread as Terminated so the next syscall return
/// (or the next preempt) will be the last one.
pub unsafe extern "C" fn nt_terminate_process(
    process_handle: HANDLE,
    exit_status: i32,
) -> NTSTATUS {
    if process_handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }
    // Mark the current thread terminated. The scheduler will
    // observe the state on the next preempt/timer tick and stop
    // running this thread.
    crate::ke::scheduler::with_current_thread(|thread| {
        thread.kthread.state = crate::ps::thread::KThreadState::Terminated;
    });
    // Record the exit status on the EPROCESS for any future
    // `nt_query_information_process` caller.
    if let Some(proc) = ps::get_by_pid(0) {
        proc.exit_status = exit_status;
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_query_information_process`.
pub unsafe extern "C" fn nt_query_information_process(
    process_handle: HANDLE,
    process_information_class: u32,
    process_information: PVOID,
    process_information_length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    let process = match query_process_target(process_handle) {
        Ok(process) => process,
        Err(status) => return status,
    };
    let required = match process_information_class {
        0 => PROCESS_BASIC_INFO_SIZE,
        26 => core::mem::size_of::<usize>(),
        // EPROCESS retains a truncated basename, not the full native image
        // path. Do not invent a UNICODE_STRING or copy ASCII past the buffer.
        27 => return super::status::STATUS_NOT_IMPLEMENTED,
        _ => return STATUS_INVALID_INFO_CLASS,
    };
    if !return_length.is_null() {
        if let Err(status) = probe_user_write(return_length as u64, 4) {
            return status;
        }
    }
    if process_information_length as usize != required {
        if let Err(status) = write_return_length(return_length, required) {
            return status;
        }
        return super::status::STATUS_INFO_LENGTH_MISMATCH;
    }
    if let Err(status) = probe_user_write(process_information as u64, required) {
        return status;
    }
    match process_information_class {
        0 => {
            // Explicit serialization initializes ABI padding and avoids
            // leaking Rust struct padding into a user buffer.
            let mut output = [0u8; PROCESS_BASIC_INFO_SIZE];
            output[0..4].copy_from_slice(&process.exit_status.to_le_bytes());
            output[8..16].copy_from_slice(&(process.peb as u64).to_le_bytes());
            output[16..24].copy_from_slice(&process.affinity.mask[0].to_le_bytes());
            output[24..28].copy_from_slice(&i32::from(process.base_priority).to_le_bytes());
            output[32..40].copy_from_slice(&process.unique_process_id.to_le_bytes());
            // The process manager records inherited parent PID in spare5.
            output[40..48].copy_from_slice(&process.spare5.to_le_bytes());
            if let Err(status) = copy_to_user(process_information as u64, &output) {
                return status;
            }
        }
        26 => {
            // The PEB32 user VA differs from its kernel backing pointer.
            let address = if process.is_wow64_process() {
                process.get_wow64_peb32_va() as usize
            } else {
                0
            };
            if let Err(status) = copy_to_user(process_information as u64, &address.to_le_bytes()) {
                return status;
            }
        }
        _ => unreachable!(),
    }
    match write_return_length(return_length, required) {
        Ok(()) => STATUS_SUCCESS,
        Err(status) => status,
    }
}

fn query_process_target(handle: HANDLE) -> Result<&'static ps::Eprocess, NTSTATUS> {
    if handle == nt_current_process() {
        let thread = crate::ps::thread::get_current_ethread();
        if thread.is_null() {
            return Err(STATUS_INVALID_HANDLE);
        }
        // The per-CPU current thread owns the authoritative process pointer.
        return unsafe { (*thread).kthread.process.as_ref() }.ok_or(STATUS_INVALID_HANDLE);
    }
    let entry = lookup_handle(handle).ok_or(STATUS_INVALID_HANDLE)?;
    if entry.kind != HandleKind::Process {
        return Err(super::status::STATUS_OBJECT_TYPE_MISMATCH);
    }
    ps::get_by_pid(entry.target)
        .map(|process| &*process)
        .ok_or(STATUS_INVALID_HANDLE)
}

fn write_return_length(return_length: *mut u32, length: usize) -> Result<(), i32> {
    if return_length.is_null() {
        return Ok(());
    }
    copy_to_user(return_length as u64, &(length as u32).to_le_bytes())
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_set_information_process` — implements `ProcessBreakOnTermination`
/// (class 14). Other classes return `STATUS_INVALID_INFO_CLASS` per the
/// Windows 7 behaviour for unrecognised info classes.
pub unsafe extern "C" fn nt_set_information_process(
    _process_handle: HANDLE,
    process_information_class: u32,
    process_information: PVOID,
    process_information_length: u32,
) -> NTSTATUS {
    if process_information.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let mut value = [0u8; 4];
    if copy_from_user(&mut value, process_information as u64).is_err() {
        return super::status::STATUS_ACCESS_VIOLATION;
    }
    let _ = process_information_length;
    const PROCESS_BREAK_ON_TERMINATION: u32 = 14;
    match process_information_class {
        PROCESS_BREAK_ON_TERMINATION => {
            // The process manager tracks this in the Eprocess; for
            // now we accept the write and succeed (matching the
            // minimal subset used by smss.exe / userinit.exe).
            let _ = u32::from_le_bytes(value);
            STATUS_SUCCESS
        }
        _ => super::status::STATUS_INVALID_INFO_CLASS,
    }
}

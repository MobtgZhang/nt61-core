//! Win7 job handles share the kernel process-group implementation. Lock order is
//! native handle table -> job table -> member list. Never terminate processes
//! while any of those locks is held. Unsupported policy enforcement is rejected.
use super::file::{self, DispatcherState, HandleKind};
use super::status::*;
use super::types::{ObjectAttributes, HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object, copy_to_user};
use crate::ps::job;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

const QUERY: u32 = 4;
const SET_ATTRIBUTES: u32 = 2;
const TERMINATE: u32 = 8;

/// Keep a private alias alive across process exit. Exit cleanup can reenter
/// both tables, so neither the native handle lock nor the job lock may be held.
pub extern "C" fn nt_terminate_job_object(handle: HANDLE, exit_status: NTSTATUS) -> NTSTATUS {
    let alias = match file::duplicate_sync_handle_with_access(handle, 0, 0, 2) {
        Ok(alias) => alias,
        Err(status) => return status,
    };
    let result = with_job(alias, TERMINATE, |job| Ok(job.job_id))
        .map(|identity| job::terminate_job(identity, exit_status as u32) as NTSTATUS);
    file::free_handle(alias);
    result.unwrap_or_else(|status| status)
}

fn with_job<T>(handle: HANDLE, access: u32, operation: impl FnOnce(&job::JobObject) -> Result<T, NTSTATUS>) -> Result<T, NTSTATUS> {
    file::with_sync_access(handle, access, |entries| {
        let entry = entries[0];
        if entry.kind != HandleKind::Job { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
        job::with_job(entry.target, operation).ok_or(STATUS_INVALID_HANDLE)?
    })
}

fn process_id(handle: HANDLE, access: u32) -> Result<u64, NTSTATUS> {
    if handle as isize == -1 { return Ok(super::sync::current_client_id().0); }
    file::with_sync_access(handle, access, |entries| {
        if entries[0].kind != HandleKind::Process { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
        Ok(entries[0].target)
    })
}

fn create_or_open(output: *mut HANDLE, access: u32, attributes: *mut ObjectAttributes, create: bool) -> NTSTATUS {
    let result = (|| {
        probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
        let attributes = super::object::capture_attributes(attributes)?;
        let access = super::object::map_sync_access(HandleKind::Job, access)?;
        let (handle, status) = file::create_object_handle(HandleKind::Job, DispatcherState::None,
            access, attributes, create, || job::create_job().map_err(|status| status as NTSTATUS))?;
        if let Err(status) = write_user_object(output as u64, &handle) {
            file::free_handle(handle);
            return Err(status);
        }
        Ok(status)
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// The native memory-copy layer captures the output and object attributes.
pub unsafe extern "C" fn nt_create_job_object(output: *mut HANDLE, access: u32, attributes: *mut ObjectAttributes) -> NTSTATUS {
    create_or_open(output, access, attributes, true)
}

/// # Safety
/// The native memory-copy layer captures the output and object attributes.
pub unsafe extern "C" fn nt_open_job_object(output: *mut HANDLE, access: u32, attributes: *mut ObjectAttributes) -> NTSTATUS {
    create_or_open(output, access, attributes, false)
}

pub extern "C" fn nt_assign_process_to_job_object(handle: HANDLE, process: HANDLE) -> NTSTATUS {
    let result = (|| {
        let pid = process_id(process, 0x101)?; // PROCESS_SET_QUOTA | PROCESS_TERMINATE
        if crate::ps::process::get_by_pid(pid).is_none() { return Err(STATUS_PROCESS_IS_TERMINATING); }
        file::with_sync_access(handle, 1, |entries| {
            if entries[0].kind != HandleKind::Job { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
            Ok(job::assign_process_to_job(entries[0].target, pid) as NTSTATUS)
        })
    })();
    result.unwrap_or_else(|status| status)
}

pub extern "C" fn nt_is_process_in_job(process: HANDLE, handle: HANDLE) -> NTSTATUS {
    let result = (|| {
        let pid = process_id(process, 0x400)?;
        let membership = job::process_job(pid);
        let matches = if membership.is_none() { false } else if handle.is_null() {
            true
        } else {
            with_job(handle, QUERY, |job| Ok(job.contains_process(pid)))?
        };
        Ok(if matches { 0x124 } else { 0x123 }) // STATUS_PROCESS_IN_JOB / NOT_IN_JOB
    })();
    result.unwrap_or_else(|status| status)
}

fn basic_limits(job: &job::JobObject) -> [u8; 64] {
    let mut bytes = [0; 64];
    let limit = job.process_limit.load(Ordering::Acquire);
    if limit != 0 {
        bytes[16..20].copy_from_slice(&8u32.to_le_bytes()); // JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        bytes[40..44].copy_from_slice(&limit.to_le_bytes());
    }
    bytes
}

/// # Safety
/// Output buffers are probed and written through the user-copy layer.
pub unsafe extern "C" fn nt_query_information_job_object(handle: HANDLE, class: u32, output: PVOID, length: u32, returned: *mut u32) -> NTSTATUS {
    let result = (|| {
        if !returned.is_null() { probe_user_write(returned as u64, 4)?; }
        if length != 0 { probe_user_write(output as u64, length as usize)?; }
        let query = |job: &job::JobObject| -> Result<NTSTATUS, NTSTATUS> {
            let mut bytes = Vec::new();
            match class {
                2 | 9 => {
                    let required = if class == 2 { 64 } else { 144 };
                    if !returned.is_null() { write_user_object(returned as u64, &required)?; }
                    if length != required { return Ok(STATUS_INFO_LENGTH_MISMATCH); }
                    bytes.try_reserve_exact(required as usize).map_err(|_| STATUS_NO_MEMORY)?;
                    bytes.resize(required as usize, 0);
                    bytes[..64].copy_from_slice(&basic_limits(job));
                }
                3 => {
                    let members = job.process_list.lock();
                    if length < 8 { return Ok(STATUS_INFO_LENGTH_MISMATCH); }
                    let count = members.len().min((length as usize - 8) / 8);
                    if !returned.is_null() { write_user_object(returned as u64, &((8 + count * 8) as u32))?; }
                    bytes.try_reserve_exact(8 + count * 8).map_err(|_| STATUS_NO_MEMORY)?;
                    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(&(count as u32).to_le_bytes());
                    for pid in members.iter().take(count) { bytes.extend_from_slice(&pid.to_le_bytes()); }
                    copy_to_user(output as u64, &bytes)?;
                    return Ok(if count == members.len() { STATUS_SUCCESS } else { STATUS_BUFFER_OVERFLOW });
                }
                1..=12 => return Ok(STATUS_NOT_SUPPORTED),
                _ => return Ok(STATUS_INVALID_INFO_CLASS),
            }
            copy_to_user(output as u64, &bytes)?;
            Ok(STATUS_SUCCESS)
        };
        if handle.is_null() {
            let id = job::process_job(super::sync::current_client_id().0).ok_or(STATUS_ACCESS_DENIED)?;
            job::with_job(id, query).ok_or(STATUS_INVALID_HANDLE)?
        } else { with_job(handle, QUERY, query) }
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// Input is captured before changing job state. Only an enforced policy can be set.
pub unsafe extern "C" fn nt_set_information_job_object(handle: HANDLE, class: u32, input: PVOID, length: u32) -> NTSTATUS {
    let result = (|| {
        if class != 2 && class != 9 { return Err(if (1..=12).contains(&class) { STATUS_NOT_SUPPORTED } else { STATUS_INVALID_INFO_CLASS }); }
        let required = if class == 2 { 64 } else { 144 };
        if length != required { return Err(STATUS_INFO_LENGTH_MISMATCH); }
        let basic = read_user_object::<[u8; 64]>(input as u64)?;
        if class == 9 {
            let extra = read_user_object::<[u8; 80]>(input as u64 + 64)?;
            // IO counters and peak fields are output-only; memory policy fields
            // at offsets 112/120 are relevant only when their flags are enabled.
            let _ = extra;
        }
        let flags = u32::from_le_bytes(basic[16..20].try_into().map_err(|_| STATUS_INVALID_PARAMETER)?);
        if flags & !8 != 0 { return Err(STATUS_NOT_SUPPORTED); }
        let limit = u32::from_le_bytes(basic[40..44].try_into().map_err(|_| STATUS_INVALID_PARAMETER)?);
        if flags & 8 != 0 && limit == 0 { return Err(STATUS_INVALID_PARAMETER); }
        with_job(handle, SET_ATTRIBUTES, |job| {
            job.set_process_limit(if flags & 8 != 0 { limit } else { 0 });
            Ok(STATUS_SUCCESS)
        })
    })();
    result.unwrap_or_else(|status| status)
}

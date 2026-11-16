//! Native object directories and symbolic links. Enumeration returns counted
//! UTF-16 strings whose pointers refer only to the caller's output allocation.

use super::file::{self, DispatcherState, HandleKind};
use super::object::{capture_attributes, map_sync_access};
use super::status::*;
use super::types::{ObjectAttributes, UnicodeString, HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{copy_from_user, copy_to_user, probe_user_write, read_user_object, write_user_object};
use alloc::vec::Vec;

/// Making an object permanent requires SeCreatePermanentPrivilege even when
/// the supplied object already has a permanent namespace reference.
pub extern "C" fn nt_make_permanent_object(handle: HANDLE) -> NTSTATUS {
    if !super::privilege::enabled(13) {
        return STATUS_PRIVILEGE_NOT_HELD;
    }
    file::set_object_permanent(handle, true).map_or_else(|status| status, |()| STATUS_SUCCESS)
}

/// Release the namespace's permanent reference. The name survives until the
/// final handle closes; duplicate handles continue to reference the same object.
pub extern "C" fn nt_make_temporary_object(handle: HANDLE) -> NTSTATUS {
    file::set_object_permanent(handle, false).map_or_else(|status| status, |()| STATUS_SUCCESS)
}

fn namespace_handle(output: *mut HANDLE, access: u32, attributes: *mut ObjectAttributes,
    kind: HandleKind, create: bool, target: Option<Vec<u16>>) -> NTSTATUS
{
    let result = (|| {
        probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
        let captured = capture_attributes(attributes)?;
        let access = map_sync_access(kind, access)?;
        let (handle, status) = file::create_named_handle(kind, DispatcherState::None,
            access, captured, create, || Ok(0), target)?;
        if let Err(error) = write_user_object(output as u64, &handle) {
            file::free_handle(handle);
            return Err(error);
        }
        Ok(status)
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// All pointer arguments designate caller memory and are captured by user_copy.
pub unsafe extern "C" fn nt_create_directory_object(output: *mut HANDLE, access: u32,
    attributes: *mut ObjectAttributes) -> NTSTATUS
{
    namespace_handle(output, access, attributes, HandleKind::Directory, true, None)
}

/// # Safety
/// All pointer arguments designate caller memory and are captured by user_copy.
pub unsafe extern "C" fn nt_open_directory_object(output: *mut HANDLE, access: u32,
    attributes: *mut ObjectAttributes) -> NTSTATUS
{
    namespace_handle(output, access, attributes, HandleKind::Directory, false, None)
}

/// # Safety
/// All pointer arguments designate caller memory and are captured by user_copy.
pub unsafe extern "C" fn nt_create_symbolic_link_object(output: *mut HANDLE, access: u32,
    attributes: *mut ObjectAttributes, target: *const UnicodeString) -> NTSTATUS
{
    let result = (|| {
        let string = read_user_object::<UnicodeString>(target as u64)?;
        if string.length == 0 || string.length & 1 != 0 || string.length > string.maximum_length {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(usize::from(string.length)).map_err(|_| STATUS_NO_MEMORY)?;
        bytes.resize(usize::from(string.length), 0);
        copy_from_user(&mut bytes, string.buffer as u64)?;
        let mut units = Vec::new();
        units.try_reserve_exact(bytes.len() / 2).map_err(|_| STATUS_NO_MEMORY)?;
        for pair in bytes.chunks_exact(2) { units.push(u16::from_le_bytes([pair[0], pair[1]])); }
        Ok(namespace_handle(output, access, attributes, HandleKind::SymbolicLink, true, Some(units)))
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// All pointer arguments designate caller memory and are captured by user_copy.
pub unsafe extern "C" fn nt_open_symbolic_link_object(output: *mut HANDLE, access: u32,
    attributes: *mut ObjectAttributes) -> NTSTATUS
{
    namespace_handle(output, access, attributes, HandleKind::SymbolicLink, false, None)
}

/// # Safety
/// All pointer arguments designate caller memory and are captured by user_copy.
pub unsafe extern "C" fn nt_query_symbolic_link_object(handle: HANDLE, output: *mut UnicodeString,
    returned: *mut u32) -> NTSTATUS
{
    let result = (|| {
        let string = read_user_object::<UnicodeString>(output as u64)?;
        probe_user_write(output as u64, core::mem::size_of::<UnicodeString>())?;
        if !returned.is_null() { probe_user_write(returned as u64, 4)?; }
        let target = file::symbolic_link_target(handle)?;
        let length = u16::try_from(target.len() * 2).map_err(|_| STATUS_NAME_TOO_LONG)?;
        let required = u32::from(length) + 2;
        if u32::from(string.maximum_length) < required {
            if !returned.is_null() { write_user_object(returned as u64, &required)?; }
            return Err(STATUS_BUFFER_TOO_SMALL);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(required as usize).map_err(|_| STATUS_NO_MEMORY)?;
        for unit in target { bytes.extend_from_slice(&unit.to_le_bytes()); }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        copy_to_user(string.buffer as u64, &bytes)?;
        write_user_object(output as u64, &length)?;
        if !returned.is_null() { write_user_object(returned as u64, &required)?; }
        Ok(STATUS_SUCCESS)
    })();
    result.unwrap_or_else(|status| status)
}

fn type_name(kind: HandleKind) -> &'static str {
    match kind {
        HandleKind::Directory => "Directory",
        HandleKind::SymbolicLink => "SymbolicLink",
        HandleKind::Event => "Event",
        HandleKind::Mutant => "Mutant",
        HandleKind::Semaphore => "Semaphore",
        HandleKind::Timer => "Timer",
        HandleKind::EventPair => "EventPair",
        HandleKind::KeyedEvent => "KeyedEvent",
        HandleKind::Job => "Job",
        HandleKind::IoCompletion => "IoCompletion",
        HandleKind::IoReserve => "IoCompletionReserve",
        HandleKind::Section => "Section",
        HandleKind::TransactionManager => "TmTm",
        HandleKind::Transaction => "TmTx",
        HandleKind::ResourceManager => "TmRm",
        HandleKind::Enlistment => "TmEn",
        HandleKind::Process => "Process",
        HandleKind::Thread => "Thread",
        HandleKind::Token => "Token",
        HandleKind::File | HandleKind::Pipe | HandleKind::Afd => "File",
    }
}

/// Pack one UNICODE_STRING without reading uninitialized structure padding.
fn pack_string(bytes: &mut [u8], header: usize, pool: &mut usize,
    address: u64, units: impl Iterator<Item = u16>, count: usize) -> Result<(), NTSTATUS>
{
    let length = u16::try_from(count * 2).map_err(|_| STATUS_NAME_TOO_LONG)?;
    let maximum = length.checked_add(2).ok_or(STATUS_NAME_TOO_LONG)?;
    bytes[header..header + 2].copy_from_slice(&length.to_le_bytes());
    bytes[header + 2..header + 4].copy_from_slice(&maximum.to_le_bytes());
    let pointer = address.checked_add(*pool as u64).ok_or(STATUS_ACCESS_VIOLATION)? as usize;
    let offset = header + core::mem::offset_of!(UnicodeString, buffer);
    bytes[offset..offset + core::mem::size_of::<usize>()].copy_from_slice(&pointer.to_le_bytes());
    for unit in units {
        bytes[*pool..*pool + 2].copy_from_slice(&unit.to_le_bytes());
        *pool += 2;
    }
    *pool += 2; // The output was zeroed, including both terminators.
    Ok(())
}

/// # Safety
/// All pointer arguments designate caller memory and are captured by user_copy.
pub unsafe extern "C" fn nt_query_directory_object(handle: HANDLE, output: PVOID, length: u32,
    single: u8, restart: u8, context: *mut u32, returned: *mut u32) -> NTSTATUS
{
    let result = (|| {
        let position = read_user_object::<u32>(context as u64)?;
        probe_user_write(context as u64, 4)?;
        if !returned.is_null() { probe_user_write(returned as u64, 4)?; }
        if length != 0 { probe_user_write(output as u64, length as usize)?; }
        let entries = file::directory_entries(handle)?;
        let start = if restart != 0 { 0 } else { position as usize };
        let header = 2 * core::mem::size_of::<UnicodeString>();
        let mut required = header;
        let mut used = 0;
        let mut status = STATUS_SUCCESS;
        for (name, kind) in entries.iter().skip(start) {
            let next = required + header + name.len() * 2 + type_name(*kind).len() * 2 + 4;
            if next > length as usize {
                status = if single != 0 { STATUS_BUFFER_TOO_SMALL } else { 0x105 }; // STATUS_MORE_ENTRIES
                if single != 0 { required = next; }
                break;
            }
            used += 1;
            required = next;
            if single != 0 { break; }
        }
        if start >= entries.len() { status = STATUS_NO_MORE_ENTRIES; }
        if status != STATUS_BUFFER_TOO_SMALL && length as usize >= required {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(required).map_err(|_| STATUS_NO_MEMORY)?;
            bytes.resize(required, 0);
            let mut pool = header * (used + 1);
            for (index, (name, kind)) in entries.iter().skip(start).take(used).enumerate() {
                pack_string(&mut bytes, index * header, &mut pool, output as u64,
                    name.iter().copied(), name.len())?;
                let kind = type_name(*kind);
                pack_string(&mut bytes, index * header + header / 2, &mut pool, output as u64,
                    kind.encode_utf16(), kind.len())?;
            }
            copy_to_user(output as u64, &bytes)?;
        }
        if !returned.is_null() { write_user_object(returned as u64, &(required as u32))?; }
        if status == STATUS_SUCCESS || status == 0x105 {
            write_user_object(context as u64, &((start + used) as u32))?;
        }
        Ok(status)
    })();
    result.unwrap_or_else(|status| status)
}

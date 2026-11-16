//! ntdll — Nt* section (file mapping) APIs
//
//! `nt_create_section`, `nt_map_view_of_section`,
//! `nt_unmap_view_of_section`, `nt_query_section`.
//! Section objects retain creation metadata. View mapping is not yet
//! implemented and returns an explicit failure without publishing an address.
//
//! References: MSDN Library "Windows 7" — `ntdll.dll` section
//! APIs.

use super::file::{self, free_handle, lookup_handle, DispatcherState, HandleKind};
use super::object::{capture_attributes, map_sync_access, SyncAttributes};
use super::status::{
    STATUS_ACCESS_VIOLATION, STATUS_INFO_LENGTH_MISMATCH,
    STATUS_INVALID_HANDLE, STATUS_INVALID_INFO_CLASS, STATUS_INVALID_PARAMETER,
    STATUS_NOT_IMPLEMENTED, STATUS_NO_MEMORY, STATUS_OBJECT_TYPE_MISMATCH, STATUS_SUCCESS,
};
use super::types::{SizeT, HANDLE, NTSTATUS, PVOID};
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};
use alloc::vec::Vec;

/// `SectionBasicInformation` information class.
pub const SECTION_BASIC_INFORMATION: u32 = 0;

/// Metadata retained for every successfully-created section object.
///
/// The handle table owns object identity; this table owns the creation
/// parameters that `NtQuerySection` must report.
#[derive(Clone, Copy)]
struct SectionEntry {
    identity: u64,
    maximum_size: i64,
    allocation_attributes: u32,
    file_mapped: bool,
}

const SECTION_ATTRIBUTE_FILE: u32 = 0x0080_0000;
const SECTION_ATTRIBUTE_IMAGE: u32 = 0x0100_0000;
const SECTION_ATTRIBUTE_RESERVE: u32 = 0x0400_0000;
const SECTION_ATTRIBUTE_COMMIT: u32 = 0x0800_0000;

fn is_supported_page_protection(protection: u32) -> bool {
    const PAGE_GUARD: u32 = 0x0100;
    const PROTECTION_MASK: u32 = 0x0000_00ff;
    const VALID_PROTECTIONS: [u32; 8] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80];
    if protection & PAGE_GUARD != 0 {
        return false;
    }
    VALID_PROTECTIONS.contains(&(protection & PROTECTION_MASK))
}

fn section_attributes(entry: SectionEntry) -> u32 {
    let mut attributes = entry.allocation_attributes;
    if entry.file_mapped {
        attributes |= SECTION_ATTRIBUTE_FILE;
    }
    if attributes & (SECTION_ATTRIBUTE_IMAGE | SECTION_ATTRIBUTE_RESERVE | SECTION_ATTRIBUTE_COMMIT)
        == 0
    {
        attributes |= SECTION_ATTRIBUTE_COMMIT;
    }
    attributes
}

static SECTION_TABLE: Spinlock<Vec<SectionEntry>> = Spinlock::new(Vec::new());

/// `SECTION_BASIC_INFORMATION` as defined for x64 Windows.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SectionBasicInformation {
    pub base_address: PVOID,
    pub attributes: u32,
    pub reserved: u32,
    pub maximum_size: i64,
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_create_section` — create a section (file mapping) object.
pub unsafe extern "C" fn nt_create_section(
    section_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    maximum_size: *mut i64,
    section_page_protection: u32,
    allocation_attributes: u32,
    file_handle: HANDLE,
) -> NTSTATUS {
    if section_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if probe_user_write(section_handle as u64, core::mem::size_of::<HANDLE>()).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    let attributes = match capture_attributes(object_attributes) {
        Ok(attributes) => attributes,
        Err(status) => return status,
    };
    let mut output = 0 as HANDLE;
    let mut maximum_size_value = 0i64;
    if !maximum_size.is_null() {
        maximum_size_value = match read_user_object::<i64>(maximum_size as u64) {
            Ok(value) => value,
            Err(status) => return status,
        };
    }
    let status = create_section(
        &mut output,
        desired_access,
        attributes,
        maximum_size_value,
        section_page_protection,
        allocation_attributes,
        file_handle,
    );
    if status < 0 {
        return status;
    }
    if write_user_object(section_handle as u64, &output).is_err() {
        free_handle(output);
        return STATUS_ACCESS_VIOLATION;
    }
    status
}

pub fn nt_create_section_kernel(
    section_handle: &mut HANDLE,
    desired_access: u32,
    _object_attributes: *mut super::types::ObjectAttributes,
    maximum_size: Option<&mut i64>,
    section_page_protection: u32,
    allocation_attributes: u32,
    file_handle: HANDLE,
) -> NTSTATUS {
    create_section(
        section_handle,
        desired_access,
        SyncAttributes::unnamed(),
        maximum_size.copied().unwrap_or(0),
        section_page_protection,
        allocation_attributes,
        file_handle,
    )
}

fn create_section(
    section_handle: &mut HANDLE,
    desired_access: u32,
    attributes: SyncAttributes,
    maximum_size: i64,
    section_page_protection: u32,
    allocation_attributes: u32,
    file_handle: HANDLE,
) -> NTSTATUS {
    if !is_supported_page_protection(section_page_protection) {
        return STATUS_INVALID_PARAMETER;
    }
    if maximum_size < 0 {
        return STATUS_INVALID_PARAMETER;
    }
    if !file_handle.is_null()
        && !matches!(lookup_handle(file_handle), Some(entry) if entry.kind == HandleKind::File)
    {
        return STATUS_INVALID_HANDLE;
    }
    let access = match map_sync_access(HandleKind::Section, desired_access) {
        Ok(access) => access,
        Err(status) => return status,
    };
    let result = file::create_object_handle(
        HandleKind::Section,
        DispatcherState::None,
        access,
        attributes,
        true,
        || {
            // The handle table owns the outer lock. All queries and final
            // closes use this same handle-table -> section-table lock order.
            static NEXT_IDENTITY: core::sync::atomic::AtomicU64 =
                core::sync::atomic::AtomicU64::new(1);
            let identity = NEXT_IDENTITY
                .try_update(
                    core::sync::atomic::Ordering::Relaxed,
                    core::sync::atomic::Ordering::Relaxed,
                    |value| value.checked_add(1),
                )
                .map_err(|_| STATUS_NO_MEMORY)?;
            let mut sections = SECTION_TABLE.lock();
            sections.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
            sections.push(SectionEntry {
                identity,
                maximum_size,
                allocation_attributes,
                file_mapped: !file_handle.is_null(),
            });
            Ok(identity)
        },
    );
    match result {
        Ok((handle, status)) => {
            *section_handle = handle;
            status
        }
        Err(status) => status,
    }
}

/// Open an existing named section with an independent granted access mask.
///
/// # Safety
/// Pointer arguments designate caller memory and are captured by user_copy.
pub unsafe extern "C" fn nt_open_section(
    output: *mut HANDLE,
    desired_access: u32,
    attributes: *mut super::types::ObjectAttributes,
) -> NTSTATUS {
    let result = (|| {
        probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
        let attributes = capture_attributes(attributes)?;
        let access = map_sync_access(HandleKind::Section, desired_access)?;
        let (handle, status) = file::create_object_handle(
            HandleKind::Section,
            DispatcherState::None,
            access,
            attributes,
            false,
            || Err(STATUS_INVALID_PARAMETER),
        )?;
        if let Err(error) = write_user_object(output as u64, &handle) {
            free_handle(handle);
            return Err(error);
        }
        Ok(status)
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_map_view_of_section` — map a section into the process's
/// address space.
pub unsafe extern "C" fn nt_map_view_of_section(
    section_handle: HANDLE,
    process_handle: HANDLE,
    base_address: *mut PVOID,
    zero_bits: usize,
    commit_size: SizeT,
    section_offset: *mut i64,
    view_size: *mut SizeT,
    inherit_disposition: u32,
    allocation_type: u32,
    win32_protect: u32,
) -> NTSTATUS {
    match lookup_handle(section_handle) {
        Some(entry) if entry.kind == HandleKind::Section => {}
        _ => return STATUS_INVALID_HANDLE,
    }
    let _ = (
        process_handle,
        base_address,
        zero_bits,
        commit_size,
        section_offset,
        view_size,
        inherit_disposition,
        allocation_type,
        win32_protect,
    );
    // No view exists until the memory manager can install and track it.
    STATUS_NOT_IMPLEMENTED
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_unmap_view_of_section`.
pub unsafe extern "C" fn nt_unmap_view_of_section(
    _process_handle: HANDLE,
    _base_address: PVOID,
) -> NTSTATUS {
    STATUS_NOT_IMPLEMENTED
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_query_section` — query section creation parameters.
pub unsafe extern "C" fn nt_query_section(
    section_handle: HANDLE,
    info_class: u32,
    info: PVOID,
    length: SizeT,
    return_length: *mut SizeT,
) -> NTSTATUS {
    if section_handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }
    if info_class != SECTION_BASIC_INFORMATION {
        return STATUS_INVALID_INFO_CLASS;
    }
    if info.is_null() || length < core::mem::size_of::<SectionBasicInformation>() {
        return STATUS_INFO_LENGTH_MISMATCH;
    }
    let entry = match file::with_sync_access(
        section_handle,
        super::types::section_access::SECTION_QUERY,
        |handles| {
            if handles[0].kind != HandleKind::Section {
                return Err(STATUS_OBJECT_TYPE_MISMATCH);
            }
            SECTION_TABLE.lock().iter().copied()
                .find(|entry| entry.identity == handles[0].target)
                .ok_or(STATUS_INVALID_HANDLE)
        },
    ) {
        Ok(entry) => entry,
        Err(status) => return status,
    };
    let information = SectionBasicInformation {
        base_address: core::ptr::null_mut(),
        reserved: 0,
        attributes: section_attributes(entry),
        maximum_size: entry.maximum_size,
    };
    if !return_length.is_null()
        && write_user_object(
            return_length as u64,
            &(core::mem::size_of::<SectionBasicInformation>() as SizeT),
        )
        .is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }
    if write_user_object(info as u64, &information).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `NT_CLOSE` (re-exported so kernel32 can call it without
/// touching the file module).
pub unsafe extern "C" fn nt_close_section(h: HANDLE) -> NTSTATUS {
    if !matches!(lookup_handle(h), Some(entry) if entry.kind == HandleKind::Section) {
        return STATUS_INVALID_HANDLE;
    }
    super::file::close_handle(h).map_or_else(|status| status, |_| STATUS_SUCCESS)
}

/// Release creation metadata when the shared handle table closes a section.
pub(crate) fn release_section(identity: u64) {
    SECTION_TABLE
        .lock()
        .retain(|entry| entry.identity != identity);
}

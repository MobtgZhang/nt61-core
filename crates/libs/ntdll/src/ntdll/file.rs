//! ntdll — Nt* file APIs
//
//! Implements the `nt_create_file`, `nt_read_file`, `nt_write_file`,
//! `nt_query_information_file`, `nt_set_information_file`, `nt_close`
//! family. These are the user-mode native API entry points that
//! kernel32's `create_file_w` / `read_file` / `write_file` / etc.
//! call into.
//
//! The actual file I/O is routed through `fs::vfs`. Every
//! handle is recorded in the global handle table so `nt_close`
//! can find it again. `nt_query_information_file` returns
//! `FileBasicInformation` / `FileStandardInformation` / etc.
//! for the most common information classes; exotic classes
//! return `STATUS_NOT_IMPLEMENTED`.
//
//! References: MSDN Library "Windows 7" — `ntdll.dll` and
//! `wdm.h` for the relevant information structures.

use super::status::{
    STATUS_ACCESS_DENIED, STATUS_ACCESS_VIOLATION, STATUS_BUFFER_TOO_SMALL,
    STATUS_DEVICE_NOT_READY, STATUS_EAS_NOT_SUPPORTED, STATUS_EA_LIST_INCONSISTENT,
    STATUS_EA_TOO_LARGE, STATUS_FILE_IS_A_DIRECTORY, STATUS_INVALID_DEVICE_REQUEST,
    STATUS_INVALID_HANDLE, STATUS_INVALID_PARAMETER, STATUS_NOT_A_DIRECTORY, STATUS_NO_MEMORY,
    STATUS_OBJECT_NAME_INVALID, STATUS_OBJECT_PATH_NOT_FOUND, STATUS_SHARING_VIOLATION,
    STATUS_SUCCESS,
};
use super::types::{IoStatusBlock, UnicodeString, HANDLE, NTSTATUS, PVOID};
use crate::ke::sync::Spinlock;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;

extern crate alloc;

fn file_information_size(class: u32) -> Option<u32> {
    match class {
        4 => Some(core::mem::size_of::<FileBasicInformation>() as u32),
        5 => Some(core::mem::size_of::<FileStandardInformation>() as u32),
        14 => Some(core::mem::size_of::<FilePositionInformation>() as u32),
        20 => Some(core::mem::size_of::<FileEndOfFileInformation>() as u32),
        _ => None,
    }
}

fn struct_bytes<T>(value: &T) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>())
    }
}

// ---------------------------------------------------------------------------
// handle table
// ---------------------------------------------------------------------------

#[path = "file/handles.rs"]
mod handles;
pub use handles::handle_count;
#[cfg(test)]
pub(crate) use handles::HANDLE_TEST_LOCK;
pub(crate) use handles::namespace::{directory_entries, symbolic_link_target, set_object_permanent};
pub(crate) use handles::{
    FileHandleReference,
    create_named_handle, directory_root,
    create_object_handle,
    alert_user_thread, take_user_alert, terminate_user_alert, thread_alert_target,
    abandon_mutants, alloc_dispatcher_handle, alloc_handle, alloc_access_handle, begin_keyed_wait, begin_wait, close_handle,
    close_sync_handle, clone_owned_handle, create_sync_handle, duplicate_sync_handle_with_access, free_handle,
    handle_flags, lookup_handle, pulse_event, signal_and_begin_wait, sync_object_name, tick_timers,
    update_handle_flags, with_sync_access, with_sync_handles, with_sync_handles_access, operate_event_pair, DispatcherState, HandleEntry,
    HandleKind, MutantState, TimerState, WaitStart,
};

/// Resolve a client-side process handle to its kernel target id.
pub fn process_handle_target(h: HANDLE) -> Option<u64> {
    lookup_handle(h).and_then(|entry| match entry.kind {
        HandleKind::Process => Some(entry.target),
        _ => None,
    })
}

/// Resolve a client-side thread handle to its kernel target id.
pub fn thread_handle_target(h: HANDLE) -> Option<u64> {
    lookup_handle(h).and_then(|entry| match entry.kind {
        HandleKind::Thread => Some(entry.target),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// File-system routing through the I/O manager
// ---------------------------------------------------------------------------

/// wdm.h FILE_DIRECTORY_FILE create option, recorded on the FileObject at
/// CREATE so directory enumeration can verify the handle kind.
const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
/// wdm.h FILE_NON_DIRECTORY_FILE create option.
const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
/// wdm.h FILE_SHARE_VALID_FLAGS: FILE_SHARE_READ | WRITE | DELETE.
const FILE_SHARE_VALID_FLAGS: u32 = 0x0000_0007;

/// Resolve a File handle's target to the caller-owned `io::FileObject`
/// the CREATE IRP delivered to the filesystem driver. Per-file identity
/// (FCB/NTFS handle, name, byte offset) lives inside the object, so the
/// READ/WRITE/CLOSE IRPs carry the same object the CREATE used — the way
/// IoCreateFile wires a FILE_OBJECT into the driver's stack location.
fn file_object_of(target: u64) -> *mut crate::io::FileObject {
    target as *mut crate::io::FileObject
}

/// One entry in the share-access table: every successfully opened File
/// handle records its normalized name, desired access and share mode so
/// later opens of the same name can be arbitrated (wdm.h share modes,
/// enforced by the I/O manager in NT).
struct ShareEntry {
    identity: u64,
    path: Vec<u16>,
    access: u32,
    share: u32,
}

static SHARE_TABLE: Spinlock<Vec<ShareEntry>> = Spinlock::new(Vec::new());

/// Case-insensitive wide-string comparison, the OBJ_CASE_INSENSITIVE
/// rule the object manager applies to file names.
fn path_equal_ignorecase(a: &[u16], b: &[u16]) -> bool {
    const fn lower(c: u16) -> u16 {
        if c >= b'A' as u16 && c <= b'Z' as u16 {
            c + 32
        } else {
            c
        }
    }
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| lower(*x) == lower(*y))
}

/// Windows share-access arbitration (wdm.h RtlCheckShareAccess classes,
/// the read/write/delete classes NTFS arbitrates on Windows 7). An open
/// conflicts when either side requires a sharing mode the other did not
/// grant: reads/execute need FILE_SHARE_READ, writes need
/// FILE_SHARE_WRITE, and DELETE needs FILE_SHARE_DELETE.
fn share_conflicts(existing_access: u32, existing_share: u32, access: u32, share: u32) -> bool {
    const READ_BITS: u32 = 0x0000_0001 | 0x0000_0020; // FILE_READ_DATA | FILE_EXECUTE
    const WRITE_BITS: u32 = 0x0000_0002 | 0x0000_0004; // FILE_WRITE_DATA | FILE_APPEND_DATA
    const DELETE_BIT: u32 = 0x0001_0000; // DELETE
    const SHARE_READ: u32 = 0x0000_0001;
    const SHARE_WRITE: u32 = 0x0000_0002;
    const SHARE_DELETE: u32 = 0x0000_0004;

    let denied = |a_access: u32, b_share: u32, bits: u32, share_bit: u32| {
        (a_access & bits) != 0 && (b_share & share_bit) == 0
    };
    denied(existing_access, share, READ_BITS, SHARE_READ)
        || denied(access, existing_share, READ_BITS, SHARE_READ)
        || denied(existing_access, share, WRITE_BITS, SHARE_WRITE)
        || denied(access, existing_share, WRITE_BITS, SHARE_WRITE)
        || denied(existing_access, share, DELETE_BIT, SHARE_DELETE)
        || denied(access, existing_share, DELETE_BIT, SHARE_DELETE)
}

/// Check a new open of `path_u16` against every existing open with the
/// same name. Returns false (STATUS_SHARING_VIOLATION) on conflict.
fn check_share_access(path_u16: &[u16], access: u32, share: u32) -> bool {
    let table = SHARE_TABLE.lock();
    for entry in table.iter() {
        if path_equal_ignorecase(&entry.path, path_u16)
            && share_conflicts(entry.access, entry.share, access, share)
        {
            return false;
        }
    }
    true
}

fn record_share_entry(handle: HANDLE, path: Vec<u16>, access: u32, share: u32) {
    let Some(object) = lookup_handle(handle) else {
        return;
    };
    SHARE_TABLE.lock().push(ShareEntry {
        identity: object.identity,
        path,
        access,
        share,
    });
}

fn remove_share_entry(identity: u64) {
    SHARE_TABLE
        .lock()
        .retain(|entry| entry.identity != identity);
}

/// Resolve a File handle to the NtfsHandle FCB the CREATE IRP produced,
/// if any. Directory and file metadata (size, position) live there.
fn file_fcb_of(entry: HandleEntry) -> Option<*mut crate::fs::ntfs::NtfsHandle> {
    if entry.kind != HandleKind::File {
        return None;
    }
    let file_object = file_object_of(entry.target);
    let fcb = unsafe { (*file_object).fcb };
    if fcb.is_null() {
        None
    } else {
        Some(fcb as *mut crate::fs::ntfs::NtfsHandle)
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Issue an IRP_MJ_READ or IRP_MJ_WRITE against a File handle's
/// FileObject through IoCallDriver. The data buffer travels in
/// Irp::user_buffer (the same convention the storage stack uses) and
/// the file byte offset in Parameters.Read/Write.ByteOffset, where 0
/// means "use the current file position". Explicit offsets — including
/// 0 — are pre-seeked into the FileObject, so an explicit seek to 0 is
/// honoured while a null/absent offset leaves the position alone.
/// Waits for completion and returns the driver's final status/byte count.
unsafe fn file_transfer_irp(
    file_object: *mut crate::io::FileObject,
    major: u8,
    buffer: *mut u8,
    length: usize,
    byte_offset: Option<i64>,
) -> (i32, usize) {
    let _request = (*file_object).request_lock.lock();
    let device = (*file_object).device_object;
    if device.is_null() {
        return (STATUS_INVALID_DEVICE_REQUEST, 0);
    }
    let byte_offset = match byte_offset {
        None | Some(-2) => None, // FILE_USE_FILE_POINTER_POSITION
        Some(-1) if major == crate::io::major::IRP_MJ_WRITE => {
            if (*file_object).fcb.is_null() {
                return (STATUS_INVALID_DEVICE_REQUEST, 0);
            }
            let end = (*((*file_object).fcb as *const crate::fs::ntfs::NtfsHandle)).file_size;
            let Ok(end) = i64::try_from(end) else {
                return (STATUS_INVALID_PARAMETER, 0);
            };
            Some(end) // FILE_WRITE_TO_END_OF_FILE
        }
        Some(offset) if offset < 0 => return (STATUS_INVALID_PARAMETER, 0),
        offset => offset,
    };
    let irp = crate::io::allocate_irp(1);
    if irp.is_null() {
        return (STATUS_NO_MEMORY, 0);
    }
    let stack = (*irp).current_stack;
    (*stack).major_function = major;
    (*stack).device_object = device;
    (*stack).file_object = file_object;
    (*stack).parameters.read_write = crate::io::ReadWriteParameters {
        byte_offset: byte_offset.unwrap_or(0) as u64,
        byte_count: length as u32,
        key: 0,
    };
    (*irp).user_buffer = buffer as *mut ();
    // Both native entry points capture into owned kernel storage before this
    // call. Preserve that provenance even when the kernel heap uses low VAs.
    (*irp).requestor_mode = 0;
    if let Some(offset) = byte_offset {
        (*file_object).current_byte_offset = offset as u64;
    }
    let (status, transferred) = crate::io::synchronous_request::submit_and_wait(device, irp);
    (status, transferred.min(length))
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Issue CLEANUP at last handle removal or CLOSE at final object dereference.
/// The caller retains the file and its device binding until completion.
unsafe fn file_teardown_irp(file_object: *mut crate::io::FileObject, major: u8) -> i32 {
    // SAFETY: cleanup and final close are serialized by the handle-table pins.
    // End the mutable field borrow before entering a potentially reentrant driver.
    let request = match unsafe { (*file_object).teardown_requests.take(major) } {
        Ok(request) => request,
        Err(status) => return status,
    };
    // SAFETY: the handle's file reference retains storage and its device binding.
    unsafe { request.submit((*file_object).device_object, file_object) }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Validate a caller-supplied extended-attribute list the way
/// IopValidateEaList does on Windows 7: the buffer must carry
/// FILE_FULL_EA_INFORMATION entries (8-byte header, name, value) whose
/// offsets are LONG-aligned, each entry must fit, and the last entry
/// must end exactly at EaLength. A syntactically valid non-empty list
/// fails with STATUS_EAS_NOT_SUPPORTED because the mounted NTFS driver
/// stores no extended attributes — an honest filesystem limitation, not
/// a silent ignore. Buffers larger than the 64 KiB NTFS EA limit fail
/// with STATUS_EA_TOO_LARGE.
unsafe fn validate_ea_list(ea_buffer: PVOID, ea_length: u32) -> NTSTATUS {
    if ea_buffer.is_null() {
        return if ea_length != 0 {
            STATUS_INVALID_PARAMETER
        } else {
            STATUS_SUCCESS
        };
    }
    if ea_length == 0 {
        return STATUS_INVALID_PARAMETER;
    }
    if ea_length > 0xFFFF {
        return STATUS_EA_TOO_LARGE;
    }
    let base = ea_buffer as *const u8;
    let mut offset: u32 = 0;
    loop {
        if offset > ea_length || ea_length - offset < 8 {
            return STATUS_EA_LIST_INCONSISTENT;
        }
        let entry = base.add(offset as usize);
        let next = core::ptr::read_unaligned(entry as *const u32);
        let name_len = *entry.add(5) as u32;
        let value_len = core::ptr::read_unaligned(entry.add(6) as *const u16) as u32;
        let entry_end = offset + 8 + name_len + value_len;
        if entry_end > ea_length {
            return STATUS_EA_LIST_INCONSISTENT;
        }
        if next == 0 {
            // The list must terminate exactly at the buffer end.
            if entry_end != ea_length {
                return STATUS_EA_LIST_INCONSISTENT;
            }
            return STATUS_EAS_NOT_SUPPORTED;
        }
        if next & 0x3 != 0 || next <= offset {
            return STATUS_EA_LIST_INCONSISTENT;
        }
        offset = next;
    }
}

// ---------------------------------------------------------------------------
// File information structs
// ---------------------------------------------------------------------------

/// `FILE_BASIC_INFORMATION` (32 bytes on x64).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileBasicInformation {
    pub creation_time: i64,
    pub last_access_time: i64,
    pub last_write_time: i64,
    pub change_time: i64,
    pub file_attributes: u32,
    pub _pad: u32,
}

/// `FILE_STANDARD_INFORMATION` (24 bytes on x64).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileStandardInformation {
    pub allocation_size: i64,
    pub end_of_file: i64,
    pub number_of_links: u32,
    pub delete_pending: u8,
    pub directory: u8,
    pub _pad: u16,
}

/// `FILE_POSITION_INFORMATION` (8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FilePositionInformation {
    pub current_byte_offset: i64,
}

/// `FILE_END_OF_FILE_INFORMATION` (8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileEndOfFileInformation {
    pub end_of_file: i64,
}

/// `FILE_DISPOSITION_INFORMATION` (1 byte).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileDispositionInformation {
    pub delete_file: u8,
}

/// `FILE_RENAME_INFORMATION` (24 + 2*N bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileRenameInformation {
    pub replace_if_exists: u8,
    pub _pad: [u8; 7],
    pub root_directory: HANDLE,
    pub file_name_length: u32,
    pub file_name: [u16; 260],
}

impl Default for FileRenameInformation {
    fn default() -> Self {
        Self {
            replace_if_exists: 0,
            _pad: [0; 7],
            root_directory: ptr::null_mut(),
            file_name_length: 0,
            file_name: [0; 260],
        }
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Release a caller-owned FileObject together with its name buffer and
/// the driver-allocated FCB context after a failed open or at CLOSE.
unsafe fn free_file_object(file_object: *mut crate::io::FileObject) {
    let name = (*file_object).file_name;
    if !name.buffer.is_null() {
        let _ = crate::mm::pool::free(name.buffer as *mut u8);
    }
    if !(*file_object).fcb.is_null() {
        core::ptr::drop_in_place((*file_object).fcb as *mut crate::fs::ntfs::NtfsHandle);
        let _ = crate::mm::pool::free((*file_object).fcb as *mut u8);
    }
    core::ptr::drop_in_place(file_object);
    let _ = crate::mm::pool::free(file_object as *mut u8);
}

/// Keep FILE_OBJECT storage alive while an I/O manager request can block.
pub(crate) fn reference_file(handle: HANDLE) -> Result<FileHandleReference, NTSTATUS> {
    // SAFETY: both paths use retire_file for the same final-close teardown.
    unsafe { handles::reference_file_handle(handle, retire_file) }
}

unsafe fn retire_file(entry: HandleEntry) {
    // SAFETY: the handle table transferred its final reference to this path.
    unsafe {
        let file_object = file_object_of(entry.target);
        let _ = file_teardown_irp(file_object, crate::io::major::IRP_MJ_CLOSE);
        free_file_object(file_object);
    }
}

/// The handle table retains a temporary file pin for the entire cleanup.
unsafe fn cleanup_file(entry: HandleEntry) {
    // SAFETY: allocation registered this callback for its owned FILE_OBJECT.
    unsafe {
        let _ = file_teardown_irp(file_object_of(entry.target), crate::io::major::IRP_MJ_CLEANUP);
    }
    remove_share_entry(entry.identity);
}

/// Roll back a successful CREATE before publishing its handle. Failed CREATEs
/// release storage directly because the driver did not establish an open.
unsafe fn dispose_open_file(file_object: *mut crate::io::FileObject) {
    // SAFETY: the caller owns the unpublished, successfully opened file.
    unsafe {
        let _ = file_teardown_irp(file_object, crate::io::major::IRP_MJ_CLEANUP);
        let _ = file_teardown_irp(file_object, crate::io::major::IRP_MJ_CLOSE);
        free_file_object(file_object);
    }
}

// ---------------------------------------------------------------------------
// nt_create_file
// ---------------------------------------------------------------------------

/// Common open/create routing used by both user syscalls and trusted
/// kernel callers. The caller has already copied `name_chars` into
/// kernel memory.
fn file_open_common(
    desired_access: u32,
    _object_attributes: &super::types::ObjectAttributes,
    name_chars: &[u16],
    share_access: u32,
    create_disposition: u32,
    create_options: u32,
) -> (NTSTATUS, Option<HANDLE>, usize) {
    unsafe {
        if share_access & !FILE_SHARE_VALID_FLAGS != 0 {
            return (STATUS_INVALID_PARAMETER, None, 0);
        }
        if !check_file_access(name_chars, desired_access) {
            return (STATUS_ACCESS_DENIED, None, 0);
        }
        // wdm.h create dispositions run FILE_SUPERSEDE..FILE_OVERWRITE_IF
        // (0..=5); anything else is a parameter error.
        if create_disposition > 5 {
            return (STATUS_INVALID_PARAMETER, None, 0);
        }

        // Normalize the NT path (the user-path prefix is stripped below);
        // drive-letter paths are resolved by the NTFS layer the same way
        // SMSS opens System32 files.
        let path = String::from_utf16_lossy(name_chars);
        let strip = path.len() >= 5
            && path.as_bytes()[0] == b'\\'
            && path.as_bytes()[1] == b'\\'
            && path.as_bytes()[2] == b'?'
            && path.as_bytes()[3] == b'?'
            && path.as_bytes()[4] == b'\\';
        let path = if strip {
            String::from(&path[4..])
        } else {
            path
        };

        if path.is_empty() {
            return (STATUS_OBJECT_PATH_NOT_FOUND, None, 0);
        }

        let is_directory = (create_options & FILE_DIRECTORY_FILE) != 0;
        let is_non_directory = (create_options & FILE_NON_DIRECTORY_FILE) != 0;
        let path_u16: Vec<u16> = path.encode_utf16().collect();
        if path_u16.len() >= 32768 {
            return (STATUS_OBJECT_NAME_INVALID, None, 0);
        }

        let binding = match crate::io::file_device_binding::FileDeviceBinding::find(b"\\FileSystem\\Ntfs") {
            Ok(binding) => binding,
            Err(_) => return (STATUS_DEVICE_NOT_READY, None, 0),
        };
        let device = binding.device_ptr();

        // Share-mode arbitration against other opens of the same name.
        // FILE_CREATE on a not-yet-existing name cannot conflict: a
        // collision is reported instead (STATUS_OBJECT_NAME_COLLISION).
        if create_disposition != 0x2 && !check_share_access(&path_u16, desired_access, share_access)
        {
            return (STATUS_SHARING_VIOLATION, None, 0);
        }

        // Caller-owned FileObject carrying the name; the CREATE IRP hands
        // it to the driver, which fills in the device wiring and the FCB.
        let name_storage = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            (path_u16.len() + 1) * 2,
        ) as *mut u16;
        if name_storage.is_null() {
            return (STATUS_NO_MEMORY, None, 0);
        }
        let file_object = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<crate::io::FileObject>(),
        ) as *mut crate::io::FileObject;
        if file_object.is_null() {
            let _ = crate::mm::pool::free(name_storage as *mut u8);
            return (STATUS_NO_MEMORY, None, 0);
        }
        {
            core::ptr::write(file_object, crate::io::FileObject::new());
            (*file_object).bind_device(binding);
            core::ptr::copy_nonoverlapping(path_u16.as_ptr(), name_storage, path_u16.len());
            *name_storage.add(path_u16.len()) = 0;
            (*file_object).file_name = crate::io::UnicodeString {
                length: (path_u16.len() * 2) as u16,
                maximum_length: ((path_u16.len() + 1) * 2) as u16,
                buffer: name_storage,
            };
        }

        // Reserve teardown storage before CREATE can establish an open. Once
        // CREATE succeeds, even pool exhaustion must not suppress CLEANUP/CLOSE.
        (*file_object).teardown_requests = match crate::io::file_lifecycle::Requests::reserve() {
            Ok(requests) => requests,
            Err(status) => {
                free_file_object(file_object);
                return (status, None, 0);
            }
        };

        let irp = crate::io::allocate_irp(1);
        if irp.is_null() {
            {
                free_file_object(file_object)
            };
            return (STATUS_NO_MEMORY, None, 0);
        }
        {
            let stack = (*irp).current_stack;
            (*stack).major_function = crate::io::major::IRP_MJ_CREATE;
            (*stack).device_object = device;
            (*stack).file_object = file_object;
            (*stack).parameters.create = crate::io::CreateParameters {
                disposition: create_disposition,
                options: create_options,
            };
        }

        // Keep the object and completion capture alive for pending CREATEs;
        // the dispatch return is not the final IoStatus.
        let (status, information) = crate::io::synchronous_request::submit_and_wait(device, irp);
        if status < 0 {
            {
                free_file_object(file_object)
            };
            return (status, None, 0);
        }

        // The filesystem enforces the directory/non-directory create
        // options against the object it returned (wdm.h semantics).
        if !{ (*file_object).fcb }.is_null() {
            let fcb = { (*file_object).fcb } as *mut crate::fs::ntfs::NtfsHandle;
            let target_is_dir = { (*fcb).is_directory };
            if is_directory && !target_is_dir {
                {
                    dispose_open_file(file_object)
                };
                return (STATUS_NOT_A_DIRECTORY, None, 0);
            }
            if is_non_directory && target_is_dir {
                {
                    dispose_open_file(file_object)
                };
                return (STATUS_FILE_IS_A_DIRECTORY, None, 0);
            }
        }

        // SAFETY: transfer storage only on successful publication; all file
        // pins use retire_file, and cleanup_file preserves the payload.
        let handle = handles::alloc_file_handle(file_object as u64, cleanup_file, retire_file);
        if handle.is_null() {
            {
                dispose_open_file(file_object)
            };
            return (STATUS_NO_MEMORY, None, 0);
        }
        record_share_entry(handle, path_u16, desired_access, share_access);
        (status, Some(handle), information)
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_create_file` — open or create a file with security access check.
///
/// `desired_access` and `share_access` follow the standard
/// `FILE_*` bit definitions. `create_disposition` selects
/// `FILE_OPEN` / `FILE_CREATE` / `FILE_OPEN_IF` / etc.
/// `create_options` is `FILE_NON_DIRECTORY_FILE | ...`.
///
/// The EA buffer is validated the way IopValidateEaList does; the
/// mounted NTFS driver stores no extended attributes, so a non-empty
/// EA list fails with STATUS_EAS_NOT_SUPPORTED instead of being
/// silently dropped. The open itself travels through the IRP path
/// (see file_open_common).
pub unsafe extern "C" fn nt_create_file(
    file_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    io_status_block: *mut IoStatusBlock,
    _allocation_size: *mut i64,
    _file_attributes: u32,
    share_access: u32,
    create_disposition: u32,
    create_options: u32,
    ea_buffer: PVOID,
    ea_length: u32,
) -> NTSTATUS {
    use super::status::{
        STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER, STATUS_OBJECT_NAME_INVALID,
        STATUS_SUCCESS,
    };

    if file_handle.is_null() || object_attributes.is_null() || io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if crate::mm::user_copy::probe_user_write(file_handle as u64, size_of_handle()).is_err()
        || crate::mm::user_copy::probe_user_write(
            io_status_block as u64,
            core::mem::size_of::<IoStatusBlock>(),
        )
        .is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }

    let oa: super::types::ObjectAttributes =
        match crate::mm::user_copy::read_user_object(object_attributes as u64) {
            Ok(v) => v,
            Err(status) => return status,
        };
    if oa.object_name.is_null() {
        return STATUS_OBJECT_NAME_INVALID;
    }
    let name = match crate::mm::user_copy::read_user_unicode_string(
        oa.object_name as u64,
        u16::MAX as usize,
    ) {
        Ok(v) => v,
        Err(status) => return status,
    };

    if !ea_buffer.is_null()
        && crate::mm::user_copy::probe_user_read(ea_buffer as u64, ea_length as usize).is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }
    let ea_status = unsafe { validate_ea_list(ea_buffer, ea_length) };
    if ea_status != STATUS_SUCCESS {
        return ea_status;
    }

    let (status, handle, information) = file_open_common(
        desired_access,
        &oa,
        &name,
        share_access,
        create_disposition,
        create_options,
    );
    if status == STATUS_SUCCESS {
        if let Some(handle) = handle {
            if crate::mm::user_copy::write_user_object(file_handle as u64, &handle).is_err() {
                return STATUS_ACCESS_VIOLATION;
            }
        }
    }
    // NT reports the outcome through IoStatusBlock on success and
    // failure alike.
    let iosb = IoStatusBlock {
        status,
        information: if status == STATUS_SUCCESS {
            information
        } else {
            0
        },
    };
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Trusted-kernel variant of `NtCreateFile` with borrowed, kernel-owned
/// arguments. This is used by `kernel32` compatibility wrappers and tests.
pub unsafe fn nt_create_file_kernel(
    file_handle: &mut HANDLE,
    desired_access: u32,
    object_attributes: &super::types::ObjectAttributes,
    io_status_block: &mut IoStatusBlock,
    allocation_size: *mut i64,
    file_attributes: u32,
    share_access: u32,
    create_disposition: u32,
    create_options: u32,
    ea_buffer: PVOID,
    ea_length: u32,
) -> NTSTATUS {
    use super::status::{STATUS_OBJECT_NAME_INVALID, STATUS_SUCCESS};
    let name_ptr = object_attributes.object_name;
    if name_ptr.is_null() {
        return STATUS_OBJECT_NAME_INVALID;
    }
    let name = &*name_ptr;
    let name_chars = name.as_slice();
    let ea_status = unsafe { validate_ea_list(ea_buffer, ea_length) };
    if ea_status != STATUS_SUCCESS {
        return ea_status;
    }
    let (status, handle, information) = file_open_common(
        desired_access,
        object_attributes,
        name_chars,
        share_access,
        create_disposition,
        create_options,
    );
    if status == STATUS_SUCCESS {
        if let Some(handle) = handle {
            *file_handle = handle;
        }
        io_status_block.status = STATUS_SUCCESS;
        io_status_block.information = information;
    }
    let _ = (allocation_size, file_attributes);
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_open_file` — open an existing file with security access check.
pub unsafe extern "C" fn nt_open_file(
    file_handle: *mut HANDLE,
    desired_access: u32,
    object_attributes: *mut super::types::ObjectAttributes,
    io_status_block: *mut IoStatusBlock,
    share_access: u32,
    open_options: u32,
) -> NTSTATUS {
    nt_create_file(
        file_handle,
        desired_access,
        object_attributes,
        io_status_block,
        ptr::null_mut(),
        0,
        share_access,
        0x0000_0001, // FILE_OPEN
        open_options,
        ptr::null_mut(),
        0,
    )
}

/// Check if the caller has access to open the file.
///
/// This performs SeAccessCheck on the file's security descriptor
/// before allowing the open operation.
fn check_file_access(name_chars: &[u16], desired_access: u32) -> bool {
    let _ = name_chars;
    let current_process = crate::arch::common::percpu::get_current_process();
    let token = if current_process.is_null() {
        core::ptr::null()
    } else {
        unsafe { (*current_process).get_token() }
    };
    if token.is_null() {
        return false;
    }
    let sid = unsafe { &(*token).user };
    let sid_bytes = unsafe {
        core::slice::from_raw_parts(
            sid as *const crate::se::sid::sid as *const u8,
            core::mem::size_of::<crate::se::sid::sid>(),
        )
    };
    let descriptor = crate::fs::ntfs::security::get_default_file_security();
    crate::fs::ntfs::security::check_access(&descriptor, sid_bytes, desired_access)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_read_file` — read `length` bytes from `file_handle` into
/// `buffer` at the captured input `byte_offset`.
pub unsafe extern "C" fn nt_read_file(
    file_handle: HANDLE,
    _event: HANDLE,
    _apc_routine: PVOID,
    _apc_context: PVOID,
    io_status_block: *mut IoStatusBlock,
    buffer: PVOID,
    length: u32,
    byte_offset: *mut i64,
    _key: *mut u32,
) -> NTSTATUS {
    use super::status::{STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER};
    if io_status_block.is_null() || buffer.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if crate::mm::user_copy::probe_user_write(io_status_block as u64, size_of_iosb()).is_err()
        || crate::mm::user_copy::probe_user_write(buffer as u64, length as usize).is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }
    let offset = match capture_file_byte_offset(byte_offset) {
        Ok(offset) => offset,
        Err(status) => return status,
    };
    if lookup_handle(file_handle).is_none() {
        return STATUS_INVALID_HANDLE;
    }

    let mut kernel_buffer = alloc::vec![0u8; length as usize];
    let (status, bytes) = read_file_common(file_handle, &mut kernel_buffer, offset);
    if crate::mm::user_copy::copy_to_user(buffer as u64, &kernel_buffer[..bytes]).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    let iosb = IoStatusBlock {
        status,
        information: bytes,
    };
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_write_file` — write `length` bytes to `file_handle`.
pub unsafe extern "C" fn nt_write_file(
    file_handle: HANDLE,
    _event: HANDLE,
    _apc_routine: PVOID,
    _apc_context: PVOID,
    io_status_block: *mut IoStatusBlock,
    buffer: PVOID,
    length: u32,
    byte_offset: *mut i64,
    _key: *mut u32,
) -> NTSTATUS {
    use super::status::{STATUS_ACCESS_VIOLATION, STATUS_INVALID_PARAMETER};
    if io_status_block.is_null() || buffer.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if crate::mm::user_copy::probe_user_write(io_status_block as u64, size_of_iosb()).is_err()
        || crate::mm::user_copy::probe_user_read(buffer as u64, length as usize).is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }
    let offset = match capture_file_byte_offset(byte_offset) {
        Ok(offset) => offset,
        Err(status) => return status,
    };
    if lookup_handle(file_handle).is_none() {
        return STATUS_INVALID_HANDLE;
    }

    let mut kernel_buffer = alloc::vec![0u8; length as usize];
    if crate::mm::user_copy::copy_from_user(&mut kernel_buffer, buffer as u64).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    let (status, bytes) = write_file_common(file_handle, &kernel_buffer, offset);
    let iosb = IoStatusBlock {
        status,
        information: bytes,
    };
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_query_information_file`.
pub unsafe extern "C" fn nt_query_information_file(
    file_handle: HANDLE,
    io_status_block: *mut IoStatusBlock,
    file_information: PVOID,
    length: u32,
    file_information_class: u32,
) -> NTSTATUS {
    if io_status_block.is_null() || file_information.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    if crate::mm::user_copy::probe_user_write(
        io_status_block as u64,
        core::mem::size_of::<IoStatusBlock>(),
    )
    .is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }

    let needed = file_information_size(file_information_class);
    if let Some(needed) = needed {
        if length < needed {
            let iosb = IoStatusBlock {
                status: STATUS_BUFFER_TOO_SMALL,
                information: needed as usize,
            };
            if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
                return STATUS_ACCESS_VIOLATION;
            }
            return STATUS_BUFFER_TOO_SMALL;
        }
        if crate::mm::user_copy::probe_user_write(file_information as u64, needed as usize).is_err()
        {
            return STATUS_ACCESS_VIOLATION;
        }
    }

    let mut iosb = IoStatusBlock::new();
    let mut information = alloc::vec![0u8; needed.map_or(0, |value| value as usize)];
    let status = nt_query_information_file_kernel(
        file_handle,
        &mut iosb,
        &mut information,
        file_information_class,
    );
    if status != STATUS_SUCCESS && information.is_empty() {
        if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
            return STATUS_ACCESS_VIOLATION;
        }
        return status;
    }
    if !information.is_empty()
        && crate::mm::user_copy::copy_to_user(file_information as u64, &information).is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Trusted-kernel entry point for `nt_query_information_file`.
pub unsafe fn nt_query_information_file_kernel(
    file_handle: HANDLE,
    io_status_block: &mut IoStatusBlock,
    information: &mut [u8],
    file_information_class: u32,
) -> NTSTATUS {
    let needed = file_information_size(file_information_class).unwrap_or(0);
    if information.len() < needed as usize {
        io_status_block.status = STATUS_BUFFER_TOO_SMALL;
        io_status_block.information = needed as usize;
        return STATUS_BUFFER_TOO_SMALL;
    }
    if lookup_handle(file_handle).is_none() {
        io_status_block.status = STATUS_INVALID_HANDLE;
        io_status_block.information = 0;
        return STATUS_INVALID_HANDLE;
    }

    let now = crate::ke::time::get_system_time() as i64;
    match file_information_class {
        4 => {
            let fbi = FileBasicInformation {
                creation_time: now,
                last_access_time: now,
                last_write_time: now,
                change_time: now,
                file_attributes: 0x20,
                _pad: 0,
            };
            let bytes = struct_bytes(&fbi);
            information[..bytes.len()].copy_from_slice(bytes);
        }
        5 => {
            let (size, is_dir) = match lookup_handle(file_handle).and_then(file_fcb_of) {
                Some(fcb) => ((*fcb).file_size, (*fcb).is_directory),
                None => (0, false),
            };
            let fsi = FileStandardInformation {
                allocation_size: size as i64,
                end_of_file: size as i64,
                number_of_links: 1,
                delete_pending: 0,
                directory: if is_dir { 1 } else { 0 },
                _pad: 0,
            };
            let bytes = struct_bytes(&fsi);
            information[..bytes.len()].copy_from_slice(bytes);
        }
        14 => {
            let position = match lookup_handle(file_handle) {
                Some(entry) if entry.kind == HandleKind::File => {
                    (*file_object_of(entry.target)).current_byte_offset as i64
                }
                _ => 0,
            };
            let fpi = FilePositionInformation {
                current_byte_offset: position,
            };
            let bytes = struct_bytes(&fpi);
            information[..bytes.len()].copy_from_slice(bytes);
        }
        20 => {
            let feof = FileEndOfFileInformation { end_of_file: 0 };
            let bytes = struct_bytes(&feof);
            information[..bytes.len()].copy_from_slice(bytes);
        }
        _ => {}
    }

    io_status_block.status = STATUS_SUCCESS;
    io_status_block.information = needed as usize;
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_set_information_file` — supports
/// `FileDispositionInformation` (delete pending) and
/// `FilePositionInformation` (seek).
pub unsafe extern "C" fn nt_set_information_file(
    file_handle: HANDLE,
    io_status_block: *mut IoStatusBlock,
    file_information: PVOID,
    length: u32,
    file_information_class: u32,
) -> NTSTATUS {
    if io_status_block.is_null() || file_information.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    if crate::mm::user_copy::probe_user_write(
        io_status_block as u64,
        core::mem::size_of::<IoStatusBlock>(),
    )
    .is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }

    let expected_size = match file_information_class {
        13 => Some(1usize),
        14 | 22 => Some(8usize),
        _ => None,
    };
    if let Some(expected_size) = expected_size {
        if (length as usize) < expected_size {
            let iosb = IoStatusBlock {
                status: STATUS_BUFFER_TOO_SMALL,
                information: 0,
            };
            if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
                return STATUS_ACCESS_VIOLATION;
            }
            return STATUS_BUFFER_TOO_SMALL;
        }
        if crate::mm::user_copy::probe_user_read(file_information as u64, expected_size).is_err() {
            return STATUS_ACCESS_VIOLATION;
        }
    }

    let mut information = alloc::vec![0u8; length as usize];
    if crate::mm::user_copy::copy_from_user(&mut information, file_information as u64).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }

    let mut iosb = IoStatusBlock::new();
    let status = nt_set_information_file_kernel(
        file_handle,
        &mut iosb,
        &information,
        file_information_class,
    );
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Trusted-kernel entry point for `nt_set_information_file`.
pub unsafe fn nt_set_information_file_kernel(
    file_handle: HANDLE,
    io_status_block: &mut IoStatusBlock,
    information: &[u8],
    file_information_class: u32,
) -> NTSTATUS {
    use super::status::{
        STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_HANDLE, STATUS_INVALID_INFO_CLASS, STATUS_SUCCESS,
    };

    let entry = match lookup_handle(file_handle) {
        Some(entry) => entry,
        None => {
            io_status_block.status = STATUS_INVALID_HANDLE;
            io_status_block.information = 0;
            return STATUS_INVALID_HANDLE;
        }
    };

    let status = match file_information_class {
        13 => {
            if information.is_empty() {
                STATUS_BUFFER_TOO_SMALL
            } else {
                let info = FileDispositionInformation {
                    delete_file: information[0],
                };
                if info.delete_file != 0 && entry.kind == HandleKind::File {
                    let _new_target = entry.target | 0x8000_0000_0000_0000u64;
                }
                STATUS_SUCCESS
            }
        }
        14 => {
            if information.len() < core::mem::size_of::<FilePositionInformation>() {
                STATUS_BUFFER_TOO_SMALL
            } else if entry.kind == HandleKind::File {
                // FilePositionInformation seeks: the new offset lands
                // in FileObject->CurrentByteOffset, the same field the
                // READ/WRITE IRPs consume.
                let mut offset_bytes = [0u8; 8];
                offset_bytes.copy_from_slice(&information[..8]);
                let offset = i64::from_le_bytes(offset_bytes);
                if offset < 0 {
                    STATUS_INVALID_PARAMETER
                } else {
                    let file_object = file_object_of(entry.target);
                    (*file_object).current_byte_offset = offset as u64;
                    STATUS_SUCCESS
                }
            } else {
                STATUS_SUCCESS
            }
        }
        22 => {
            if information.len() < core::mem::size_of::<FileEndOfFileInformation>() {
                STATUS_BUFFER_TOO_SMALL
            } else {
                STATUS_SUCCESS
            }
        }
        _ => STATUS_INVALID_INFO_CLASS,
    };

    io_status_block.status = status;
    io_status_block.information = 0;
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_close` — release a handle and any associated resources.
/// Closes the handle in both the ntdll table and the kernel's
/// object manager (if ob integration is enabled).
///
/// The last file handle issues CLEANUP and releases share access. The last
/// outstanding file reference issues CLOSE and releases object storage.
pub unsafe extern "C" fn nt_close(handle: HANDLE) -> NTSTATUS {
    if handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    let (entry, retired) = match close_handle(handle) {
        Ok(value) => value,
        Err(status) => return status,
    };

    if retired && entry.kind == HandleKind::File {
        retire_file(entry);
    }

    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_delete_file` — delete a file by name through the mounted
/// NTFS filesystem. Maps to `NtDeleteFile` in the real NT API.
pub unsafe extern "C" fn nt_delete_file(
    object_attributes: *mut super::types::ObjectAttributes,
) -> NTSTATUS {
    use super::status::{STATUS_OBJECT_NAME_INVALID, STATUS_OBJECT_NAME_NOT_FOUND};
    use super::types::ObjectAttributes;
    if object_attributes.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let oa: ObjectAttributes =
        match crate::mm::user_copy::read_user_object(object_attributes as u64) {
            Ok(v) => v,
            Err(status) => return status,
        };
    if oa.object_name.is_null() {
        return STATUS_OBJECT_NAME_INVALID;
    }
    let name = match crate::mm::user_copy::read_user_unicode_string(
        oa.object_name as u64,
        u16::MAX as usize,
    ) {
        Ok(v) => v,
        Err(status) => return status,
    };
    match crate::fs::ntfs::get_mounted_fs() {
        Some(fs) => {
            if crate::fs::ntfs::delete_file(fs, &name).is_ok() {
                STATUS_SUCCESS
            } else {
                STATUS_OBJECT_NAME_NOT_FOUND
            }
        }
        None => STATUS_DEVICE_NOT_READY,
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_flush_buffers_file` — flush the file's buffers. Stub.
pub unsafe extern "C" fn nt_flush_buffers_file(
    file_handle: HANDLE,
    io_status_block: *mut IoStatusBlock,
) -> NTSTATUS {
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if lookup_handle(file_handle).is_none() {
        return STATUS_INVALID_HANDLE;
    }
    let iosb = IoStatusBlock {
        status: STATUS_SUCCESS,
        information: 0,
    };
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return super::status::STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_query_volume_information_file` — query information about a volume
pub unsafe extern "C" fn nt_query_volume_information_file(
    _file_handle: HANDLE,
    io_status_block: *mut IoStatusBlock,
    fs_information: PVOID,
    length: u32,
    _fs_information_class: u32,
) -> NTSTATUS {
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let _ = (fs_information, length);

    let iosb = IoStatusBlock {
        status: STATUS_SUCCESS,
        information: 0,
    };
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return super::status::STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `nt_set_volume_information_file` — set information about a volume
pub unsafe extern "C" fn nt_set_volume_information_file(
    _file_handle: HANDLE,
    io_status_block: *mut IoStatusBlock,
    fs_information: PVOID,
    length: u32,
    fs_information_class: u32,
) -> NTSTATUS {
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let _ = (fs_information, length, fs_information_class);

    let iosb = IoStatusBlock {
        status: STATUS_SUCCESS,
        information: 0,
    };
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return super::status::STATUS_ACCESS_VIOLATION;
    }
    STATUS_SUCCESS
}

// ---------------------------------------------------------------------------
// Directory enumeration — nt_query_directory_file
// ---------------------------------------------------------------------------

#[path = "file/directory_query.rs"]
mod directory_query;

/// # Safety
/// The file payload remains pinned until all metadata reads and copyout finish.
/// User buffers are accessed exclusively through the user-copy interface.
pub unsafe extern "C" fn nt_query_directory_file(
    file_handle: HANDLE,
    _event: HANDLE,
    _apc_routine: PVOID,
    _apc_context: PVOID,
    io_status_block: *mut IoStatusBlock,
    file_information: PVOID,
    length: u32,
    file_information_class: u32,
    return_single_entry: u8,
    file_name: *mut UnicodeString,
    restart_scan: u8,
) -> NTSTATUS {
    // Win7's I/O front end touches IOSB, validates the class/minimum length,
    // probes the output, then captures the name on every request before lookup.
    if crate::mm::user_copy::probe_user_write(io_status_block as u64, size_of_iosb()).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    let (minimum, alignment) = match file_information_class {
        1 | 2 => (0x48, 8),
        3 => (0x60, 8),
        12 => (0x10, 4),
        37 => (0x70, 8),
        38 => (0x58, 8),
        _ => return super::status::STATUS_INVALID_INFO_CLASS,
    };
    if length < minimum {
        return super::status::STATUS_INFO_LENGTH_MISMATCH;
    }
    if file_information as usize & (alignment - 1) != 0 {
        return super::status::STATUS_DATATYPE_MISALIGNMENT;
    }
    if crate::mm::user_copy::probe_user_write(file_information as u64, length as usize).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    let expression = if !file_name.is_null() {
        let header: UnicodeString = match crate::mm::user_copy::read_user_object(file_name as u64) {
            Ok(header) => header,
            Err(status) => return status,
        };
        if header.length & 1 != 0 {
            return STATUS_INVALID_PARAMETER;
        }
        if header.length == 0 {
            None
        } else {
            if (header.buffer as u64).checked_add(u64::from(header.length))
                .is_none_or(|end| end > crate::mm::constants::USER_LIMIT)
            {
                return STATUS_ACCESS_VIOLATION;
            }
            if header.length > 0x200 {
                return STATUS_INVALID_PARAMETER;
            }
            let mut captured = Vec::new();
            if captured.try_reserve_exact(usize::from(header.length) / 2).is_err() {
                return super::status::STATUS_INSUFFICIENT_RESOURCES;
            }
            captured.resize(usize::from(header.length) / 2, 0u16);
            let bytes = core::slice::from_raw_parts_mut(captured.as_mut_ptr().cast::<u8>(), captured.len() * 2);
            if crate::mm::user_copy::copy_from_user(bytes, header.buffer as u64).is_err() {
                return STATUS_ACCESS_VIOLATION;
            }
            Some(captured)
        }
    } else { None };
    // Check FILE_LIST_DIRECTORY and retain the payload atomically with close.
    let reference = match handles::reference_file_handle_with_access(file_handle, 1, retire_file) {
        Ok(reference) => reference,
        Err(status) => return status,
    };
    let file_object = file_object_of(reference.entry().target);
    let _request = (*file_object).request_lock.lock();
    if (*file_object).fcb.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }
    let ntfs_handle = &*((*file_object).fcb as *const crate::fs::ntfs::NtfsHandle);
    if !ntfs_handle.is_directory {
        return STATUS_NOT_A_DIRECTORY;
    }
    let filesystem = match crate::fs::ntfs::get_mounted_fs() {
        Some(filesystem) => filesystem,
        None => return STATUS_DEVICE_NOT_READY,
    };
    let entries = match crate::fs::ntfs::list_raw_directory_checked(filesystem, ntfs_handle.mft_record) {
        Ok(entries) => entries,
        Err(status) => return status as NTSTATUS,
    };
    // request_lock protects both the cursor and first-expression capture. The
    // payload pin ensures close cannot retire this state during user copyout.
    let search = &mut (*file_object).directory_search;
    let first_query = search.expression().is_none();
    // Subsequent names have already been validated/captured by the front
    // end, but the filesystem retains the first query's expression.
    let expression = if first_query { expression } else { None };
    // Non-ASCII collation comes from this NTFS volume, never the host's Unicode
    // version. ASCII's case mapping is fixed by the NTFS $UpCase contract.
    let needs_table = expression.as_deref().or_else(|| search.expression())
        .is_some_and(|pattern| pattern.iter().any(|&unit| unit >= 0x80))
        || entries.iter().any(|raw| raw.name[..usize::from(raw.name_len)].iter().any(|&unit| unit >= 0x80));
    let table = if needs_table {
        match crate::fs::ntfs::upcase::read_table(filesystem) {
            Ok(table) => Some(table),
            Err(status) => return status as NTSTATUS,
        }
    } else { None };
    let upcase = |unit: u16| -> u16 {
        if let Some(table) = &table { crate::fs::ntfs::upcase::lookup(table, unit) }
        else if (b'a' as u16..=b'z' as u16).contains(&unit) { unit - 32 }
        else { unit }
    };
    if let Err(status) = search.begin(expression.as_deref(), restart_scan != 0, upcase) {
        return status;
    }
    let mut matching = Vec::new();
    let mut positions = Vec::new();
    for (index, raw) in entries.iter().enumerate().skip(search.position()) {
        let matches = match search.matches(&raw.name[..usize::from(raw.name_len)], upcase) {
            Ok(matches) => matches,
            Err(status) => return status,
        };
        if matches {
            if matching.try_reserve(1).is_err() || positions.try_reserve(1).is_err() {
                return super::status::STATUS_INSUFFICIENT_RESOURCES;
            }
            matching.push(*raw);
            positions.push(index + 1);
            if return_single_entry != 0 { break; }
        }
    }
    let mut output = match directory_query::serialize(
        &matching, file_information_class, length as usize, return_single_entry != 0,
    ) {
        Ok(output) => output,
        Err(status) => return status,
    };
    let status = if matching.is_empty() {
        if first_query { super::status::STATUS_NO_SUCH_FILE }
        else { super::status::STATUS_NO_MORE_FILES }
    } else if !first_query && matches!(output.disposition,
        directory_query::Disposition::PartialName | directory_query::Disposition::HeaderTooSmall)
    {
        // NT's subsequent-query short-buffer contract is SUCCESS with zero
        // bytes and the pending entry retained for a larger-buffer retry.
        output.bytes.clear();
        STATUS_SUCCESS
    } else {
        output.status()
    };
    if !output.bytes.is_empty()
        && crate::mm::user_copy::copy_to_user(file_information as u64, &output.bytes).is_err()
    {
        return STATUS_ACCESS_VIOLATION;
    }
    let iosb = IoStatusBlock { status, information: output.bytes.len() };
    if crate::mm::user_copy::write_user_object(io_status_block as u64, &iosb).is_err() {
        return STATUS_ACCESS_VIOLATION;
    }
    if matching.is_empty() {
        search.commit(entries.len(), 0);
    } else if output.entries_written != 0 {
        search.commit(positions[output.entries_written - 1], output.entries_written);
    }
    status
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register a pipe end with the handle table. `pipe_id` is the
/// host-side identifier returned by `io::pipe::create_anonymous`;
/// `is_write` distinguishes the read end from the write end so
/// nt_read_file/nt_write_file can route correctly.
pub fn alloc_pipe_handle(pipe_id: u64) -> HANDLE {
    // Resolve the pipe slot for this id and decide read/write.
    let (slot, is_write) = match crate::io::pipe::slot_for_id(pipe_id) {
        Some(s) => s,
        None => return core::ptr::null_mut(),
    };
    let target = (slot as u64) | ((is_write as u64) << 32);
    alloc_handle(HandleKind::Pipe, target)
}

/// Extract the pipe slot index from a Pipe handle's target.
pub(crate) fn pipe_slot_for(target: u64) -> usize {
    (target & 0xFFFFFFFF) as usize
}

/// Extract the write-direction flag from a Pipe handle's target.
pub(crate) fn pipe_is_write(target: u64) -> bool {
    ((target >> 32) & 0x1) != 0
}

pub(crate) fn wide_to_string(s: &UnicodeString) -> Option<String> {
    if s.buffer.is_null() {
        return None;
    }
    let slice = s.as_slice();
    let mut out = String::new();
    for &c in slice {
        if c == 0 {
            break;
        }
        if let Some(ch) = char::from_u32(c as u32) {
            out.push(ch);
        }
    }
    Some(out)
}

fn size_of_handle() -> usize {
    core::mem::size_of::<HANDLE>()
}

fn size_of_iosb() -> usize {
    core::mem::size_of::<IoStatusBlock>()
}

/// Win7 captures LARGE_INTEGER input with DWORD alignment, including x64.
/// Copy it once before resolving the handle, and never probe it for writing.
fn capture_file_byte_offset(pointer: *const i64) -> Result<Option<i64>, NTSTATUS> {
    if pointer.is_null() {
        return Ok(None);
    }
    if pointer as usize & 3 != 0 {
        return Err(super::status::STATUS_DATATYPE_MISALIGNMENT);
    }
    crate::mm::user_copy::read_user_object(pointer as u64).map(Some)
}

/// Common read routing. File handles transfer through an IRP_MJ_READ
/// issued with IoCallDriver against the FileObject the CREATE IRP
/// produced (see file_transfer_irp); pipe handles keep using the pipe
/// slots. `byte_offset` mirrors NtReadFile: None reads at the current
/// file position, Some(offset) reads there.
fn read_file_common(
    file_handle: HANDLE,
    buffer: &mut [u8],
    byte_offset: Option<i64>,
) -> (NTSTATUS, usize) {
    use super::status::{STATUS_END_OF_FILE, STATUS_INVALID_HANDLE};
    let entry = match lookup_handle(file_handle) {
        Some(entry) => entry,
        None => return (STATUS_INVALID_HANDLE, 0),
    };
    match entry.kind {
        HandleKind::File => {
            let reference = match reference_file(file_handle) {
                Ok(reference) => reference,
                Err(status) => return (status, 0),
            };
            let (status, bytes) = unsafe {
                file_transfer_irp(
                    file_object_of(reference.entry().target),
                    crate::io::major::IRP_MJ_READ,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    byte_offset,
                )
            };
            let status = if status >= 0 && bytes == 0 && !buffer.is_empty() {
                // A read of a non-zero length that transferred nothing
                // is an end-of-file condition in NT.
                STATUS_END_OF_FILE
            } else {
                status
            };
            (status, bytes)
        }
        HandleKind::Pipe => {
            if pipe_is_write(entry.target) {
                return (STATUS_INVALID_HANDLE, 0);
            }
            let bytes = crate::io::pipe::read(pipe_slot_for(entry.target), buffer);
            let status = if bytes == 0 { STATUS_END_OF_FILE } else { 0 };
            (status, bytes)
        }
        _ => (STATUS_INVALID_HANDLE, 0),
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Trusted-kernel variant of `NtReadFile` with borrowed kernel buffers.
pub unsafe fn nt_read_file_kernel(
    file_handle: HANDLE,
    io_status_block: &mut IoStatusBlock,
    buffer: &mut [u8],
    byte_offset: Option<&mut i64>,
) -> NTSTATUS {
    let (status, bytes) = read_file_common(file_handle, buffer, byte_offset.as_deref().copied());
    io_status_block.status = status;
    io_status_block.information = bytes;
    if bytes > 0 {
        if let Some(byte_offset) = byte_offset {
            *byte_offset += bytes as i64;
        }
    }
    status
}

/// Common write routing — IRP_MJ_WRITE for File handles, pipe slots for
/// pipe ends. `byte_offset` mirrors NtWriteFile: None writes at the
/// current file position, Some(offset) writes there.
fn write_file_common(
    file_handle: HANDLE,
    buffer: &[u8],
    byte_offset: Option<i64>,
) -> (NTSTATUS, usize) {
    use super::status::{STATUS_INVALID_HANDLE, STATUS_SUCCESS};
    let entry = match lookup_handle(file_handle) {
        Some(entry) => entry,
        None => return (STATUS_INVALID_HANDLE, 0),
    };
    match entry.kind {
        HandleKind::File => {
            let reference = match reference_file(file_handle) {
                Ok(reference) => reference,
                Err(status) => return (status, 0),
            };
            unsafe {
                file_transfer_irp(
                    file_object_of(reference.entry().target),
                    crate::io::major::IRP_MJ_WRITE,
                    buffer.as_ptr() as *mut u8,
                    buffer.len(),
                    byte_offset,
                )
            }
        }
        HandleKind::Pipe => {
            if !pipe_is_write(entry.target) {
                return (STATUS_INVALID_HANDLE, 0);
            }
            let bytes = crate::io::pipe::write(pipe_slot_for(entry.target), buffer);
            (STATUS_SUCCESS, bytes)
        }
        _ => (STATUS_INVALID_HANDLE, 0),
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// Trusted-kernel variant of `NtWriteFile` with borrowed kernel buffers.
pub unsafe fn nt_write_file_kernel(
    file_handle: HANDLE,
    io_status_block: &mut IoStatusBlock,
    buffer: &[u8],
    byte_offset: Option<&mut i64>,
) -> NTSTATUS {
    let (status, bytes) = write_file_common(file_handle, buffer, byte_offset.as_deref().copied());
    io_status_block.status = status;
    io_status_block.information = bytes;
    if bytes > 0 {
        if let Some(byte_offset) = byte_offset {
            *byte_offset += bytes as i64;
        }
    }
    status
}

#[path = "file/metadata.rs"]
pub mod metadata;

pub use metadata::{nt_query_attributes_file, nt_query_full_attributes_file};

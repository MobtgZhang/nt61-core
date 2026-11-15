//! Shared dispatcher object naming and access contracts. This namespace currently
//! provides BaseNamedObjects and session 0's BaseNamedObjects; directory handles,
//! custom security descriptors and NLS case tables require further integration.
use super::super::file::{self, DispatcherState, HandleKind};
use super::super::status::*;
use super::super::types::{ObjectAttributes, UnicodeString, HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{
    copy_from_user, copy_to_user, probe_user_write, read_user_object, write_user_object,
};
use alloc::vec::Vec;

pub(crate) struct SyncAttributes {
    pub name: Option<Vec<u16>>,
    pub flags: u8,
    pub case_insensitive: bool,
    pub open_if: bool,
}

impl SyncAttributes {
    fn unnamed() -> Self {
        Self {
            name: None,
            flags: 0,
            case_insensitive: false,
            open_if: false,
        }
    }
}

/// Generic mappings from the NT dispatcher object types. Security descriptor
/// evaluation is separate from the granted mask retained in each handle slot.
pub(crate) fn map_sync_access(kind: HandleKind, access: u32) -> Result<u32, NTSTATUS> {
    let specific = match kind {
        HandleKind::Event | HandleKind::Semaphore | HandleKind::Timer => 3,
        HandleKind::Mutant => 1,
        _ => return Err(STATUS_OBJECT_TYPE_MISMATCH),
    };
    let all = 0x001f_0000 | specific;
    let mut mapped = access & 0x0fff_ffff;
    if access & 0x8000_0000 != 0 {
        mapped |= 0x0002_0001;
    }
    if access & 0x4000_0000 != 0 {
        mapped |= 0x0002_0000 | (specific & 2);
    }
    if access & 0x2000_0000 != 0 {
        mapped |= 0x0012_0000;
    }
    if access & 0x1000_0000 != 0 {
        mapped |= all;
    }
    // No token/ACL evaluation is available here to resolve MAXIMUM_ALLOWED or
    // grant ACCESS_SYSTEM_SECURITY. Do not silently widen either request.
    if mapped & 0x0100_0000 != 0 {
        return Err(STATUS_PRIVILEGE_NOT_HELD);
    }
    if mapped & 0x0200_0000 != 0 {
        return Err(STATUS_NOT_SUPPORTED);
    }
    if mapped & !all != 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    Ok(mapped)
}

pub(crate) fn equal_sync_name(left: &[u16], right: &[u16], insensitive: bool) -> bool {
    fn upper(value: u16) -> u16 {
        if (b'a' as u16..=b'z' as u16).contains(&value) {
            value - 32
        } else {
            value
        }
    }
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(&a, &b)| a == b || (insensitive && upper(a) == upper(b)))
}

fn validate_name(name: &[u16], insensitive: bool) -> Result<(), NTSTATUS> {
    if name.is_empty() {
        return Err(STATUS_OBJECT_NAME_INVALID);
    }
    if name[0] != b'\\' as u16 {
        return Err(STATUS_OBJECT_PATH_SYNTAX_BAD);
    }
    let prefixes = ["\\BaseNamedObjects\\", "\\Sessions\\0\\BaseNamedObjects\\"];
    for prefix in prefixes {
        let prefix: Vec<u16> = prefix.encode_utf16().collect();
        if name.len() > prefix.len() && equal_sync_name(&name[..prefix.len()], &prefix, insensitive)
        {
            let leaf = &name[prefix.len()..];
            if leaf.contains(&0) {
                return Err(STATUS_OBJECT_NAME_INVALID);
            }
            if leaf.contains(&(b'\\' as u16)) {
                return Err(STATUS_OBJECT_PATH_NOT_FOUND);
            }
            // ASCII folding is exact for this range. Explicitly reject the
            // missing NLS contract rather than give non-ASCII false negatives.
            if insensitive && name.iter().any(|&unit| unit > 0x7f) {
                return Err(STATUS_NOT_SUPPORTED);
            }
            return Ok(());
        }
    }
    Err(STATUS_OBJECT_PATH_NOT_FOUND)
}

fn capture_attributes(pointer: *mut ObjectAttributes) -> Result<SyncAttributes, NTSTATUS> {
    if pointer.is_null() {
        return Ok(SyncAttributes::unnamed());
    }
    let attributes = read_user_object::<ObjectAttributes>(pointer as u64)?;
    if attributes.length as usize != core::mem::size_of::<ObjectAttributes>() {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if attributes.attributes & 0x200 != 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    if attributes.attributes & 0x10 != 0 {
        return Err(STATUS_PRIVILEGE_NOT_HELD);
    }
    if attributes.attributes & !(0x2 | 0x40 | 0x80) != 0 {
        return Err(STATUS_NOT_SUPPORTED);
    }
    if !attributes.root_directory.is_null() {
        return Err(STATUS_NOT_SUPPORTED);
    }
    if !attributes.security_descriptor.is_null()
        || !attributes.security_quality_of_service.is_null()
    {
        return Err(STATUS_NOT_SUPPORTED);
    }
    let mut result = SyncAttributes::unnamed();
    result.flags = u8::from(attributes.attributes & 2 != 0);
    result.case_insensitive = attributes.attributes & 0x40 != 0;
    result.open_if = attributes.attributes & 0x80 != 0;
    if !attributes.object_name.is_null() {
        let string = read_user_object::<UnicodeString>(attributes.object_name as u64)?;
        if string.length & 1 != 0 || string.length > string.maximum_length {
            return Err(STATUS_OBJECT_NAME_INVALID);
        }
        if string.length != 0 {
            let length = usize::from(string.length);
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| STATUS_NO_MEMORY)?;
            bytes.resize(length, 0);
            copy_from_user(&mut bytes, string.buffer as u64)?;
            let name = bytes
                .chunks_exact(2)
                .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                .collect::<Vec<_>>();
            validate_name(&name, result.case_insensitive)?;
            result.name = Some(name);
        }
    }
    Ok(result)
}

pub(crate) fn create_native_sync(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    kind: HandleKind,
    target: u64,
    dispatcher: DispatcherState,
) -> NTSTATUS {
    native_sync(output, access, attributes, kind, target, dispatcher, true)
}

pub(crate) fn open_native_sync(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    kind: HandleKind,
) -> NTSTATUS {
    native_sync(
        output,
        access,
        attributes,
        kind,
        0,
        DispatcherState::None,
        false,
    )
}

fn native_sync(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    kind: HandleKind,
    target: u64,
    dispatcher: DispatcherState,
    create: bool,
) -> NTSTATUS {
    let result = (|| {
        probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
        let captured = capture_attributes(attributes)?;
        let access = map_sync_access(kind, access)?;
        let (handle, status) =
            file::create_sync_handle(kind, target, dispatcher, access, captured, create)?;
        if let Err(error) = write_user_object(output as u64, &handle) {
            file::free_handle(handle);
            return Err(error);
        }
        Ok(status)
    })();
    result.unwrap_or_else(|status| status)
}

/// Duplicate supported dispatcher handles within the current process. Handle
/// access may be reduced, attributes are independent, and shared state is pinned.
/// Cross-process tables and object security revalidation remain unavailable.
pub fn duplicate_object_kernel(
    source_process: HANDLE,
    source: HANDLE,
    target_process: HANDLE,
    access: u32,
    attributes: u32,
    options: u32,
) -> Result<HANDLE, NTSTATUS> {
    if source_process as isize != -1 {
        return Err(STATUS_NOT_SUPPORTED);
    }
    // OBJ_PROTECT_CLOSE is 0x1; 0x400 is OBJ_FORCE_ACCESS_CHECK and
    // must never accidentally turn into handle protection.
    if attributes & !(1 | 2) != 0 || options & !7 != 0 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    if target_process.is_null() {
        if options & 1 == 0 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        file::close_sync_handle(source)?;
        return Ok(core::ptr::null_mut());
    }
    if target_process as isize != -1 {
        return Err(STATUS_NOT_SUPPORTED);
    }
    let flags = u8::from(attributes & 2 != 0) | (u8::from(attributes & 1 != 0) << 1);
    file::duplicate_sync_handle_with_access(source, access, flags, options)
}

/// # Safety
/// Output is a calling-process user address and is captured via user_copy.
pub unsafe extern "C" fn nt_duplicate_object(
    source_process: HANDLE,
    source: HANDLE,
    target_process: HANDLE,
    target: *mut HANDLE,
    access: u32,
    attributes: u32,
    options: u32,
) -> NTSTATUS {
    if target.is_null() {
        // A close-only operation has no output handle. Creating an alias with
        // no destination would make it unreachable in this implementation.
        if !target_process.is_null() {
            return STATUS_NOT_SUPPORTED;
        }
    } else if let Err(status) = probe_user_write(target as u64, core::mem::size_of::<HANDLE>()) {
        return status;
    }
    match duplicate_object_kernel(
        source_process,
        source,
        target_process,
        access,
        attributes,
        options,
    ) {
        Ok(_) if target.is_null() => STATUS_SUCCESS,
        Ok(handle) => match write_user_object(target as u64, &handle) {
            Ok(()) => STATUS_SUCCESS,
            Err(status) => {
                file::free_handle(handle);
                status
            }
        },
        Err(status) => status,
    }
}

/// Open a named dispatcher object using already-trusted Win32 pointers. The
/// lookup preserves the caller's requested access and handle inheritance.
///
/// # Safety
/// Name must be a readable, terminated caller-owned UTF-16 string.
pub(crate) unsafe fn open_sync_win32(
    kind: HandleKind,
    access: u32,
    inherit: bool,
    name: *const u16,
) -> Result<HANDLE, NTSTATUS> {
    // SAFETY: the wrapper propagates its caller's readable-name contract.
    let mut attributes = unsafe { win32_sync_attributes(core::ptr::null(), name)? };
    attributes.flags = u8::from(inherit);
    let access = map_sync_access(kind, access)?;
    file::create_sync_handle(kind, 0, DispatcherState::None, access, attributes, false)
        .map(|(handle, _)| handle)
}

/// ObjectNameInformation for the supported dispatcher namespace. The returned
/// UNICODE_STRING points inside the caller's buffer, never at kernel storage.
pub(super) fn query_sync_name(
    handle: HANDLE,
    information: PVOID,
    length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    let result = (|| {
        if length != 0 {
            probe_user_write(information as u64, length as usize)?;
        }
        if !return_length.is_null() {
            probe_user_write(return_length as u64, 4)?;
        }
        let name = file::sync_object_name(handle)?;
        let header = core::mem::size_of::<UnicodeString>();
        let string_bytes = name.as_ref().map_or(0, |name| name.len() * 2 + 2);
        let required = header + string_bytes;
        if !return_length.is_null() {
            write_user_object(return_length as u64, &(required as u32))?;
        }
        if (length as usize) < required {
            return Err(STATUS_INFO_LENGTH_MISMATCH);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(required)
            .map_err(|_| STATUS_NO_MEMORY)?;
        bytes.resize(required, 0);
        if let Some(name) = name {
            // UNICODE_STRING lengths include only the string/terminator,
            // excluding its header. MaximumLength cannot exceed USHORT.
            let maximum = u16::try_from(string_bytes).map_err(|_| STATUS_NAME_TOO_LONG)?;
            bytes[..2].copy_from_slice(&(maximum - 2).to_le_bytes());
            bytes[2..4].copy_from_slice(&maximum.to_le_bytes());
            let buffer = (information as usize)
                .checked_add(header)
                .ok_or(STATUS_ACCESS_VIOLATION)?;
            let pointer_offset = core::mem::offset_of!(UnicodeString, buffer);
            bytes[pointer_offset..pointer_offset + core::mem::size_of::<usize>()]
                .copy_from_slice(&buffer.to_le_bytes());
            for (index, unit) in name.iter().enumerate() {
                bytes[header + index * 2..header + index * 2 + 2]
                    .copy_from_slice(&unit.to_le_bytes());
            }
        }
        copy_to_user(information as u64, &bytes)?;
        Ok(STATUS_SUCCESS)
    })();
    result.unwrap_or_else(|status| status)
}

/// Capture Win32's trusted SECURITY_ATTRIBUTES and null-terminated name without
/// passing kernel stack addresses through user probing. Local/default names use
/// session 0, Global names use the global BaseNamedObjects directory.
///
/// # Safety
/// Both pointers, when non-null, must be readable caller-owned memory.
pub(crate) unsafe fn win32_sync_attributes(
    security: *const u8,
    name: *const u16,
) -> Result<SyncAttributes, NTSTATUS> {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SecurityAttributes {
        length: u32,
        descriptor: super::super::types::PVOID,
        inherit: i32,
    }
    let mut result = SyncAttributes::unnamed();
    result.open_if = true;
    if !security.is_null() {
        // SAFETY: caller supplies a readable SECURITY_ATTRIBUTES.
        let security = unsafe { security.cast::<SecurityAttributes>().read_unaligned() };
        if security.length as usize != core::mem::size_of::<SecurityAttributes>() {
            return Err(STATUS_INVALID_PARAMETER);
        }
        if !security.descriptor.is_null() {
            return Err(STATUS_NOT_SUPPORTED);
        }
        result.flags = u8::from(security.inherit != 0);
    }
    if !name.is_null() {
        let mut leaf = Vec::new();
        for index in 0..=32767usize {
            // SAFETY: caller supplies a terminated readable WCHAR string.
            let unit = unsafe { name.add(index).read_unaligned() };
            if unit == 0 {
                break;
            }
            if index == 32767 {
                return Err(STATUS_OBJECT_NAME_INVALID);
            }
            leaf.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
            leaf.push(unit);
        }
        let global: Vec<u16> = "Global\\".encode_utf16().collect();
        let local: Vec<u16> = "Local\\".encode_utf16().collect();
        let (prefix, leaf) = if leaf.starts_with(&global) {
            ("\\BaseNamedObjects\\", &leaf[global.len()..])
        } else if leaf.starts_with(&local) {
            ("\\Sessions\\0\\BaseNamedObjects\\", &leaf[local.len()..])
        } else {
            ("\\Sessions\\0\\BaseNamedObjects\\", leaf.as_slice())
        };
        if leaf.is_empty() {
            return Err(STATUS_OBJECT_NAME_INVALID);
        }
        let mut full: Vec<u16> = prefix.encode_utf16().collect();
        full.try_reserve(leaf.len()).map_err(|_| STATUS_NO_MEMORY)?;
        full.extend_from_slice(leaf);
        if full.len() > 32767 {
            return Err(STATUS_OBJECT_NAME_INVALID);
        }
        validate_name(&full, false)?;
        result.name = Some(full);
    }
    Ok(result)
}

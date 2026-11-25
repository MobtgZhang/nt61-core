use super::super::{error, put, word, GuestImage};
use crate::arch::common::btl::{ir::DecodeMode, memory::GuestMemory};
use alloc::vec::Vec;

const HANDLE_BASE: u64 = 0x7200_0000;
const HANDLE_END: u64 = 0x7300_0000;
const MAX_HANDLES: usize = 4096;
const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
const READ_CONTROL: u32 = 0x0002_0000;
const GENERIC_READ: u32 = 0x8000_0000;
const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
const INVALID_HANDLE: u32 = 6;
const ACCESS_DENIED: u32 = 5;
const NOT_ENOUGH_MEMORY: u32 = 8;
const NOT_SUPPORTED: u32 = 50;
const INVALID_PARAMETER: u32 = 87;
const INSUFFICIENT_BUFFER: u32 = 122;
const NOT_ALL_ASSIGNED: u32 = 1300;
const NOACCESS: u32 = 998;
const TOKEN_PRIMARY: u32 = 1;
const TOKEN_ELEVATION_TYPE_DEFAULT: u32 = 1;
const CHANGE_NOTIFY_LUID: u64 = 23;
const ENABLED_BY_DEFAULT: u32 = 1;
const ENABLED: u32 = 2;
const REMOVED: u32 = 4;
const PRIVILEGE_ENTRY_SIZE: usize = 12;
const PRIVILEGE_HEADER_SIZE: usize = 4;
const PRIVILEGE_STATE_SIZE: usize = PRIVILEGE_HEADER_SIZE + PRIVILEGE_ENTRY_SIZE;

pub(in super::super) struct State {
    handles: Vec<(u64, u32)>,
    issued: u32,
    change_notify: Option<u32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            handles: Vec::new(),
            issued: 0,
            change_notify: Some(ENABLED_BY_DEFAULT | ENABLED),
        }
    }
}

impl State {
    fn require_access(&self, handle: u64, required: u32) -> Result<(), u32> {
        let access = self.handles.iter().find(|entry| entry.0 == handle)
            .ok_or(INVALID_HANDLE)?.1;
        if access & required != required {
            return Err(ACCESS_DENIED);
        }
        Ok(())
    }

    pub(in super::super) fn close(&mut self, handle: u64) -> Option<bool> {
        if !(HANDLE_BASE..HANDLE_END).contains(&handle) {
            return None;
        }
        let index = self.handles.iter().position(|entry| entry.0 == handle);
        Some(index.is_some_and(|index| {
            self.handles.swap_remove(index);
            true
        }))
    }
}

fn open(image: &mut GuestImage, process: u64, access: u32, output: u64) -> Result<u64, u32> {
    let (current, width) = match image.thread.context.mode {
        DecodeMode::X86 => (u64::from(u32::MAX), 4),
        DecodeMode::X64 => (u64::MAX, 8),
    };
    if process != current {
        return Err(INVALID_HANDLE);
    }
    let generic_read_access = TOKEN_QUERY | READ_CONTROL;
    let allowed = generic_read_access | TOKEN_ADJUST_PRIVILEGES;
    let requested = if access & GENERIC_READ != 0 {
        (access & !GENERIC_READ) | generic_read_access
    } else {
        access
    };
    if requested & !(allowed | MAXIMUM_ALLOWED) != 0 {
        return Err(ACCESS_DENIED);
    }
    let granted = if requested & MAXIMUM_ALLOWED != 0 { allowed } else { requested };
    if output == 0 || !image.memory.probe_write(output, width) {
        return Err(NOACCESS);
    }
    let state = &mut image.win32.tokens;
    let handle = HANDLE_BASE + u64::from(state.issued) * 4;
    if state.handles.len() >= MAX_HANDLES || handle >= HANDLE_END {
        return Err(NOT_ENOUGH_MEMORY);
    }
    state.handles.try_reserve(1).map_err(|_| NOT_ENOUGH_MEMORY)?;
    put(&mut image.memory, output, handle, width)?;
    state.handles.push((handle, granted));
    state.issued += 1;
    Ok(1)
}

fn query(image: &mut GuestImage, args: &[u64]) -> Result<u64, u32> {
    let class = args[1] as u32;
    image.win32.tokens.require_access(args[0], TOKEN_QUERY)?;
    let mut bytes = [0; PRIVILEGE_STATE_SIZE];
    let size = match class {
        3 => encode_privileges(&mut bytes, image.win32.tokens.change_notify),
        8 | 18 | 20 => {
            let value = match class {
                8 => TOKEN_PRIMARY,
                18 => TOKEN_ELEVATION_TYPE_DEFAULT,
                _ => 0,
            };
            bytes[..4].copy_from_slice(&value.to_le_bytes());
            4
        }
        1..=28 => return Err(NOT_SUPPORTED),
        _ => return Err(INVALID_PARAMETER),
    };
    let return_length = args[4];
    if return_length == 0 || !image.memory.probe_write(return_length, 4) {
        return Err(NOACCESS);
    }
    if (args[3] as u32) < size as u32 {
        put(&mut image.memory, return_length, size as u64, 4)?;
        return Err(INSUFFICIENT_BUFFER);
    }
    let output = args[2];
    if output == 0 || !image.memory.probe_write(output, size) {
        return Err(NOACCESS);
    }
    if !image.memory.write_virt(output, &bytes[..size]) {
        return Err(NOACCESS);
    }
    put(&mut image.memory, return_length, size as u64, 4)?;
    Ok(1)
}

fn encode_privileges(bytes: &mut [u8; PRIVILEGE_STATE_SIZE], change_notify: Option<u32>) -> usize {
    bytes[..4].copy_from_slice(&u32::from(change_notify.is_some()).to_le_bytes());
    if let Some(attributes) = change_notify {
        bytes[4..12].copy_from_slice(&CHANGE_NOTIFY_LUID.to_le_bytes());
        bytes[12..16].copy_from_slice(&attributes.to_le_bytes());
        PRIVILEGE_STATE_SIZE
    } else {
        PRIVILEGE_HEADER_SIZE
    }
}

fn adjust(image: &mut GuestImage, args: &[u64]) -> Result<u64, u32> {
    let &[handle, disable_all, new_state, buffer_length, previous, return_length, ..] = args else {
        return Err(INVALID_PARAMETER);
    };
    let access = TOKEN_ADJUST_PRIVILEGES | if previous != 0 { TOKEN_QUERY } else { 0 };
    image.win32.tokens.require_access(handle, access)?;
    let original = image.win32.tokens.change_notify;
    let mut updated = original;
    let mut all_assigned = true;
    if disable_all as u32 != 0 {
        updated = original.map(|attributes| attributes & !ENABLED);
    } else {
        if new_state == 0 {
            return Err(INVALID_PARAMETER);
        }
        let count = word(&mut image.memory, new_state, 4).map_err(|_| NOACCESS)?;
        if count > 4096 {
            return Err(INVALID_PARAMETER);
        }
        let first = new_state.checked_add(PRIVILEGE_HEADER_SIZE as u64).ok_or(NOACCESS)?;
        first.checked_add(count * PRIVILEGE_ENTRY_SIZE as u64).ok_or(NOACCESS)?;
        let mut bytes = [0; PRIVILEGE_ENTRY_SIZE];
        for index in 0..count {
            let entry = first + index * PRIVILEGE_ENTRY_SIZE as u64;
            if image.memory.read_virt(entry, &mut bytes) != bytes.len() {
                return Err(NOACCESS);
            }
            let luid = u64::from_le_bytes(bytes[..8].try_into().unwrap());
            let attributes = u32::from_le_bytes(bytes[8..].try_into().unwrap());
            if luid != CHANGE_NOTIFY_LUID || updated.is_none() {
                all_assigned = false;
            } else if attributes & REMOVED != 0 {
                updated = None;
            } else {
                updated = updated.map(|old| (old & !ENABLED) | (attributes & ENABLED));
            }
        }
    }
    if previous != 0 {
        let changed = updated.is_some() && updated != original;
        let mut bytes = [0; PRIVILEGE_STATE_SIZE];
        let size = encode_privileges(&mut bytes, if changed { original } else { None });
        if return_length == 0 || !image.memory.probe_write(return_length, 4) {
            return Err(NOACCESS);
        }
        if (buffer_length as u32) < size as u32 {
            put(&mut image.memory, return_length, size as u64, 4)?;
            return Err(INSUFFICIENT_BUFFER);
        }
        if !image.memory.write_virt(previous, &bytes[..size]) {
            return Err(NOACCESS);
        }
        put(&mut image.memory, return_length, size as u64, 4)?;
    }
    image.win32.tokens.change_notify = updated;
    error(image, if all_assigned { 0 } else { NOT_ALL_ASSIGNED });
    Ok(1)
}

pub(in super::super) fn invoke(image: &mut GuestImage, name: &str, args: &[u64]) -> Option<Result<u64, u32>> {
    Some(match name {
        "OpenProcessToken" => open(image, args[0], args[1] as u32, args[2]),
        "GetTokenInformation" => query(image, args),
        "AdjustTokenPrivileges" => adjust(image, args),
        _ => return None,
    })
}

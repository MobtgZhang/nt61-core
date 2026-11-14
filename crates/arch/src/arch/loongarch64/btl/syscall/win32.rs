//! BTL — Win32 API thunk layer for x86 guests.

#![cfg(target_arch = "loongarch64")]

/// Win32 API service table index used to dispatch into win32k.sys.
pub const WIN32K_SERVICE_TABLE: usize = 1;

/// Win32k guest/native service layouts require dedicated structure and handle
/// thunks. Do not fabricate a usable service ID from an unchecked guest index.
pub fn translate_win32(_idx: u32) -> Option<u32> { None }

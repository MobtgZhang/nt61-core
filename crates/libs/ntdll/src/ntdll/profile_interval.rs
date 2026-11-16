//! Profile interval configuration. Unsupported hardware sources are rejected;
//! this module does not claim that sampling profiles are implemented.
use super::{status::*, types::NTSTATUS};
use crate::mm::user_copy::write_user_object;
use core::sync::atomic::{AtomicU32, Ordering};
static TIME_INTERVAL: AtomicU32 = AtomicU32::new(10000);

pub extern "C" fn nt_query_interval_profile(source: u32, output: *mut u32) -> NTSTATUS {
    if source != 0 {
        return STATUS_NOT_SUPPORTED;
    }
    write_user_object(output as u64, &TIME_INTERVAL.load(Ordering::Acquire))
        .err()
        .unwrap_or(STATUS_SUCCESS)
}

pub extern "C" fn nt_set_interval_profile(interval: u32, source: u32) -> NTSTATUS {
    if !super::privilege::enabled(13) {
        return STATUS_PRIVILEGE_NOT_HELD;
    }
    if source != 0 {
        return STATUS_NOT_SUPPORTED;
    }
    // The time source is quantized to the kernel's timer resolution.
    TIME_INTERVAL.store(interval.max(10000), Ordering::Release);
    STATUS_SUCCESS
}

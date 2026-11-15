//! Native clock queries shared by every architecture.
//!
//! All output ranges are probed before sampling or writing. The performance
//! frequency is written before the counter so a failed frequency copy cannot
//! leave the required counter output modified. Concurrent user unmapping is
//! still subject to the user-copy layer's usual partial-copy behavior.
use super::status::STATUS_SUCCESS;
use super::types::NTSTATUS;

#[path = "clock_source.rs"]
mod source;

/// Trusted in-kernel sampling for Win32 wrappers. No user address is accepted
/// here, so stack-local kernel outputs never enter the user-copy path.
pub(crate) fn sample_performance_counter() -> (u64, u64) {
    source::sample()
}

/// Return 100-ns intervals since 1601-01-01 UTC from the kernel timekeeper.
///
/// # Safety
/// The output is a user address and is validated by the user-copy layer.
pub unsafe extern "C" fn nt_query_system_time(system_time: *mut i64) -> NTSTATUS {
    if let Err(status) = crate::mm::user_copy::probe_user_write(system_time as u64, 8) {
        return status;
    }
    let value = crate::ke::time::get_system_time() as i64;
    match crate::mm::user_copy::write_user_object(system_time as u64, &value) {
        Ok(()) => STATUS_SUCCESS,
        Err(status) => status,
    }
}

/// Return a monotonic platform counter and its rate in counts per second.
/// `frequency` is optional; `counter` is required.
///
/// # Safety
/// Both output addresses are validated by the user-copy layer.
pub unsafe extern "C" fn nt_query_performance_counter(
    counter: *mut i64,
    frequency: *mut i64,
) -> NTSTATUS {
    if let Err(status) = crate::mm::user_copy::probe_user_write(counter as u64, 8) {
        return status;
    }
    if !frequency.is_null() {
        if let Err(status) = crate::mm::user_copy::probe_user_write(frequency as u64, 8) {
            return status;
        }
    }
    let (value, rate) = sample_performance_counter();
    if !frequency.is_null() {
        if let Err(status) =
            crate::mm::user_copy::write_user_object(frequency as u64, &(rate as i64))
        {
            return status;
        }
    }
    match crate::mm::user_copy::write_user_object(counter as u64, &(value as i64)) {
        Ok(()) => STATUS_SUCCESS,
        Err(status) => status,
    }
}

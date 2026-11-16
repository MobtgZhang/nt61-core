//! The present HAL uses a fixed 1 kHz clock on each supported architecture.
//! Report its real limits and retain the per-process resolution request, rather
//! than promising a tick interval that the interrupt controller cannot deliver.
use super::status::*;
use super::types::NTSTATUS;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{probe_user_write, write_user_object};
use alloc::vec::Vec;

static REQUESTS: Spinlock<Vec<u64>> = Spinlock::new(Vec::new());

fn increment() -> u32 {
    10_000_000 / crate::ke::time::TICKS_PER_SECOND
}

/// # Safety
/// All three addresses are user outputs, validated before the first write.
pub unsafe extern "C" fn nt_query_timer_resolution(
    minimum: *mut u32,
    maximum: *mut u32,
    actual: *mut u32,
) -> NTSTATUS {
    let result = (|| {
        for address in [minimum, maximum, actual] {
            probe_user_write(address as u64, 4)?;
        }
        for address in [minimum, maximum, actual] {
            write_user_object(address as u64, &increment())?;
        }
        Ok(STATUS_SUCCESS)
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// Current is a writable calling-process user address.
pub unsafe extern "C" fn nt_set_timer_resolution(
    _desired: u32,
    set: u8,
    current: *mut u32,
) -> NTSTATUS {
    let result = (|| {
        probe_user_write(current as u64, 4)?;
        let (pid, _) = super::sync::current_client_id();
        let mut requests = REQUESTS.lock();
        let previous = requests.iter().position(|&owner| owner == pid);
        if set != 0 && previous.is_none() {
            requests.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
        }
        // No request is changed if the user output faults.
        write_user_object(current as u64, &increment())?;
        if set != 0 {
            if previous.is_none() {
                requests.push(pid);
            }
        } else if let Some(index) = previous {
            requests.swap_remove(index);
        } else {
            return Ok(0xc000_0245u32 as i32); // STATUS_TIMER_RESOLUTION_NOT_SET
        }
        Ok(STATUS_SUCCESS)
    })();
    result.unwrap_or_else(|status| status)
}

pub(crate) fn process_exit(pid: u64) {
    REQUESTS.lock().retain(|&owner| owner != pid);
}

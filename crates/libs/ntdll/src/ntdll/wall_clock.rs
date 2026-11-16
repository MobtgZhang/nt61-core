//! Privileged wall-clock changes over the common HAL RTC interface.
//! Hardware clocks are UTC on the supported platforms. Local-RTC time-zone
//! refresh (the null-input form) still needs the time-zone policy backend.
use super::{status::*, types::NTSTATUS};
use crate::hal::common::cmos;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};

static SET_CLOCK: Spinlock<()> = Spinlock::new(());

/// # Safety
/// Input and optional output are caller addresses, accessed only via user-copy.
pub unsafe extern "C" fn nt_set_system_time(
    new_time: *const i64,
    previous_time: *mut i64,
) -> NTSTATUS {
    if new_time.is_null() {
        return STATUS_NOT_IMPLEMENTED;
    }
    // LARGE_INTEGER is four-byte aligned in the native probing contract.
    if new_time as usize & 3 != 0 || previous_time as usize & 3 != 0 {
        return 0x80000002u32 as i32;
    }
    let value: i64 = match read_user_object(new_time as u64) {
        Ok(value) => value,
        Err(status) => return status,
    };
    if !previous_time.is_null() {
        if let Err(status) = probe_user_write(previous_time as u64, 8) {
            return status;
        }
    }
    if !super::privilege::enabled(12) {
        return STATUS_PRIVILEGE_NOT_HELD;
    }
    let Some(fields) = u64::try_from(value)
        .ok()
        .and_then(cmos::nt_time_to_time_fields)
    else {
        return STATUS_INVALID_PARAMETER;
    };
    let previous = {
        // Never hold the clock ISR's short update guard during device I/O.
        // This separate guard orders full RTC/software-clock transactions.
        let _guard = SET_CLOCK.lock();
        if !cmos::set_real_time_clock(&fields) {
            return STATUS_UNSUCCESSFUL;
        }
        crate::ke::time::set_system_time(value as u64) as i64
    };
    if !previous_time.is_null() {
        if let Err(status) = write_user_object(previous_time as u64, &previous) {
            return status;
        }
    }
    STATUS_SUCCESS
}

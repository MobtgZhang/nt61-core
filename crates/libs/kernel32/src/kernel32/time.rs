//! kernel32 — time APIs
//
//! `get_system_time`, `get_local_time`, `get_tick_count`,
//! `get_tick_count64`, `query_performance_counter`,
//! `query_performance_frequency`, `get_system_time_as_file_time`.
//! All values are sourced from the kernel's time source.

use super::types::DWORD;

/// `SYSTEMTIME` — 16-byte struct.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

/// `FILETIME` — 8-byte struct (100ns since 1601-01-01).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileTime {
    pub low: u32,
    pub high: u32,
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `get_system_time`.
pub unsafe extern "C" fn get_system_time(out: *mut SystemTime) {
    if out.is_null() {
        return;
    }
    *out = system_time_from_filetime(crate::ke::time::get_system_time());
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `get_local_time` — same as `get_system_time` since we
/// have no concept of time zones.
pub unsafe extern "C" fn get_local_time(out: *mut SystemTime) {
    get_system_time(out);
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `get_system_time_as_file_time`.
pub unsafe extern "C" fn get_system_time_as_file_time(out: *mut FileTime) {
    if out.is_null() {
        return;
    }
    let ft = crate::ke::time::get_system_time();
    (*out).low = ft as u32;
    (*out).high = (ft >> 32) as u32;
}

/// `get_tick_count` — milliseconds since boot, lower 32 bits.
pub extern "C" fn get_tick_count() -> DWORD {
    get_tick_count64() as DWORD
}

/// `get_tick_count64` — milliseconds since boot, full 64 bits.
pub extern "C" fn get_tick_count64() -> u64 {
    crate::ke::time::get_interrupt_time() / 10_000
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `query_performance_counter`.
pub unsafe extern "C" fn query_performance_counter(counter: *mut i64) -> i32 {
    if counter.is_null() {
        return 0;
    }
    *counter = crate::libs::ntdll::clock::sample_performance_counter().0 as i64;
    1
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// `query_performance_frequency` — native clock rate (currently 1000 Hz).
pub unsafe extern "C" fn query_performance_frequency(freq: *mut i64) -> i32 {
    if freq.is_null() {
        return 0;
    }
    *freq = crate::libs::ntdll::clock::sample_performance_counter().1 as i64;
    1
}

/// Delay for the requested milliseconds; zero yields the current quantum.
pub extern "C" fn sleep(ms: DWORD) {
    let _ = super::sleep::sleep_ex(ms, false);
}

/// `sleep_ex` — same as `sleep` plus the alertable flag.
pub extern "C" fn sleep_ex(ms: DWORD, alertable: i32) -> DWORD {
    super::sleep::sleep_ex(ms, alertable != 0)
}

/// Decode NT epoch 100-ns units using the proleptic Gregorian calendar.
fn system_time_from_filetime(filetime: u64) -> SystemTime {
    let seconds = filetime / 10_000_000;
    let days = seconds / 86_400;
    // 1601 begins a full Gregorian 400-year cycle. Skipping complete cycles
    // also bounds the conversion cost for values near the u64 FILETIME limit.
    let cycles = days / 146_097;
    let mut year = 1601 + cycles as i64 * 400;
    let mut remaining = (days % 146_097) as i64;
    loop {
        let year_days = if is_leap(year) { 366 } else { 365 };
        if remaining < year_days {
            break;
        }
        remaining -= year_days;
        year += 1;
    }
    let month_days = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0;
    while remaining >= month_days[month] {
        remaining -= month_days[month];
        month += 1;
    }
    let today = seconds % 86_400;
    SystemTime {
        year: year as u16,
        month: month as u16 + 1,
        // SYSTEMTIME numbers Sunday as zero; 1601-01-01 was Monday.
        day_of_week: ((days + 1) % 7) as u16,
        day: remaining as u16 + 1,
        hour: (today / 3600) as u16,
        minute: ((today % 3600) / 60) as u16,
        second: (today % 60) as u16,
        milliseconds: ((filetime / 10_000) % 1000) as u16,
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

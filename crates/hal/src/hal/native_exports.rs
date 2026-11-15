//! Native HAL export wrappers. The internal compact calendar structure is not
//! WDK TIME_FIELDS: its members must be converted individually.
#![allow(non_snake_case)]

/// WDK TIME_FIELDS: eight signed SHORTs; weekday is Sunday=0.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimeFields {
    pub year: i16,
    pub month: i16,
    pub day: i16,
    pub hour: i16,
    pub minute: i16,
    pub second: i16,
    pub milliseconds: i16,
    pub weekday: i16,
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_HalQueryRealTimeClock(out: *mut TimeFields) -> u8 {
    if out.is_null() {
        return 0;
    }
    let Some(time) = crate::hal::common::cmos::query_real_time_clock() else {
        return 0;
    };
    if !(1601..=9999).contains(&time.year)
        || !(1..=12).contains(&time.month)
        || !(1..=31).contains(&time.day)
        || time.hour > 23
        || time.minute > 59
        || time.second() > 59
        || !(1..=7).contains(&time.weekday)
    {
        return 0;
    }
    let leap = time.year % 4 == 0 && (time.year % 100 != 0 || time.year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if time.day > month_days[usize::from(time.month - 1)] {
        return 0;
    }
    out.write(TimeFields {
        year: time.year as i16,
        month: i16::from(time.month),
        day: i16::from(time.day),
        hour: i16::from(time.hour),
        minute: i16::from(time.minute),
        second: i16::from(time.second()),
        // The supplied Win7 x64 HAL rounds its seconds-resolution RTC to
        // the middle of the second (RVA 0x1506: MOV 0x1f4 -> TIME_FIELDS+12).
        milliseconds: if cfg!(target_arch = "x86_64") { 500 } else { 0 },
        weekday: i16::from(time.weekday - 1),
    });
    1
}

#[used]
static EXPORT_ROOT: unsafe extern "efiapi" fn(*mut TimeFields) -> u8 =
    nt61_export_HalQueryRealTimeClock;

pub fn retain() {
    unsafe {
        core::ptr::read_volatile(&EXPORT_ROOT);
    }
}

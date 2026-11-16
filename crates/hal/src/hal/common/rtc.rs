//! Calendar conversion and register protocols for the supported MMIO RTCs.

use super::cmos::TimeFields;

fn leap_year(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_days(year: u16) -> [u8; 12] {
    [
        31,
        if leap_year(year) { 29 } else { 28 },
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
    ]
}

/// Encode a Gregorian calendar in NT-epoch seconds. Validate before device I/O.
pub(crate) fn time_fields_to_nt_seconds(time: &TimeFields) -> Option<u64> {
    if !(1601..=9999).contains(&time.year)
        || !(1..=12).contains(&time.month)
        || time.day == 0
        || time.day > month_days(time.year)[usize::from(time.month - 1)]
        || time.hour > 23
        || time.minute > 59
        || time.second() > 59
    {
        return None;
    }
    let mut days = 0u64;
    for year in 1601..time.year {
        days += if leap_year(year) { 366 } else { 365 };
    }
    for &count in &month_days(time.year)[..usize::from(time.month - 1)] {
        days += u64::from(count);
    }
    days += u64::from(time.day - 1);
    Some(
        days * 86_400
            + u64::from(time.hour) * 3600
            + u64::from(time.minute) * 60
            + u64::from(time.second()),
    )
}

pub(crate) fn time_fields_to_unix_seconds(time: &TimeFields) -> Option<u64> {
    time_fields_to_nt_seconds(time)?.checked_sub(11_644_473_600)
}

/// Goldfish time halves are independent writable registers. Serialize both
/// writes with reads; HIGH then LOW leaves the desired low part at commit.
pub(crate) fn write_goldfish(time: &TimeFields, mut write: impl FnMut(usize, u32)) -> bool {
    let Some(nanos) = time_fields_to_unix_seconds(time)
        .and_then(|value| value.checked_mul(1_000_000_000))
        .filter(|&value| value <= i64::MAX as u64)
    else {
        return false;
    };
    write(4, (nanos >> 32) as u32);
    write(0, nanos as u32);
    true
}

/// LS7A writes year first so a new February 29 is interpreted in its new year.
pub(crate) fn write_ls7a(time: &TimeFields, mut write: impl FnMut(usize, u32)) -> bool {
    if time.year < 1900 || time_fields_to_nt_seconds(time).is_none() {
        return false;
    }
    let fields = u32::from(time.month) << 26
        | u32::from(time.day) << 21
        | u32::from(time.hour) << 16
        | u32::from(time.minute) << 10
        | u32::from(time.second()) << 4;
    write(0x28, u32::from(time.year - 1900));
    write(0x24, fields);
    true
}

/// Decode an unsigned Unix timestamp, rejecting dates outside the NT range.
pub(crate) fn unix_seconds_to_time_fields(timestamp: u64) -> Option<TimeFields> {
    // 10000-01-01, beyond the last date representable by SYSTEMTIME.
    if timestamp >= 253_402_300_800 {
        return None;
    }
    let days = timestamp / 86_400;
    let mut remaining = days;
    let mut year = 1970;
    loop {
        let count = if leap_year(year) { 366 } else { 365 };
        if remaining < count {
            break;
        }
        remaining -= count;
        year += 1;
    }
    let mut month = 0;
    let counts = month_days(year);
    while remaining >= u64::from(counts[month]) {
        remaining -= u64::from(counts[month]);
        month += 1;
    }
    let seconds = timestamp % 86_400;
    Some(TimeFields::new(
        year,
        (month + 1) as u8,
        (remaining + 1) as u8,
        (seconds / 3600) as u8,
        (seconds / 60 % 60) as u8,
        (seconds % 60) as u8,
        ((days + 4) % 7 + 1) as u8,
    ))
}

/// Goldfish TIME_LOW latches TIME_HIGH. Callers serialize the whole pair.
pub(crate) fn read_goldfish(mut read: impl FnMut(usize) -> u32) -> Option<TimeFields> {
    let low = read(0);
    let high = read(4);
    let nanoseconds = (u64::from(high) << 32) | u64::from(low);
    // The protocol's high word is signed. A pre-epoch clock must not
    // wrap into a plausible date hundreds of years in the future.
    i64::try_from(nanoseconds).ok()?;
    unix_seconds_to_time_fields(nanoseconds / 1_000_000_000)
}

/// LS7A TOY registers hold calendar fields and a year relative to 1900.
/// Both samples must agree: READ0 and READ1 are separate, live registers.
pub(crate) fn read_ls7a(mut read: impl FnMut(usize) -> u32) -> Option<TimeFields> {
    for _ in 0..8 {
        let year = read(0x30);
        let fields = read(0x2c);
        let year_after = read(0x30);
        let fields_after = read(0x2c);
        // Ignore the fractional-second nibble; it may advance independently.
        if year != year_after || fields >> 4 != fields_after >> 4 {
            continue;
        }
        let year = u16::try_from(year.checked_add(1900)?).ok()?;
        let month = ((fields >> 26) & 0x3f) as u8;
        let day = ((fields >> 21) & 0x1f) as u8;
        let hour = ((fields >> 16) & 0x1f) as u8;
        let minute = ((fields >> 10) & 0x3f) as u8;
        let second = ((fields >> 4) & 0x3f) as u8;
        if !(1900..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > month_days(year)[usize::from(month - 1)]
            || hour > 23
            || minute > 59
            || second > 59
        {
            return None;
        }
        let mut days = 0u64;
        for y in 1601..year {
            days += if leap_year(y) { 366 } else { 365 };
        }
        for &count in &month_days(year)[..usize::from(month - 1)] {
            days += u64::from(count);
        }
        days += u64::from(day - 1);
        return Some(TimeFields::new(
            year,
            month,
            day,
            hour,
            minute,
            second,
            ((days + 1) % 7 + 1) as u8,
        ));
    }
    None
}

//! Architecture-agnostic real-time clock interface.
//!
//! Kernel subsystems should call [`query_real_time_clock`] instead of
//! reaching into a platform-specific CMOS driver. Platforms without an
//! RTC report `None`, which callers must treat as "time unavailable"
//! rather than substituting a fake system time.

/// Wall-clock fields returned by [`query_real_time_clock`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimeFields {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    second: u8,
    pub weekday: u8,
}

impl TimeFields {
    pub const fn new(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        weekday: u8,
    ) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            weekday,
        }
    }

    pub const fn second(&self) -> u8 {
        self.second
    }

    pub const fn get_second(&self) -> u8 {
        self.second
    }
}

/// Read the platform clock. Returns `None` when no RTC is available.
pub fn query_real_time_clock() -> Option<TimeFields> {
    backend::query_real_time_clock()
}

/// Program the supported platform RTC. Unsupported dates fail before writing.
/// The supported platforms keep their hardware clocks in UTC.
pub fn set_real_time_clock(time: &TimeFields) -> bool {
    backend::set_real_time_clock(time)
}

pub fn time_fields_to_nt_time(time: &TimeFields) -> Option<u64> {
    super::rtc::time_fields_to_nt_seconds(time)?.checked_mul(10_000_000)
}

pub fn nt_time_to_time_fields(time: u64) -> Option<TimeFields> {
    // Convert through a Gregorian day count without imposing Unix's 1970 floor.
    let seconds = time / 10_000_000;
    if seconds >= 265_046_774_400 {
        return None;
    } // 10000-01-01
    let mut days = seconds / 86_400;
    let weekday = ((days + 1) % 7 + 1) as u8;
    let mut year = 1601u16;
    let leap = |year: u16| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    loop {
        let count = if leap(year) { 366 } else { 365 };
        if days < count {
            break;
        }
        days -= count;
        year += 1;
    }
    let counts = [
        31,
        if leap(year) { 29 } else { 28 },
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
    let mut month = 0usize;
    while days >= counts[month] {
        days -= counts[month];
        month += 1;
    }
    Some(TimeFields::new(
        year,
        (month + 1) as u8,
        (days + 1) as u8,
        (seconds / 3600 % 24) as u8,
        (seconds / 60 % 60) as u8,
        (seconds % 60) as u8,
        weekday,
    ))
}

mod backend {
    #[cfg(target_arch = "x86_64")]
    pub fn set_real_time_clock(time: &super::TimeFields) -> bool {
        crate::hal::x86_64::cmos::hal_set_real_time_clock(
            &crate::hal::x86_64::cmos::TimeFields::new(
                time.year,
                time.month,
                time.day,
                time.hour,
                time.minute,
                time.second(),
                time.weekday,
            ),
        )
    }

    #[cfg(target_arch = "aarch64")]
    pub fn set_real_time_clock(time: &super::TimeFields) -> bool {
        crate::hal::aarch64::rtc::set_real_time_clock(time)
    }

    #[cfg(target_arch = "riscv64")]
    pub fn set_real_time_clock(time: &super::TimeFields) -> bool {
        crate::hal::riscv64::rtc::set_real_time_clock(time)
    }

    #[cfg(target_arch = "loongarch64")]
    pub fn set_real_time_clock(time: &super::TimeFields) -> bool {
        crate::hal::loongarch64::rtc::set_real_time_clock(time)
    }

    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    )))]
    pub fn set_real_time_clock(_: &super::TimeFields) -> bool {
        false
    }
    #[cfg(target_arch = "x86_64")]
    pub fn query_real_time_clock() -> Option<super::TimeFields> {
        crate::hal::x86_64::cmos::hal_query_real_time_clock().map(|time| super::TimeFields {
            year: time.year,
            month: time.month,
            day: time.day,
            hour: time.hour,
            minute: time.minute,
            second: time.second(),
            weekday: time.weekday,
        })
    }

    #[cfg(target_arch = "aarch64")]
    pub fn query_real_time_clock() -> Option<super::TimeFields> {
        crate::hal::aarch64::rtc::query_real_time_clock()
    }

    #[cfg(target_arch = "riscv64")]
    pub fn query_real_time_clock() -> Option<super::TimeFields> {
        crate::hal::riscv64::rtc::query_real_time_clock()
    }

    #[cfg(target_arch = "loongarch64")]
    pub fn query_real_time_clock() -> Option<super::TimeFields> {
        crate::hal::loongarch64::rtc::query_real_time_clock()
    }

    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64"
    )))]
    pub fn query_real_time_clock() -> Option<super::TimeFields> {
        None
    }
}

//! PL031 RTC driver for the supported QEMU ARM `virt` platform.

use core::ptr::read_volatile;

const RTC_BASE: u64 = 0x0901_0000;

pub fn query_real_time_clock() -> Option<crate::hal::common::cmos::TimeFields> {
    crate::hal::common::rtc::unix_seconds_to_time_fields(unsafe {
        read_volatile(RTC_BASE as *const u32)
    } as u64)
}

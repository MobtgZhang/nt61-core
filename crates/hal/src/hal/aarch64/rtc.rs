//! PL031 RTC driver for the supported QEMU ARM `virt` platform.

use crate::hal::common::{cmos::TimeFields, rtc};
use core::ptr::{read_volatile, write_volatile};

const RTC_BASE: u64 = 0x0901_0000;
static RTC_LOCK: spin::Mutex<()> = spin::Mutex::new(());

pub fn query_real_time_clock() -> Option<crate::hal::common::cmos::TimeFields> {
    let _guard = RTC_LOCK.lock();
    crate::hal::common::rtc::unix_seconds_to_time_fields(unsafe {
        read_volatile(RTC_BASE as *const u32)
    } as u64)
}

pub fn set_real_time_clock(time: &TimeFields) -> bool {
    let Some(seconds) =
        rtc::time_fields_to_unix_seconds(time).and_then(|value| u32::try_from(value).ok())
    else {
        return false;
    };
    let _guard = RTC_LOCK.lock();
    // SAFETY: the ARM virt platform maps the PL031 register page. LR is the
    // load register; DR is read-only. DSB completes the device transaction.
    unsafe {
        write_volatile((RTC_BASE + 8) as *mut u32, seconds);
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    true
}

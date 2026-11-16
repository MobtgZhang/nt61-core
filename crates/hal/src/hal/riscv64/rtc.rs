//! Goldfish RTC on the supported QEMU RISC-V `virt` platform.

use crate::hal::common::{cmos::TimeFields, rtc};
use core::ptr::{read_volatile, write_volatile};

const RTC_BASE: u64 = 0x0010_1000;
// TIME_LOW latches a device-global TIME_HIGH, so CPUs must not interleave.
static RTC_LOCK: spin::Mutex<()> = spin::Mutex::new(());

pub fn query_real_time_clock() -> Option<TimeFields> {
    let _guard = RTC_LOCK.lock();
    if !crate::arch::identity_map_region(RTC_BASE, 4096) {
        return None;
    }
    rtc::read_goldfish(|offset| unsafe {
        let value = read_volatile((RTC_BASE as usize + offset) as *const u32);
        // Volatile orders the compiler; FENCE also orders device reads on
        // RISC-V, where reading HIGH before LOW would use an old latch.
        core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
        value
    })
}

pub fn set_real_time_clock(time: &TimeFields) -> bool {
    let _guard = RTC_LOCK.lock();
    if !crate::arch::identity_map_region(RTC_BASE, 4096) {
        return false;
    }
    rtc::write_goldfish(time, |offset, value| {
        // SAFETY: the RTC page is mapped above; the guard serializes both halves.
        unsafe {
            write_volatile((RTC_BASE as usize + offset) as *mut u32, value);
            core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
        }
    })
}

//! LS7A time-of-year RTC on the supported QEMU LoongArch `virt` platform.

use crate::hal::common::{cmos::TimeFields, rtc};
use core::ptr::{read_volatile, write_volatile};

const RTC_BASE: u64 = 0x100d_0100;
const RTC_CONTROL: usize = 0x40;
const TOY_ENABLE: u32 = (1 << 11) | (1 << 8); // TOYEN and oscillator EO
static RTC_LOCK: spin::Mutex<()> = spin::Mutex::new(());

pub fn query_real_time_clock() -> Option<TimeFields> {
    let _guard = RTC_LOCK.lock();
    let base = if crate::arch::loongarch64::paging::paging_enabled() {
        crate::arch::loongarch64::paging::mmio_virt(RTC_BASE)
    } else {
        RTC_BASE
    } as usize;
    // Firmware normally enables the TOY clock. Preserve alarm/RTC controls
    // when enabling it on a machine that boots without that firmware step.
    let control = unsafe { read_volatile((base + RTC_CONTROL) as *const u32) };
    if control & TOY_ENABLE != TOY_ENABLE {
        unsafe {
            write_volatile((base + RTC_CONTROL) as *mut u32, control | TOY_ENABLE);
        }
    }
    rtc::read_ls7a(|offset| unsafe { read_volatile((base + offset) as *const u32) })
}

pub fn set_real_time_clock(time: &TimeFields) -> bool {
    // Validate before touching the oscillator control register.
    if time.year < 1900 || rtc::time_fields_to_nt_seconds(time).is_none() {
        return false;
    }
    let _guard = RTC_LOCK.lock();
    let base = if crate::arch::loongarch64::paging::paging_enabled() {
        crate::arch::loongarch64::paging::mmio_virt(RTC_BASE)
    } else {
        RTC_BASE
    } as usize;
    // SAFETY: the platform RTC page is mapped in the MMIO segment. Preserve
    // alarm controls and enable the time-of-year oscillator before loading it.
    unsafe {
        let control = read_volatile((base + RTC_CONTROL) as *const u32);
        write_volatile((base + RTC_CONTROL) as *mut u32, control | TOY_ENABLE);
        core::arch::asm!("dbar 0", options(nostack, preserves_flags));
    }
    rtc::write_ls7a(time, |offset, value| unsafe {
        // SAFETY: offsets are the two TOY write registers in the mapped page.
        write_volatile((base + offset) as *mut u32, value);
        core::arch::asm!("dbar 0", options(nostack, preserves_flags));
    })
}

//! Execute the native wall-clock transaction and Gregorian conversion in production.
#![allow(dead_code, non_camel_case_types)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[path = "../../hal/src/hal/common/cmos.rs"]
pub mod common_cmos;
#[path = "../../hal/src/hal/common/rtc.rs"]
pub mod common_rtc;
pub use common_cmos as cmos;
pub use common_rtc as rtc;
#[path = "../src/ke/time.rs"]
pub mod kernel_time;
#[path = "../../libs/ntdll/src/ntdll/status.rs"]
pub mod status;
pub mod types {
    pub type NTSTATUS = i32;
}
#[path = "../../libs/ntdll/src/ntdll/wall_clock.rs"]
mod wall_clock;

static TEST_LOCK: Mutex<()> = Mutex::new(());
static DEVICE_OK: AtomicBool = AtomicBool::new(true);
static DEVICE_WRITES: AtomicUsize = AtomicUsize::new(0);
mod privilege {
    std::thread_local! { pub static ENABLED: core::cell::Cell<bool> = const { core::cell::Cell::new(false) }; }
    pub fn enabled(number: u32) -> bool {
        assert_eq!(number, 12);
        ENABLED.with(|value| value.get())
    }
}
mod arch {
    pub enum PlatformTimer {
        Hpet,
        Pit,
        Generic,
    }
    pub fn platform_timer() -> PlatformTimer {
        PlatformTimer::Generic
    }
}
mod hal {
    pub mod serial {
        pub fn write_string(_: &str) {}
    }
    pub mod common {
        pub use crate::common_cmos as cmos;
    }
    pub mod x86_64 {
        pub mod cmos {
            pub use crate::common_cmos::TimeFields;
            pub fn hal_query_real_time_clock() -> Option<TimeFields> {
                None
            }
            pub fn hal_set_real_time_clock(time: &TimeFields) -> bool {
                assert!(crate::common_cmos::time_fields_to_nt_time(time).is_some());
                crate::DEVICE_WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                crate::DEVICE_OK.load(std::sync::atomic::Ordering::Relaxed)
            }
        }
    }
}
mod ke {
    pub use crate::kernel_time as time;
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
        impl<T> Spinlock<T> {
            pub const fn new(value: T) -> Self {
                Self(std::sync::Mutex::new(value))
            }
            pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
                self.0.lock().unwrap()
            }
        }
    }
}
mod mm {
    pub mod user_copy {
        std::thread_local! { pub static FAIL_WRITE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) }; }
        pub fn probe_user_write(address: u64, _: usize) -> Result<(), i32> {
            if address < 4096 {
                Err(crate::status::STATUS_ACCESS_VIOLATION)
            } else {
                Ok(())
            }
        }
        pub fn read_user_object<T: Copy>(address: u64) -> Result<T, i32> {
            probe_user_write(address, core::mem::size_of::<T>())?;
            // SAFETY: test-owned readable object; deliberately accepts four-byte alignment.
            Ok(unsafe { core::ptr::read_unaligned(address as *const T) })
        }
        pub fn write_user_object<T: Copy>(address: u64, value: &T) -> Result<(), i32> {
            probe_user_write(address, core::mem::size_of::<T>())?;
            if FAIL_WRITE.with(|slot| slot.get()) {
                return Err(crate::status::STATUS_ACCESS_VIOLATION);
            }
            // SAFETY: test-owned writable object, after the simulated probe.
            unsafe {
                core::ptr::write_unaligned(address as *mut T, *value);
            }
            Ok(())
        }
    }
}
use status::*;
use wall_clock::nt_set_system_time;
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap();
    privilege::ENABLED.with(|v| v.set(false));
    mm::user_copy::FAIL_WRITE.with(|v| v.set(false));
    DEVICE_OK.store(true, Ordering::Relaxed);
    DEVICE_WRITES.store(0, Ordering::Relaxed);
    kernel_time::set_system_time(116_444_736_000_000_000);
    guard
}
#[test]
fn probe_and_privilege_failures_never_touch_either_clock() {
    let _guard = setup();
    let before = kernel_time::get_time_snapshot();
    let value = 132_271_296_000_000_000i64;
    unsafe {
        assert_eq!(
            nt_set_system_time(&value, core::ptr::null_mut()),
            STATUS_PRIVILEGE_NOT_HELD
        );
        assert_eq!(
            nt_set_system_time(4usize as *const i64, core::ptr::null_mut()),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            nt_set_system_time(&value, 4usize as *mut i64),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(
            nt_set_system_time(5usize as *const i64, core::ptr::null_mut()),
            0x80000002u32 as i32
        );
    }
    assert_eq!(kernel_time::get_time_snapshot(), before);
    assert_eq!(DEVICE_WRITES.load(Ordering::Relaxed), 0);
}
#[test]
fn hardware_failure_leaves_the_software_clock_unchanged() {
    let _guard = setup();
    privilege::ENABLED.with(|v| v.set(true));
    DEVICE_OK.store(false, Ordering::Relaxed);
    let before = kernel_time::get_time_snapshot();
    let value = 132_271_296_000_000_000i64;
    assert_eq!(
        unsafe { nt_set_system_time(&value, core::ptr::null_mut()) },
        STATUS_UNSUCCESSFUL
    );
    assert_eq!(kernel_time::get_time_snapshot(), before);
    assert_eq!(DEVICE_WRITES.load(Ordering::Relaxed), 1);
}
#[test]
fn aliased_input_is_captured_and_uptime_survives_a_backward_adjustment() {
    let _guard = setup();
    privilege::ENABLED.with(|v| v.set(true));
    kernel_time::advance_ticks(13);
    let before = kernel_time::get_time_snapshot();
    let mut value = 123_456_789i64;
    let address = &mut value as *mut i64;
    assert_eq!(
        unsafe { nt_set_system_time(address, address) },
        STATUS_SUCCESS
    );
    let after = kernel_time::get_time_snapshot();
    assert_eq!(value, before.system_time as i64);
    assert_eq!(after.system_time, 123_456_789);
    assert_eq!(after.interrupt_time, before.interrupt_time);
    assert_eq!(
        after.wall_clock_base.wrapping_add(after.interrupt_time),
        after.system_time
    );
}
#[test]
fn failed_previous_time_copy_does_not_roll_back_a_committed_clock() {
    let _guard = setup();
    privilege::ENABLED.with(|v| v.set(true));
    mm::user_copy::FAIL_WRITE.with(|v| v.set(true));
    let value = 132_271_296_000_000_000i64;
    let mut previous = 0;
    assert_eq!(
        unsafe { nt_set_system_time(&value, &mut previous) },
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(kernel_time::get_system_time(), value as u64);
    assert_eq!(previous, 0);
}
#[test]
fn invalid_dates_and_timezone_refresh_are_explicitly_rejected() {
    let _guard = setup();
    privilege::ENABLED.with(|v| v.set(true));
    for value in [-1, 2_650_467_744_000_000_000, i64::MAX] {
        assert_eq!(
            unsafe { nt_set_system_time(&value, core::ptr::null_mut()) },
            STATUS_INVALID_PARAMETER
        );
    }
    assert_eq!(
        unsafe { nt_set_system_time(core::ptr::null(), core::ptr::null_mut()) },
        STATUS_NOT_IMPLEMENTED
    );
    assert_eq!(DEVICE_WRITES.load(Ordering::Relaxed), 0);
}
#[test]
fn gregorian_roundtrip_includes_epoch_century_leap_days_and_last_date() {
    use common_cmos::{nt_time_to_time_fields, time_fields_to_nt_time, TimeFields};
    for fields in [
        TimeFields::new(1601, 1, 1, 0, 0, 0, 2),
        TimeFields::new(1900, 3, 1, 12, 30, 59, 5),
        TimeFields::new(2000, 2, 29, 23, 59, 59, 3),
        TimeFields::new(9999, 12, 31, 23, 59, 59, 6),
    ] {
        let time = time_fields_to_nt_time(&fields).unwrap();
        assert_eq!(nt_time_to_time_fields(time), Some(fields));
        assert_eq!(nt_time_to_time_fields(time + 9_999_999), Some(fields));
    }
    assert_eq!(nt_time_to_time_fields(2_650_467_744_000_000_000), None);
    assert_eq!(
        time_fields_to_nt_time(&TimeFields::new(1900, 2, 29, 0, 0, 0, 1)),
        None
    );
}

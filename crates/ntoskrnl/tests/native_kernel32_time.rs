//! Execute real kernel32/native clock and ke::time sources on the host.
#![allow(dead_code, non_camel_case_types)]
#[path = "native_kernel32_time/kernel32.rs"]
mod kernel32;
#[path = "native_kernel32_time/ntdll.rs"]
pub mod ntdll;
mod libs {
    pub use crate::ntdll;
}
#[path = "native_kernel32_time/ke.rs"]
mod ke;

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
        pub mod cmos {
            pub struct TimeFields {
                pub year: u16,
                pub month: u8,
                pub day: u8,
                pub hour: u8,
                pub minute: u8,
            }
            impl TimeFields {
                pub fn second(&self) -> u8 {
                    0
                }
            }
            pub fn query_real_time_clock() -> Option<TimeFields> {
                None
            }
        }
    }
}
mod mm {
    pub mod user_copy {
        pub fn probe_user_write(_: u64, _: usize) -> Result<(), i32> {
            panic!("trusted wrapper entered user_copy")
        }
        pub fn write_user_object<T: Copy>(_: u64, _: &T) -> Result<(), i32> {
            panic!("trusted wrapper entered user_copy")
        }
    }
}
use kernel32::time::*;
#[test]
fn win32_clock_queries_keep_nt_epoch_uptime_and_native_counter_units() {
    ke::time::init();
    // Constants from UTC calendar dates, including Gregorian leap day and
    // weekdays (Sunday=0). Millisecond truncation retains FILETIME verbatim.
    for (filetime, expected) in [
        (0, (1601, 1, 1, 1, 0, 0, 0, 0)),
        (116_444_736_000_000_000, (1970, 1, 1, 4, 0, 0, 0, 0)),
        (125_962_560_000_000_000, (2000, 2, 29, 2, 0, 0, 0, 0)),
        (134_392_196_961_234_567, (2026, 11, 15, 0, 12, 34, 56, 123)),
    ] {
        ke::time::set_system_time(filetime);
        let mut out = SystemTime::default();
        unsafe { get_system_time(&mut out) };
        assert_eq!(
            (
                out.year,
                out.month,
                out.day,
                out.day_of_week,
                out.hour,
                out.minute,
                out.second,
                out.milliseconds
            ),
            expected
        );
        let mut raw = FileTime::default();
        unsafe { get_system_time_as_file_time(&mut raw) };
        assert_eq!((u64::from(raw.high) << 32) | u64::from(raw.low), filetime);
    }
    ke::time::advance_ticks(1234);
    let (mut counter, mut frequency) = (0, 0);
    assert_eq!(unsafe { query_performance_counter(&mut counter) }, 1);
    assert_eq!(unsafe { query_performance_frequency(&mut frequency) }, 1);
    assert_eq!((counter, frequency), (1234, 1000));
    assert_eq!(get_tick_count64(), 1234);
    assert_eq!(get_tick_count(), 1234);
    assert_eq!(
        crate::libs::ntdll::clock::sample_performance_counter(),
        (1234, 1000)
    );
    ke::time::set_system_time(0);
    assert_eq!(get_tick_count64(), 1234);
    assert_eq!(unsafe { query_performance_counter(&mut counter) }, 1);
    assert_eq!(counter, 1234);
    ke::time::advance_ticks(1000);
    assert_eq!(unsafe { query_performance_counter(&mut counter) }, 1);
    assert_eq!(counter, 2234);
    assert_eq!(get_tick_count64(), 2234);
    assert_eq!(
        unsafe { query_performance_counter(core::ptr::null_mut()) },
        0
    );
    assert_eq!(
        unsafe { query_performance_frequency(core::ptr::null_mut()) },
        0
    );
}

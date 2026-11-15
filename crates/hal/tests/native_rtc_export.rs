//! Host-test the production efiapi wrapper with a deterministic RTC backend.
#![allow(dead_code)]
mod hal {
    pub mod common {
        pub mod cmos {
            #[derive(Clone, Copy)]
            pub struct TimeFields {
                pub year: u16,
                pub month: u8,
                pub day: u8,
                pub hour: u8,
                pub minute: u8,
                pub second: u8,
                pub weekday: u8,
            }
            impl TimeFields {
                pub fn second(&self) -> u8 {
                    self.second
                }
            }
            pub static CLOCK: std::sync::Mutex<Option<TimeFields>> = std::sync::Mutex::new(None);
            pub fn query_real_time_clock() -> Option<TimeFields> {
                *CLOCK.lock().unwrap()
            }
        }
    }
}
#[path = "../src/hal/native_exports.rs"]
mod api;

#[test]
fn time_fields_abi_calendar_weekday_failure_and_x64_half_second_match_reference() {
    use hal::common::cmos::{TimeFields, CLOCK};
    assert_eq!(core::mem::size_of::<api::TimeFields>(), 16);
    assert_eq!(core::mem::offset_of!(api::TimeFields, milliseconds), 12);
    assert_eq!(core::mem::offset_of!(api::TimeFields, weekday), 14);
    let query: unsafe extern "efiapi" fn(*mut api::TimeFields) -> u8 =
        api::nt61_export_HalQueryRealTimeClock;
    let mut output = api::TimeFields {
        year: 123,
        ..Default::default()
    };
    unsafe {
        assert_eq!(query(&mut output), 0);
        assert_eq!(output.year, 123);
        assert_eq!(query(core::ptr::null_mut()), 0);
        *CLOCK.lock().unwrap() = Some(TimeFields {
            year: 2024,
            month: 2,
            day: 29,
            hour: 23,
            minute: 59,
            second: 58,
            weekday: 5,
        });
        assert_eq!(query(&mut output), 1);
        assert_eq!(
            (
                output.year,
                output.month,
                output.day,
                output.hour,
                output.minute,
                output.second,
                output.weekday
            ),
            (2024, 2, 29, 23, 59, 58, 4)
        );
        assert_eq!(
            output.milliseconds,
            if cfg!(target_arch = "x86_64") { 500 } else { 0 }
        );
        let valid = output;
        for (year, month, day, weekday) in [
            (2023, 2, 29, 5),
            (2100, 2, 29, 1),
            (2024, 4, 31, 1),
            (2024, 0, 1, 1),
            (2024, 1, 1, 0),
            (2024, 1, 1, 8),
            (1600, 1, 1, 1),
        ] {
            *CLOCK.lock().unwrap() = Some(TimeFields {
                year,
                month,
                day,
                weekday,
                hour: 0,
                minute: 0,
                second: 0,
            });
            assert_eq!(query(&mut output), 0);
            assert_eq!(output, valid);
        }
        *CLOCK.lock().unwrap() = Some(TimeFields {
            year: 2024,
            month: 3,
            day: 3,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 1,
        });
        assert_eq!(query(&mut output), 1);
        assert_eq!(output.weekday, 0);
    }
}

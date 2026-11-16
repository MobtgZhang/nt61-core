//! Protocol simulations execute the production RTC calendar/MMIO logic.
//! They do not access device memory or prove real hardware operation.
mod cmos {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        pub const fn second(&self) -> u8 {
            self.second
        }
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
    }
}
#[path = "../src/hal/common/rtc.rs"]
mod rtc;
use cmos::TimeFields as Time;
const CALENDARS: [(u64, Time); 7] = [
    (0, Time::new(1970, 1, 1, 0, 0, 0, 5)),
    (131_696, Time::new(1970, 1, 2, 12, 34, 56, 6)),
    (951_868_799, Time::new(2000, 2, 29, 23, 59, 59, 3)),
    (951_868_800, Time::new(2000, 3, 1, 0, 0, 0, 4)),
    (4_107_542_399, Time::new(2100, 2, 28, 23, 59, 59, 1)),
    (4_107_542_400, Time::new(2100, 3, 1, 0, 0, 0, 2)),
    (253_402_300_799, Time::new(9999, 12, 31, 23, 59, 59, 6)),
];

#[test]
fn calendar_encoding_roundtrips_and_rejects_invalid_input_before_writes() {
    for (seconds, time) in CALENDARS {
        assert_eq!(rtc::time_fields_to_unix_seconds(&time), Some(seconds));
        assert_eq!(
            rtc::time_fields_to_nt_seconds(&time),
            Some(seconds + 11_644_473_600)
        );
    }
    let mut writes = Vec::new();
    for time in [
        Time::new(2100, 2, 29, 0, 0, 0, 1),
        Time::new(2000, 0, 1, 0, 0, 0, 1),
        Time::new(2000, 1, 0, 0, 0, 0, 1),
    ] {
        assert!(!rtc::write_goldfish(&time, |a, v| writes.push((a, v))));
        assert!(!rtc::write_ls7a(&time, |a, v| writes.push((a, v))));
    }
    assert!(writes.is_empty());
}

#[test]
fn goldfish_write_encodes_nanoseconds_high_then_low_and_rejects_overflow() {
    for (seconds, time) in CALENDARS {
        let mut writes = Vec::new();
        let result = rtc::write_goldfish(&time, |a, v| writes.push((a, v)));
        if seconds > i64::MAX as u64 / 1_000_000_000 {
            assert!(!result);
            assert!(writes.is_empty());
        } else {
            let nanos = seconds * 1_000_000_000;
            assert!(result);
            assert_eq!(writes, [(4, (nanos >> 32) as u32), (0, nanos as u32)]);
        }
    }
}

#[test]
fn ls7a_write_uses_year_before_calendar_and_never_writes_read_registers() {
    let time = Time::new(2000, 2, 29, 23, 59, 58, 3);
    let mut writes = Vec::new();
    assert!(rtc::write_ls7a(&time, |a, v| writes.push((a, v))));
    assert_eq!(writes, [(0x28, 100), (0x24, fields(time))]);
    writes.clear();
    assert!(!rtc::write_ls7a(
        &Time::new(1899, 1, 1, 0, 0, 0, 1),
        |a, v| writes.push((a, v))
    ));
    assert!(writes.is_empty());
}
fn fields(time: Time) -> u32 {
    u32::from(time.month) << 26
        | u32::from(time.day) << 21
        | u32::from(time.hour) << 16
        | u32::from(time.minute) << 10
        | u32::from(time.second) << 4
}
fn read_ls7a_script(script: &[(usize, u32)]) -> (Option<Time>, usize) {
    let mut calls = 0;
    let result = rtc::read_ls7a(|offset| {
        let &(expected, value) = script.get(calls).expect("bounded register sampling");
        assert_eq!(offset, expected, "register read order at sample {calls}");
        calls += 1;
        value
    });
    (result, calls)
}
fn stable_ls7a(time: Time) -> Option<Time> {
    let year = u32::from(time.year) - 1900;
    let value = fields(time);
    let (result, calls) =
        read_ls7a_script(&[(0x30, year), (0x2c, value), (0x30, year), (0x2c, value)]);
    assert_eq!(calls, 4);
    result
}
#[test]
fn unix_calendar_matches_epoch_weekdays_leap_century_and_last_nt_date() {
    // Fixed Gregorian/UTC vectors, independently checked with Python datetime.
    // Weekdays use the HAL internal convention Sunday=1 .. Saturday=7.
    for (seconds, expected) in CALENDARS {
        assert_eq!(rtc::unix_seconds_to_time_fields(seconds), Some(expected));
    }
    assert_eq!(rtc::unix_seconds_to_time_fields(253_402_300_800), None);
    assert_eq!(rtc::unix_seconds_to_time_fields(u64::MAX), None);
}
#[test]
fn goldfish_latches_high_by_reading_low_first_and_discards_fractional_nanoseconds() {
    for (seconds, expected) in CALENDARS
        .into_iter()
        .filter(|(seconds, _)| *seconds < 10_000_000_000)
    {
        let nanoseconds = seconds * 1_000_000_000 + 987_654_321;
        let mut order = Vec::new();
        let result = rtc::read_goldfish(|offset| {
            order.push(offset);
            match offset {
                0 => nanoseconds as u32,
                4 => (nanoseconds >> 32) as u32,
                _ => panic!("unexpected register"),
            }
        });
        assert_eq!(order, [0, 4]);
        assert_eq!(result, Some(expected));
    }
}
#[test]
fn goldfish_signed_negative_values_are_rejected_without_extra_reads() {
    for nanoseconds in [1u64 << 63, u64::MAX, 0x8000000000000001] {
        let mut calls = 0;
        assert_eq!(
            rtc::read_goldfish(|offset| {
                assert_eq!(offset, if calls == 0 { 0 } else { 4 });
                calls += 1;
                if offset == 0 {
                    nanoseconds as u32
                } else {
                    (nanoseconds >> 32) as u32
                }
            }),
            None
        );
        assert_eq!(calls, 2);
    }
}
#[test]
fn ls7a_calendar_matches_real_dates_and_weekdays_including_9999() {
    for (_, expected) in CALENDARS {
        assert_eq!(stable_ls7a(expected), Some(expected));
    }
    assert_eq!(
        stable_ls7a(Time::new(1900, 1, 1, 0, 0, 0, 2)),
        Some(Time::new(1900, 1, 1, 0, 0, 0, 2))
    );
}
#[test]
fn ls7a_retries_year_rollover_then_returns_a_coherent_new_year() {
    let old = Time::new(1999, 12, 31, 23, 59, 59, 6);
    let new = Time::new(2000, 1, 1, 0, 0, 0, 7);
    let script = [
        (0x30, 99),
        (0x2c, fields(old)),
        (0x30, 100),
        (0x2c, fields(new)),
        (0x30, 100),
        (0x2c, fields(new)),
        (0x30, 100),
        (0x2c, fields(new)),
    ];
    let (result, calls) = read_ls7a_script(&script);
    assert_eq!(calls, 8);
    assert_eq!(result, Some(new));
}
#[test]
fn ls7a_retries_changed_seconds_but_ignores_independent_fraction_nibble() {
    let before = Time::new(2000, 2, 29, 12, 34, 58, 3);
    let after = Time::new(2000, 2, 29, 12, 34, 59, 3);
    let script = [
        (0x30, 100),
        (0x2c, fields(before)),
        (0x30, 100),
        (0x2c, fields(after)),
        (0x30, 100),
        (0x2c, fields(after) | 1),
        (0x30, 100),
        (0x2c, fields(after) | 14),
    ];
    let (result, calls) = read_ls7a_script(&script);
    assert_eq!(calls, 8);
    assert_eq!(result, Some(after));
}
#[test]
fn ls7a_unstable_hardware_fails_after_exactly_eight_samples() {
    let value = fields(Time::new(2000, 1, 1, 0, 0, 0, 7));
    for unstable_year in [false, true] {
        let mut script = Vec::new();
        for _ in 0..8 {
            script.extend([
                (0x30, 100),
                (0x2c, value),
                (0x30, if unstable_year { 101 } else { 100 }),
                (0x2c, if unstable_year { value } else { value + 16 }),
            ]);
        }
        let (result, calls) = read_ls7a_script(&script);
        assert_eq!(result, None);
        assert_eq!(calls, 32);
    }
}
#[test]
fn ls7a_rejects_impossible_dates_times_and_out_of_range_years() {
    let valid = Time::new(2000, 2, 29, 23, 59, 59, 3);
    for time in [
        Time {
            year: 2100,
            ..valid
        },
        Time {
            year: 1900,
            ..valid
        },
        Time { month: 0, ..valid },
        Time { month: 13, ..valid },
        Time { day: 0, ..valid },
        Time {
            month: 4,
            day: 31,
            ..valid
        },
        Time { hour: 24, ..valid },
        Time {
            minute: 60,
            ..valid
        },
        Time {
            second: 60,
            ..valid
        },
        Time {
            year: 10000,
            ..valid
        },
    ] {
        assert_eq!(stable_ls7a(time), None, "{time:?}");
    }
    for year in [u32::MAX, u32::MAX - 1899, 65536 - 1900] {
        let value = fields(valid);
        let (result, calls) =
            read_ls7a_script(&[(0x30, year), (0x2c, value), (0x30, year), (0x2c, value)]);
        assert_eq!(result, None);
        assert_eq!(calls, 4);
    }
}

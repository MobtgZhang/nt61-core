//! Exercise the shipping Win32 restart logic with interrupted native waits.
use std::{cell::RefCell, collections::VecDeque};

#[path = "../../libs/kernel32/src/kernel32/sleep.rs"]
mod sleep;

#[derive(Default)]
struct State {
    uptime: u64,
    completions: VecDeque<(u64, i32)>,
    intervals: Vec<(u8, i64)>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

mod ke {
    pub mod time {
        pub struct TimeSnapshot { pub interrupt_time: u64 }
        pub fn get_time_snapshot() -> TimeSnapshot {
            TimeSnapshot { interrupt_time: crate::STATE.with(|s| s.borrow().uptime) }
        }
    }
}
mod libs {
    pub mod ntdll {
        pub mod sync {
            pub fn nt_delay_execution_kernel(alertable: u8, interval: i64) -> i32 {
                crate::STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.intervals.push((alertable, interval));
                    let (elapsed, status) = s.completions.pop_front().expect("unexpected extra wait");
                    s.uptime += elapsed;
                    status
                })
            }
        }
    }
}

#[test]
fn alerts_preserve_the_original_deadline_and_infinite_delay() {
    let cases: &[(u32, bool, &[(u64, i32)], &[(u8, i64)], u32)] = &[
        (10, false, &[(30_000, 0x101), (40_000, 0x101), (30_000, 0)],
            &[(0, -100_000), (0, -70_000), (0, -30_000)], 0),
        (10, true, &[(200_000, 0x101)], &[(1, -100_000)], 0),
        (10, true, &[(2_000, 0xc0)], &[(1, -100_000)], 0xc0),
        (u32::MAX, true, &[(u64::from(u32::MAX) * 10_000 + 1, 0x101), (1, 0xc0)],
            &[(1, i64::MIN), (1, i64::MIN)], 0xc0),
    ];
    for &(milliseconds, alertable, completions, expected, result) in cases {
        STATE.with(|s| *s.borrow_mut() = State {
            completions: completions.iter().copied().collect(),
            ..State::default()
        });
        assert_eq!(sleep::sleep_ex(milliseconds, alertable), result);
        STATE.with(|s| {
            let s = s.borrow();
            assert_eq!(s.intervals, expected);
            assert!(s.completions.is_empty());
        });
    }
}

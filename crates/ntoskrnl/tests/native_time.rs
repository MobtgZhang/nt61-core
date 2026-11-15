//! Exercise production software-clock state and epoch conversion with a host
//! lock in place of the IRQ-safe kernel lock; no copy of the clock logic.
#![allow(dead_code)]
mod ke {
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
                Some(TimeFields {
                    year: 1970,
                    month: 1,
                    day: 1,
                    hour: 0,
                    minute: 0,
                })
            }
        }
    }
}
#[path = "../src/ke/time.rs"]
mod time;

#[test]
fn wall_clock_set_and_ticks_publish_coherent_state_without_moving_uptime() {
    time::init();
    assert_eq!(time::get_system_time(), 116_444_736_000_000_000);
    assert_eq!(time::get_unix_time(), 0);
    assert_eq!(time::get_tick_count(), 0);
    time::advance_ticks(123);
    assert_eq!(time::get_tick_count(), 123);
    let before = time::get_time_snapshot();
    let new_time = time::EPOCH_DIFF_100NS + 42 * 10_000_000;
    assert_eq!(time::set_system_time(new_time), before.system_time);
    let after = time::get_time_snapshot();
    assert_eq!(after.system_time, new_time);
    assert_eq!(after.interrupt_time, before.interrupt_time);
    assert_eq!(
        after.wall_clock_base.wrapping_add(after.interrupt_time),
        new_time
    );
    assert_eq!(time::get_unix_time(), 42);
    assert_eq!(time::get_tick_count(), 123);
    time::advance_tick();
    let ticked = time::get_time_snapshot();
    assert_eq!(ticked.system_time, new_time + 10_000);
    assert_eq!(ticked.interrupt_time, before.interrupt_time + 10_000);

    // Setting an early clock value must preserve arithmetic coherence too.
    time::set_system_time(1);
    assert_eq!(time::get_tick_count(), 124);
    let early = time::get_time_snapshot();
    assert_eq!(early.wall_clock_base.wrapping_add(early.interrupt_time), 1);
    assert_eq!(time::get_unix_time(), 0);

    // The same production lock serializes ISR-style ticks and setters from
    // different CPUs. Every observed triple must come from one commit.
    let setter = std::thread::spawn(|| {
        for index in 0..5000 {
            time::set_system_time(time::EPOCH_DIFF_100NS + index * 1_000_000);
        }
    });
    let ticker = std::thread::spawn(|| {
        for _ in 0..5000 {
            time::advance_tick();
        }
    });
    for _ in 0..5000 {
        let snapshot = time::get_time_snapshot();
        assert_eq!(
            snapshot
                .wall_clock_base
                .wrapping_add(snapshot.interrupt_time),
            snapshot.system_time
        );
    }
    setter.join().unwrap();
    ticker.join().unwrap();
    assert_eq!(
        time::get_interrupt_time(),
        early.interrupt_time + 5000 * 10_000
    );
}

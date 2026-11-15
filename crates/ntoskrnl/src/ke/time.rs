//! Time Management
//
//! System time and timers. The kernel tracks three time scales:
//
//!   * **Tick count** — the millisecond counter returned by
//!     `get_tick_count()`. This is the basis for most timer-queue
//!     math; the bootstrap increments it from the clock ISR.
//!   * **System time** — 100-ns intervals since 1601-01-01
//!     (the NT epoch). The HAL queries the RTC for the boot time
//!     and then the tick counter drives it forward.
//!   * **Interrupt time** — 100-ns intervals since boot, used by
//!     ISR-time code that needs a fast monotonic clock.
//
//!     The software counters advance at the configured clock interrupt rate;
//!     the source label records which platform timer supplies those interrupts.

use crate::ke::sync::Spinlock;
use core::sync::atomic::{AtomicU64, Ordering};

/// Tick frequency in Hz. The PIT/HAL is programmed to this rate.
pub const TICKS_PER_SECOND: u32 = 1000;
/// 100-ns intervals per millisecond. NT epoch math.
pub const HUNDRED_NS_PER_MS: u64 = 10_000;
/// Number of 100-ns intervals between 1601-01-01 and 1970-01-01.
pub const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
/// Number of milliseconds in one day (24*60*60*1000).
pub const MS_PER_DAY: u64 = 86_400_000;

/// Software wall-clock value in 100-ns NT epoch units.
static SYSTEM_TIME: AtomicU64 = AtomicU64::new(0);
static WALL_CLOCK_BASE: AtomicU64 = AtomicU64::new(0);
static INTERRUPT_TIME: AtomicU64 = AtomicU64::new(0);
/// Counter for the number of times `ke::time::advance_tick` was
/// called. Used by the smoke test to verify the timer ISR is
/// running.
static ADVANCE_COUNT: AtomicU64 = AtomicU64::new(0);
/// IRQ-safe lock serializes clock interrupts and wall-clock adjustments across
/// CPUs. No device I/O or user-memory access occurs while this lock is held.
static CLOCK_UPDATE: Spinlock<()> = Spinlock::new(());

/// Coherent software-clock state; separate getter calls may straddle a set/tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSnapshot {
    pub system_time: u64,
    pub wall_clock_base: u64,
    pub interrupt_time: u64,
}

pub fn get_time_snapshot() -> TimeSnapshot {
    let _guard = CLOCK_UPDATE.lock();
    TimeSnapshot {
        system_time: SYSTEM_TIME.load(Ordering::Relaxed),
        wall_clock_base: WALL_CLOCK_BASE.load(Ordering::Relaxed),
        interrupt_time: INTERRUPT_TIME.load(Ordering::Relaxed),
    }
}

/// Atomically publish a new wall-clock value and its corresponding boot base.
/// Returns the software time at the commit point. Interrupt time and QPC never
/// move when setting the wall clock. Base subtraction uses the same modulo-u64
/// arithmetic as the tick counters so even a value earlier than uptime remains
/// coherent: base.wrapping_add(interrupt_time) == system_time.
///
/// Call only after the external RTC update succeeds. The RTC transaction itself
/// is outside this short critical section; queries can observe the old software
/// clock during that I/O. NMI callers must not acquire this lock.
pub fn set_system_time(value: u64) -> u64 {
    let _guard = CLOCK_UPDATE.lock();
    let previous = SYSTEM_TIME.load(Ordering::Relaxed);
    let uptime = INTERRUPT_TIME.load(Ordering::Relaxed);
    WALL_CLOCK_BASE.store(value.wrapping_sub(uptime), Ordering::Relaxed);
    SYSTEM_TIME.store(value, Ordering::Relaxed);
    previous
}
/// Source of time — chosen during `init()`.
static mut TIME_SOURCE: TimeSource = TimeSource::Stub;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSource {
    Stub,
    Tsc,
    Hpet,
    Pit,
}

/// Pick the best available time source. The HAL is the source of
/// truth on x86_64; we ask it for a HPET / PIT preference.
fn pick_time_source() -> TimeSource {
    match crate::arch::platform_timer() {
        crate::arch::PlatformTimer::Hpet => TimeSource::Hpet,
        crate::arch::PlatformTimer::Pit => TimeSource::Pit,
        crate::arch::PlatformTimer::Generic => TimeSource::Tsc,
    }
}

/// Initialize time subsystem.
pub fn init() {
    crate::hal::serial::write_string("[ke.time] enter\r\n");
    let wall_clock = wall_clock_100ns();
    let _guard = CLOCK_UPDATE.lock();
    WALL_CLOCK_BASE.store(wall_clock, Ordering::SeqCst);
    SYSTEM_TIME.store(wall_clock, Ordering::SeqCst);
    INTERRUPT_TIME.store(0, Ordering::SeqCst);
    ADVANCE_COUNT.store(0, Ordering::SeqCst);
    let source = pick_time_source();
    unsafe { TIME_SOURCE = source };
    // // kprintln!(
    // //         "    Time: source={:?} freq={}Hz ({}ms/tick)",
    // //         source, TICKS_PER_SECOND, 1000 / TICKS_PER_SECOND
    // //     );
    // // kprintln!("    Time: tick_count=0 system_time=0 interrupt_time=0");
}

/// Read the platform RTC and convert it to NT epoch 100-ns intervals.
/// Returns zero when the platform has no RTC or reports an invalid date.
fn wall_clock_100ns() -> u64 {
    let Some(time) = crate::hal::common::cmos::query_real_time_clock() else {
        return 0;
    };
    if !(1601..=9999).contains(&(time.year as u64))
        || !(1..=12).contains(&time.month)
        || !(1..=31).contains(&time.day)
    {
        return 0;
    }
    fn is_leap(year: u16) -> bool {
        (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
    }
    let mut days = 0u64;
    for year in 1601..time.year {
        days += if is_leap(year) { 366 } else { 365 };
    }
    let month_days = if is_leap(time.year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for month in 1..time.month {
        days += month_days[(month - 1) as usize] as u64;
    }
    days += time.day as u64 - 1;
    let seconds_of_day = time.hour as u64 * 3_600 + time.minute as u64 * 60 + time.second() as u64;
    (days * 86_400 + seconds_of_day) * 10_000_000
}

/// Advance the time counters by one tick. Called from the clock
/// ISR; the smoke test can also drive it directly to verify the
/// math.
pub fn advance_tick() {
    let _guard = CLOCK_UPDATE.lock();
    let _ = SYSTEM_TIME.fetch_add(HUNDRED_NS_PER_MS, Ordering::Relaxed);
    let _ = INTERRUPT_TIME.fetch_add(HUNDRED_NS_PER_MS, Ordering::Relaxed);
    ADVANCE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Advance the time counters by `n` ticks. Used by the smoke test
/// to fast-forward without spinning.
pub fn advance_ticks(n: u64) {
    let _guard = CLOCK_UPDATE.lock();
    let delta = n * HUNDRED_NS_PER_MS;
    SYSTEM_TIME.fetch_add(delta, Ordering::Relaxed);
    INTERRUPT_TIME.fetch_add(delta, Ordering::Relaxed);
    ADVANCE_COUNT.fetch_add(n, Ordering::Relaxed);
}

/// Get current system time in 100ns intervals since 1601-01-01.
pub fn get_system_time() -> u64 {
    get_time_snapshot().system_time
}

/// Get system time as seconds since 1970-01-01; clamp pre-Unix dates to zero.
pub fn get_unix_time() -> u64 {
    let st = get_system_time();
    if st < EPOCH_DIFF_100NS {
        0
    } else {
        (st - EPOCH_DIFF_100NS) / 10_000_000
    }
}

/// Wall-clock boot base in 100-ns intervals since the NT epoch.
pub fn get_wall_clock_base() -> u64 {
    get_time_snapshot().wall_clock_base
}

/// Get current interrupt time in 100ns intervals since boot.
pub fn get_interrupt_time() -> u64 {
    get_time_snapshot().interrupt_time
}

/// Get tick count (ms since boot).
pub fn get_tick_count() -> u32 {
    (get_interrupt_time() / HUNDRED_NS_PER_MS) as u32
}

/// Time source chosen at init.
pub fn source() -> TimeSource {
    unsafe { TIME_SOURCE }
}

/// Number of times `advance_tick` has been called.
pub fn advance_count() -> u64 {
    ADVANCE_COUNT.load(Ordering::Relaxed)
}

/// Smoke test for the time subsystem.
///
/// Drives `advance_ticks` a few times and checks the math
/// (`get_tick_count`, `get_system_time`, `get_interrupt_time`).
pub fn smoke_test() -> bool {
    let before = get_tick_count();
    let adv_before = advance_count();
    let st_before = get_system_time();
    let it_before = get_interrupt_time();
    advance_ticks(7);
    let after = get_tick_count();
    let adv_after = advance_count();
    let st_after = get_system_time();
    let it_after = get_interrupt_time();

    if after != before + 7 {
        // // kprintln!(
        // //             "    [TIME SMOKE FAIL] tick_count: {} -> {}",
        // //             before, after
        // //         );
        return false;
    }
    if adv_after != adv_before + 7 {
        // // kprintln!(
        // //             "    [TIME SMOKE FAIL] advance_count: {} -> {}",
        // //             adv_before, adv_after
        // //         );
        return false;
    }
    if st_after != st_before + 7 * HUNDRED_NS_PER_MS {
        // // kprintln!("    [TIME SMOKE FAIL] system_time math");
        return false;
    }
    if it_after != it_before + 7 * HUNDRED_NS_PER_MS {
        // // kprintln!("    [TIME SMOKE FAIL] interrupt_time math");
        return false;
    }
    // // kprintln!(
    // //         "    [TIME SMOKE OK] tick_count={} system_time={}us interrupt_time={}us",
    // //         after,
    // //         st_after / 10,
    // //         it_after / 10
    // //     );
    true
}

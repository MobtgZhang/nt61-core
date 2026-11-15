//! Use the interrupt-driven monotonic kernel clock on every architecture.
//! The current clock ISR advances by 1 ms, so expose actual millisecond counts
//! with a matching 1000 Hz frequency. This avoids pairing uncalibrated hardware
//! counters with guessed rates and requires no additional privileged reads.
//! Resolution and progress depend on delivery of the platform clock interrupt.

pub(super) fn sample() -> (u64, u64) {
    (crate::ke::time::get_interrupt_time() / 10_000, 1000)
}

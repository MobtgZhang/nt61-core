//! ARM Generic Timer driver for AArch64.
//!
//! Provides a 1 kHz monotonic timer that the kernel uses for ticks,
//! timeouts, and the HAL smoke tests. The driver depends on the
//! platform providing:
//!
//! * `CNTFRQ_EL0` — the fixed clock rate of the system counter.
//! * `CNTPCT_EL0` — the physical count value.
//! * `CNTV_CTL_EL0` / `CNTV_TVAL_EL0` — the virtual timer.
//!
//! In SMP systems each CPU has its own timer; we currently configure
//! the BSP timer only and leave secondary timers to the SMP bring-up
//! path.

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::arch::aarch64::soc;

pub const VIRTUAL_TIMER_IRQ: u32 = 27;
static COUNTS_PER_TICK: AtomicU64 = AtomicU64::new(0);

/// Returns the system counter frequency in Hz.
pub fn counter_freq() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, CNTFRQ_EL0", out(reg) v, options(nostack)) };
    v
}

/// Returns the current physical counter value.
pub fn counter() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, CNTPCT_EL0", out(reg) v, options(nostack)) };
    v
}

/// Start the boot CPU's clock after the executive is ready for ticks.
pub fn start_clock() {
    let counts = counter_freq() / crate::ke::time::TICKS_PER_SECOND as u64;
    if counts == 0 || counts > i32::MAX as u64 {
        return;
    }
    if !crate::hal::aarch64::apic::enable_virtual_timer_irq() {
        return;
    }
    COUNTS_PER_TICK.store(counts, Ordering::Relaxed);
    arm_next_tick();
}

/// Clear the timer IRQ line before the interrupt controller signals EOI.
pub fn mask_irq() {
    unsafe {
        asm!("msr CNTV_CTL_EL0, {control}", "isb", control = in(reg) 3u64, options(nostack));
    }
}

/// Arm the timer after clock work so overdue ticks cannot starve interrupted code.
pub fn arm_next_tick() {
    let counts = COUNTS_PER_TICK.load(Ordering::Relaxed);
    unsafe {
        asm!(
            "msr CNTV_TVAL_EL0, {counts}",
            "msr CNTV_CTL_EL0, {control}",
            "isb",
            counts = in(reg) counts,
            control = in(reg) 1u64,
            options(nostack),
        );
    }
}

/// Monotonic milliseconds since boot.
pub fn time_ms() -> u64 {
    let c = counter();
    let f = counter_freq();
    if f == 0 {
        return 0;
    }
    c * 1000 / f
}

/// Boot timestamp captured during HAL initialization.
static BOOT_MS: AtomicU64 = AtomicU64::new(0);

/// Convenience: capture the boot time.
pub fn mark_boot() {
    BOOT_MS.store(time_ms(), Ordering::Relaxed);
}

/// Return the boot timestamp in milliseconds.
pub fn boot_ms() -> u64 {
    BOOT_MS.load(Ordering::Relaxed)
}

/// Smoke test: validate that `CNTPCT_EL0` advances.
pub fn smoke_test() -> bool {
    let a = counter();
    // Tight loop to wait for at least one tick.
    let mut b = a;
    for _ in 0..1024 {
        b = counter();
        if b != a {
            break;
        }
    }
    b > a
}

/// Pick the right timer backend based on the SoC descriptor. Future
/// release will wire this through `soc::TimerType::SoCTimer`.
pub fn timer_type_for_soc() -> soc::TimerType {
    soc::TimerType::GenericTimer
}

//! LoongArch64 constant timer.
use core::arch::asm;

pub fn init() {
    const ENABLE_PERIODIC: u64 = 0b11;
    let base_frequency = cpu_config(4) as u64;
    let ratio = cpu_config(5);
    let multiplier = u64::from(ratio & 0xffff);
    let divisor = u64::from(ratio >> 16);
    let Some(frequency) = (base_frequency * multiplier).checked_div(divisor) else {
        return;
    };
    let interval = (frequency / u64::from(crate::ke::time::TICKS_PER_SECOND)) & !0b11;
    if interval == 0 {
        return;
    }
    unsafe {
        asm!("csrwr {value}, 0x44", value = inout(reg) 1u64 => _, options(nostack));
        asm!("csrwr {value}, 0x41", value = inout(reg) interval | ENABLE_PERIODIC => _, options(nostack));
    }
}

fn cpu_config(index: u32) -> u32 {
    let value: u64;
    unsafe {
        asm!("cpucfg {value}, {index}", value = out(reg) value, index = in(reg) u64::from(index), options(nostack));
    }
    value as u32
}

pub fn read_count() -> u64 {
    let value: u64;
    unsafe { asm!("rdtime.d {value}, $zero", value = out(reg) value, options(nostack)); }
    value
}

/// Approximate busy-wait loop used by Phase-1 syscalls.
///
/// `intervals` is in 100-ns units (the NT `NT_DELAY_EXECUTION`
/// argument). We don't have a calibrated loop here, so we just spin
/// `intervals` times — callers should treat the result as
/// "definitely long enough, possibly much longer".
pub fn busy_wait_100ns(intervals: u64) {
    // 1 iteration ≈ a handful of cycles; 1000 iterations ≈ a few
    // microseconds on 3A6000. Scale accordingly.
    let iters = intervals.saturating_mul(10);
    for _ in 0..iters {
        core::hint::spin_loop();
    }
}

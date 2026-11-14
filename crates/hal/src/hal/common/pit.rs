//! Architecture-agnostic timer support.
//
//! This module provides a compatible interface to the platform timer.
//! On x86_64, it re-exports the PIT implementation.
//! Other platforms use their architectural monotonic counters.

#[cfg(target_arch = "x86_64")]
pub use crate::hal::x86_64::pit::*;

#[cfg(target_arch = "aarch64")]
mod aarch64_timer {
    use crate::hal::aarch64::timer;

    pub fn get_ticks() -> u64 {
        timer::time_ms()
    }

    pub fn increment_ticks() {}

    pub fn get_system_time_ms() -> u64 {
        timer::time_ms()
    }

    pub fn get_system_time_us() -> u64 {
        timer::time_ms().saturating_mul(1000)
    }
}

#[cfg(target_arch = "aarch64")]
pub use aarch64_timer::*;

#[cfg(target_arch = "riscv64")]
mod riscv_timer {
    pub fn get_ticks() -> u64 {
        crate::arch::riscv64::pit::time()
    }

    pub fn increment_ticks() {}

    pub fn get_system_time_ms() -> u64 {
        get_ticks() / crate::arch::riscv64::pit::TIMER_TICKS_PER_MS
    }

    pub fn get_system_time_us() -> u64 {
        get_ticks() / 10
    }
}

#[cfg(target_arch = "riscv64")]
pub use riscv_timer::*;

#[cfg(target_arch = "loongarch64")]
mod loongarch_timer {
    /// The stable counter runs independently of interrupt delivery.
    pub fn get_ticks() -> u64 {
        let ticks: u64;
        // SAFETY: RDTIME is available at kernel privilege on LoongArch64.
        unsafe {
            core::arch::asm!("rdtime.d {}, $zero", out(reg) ticks, options(nomem, nostack));
        }
        ticks
    }

    pub fn increment_ticks() {}

    fn frequency() -> u64 {
        // SAFETY: CPUCFG words 4/5 report the stable-counter clock and ratio.
        let (base, ratio) = unsafe {
            (
                crate::arch::loongarch64::soc::cpucfg_read(4),
                crate::arch::loongarch64::soc::cpucfg_read(5),
            )
        };
        let multiplier = ratio & 0xffff;
        let divisor = (ratio >> 16) & 0xffff;
        if base == 0 || multiplier == 0 || divisor == 0 {
            return 100_000_000; // QEMU virt stable-counter default.
        }
        base.saturating_mul(multiplier) / divisor
    }

    pub fn get_system_time_ms() -> u64 {
        (get_ticks() as u128 * 1000 / frequency().max(1) as u128) as u64
    }

    pub fn get_system_time_us() -> u64 {
        (get_ticks() as u128 * 1_000_000 / frequency().max(1) as u128) as u64
    }
}

#[cfg(target_arch = "loongarch64")]
pub use loongarch_timer::*;

//! HPET (High Precision Event Timer)
//
//! The HPET is a 64-bit counter with at least 3 compare registers,
//! specified by the IA-PC HPET specification. We parse the ACPI
//! HPET table to discover the MMIO base, map it through
//! `mm::syspte::map_io_space`, and expose a high-resolution
//! performance counter that maps directly to the
//! `query_performance_counter` / `query_performance_frequency` Win32
//! API.

#![cfg(target_arch = "x86_64")]

use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

const HPET_REG_CAP: u64 = 0x00;
const HPET_REG_CFG: u64 = 0x10;
const HPET_REG_COUNTER: u64 = 0xF0;

const HPET_CFG_ENABLE: u32 = 1 << 0;
const HPET_CFG_LEGACY: u32 = 1 << 1;

static HPET_VA: AtomicU64 = AtomicU64::new(0);
static HPET_FREQ_HZ: AtomicU64 = AtomicU64::new(0);
static HPET_PERIOD_FS: AtomicU64 = AtomicU64::new(0);

// ACPI HPET layout: header (36), event block ID (4), GAS (12),
// sequence (1), minimum tick (2), page protection (1). GAS includes four
// metadata bytes before its 64-bit address; it is not a bare u64.
const HPET_TABLE_SIZE: usize = 56;

fn table_register_base(table: &[u8]) -> Option<u64> {
    if table.len() < HPET_TABLE_SIZE || &table[..4] != b"HPET" {
        return None;
    }
    let length = u32::from_le_bytes(table[4..8].try_into().ok()?) as usize;
    if length != HPET_TABLE_SIZE
        || table[..length].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)) != 0
        // Only system-memory GAS with a naturally aligned 64-bit register
        // block is supported. HPET does not use the System I/O address space.
        || table[40] != 0 || table[41] != 64 || table[42] != 0
        || !matches!(table[43], 0 | 4)
    {
        return None;
    }
    let base = u64::from_le_bytes(table[44..52].try_into().ok()?);
    if base == 0 || base & 0x3ff != 0 {
        return None;
    }
    Some(base)
}

fn hpet_va() -> u64 {
    HPET_VA.load(Ordering::Acquire)
}

fn hpet_reg(off: u64) -> u32 {
    let va = hpet_va();
    if va == 0 {
        return 0;
    }
    unsafe { ptr::read_volatile((va + off) as *const u32) }
}

fn hpet_reg_write(off: u64, val: u32) {
    let va = hpet_va();
    if va == 0 {
        return;
    }
    unsafe {
        ptr::write_volatile((va + off) as *mut u32, val);
    }
}

fn hpet_reg64(off: u64) -> u64 {
    let va = hpet_va();
    if va == 0 {
        return 0;
    }
    unsafe { ptr::read_volatile((va + off) as *const u64) }
}

fn hpet_reg_write64(off: u64, val: u64) {
    let va = hpet_va();
    if va == 0 {
        return;
    }
    unsafe {
        ptr::write_volatile((va + off) as *mut u64, val);
    }
}

/// Initialize from a mapped ACPI HPET table address returned by `find_table`.
/// Zero disables discovery. The table address is a virtual address, distinct
/// from the physical MMIO base encoded inside its Generic Address Structure.
pub fn init(acpi_hpet_table_va: u64) -> bool {
    if acpi_hpet_table_va == 0 {
        return false;
    }
    let mut table = [0u8; HPET_TABLE_SIZE];
    for (index, byte) in table.iter_mut().enumerate() {
        // SAFETY: the ACPI discovery path supplies a mapped HPET table; use
        // byte reads because ACPI structures need not have Rust alignment.
        *byte = unsafe { ptr::read_volatile((acpi_hpet_table_va as *const u8).add(index)) };
    }
    let Some(base) = table_register_base(&table) else {
        return false;
    };
    let page_base = base & !0xfff;
    let page_offset = base - page_base;
    let Some(mapping) = crate::mm::syspte::map_io_space(page_base, 1) else {
        return false;
    };
    let va = mapping + page_offset;
    // SAFETY: the MMIO page covers HPET's capability/configuration registers.
    let cap = unsafe { ptr::read_volatile((va + HPET_REG_CAP) as *const u64) };
    let period_fs = cap >> 32;
    // HPET specification maximum period is 100 ns; a 64-bit counter is
    // required by this interface. Reject an unsupported 32-bit counter.
    if period_fs == 0 || period_fs > 100_000_000 || cap & (1 << 13) == 0 {
        crate::mm::syspte::unmap_io_space(mapping, 1);
        return false;
    }
    // Enable the free-running counter without legacy replacement: no HPET
    // comparator IRQ has been configured, so redirecting PIT/RTC IRQs here
    // would steal the existing system timer and real-time-clock interrupts.
    // SAFETY: the mapped configuration register is naturally aligned.
    unsafe {
        let cfg = ptr::read_volatile((va + HPET_REG_CFG) as *const u32);
        ptr::write_volatile(
            (va + HPET_REG_CFG) as *mut u32,
            (cfg | HPET_CFG_ENABLE) & !HPET_CFG_LEGACY,
        );
    }
    HPET_PERIOD_FS.store(period_fs, Ordering::Release);
    HPET_FREQ_HZ.store(1_000_000_000_000_000u64 / period_fs, Ordering::Release);
    HPET_VA.store(va, Ordering::Release);
    true
}

/// The HPET main counter frequency in Hz. Returns 0 if the HPET
/// was not initialised.
pub fn hpet_freq_hz() -> u64 {
    HPET_FREQ_HZ.load(Ordering::Acquire)
}

/// Period between HPET ticks, in femtoseconds.
pub fn hpet_period_fs() -> u64 {
    HPET_PERIOD_FS.load(Ordering::Acquire)
}

/// read the 32-bit low half of the HPET main counter.
pub fn counter_lo() -> u32 {
    hpet_reg(HPET_REG_COUNTER)
}

/// Read a coherent 64-bit counter using high/low/high sampling.
fn coherent_counter(mut read: impl FnMut(u64) -> u32) -> u64 {
    loop {
        let high = read(HPET_REG_COUNTER + 4);
        let low = read(HPET_REG_COUNTER);
        if high == read(HPET_REG_COUNTER + 4) {
            return (u64::from(high) << 32) | u64::from(low);
        }
        // A rollover may happen before OR after the low read. Retrying is
        // necessary; pairing the old low with the newer high can jump by 2^32.
    }
}

/// Read the 64-bit HPET main counter without tearing across a low-word wrap.
pub fn counter() -> u64 {
    if hpet_va() == 0 {
        return 0;
    }
    coherent_counter(hpet_reg)
}

/// Equivalent of Win32 `query_performance_frequency`. Returns the
/// counter rate in Hz.
pub fn hal_query_performance_frequency() -> i64 {
    hpet_freq_hz() as i64
}

/// Equivalent of Win32 `query_performance_counter`. Returns the
/// current counter value.
pub fn hal_query_performance_counter() -> i64 {
    counter() as i64
}

/// Block for `us` microseconds using the HPET as the time base.
/// Returns `true` if the HPET was available; `false` if the call
/// fell back to a coarse spin.
pub fn delay_us(us: u32) -> bool {
    // Busy-spin by contract, but bounded: an unbounded request would
    // pin the CPU for over an hour. Callers needing longer waits must
    // use timer/scheduler objects instead of a spinning delay.
    const MAX_BUSY_DELAY_US: u32 = 1_000_000;
    if us > MAX_BUSY_DELAY_US {
        return false;
    }
    let hz = hpet_freq_hz();
    if hz == 0 {
        return false;
    }
    let start = counter();
    let ticks = (hz * us as u64) / 1_000_000;
    if ticks == 0 {
        return false;
    }
    loop {
        let now = counter();
        if now.wrapping_sub(start) >= ticks {
            return true;
        }
        core::hint::spin_loop();
    }
}

/// Program timer 0 with `ticks` from `now`. `now + ticks` is
/// written into the comparator. The caller is responsible for
/// enabling the matching IRQ in the I/O APIC.
pub fn program_timer0(delay_ticks: u32) {
    let cur = counter();
    let target = cur.wrapping_add(delay_ticks as u64);
    // Timer 0 comparator lives at offset 0x108. Write the full
    // 64-bit target so the comparator does not silently truncate
    // to the low 32 bits.
    hpet_reg_write64(0x108, target);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> [u8; HPET_TABLE_SIZE] {
        let mut table = [0u8; HPET_TABLE_SIZE];
        table[..4].copy_from_slice(b"HPET");
        table[4..8].copy_from_slice(&(HPET_TABLE_SIZE as u32).to_le_bytes());
        table[8] = 1;
        table[41] = 64;
        table[44..52].copy_from_slice(&0xfed0_0400u64.to_le_bytes());
        fix_checksum(&mut table);
        table
    }

    fn fix_checksum(table: &mut [u8]) {
        table[9] = 0;
        table[9] = table
            .iter()
            .fold(0u8, |sum, b| sum.wrapping_add(*b))
            .wrapping_neg();
    }

    #[test]
    fn hpet_gas_address_follows_its_four_metadata_bytes() {
        assert_eq!(table_register_base(&table()), Some(0xfed0_0400));
    }

    #[test]
    fn rejects_malformed_or_unsupported_hpet_tables() {
        let original = table();
        assert_eq!(table_register_base(&original[..55]), None);
        for (offset, value) in [
            (0, b'X'),
            (4, 55),
            (9, 42),
            (40, 1),
            (41, 32),
            (42, 1),
            (43, 1),
            (44, 1),
        ] {
            let mut bad = original;
            bad[offset] = value;
            if offset != 9 {
                fix_checksum(&mut bad);
            }
            assert_eq!(table_register_base(&bad), None, "offset {offset}");
        }
    }

    #[test]
    fn retries_counter_rollover_before_or_after_low_read() {
        for low in [0xffff_ffff, 0] {
            let values = [7, low, 8, 8, 1, 8];
            let mut values = values.into_iter();
            assert_eq!(
                coherent_counter(|_| values.next().unwrap()),
                (8u64 << 32) | 1
            );
            assert!(values.next().is_none());
        }
    }
}

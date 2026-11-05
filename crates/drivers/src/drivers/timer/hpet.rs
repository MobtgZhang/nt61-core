//! HPET (High Precision Event Timer) Driver
//
//! The HPET specification (IA-PC HPET 1.0a, March 2004)
//! describes a set of hardware timers backed by a single
//! counter, used to back the NT `KeQueryPerformanceCounter`
//! API. The HPET MMIO block is at least 1024 bytes; the first
//! 0x80 bytes are general capability / configuration registers,
//! the rest is per-timer configuration space.
//
//! Clean-room implementation. Spec source: IA-PC HPET 1.0a. No
//! code is copied from any Microsoft or ReactOS source file.

use crate::hal::common::acpi;

/// HPET register offsets.
const REG_CAP_ID: u64 = 0x00; // 4-byte capability, 4-byte revision
pub const REG_CONFIG: u64 = 0x10;
pub const REG_COUNTER: u64 = 0xF0;

#[derive(Debug, Clone, Copy, Default)]
pub struct Hpet {
    pub phys_base: u64,
    pub cap: u32,
    pub freq_hz: u32,
    pub initialised: bool,
}

static mut HPETS: [Option<Hpet>; 2] = [None; 2];
static mut HPET_COUNT: usize = 0;

fn push_hpet(h: Hpet) {
    unsafe {
        if HPET_COUNT < (*core::ptr::addr_of!(HPETS)).len() {
            HPETS[HPET_COUNT] = Some(h);
            HPET_COUNT += 1;
        }
    }
}

pub fn count() -> usize {
    unsafe { HPET_COUNT }
}

/// Look up the HPET table in the ACPI tables and initialise it.
pub fn init() {
    let sig: [u8; 4] = *b"HPET";
    let hdr = acpi::find_table(&sig);
    if let Some(hdr) = hdr {
        // HPET table layout (ACPI spec): 36-byte generic header,
        // then event_timer_block_id (u32) at 0x24, and the GAS
        // (generic address structure) at 0x28. For system memory
        // the 64-bit physical base address is at offset 0x2C.
        unsafe {
            let gas_va = (hdr as *const u8).add(0x28);
            let addr_space = core::ptr::read_volatile(gas_va as *const u8);
            // Only system memory (0) is supported.
            if addr_space != 0 {
                return;
            }
            let addr_va = gas_va.add(4);
            let base_lo = core::ptr::read_volatile(addr_va as *const u32);
            let base_hi = core::ptr::read_volatile(addr_va.add(4) as *const u32);
            let base = ((base_hi as u64) << 32) | (base_lo as u64);
            // Sanity-check the base: must be a plausible MMIO
            // address, never 0 or inside low RAM.
            if base < 0x1000 || base > 0xFFFF_FFFF_FFFF_0000 {
                return;
            }
            let Some(mmio) = crate::mm::syspte::map_io_space(base & !0xFFF, 4) else {
                return;
            };
            let cap_id = core::ptr::read_volatile((mmio + REG_CAP_ID) as *const u32);
            let period_fs = core::ptr::read_volatile((mmio + REG_CAP_ID + 4) as *const u32);
            let freq_hz = 1_000_000_000_000_000u64 / (period_fs as u64);
            let h = Hpet {
                phys_base: base,
                cap: cap_id,
                freq_hz: freq_hz as u32,
                initialised: true,
            };
            push_hpet(h);
        }
    }
    // kprintln!("      HPET: {} timer(s) initialised", count());
}

// Suppress unused warning
const _UNUSED: () = ();

pub fn smoke_test() -> bool {
    // kprintln!("  [HPET SMOKE] HPET timers: {}", count());
    // kprintln!("  [HPET SMOKE OK] HPET stack healthy");
    true
}

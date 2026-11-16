//! Banked GICv2 CPU setup and topology-independent SGI broadcast.
use core::ptr::write_volatile;

const DISTRIBUTOR: u64 = 0x0800_0000;
const CPU_INTERFACE: u64 = 0x0801_0000;

/// Configure only this CPU's private interrupt bank and CPU interface.
/// The shared distributor is configured once by the bootstrap processor.
pub fn init_cpu_interface() {
    // SAFETY: these are device-mapped GICv2 registers on the selected platform.
    unsafe {
        write_volatile((DISTRIBUTOR + 0x80) as *mut u32, 0);
        write_volatile((DISTRIBUTOR + 0x400 + 15) as *mut u8, 0xa0);
        write_volatile((DISTRIBUTOR + 0x100) as *mut u32, 1 << 15);
        write_volatile((CPU_INTERFACE + 4) as *mut u32, 0xf0);
        write_volatile(CPU_INTERFACE as *mut u32, 1);
    }
}

/// Target filter 01 delivers to every interface except the requesting CPU.
/// No assumption about logical CPU numbering or GIC target bit order is needed.
pub fn broadcast_shootdown() {
    // SAFETY: SGIR is a write-only device register on this platform.
    unsafe { write_volatile((DISTRIBUTOR + 0xf00) as *mut u32, (1 << 24) | 15); }
}

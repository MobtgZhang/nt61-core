//! IOCSR software interrupts. Hardware core IDs are carried in the send word,
//! not in the IOCSR register address. The IPI line is ESTAT/ECFG bit 12.
use super::interrupt::{read_u32, write_u32};

pub const INTERRUPT_BIT: u64 = 1 << 12;
const STATUS: u64 = 0x1000;
const ENABLE: u64 = 0x1004;
const CLEAR: u64 = 0x100c;
const SEND: u64 = 0x1040;
const SHOOTDOWN: u32 = 1;

pub fn init_cpu() {
    // Clear the startup mailbox signal before enabling runtime delivery.
    unsafe {
        write_u32(CLEAR, u32::MAX);
        write_u32(ENABLE, SHOOTDOWN);
    }
}

pub fn send(hardware_cpu: u32) {
    assert!(hardware_cpu < (1 << 10), "IOCSR hardware CPU ID exceeds send field");
    // Bit 31 requests blocking delivery, bits [25:16] select the hardware
    // core, and bits [4:0] select IPI action zero (mask 1 in STATUS).
    unsafe { write_u32(SEND, (1 << 31) | (hardware_cpu << 16)); }
}

pub fn clear() {
    unsafe { write_u32(CLEAR, SHOOTDOWN); }
}

pub fn pending() -> bool {
    unsafe { read_u32(STATUS) & SHOOTDOWN != 0 }
}

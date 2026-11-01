//! 7A1000 UART driver for LoongArch64
//
//! Targets the QEMU 'virt' LoongArch machine, which maps a
//! 7A1000-compatible UART at physical address 0x1FE0_01E0. The
//! 7A1000 is 16550a-compatible.

use core::ptr::{read_volatile, write_volatile};

const UART_BASE: u64 = 0x1FE0_01E0;

/// Register offsets (DLAB=0 view) and bit masks used for the
/// receive path. The 7A1000 UART is 16550-compatible.
const REG_DATA: usize = 0;
const REG_IER: usize = 1;
const REG_LSR: usize = 5;
/// IER bit 0: Received Data Available interrupt.
const IER_RX: u8 = 0x01;
/// LSR bit 0: Data Ready — a byte waits in the receive FIFO.
const LSR_DR: u8 = 0x01;

const CRMD_IE: u64 = 1 << 2;
const CRMD_DA: u64 = 1 << 3;
const CRMD_PG: u64 = 1 << 4;

/// QEMU's DMW windows do not reliably cover the LS7A UART after
/// paging is enabled. Briefly return to direct address mode for each
/// access, masking interrupts to avoid entering mapped kernel code
/// while CRMD.PG is cleared.
#[inline(never)]
fn with_uart_da_mode<T>(operation: impl FnOnce() -> T) -> T {
    let mut crmd: u64;
    unsafe {
        core::arch::asm!("csrrd {}, 0x0", out(reg) crmd, options(nostack));
    }
    if crmd & CRMD_PG == 0 {
        return operation();
    }

    let protected = (crmd & !CRMD_IE & !CRMD_PG) | CRMD_DA;
    unsafe {
        core::arch::asm!("csrwr {}, 0x0", in(reg) protected, options(nostack));
    }
    let result = operation();
    unsafe {
        core::arch::asm!("csrwr {}, 0x0", in(reg) crmd, options(nostack));
    }
    result
}

#[inline]
fn read_reg(offset: usize) -> u8 {
    with_uart_da_mode(|| unsafe { read_volatile((UART_BASE as usize + offset) as *const u8) })
}

#[inline]
fn write_reg(offset: usize, value: u8) {
    with_uart_da_mode(|| unsafe {
        write_volatile((UART_BASE as usize + offset) as *mut u8, value)
    });
}

/// Initialize the UART.
pub fn init() {
    let _ = read_reg(0);
}

/// write a single byte.
pub fn write_char(c: u8) {
    let _tx_guard = crate::hal::common::serial::lock_uart_tx();
    while (read_reg(5) & 0x20) == 0 {
        core::hint::spin_loop();
    }
    write_reg(0, c);
}

/// write a string.
pub fn write_string(s: &str) {
    #[cfg(not(feature = "log_level_debug"))]
    {
        let _ = s;
    }
    #[cfg(feature = "log_level_debug")]
    {
        // Hold the transmit lock for the whole string so two CPUs
        // cannot interleave their bytes mid-line.
        let _tx_guard = crate::hal::common::serial::lock_uart_tx();
        for c in s.bytes() {
            while (read_reg(5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            write_reg(0, c as u8);
        }
    }
}

/// Writes diagnostic strings even in non-debug builds.
pub fn write_string_always(s: &str) {
    #[cfg(test)]
    {
        let _ = s;
    }
    #[cfg(not(test))]
    {
        // LoongArch64: the UART lives at a physical address that is
        // only reachable in direct-address mode (CRMD.DA=1, PG=0).
        // Briefly drop PG, write the bytes, and restore CRMD so the
        // caller's mapped-mode execution is unaffected.
        let mut prev_crmd: u64;
        unsafe { core::arch::asm!("csrrd {}, 0x0", out(reg) prev_crmd, options(nostack)) };
        let direct = (prev_crmd | (1 << 3)) & !(1 << 4);
        unsafe { core::arch::asm!("csrwr {}, 0x0", in(reg) direct, options(nostack)) };
        let _tx_guard = crate::hal::common::serial::lock_uart_tx();
        for c in s.bytes() {
            write_char(c);
        }
        unsafe { core::arch::asm!("csrwr {}, 0x0", in(reg) prev_crmd, options(nostack)) };
    }
}

pub fn write_hex_u64_always(v: u64) {
    for i in (0..16).rev() {
        let nibble = ((v >> (i * 4)) & 0xF) as u8;
        let c = if nibble < 10 {
            b'0' + nibble
        } else {
            b'A' + nibble - 10
        };
        write_char(c);
    }
}

/// write a single byte (alias used by HAL callers). Provided for
/// parity with the other architectures' `serial::put_char`.
#[inline(always)]
pub fn put_char(c: u8) {
    write_char(c);
}

/// Try to read one byte from the UART. Returns `None` if the
/// receive FIFO is empty. Mirrors `hal::aarch64::serial` /
/// `hal::riscv64::serial` so the unified keyboard ring works on
/// every architecture.
pub fn try_get_char() -> Option<u8> {
    if !data_available() {
        return None;
    }
    Some(read_data_reg())
}

/// True when the receive FIFO holds at least one byte.
#[inline]
pub fn data_available() -> bool {
    (read_reg(REG_LSR) & LSR_DR) != 0
}

/// Arm the 16550 received-data interrupt (IER bit 0). The
/// corresponding LoongArch EXTIOI / CPU-INT handler must call
/// `crate::hal::common::keyboard_unified::serial_rx_irq` to
/// drain the FIFO into the keyboard ring; the HAL never enables
/// the IRQ on its own because enabling hardware IRQs without a
/// registered handler would storm.
pub fn enable_rx_irq() {
    let ier = read_reg(REG_IER);
    write_reg(REG_IER, ier | IER_RX);
}

/// Mask the 16550 received-data interrupt again.
pub fn disable_rx_irq() {
    let ier = read_reg(REG_IER);
    write_reg(REG_IER, ier & !IER_RX);
}

/// Read one byte unconditionally from the data register. Only
/// valid after `data_available` returned true; used by the
/// unified keyboard ring's FIFO drain loop.
#[inline]
pub fn read_data_reg() -> u8 {
    read_reg(REG_DATA)
}

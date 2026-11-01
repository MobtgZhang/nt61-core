//! LoongArch 64 8250/16550 UART
//!
//! Provides the same public surface as `crate::hal::x86_64::serial`
//! so that cross-platform code (`system_image`, `rtl::sac`, etc.) can
//! invoke `crate::hal::serial::*` without per-architecture guards.

use core::fmt;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

static UART_BASE: AtomicU64 = AtomicU64::new(0);

const UART_PHYS_BASE: u64 = 0x1FE0_01E0;
const UART_THR: u64 = 0;
const UART_LSR: u64 = 5;

const CRMD_IE: u64 = 1 << 2;
const CRMD_DA: u64 = 1 << 3;
const CRMD_PG: u64 = 1 << 4;

/// QEMU's LoongArch DMW windows do not reliably cover the UART once
/// paging is enabled. Temporarily return to the firmware's direct
/// address mode for every UART MMIO access. Interrupts are masked
/// while CRMD.PG is cleared because exception vectors and saved
/// kernel state expect mapped mode.
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
fn read_uart(offset: u64) -> u8 {
    with_uart_da_mode(|| unsafe { ptr::read_volatile((UART_PHYS_BASE + offset) as *const u8) })
}

#[inline]
fn write_uart(offset: u64, value: u8) {
    with_uart_da_mode(|| unsafe {
        ptr::write_volatile((UART_PHYS_BASE + offset) as *mut u8, value)
    });
}

/// Initialise the UART. `base == 0` uses the LS7A default at
/// `0x1FE0_01E0` (the address the Loongson machines and QEMU
/// virt platform expose).
pub fn init() {
    serial_init(0);
}

/// Architecture-specified base address. Provided for symmetry with
/// the x86_64 driver.
pub fn serial_init(base: u64) {
    let b = if base != 0 { base } else { UART_PHYS_BASE };
    UART_BASE.store(b, Ordering::Release);
    let _ = b;
    write_uart(1, 0x00);
    write_uart(3, 0x80);
    write_uart(0, 0x01);
    write_uart(1, 0x00);
    write_uart(3, 0x03);
    write_uart(2, 0xC7);
}

/// write a single character to the UART. Honours the global
/// `hal::common::serial_disable` gate.
pub fn write_char(c: u8) {
    if crate::hal::common::serial_disable::is_disabled() {
        return;
    }
    while read_uart(UART_LSR) & 0x20 == 0 {}
    write_uart(UART_THR, c);
}

/// write a string to the UART. Honours the gate.
pub fn write_string(s: &str) {
    if crate::hal::common::serial_disable::is_disabled() {
        return;
    }
    for c in s.bytes() {
        write_char(c);
    }
}

/// Writes diagnostic strings even in non-debug builds.
pub fn write_string_always(s: &str) {
    for c in s.bytes() {
        while read_uart(UART_LSR) & 0x20 == 0 {}
        write_uart(UART_THR, c);
    }
}

pub fn write_hex_u64_always(value: u64) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [b'0'; 16];
    for (index, byte) in buf.iter_mut().enumerate() {
        let nibble = (value >> (60 - index * 4)) & 0xf;
        *byte = HEX[nibble as usize];
    }
    write_string_always("0x");
    write_string_always(core::str::from_utf8(&buf).unwrap_or(""));
}

/// write a string followed by CRLF.
pub fn write_line(s: &str) {
    write_string(s);
    write_string("\r\n");
}

/// write a `u64` as 16 hex digits.
pub fn write_u64_hex(val: u64) {
    if !log_level_debug() {
        return;
    }

    use core::fmt::Write;
    struct HexWriter<'a>(&'a mut [u8]);
    impl<'a> fmt::Write for HexWriter<'a> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0[..s.len()].copy_from_slice(s.as_bytes());
            Ok(())
        }
    }
    let mut buf = [0u8; 16];
    let _ = write!(HexWriter(&mut buf), "{:016x}", val);
    write_string(core::str::from_utf8(&buf).unwrap_or(""));
}

/// write a `u64` as 16 hex digits.
///
/// Available cross-arch through the unified
/// `crate::hal::serial::write_hex_u64` facade. We deliberately
/// define both the kernel-side alias (`write_hex_u64`) and the
/// boot-side name (`write_u64_hex`) so the cross-arch callers
/// (`mm::pool`, `arch/aarch64/paging`, etc.) link regardless of
/// which side of the build they end up on.
pub fn write_hex_u64(val: u64) {
    if !log_level_debug() {
        return;
    }

    write_u64_hex(val);
}

/// write a `usize` as hex without padding.
pub fn write_usize_hex(val: usize) {
    if !log_level_debug() {
        return;
    }

    use core::fmt::Write;
    let mut buf = [0u8; 32];
    let mut s = String::new();
    let _ = write!(s, "{:x}", val);
    let bytes = s.as_bytes();
    buf[..bytes.len()].copy_from_slice(bytes);
    write_string(core::str::from_utf8(&bytes).unwrap_or(""));
}

/// write a `u32` as 8 hex digits.
pub fn write_u32_hex(val: u32) {
    if !log_level_debug() {
        return;
    }

    use core::fmt::Write;
    let mut buf = [0u8; 8];
    let _ = write!(WriteToBuf(&mut buf), "{:08x}", val);
    write_string(core::str::from_utf8(&buf).unwrap_or(""));
}

fn log_level_debug() -> bool {
    cfg!(feature = "log_level_debug")
}

struct WriteToBuf<'a>(&'a mut [u8]);
impl<'a> fmt::Write for WriteToBuf<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let n = s.len().min(self.0.len());
        self.0[..n].copy_from_slice(&s.as_bytes()[..n]);
        Ok(())
    }
}

/// read one byte from the UART. Returns `None` if no data is ready.
pub fn read_char() -> Option<u8> {
    if read_uart(UART_LSR) & 0x01 == 0 {
        return None;
    }
    Some(read_uart(UART_THR))
}

/// Returns true if data is waiting in the receive FIFO.
pub fn data_available() -> bool {
    (read_uart(UART_LSR) & 0x01) != 0
}

/// Legacy alias for `init()`-style entry points.
pub fn serial_putc(b: u8) {
    write_char(b);
}

/// Legacy alias for `write_string`.
pub fn serial_puts(s: &str) {
    write_string(s);
}

/// Legacy alias for `read_char`.
pub fn serial_getc() -> i16 {
    match read_char() {
        Some(b) => b as i16,
        None => -1,
    }
}

/// Light-weight formatter helper, mirrors `crate::hal::x86_64::serial`.
pub struct Stdout;

impl Stdout {
    pub fn new() -> Self {
        Stdout
    }
    pub fn write(&self, s: &str) {
        write_string(s);
    }
    pub fn write_byte(&self, b: u8) {
        write_char(b);
    }
    pub fn flush(&self) { /* MMIO writes are synchronous. */
    }
}

extern crate alloc;
use alloc::string::String;

/// Reserved for Phase 2 diagnostics. Kept here so the link order is
/// stable across builds.
fn _keep() {}

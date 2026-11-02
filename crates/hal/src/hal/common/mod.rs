//! Common Hardware Abstraction Layer
//!
//! Architecture-independent hardware interfaces

pub mod acpi;
pub mod cmos;
pub mod console;
pub mod dma;
pub mod framebuffer;
pub mod framebuffer_impl;
pub mod interrupts;
pub mod io_port;
pub mod keyboard_input;
// Unified keyboard ring: x86_64 has its own PS/2 + USB-HID
// implementation at hal::x86_64::keyboard_unified, so this
// module only compiles on the other three architectures.
#[cfg(not(target_arch = "x86_64"))]
pub mod keyboard_unified;
pub mod pci;
pub mod pit;
pub mod power;
pub mod relock;
pub mod serial;
pub mod serial_disable;
pub mod system_info;
pub mod text_console;
pub mod timer;

/// Initialize all hardware (common part)
pub fn init() {
    acpi::init();
    pci::init();
}

/// Shutdown hardware
pub fn shutdown() {
    // ACPI S5 shutdown via the QEMU/Bochs/VMware ACPI PM1a control
    // port. QEMU exposes the PM1a_CNT register at I/O port 0x604;
    // Bochs uses 0xB004. Writing SLP_EN | SLP_TYP=S5 triggers an
    // immediate shutdown on emulated hardware. On physical hardware
    // the firmware sets SLP_TYP from the _S5 object; a real AML
    // interpreter would be needed to evaluate it, so we fall back
    // to the halt loop there.
    #[cfg(target_arch = "x86_64")]
    {
        // QEMU: PM1a_CNT at 0x604, S5 sleep type 5 << 10, SLP_EN bit 13.
        crate::hal::x86_64::io_port::write_port_ushort(0x604, (5 << 10) | (1 << 13));
        // Bochs: PM1a_CNT at 0xB004.
        crate::hal::x86_64::io_port::write_port_ushort(0xB004, (5 << 10) | (1 << 13));
    }
    // Halt all CPUs — the fallback for hardware that does not
    // respond to the ACPI shutdown ports.
    loop {
        crate::arch::halt();
    }
}

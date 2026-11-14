//! Architecture-agnostic firmware power transitions.

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

// Resolve ACPI and map its byte-wide reset register during normal boot.
// A bugcheck must not allocate page tables or take registry/ACPI locks.
static RESET_ADDRESS: AtomicU64 = AtomicU64::new(0);
static RESET_VALUE: AtomicU8 = AtomicU8::new(0);
#[cfg(target_arch = "aarch64")]
static PSCI_CONDUIT: AtomicU8 = AtomicU8::new(0);

pub fn init() {
    let Some(fadt) = super::acpi::parse_fadt() else {
        return;
    };
    #[cfg(target_arch = "aarch64")]
    if fadt.arm_boot_arch & 1 != 0 {
        PSCI_CONDUIT.store(
            if fadt.arm_boot_arch & 2 != 0 { 2 } else { 1 },
            Ordering::Release,
        );
    }
    if fadt.flags & super::acpi::fadt_flags::RESET_REGISTER == 0
        || fadt.reset_register == 0
        || fadt.reset_address_space != 0
        || fadt.reset_bit_width != 8
        || fadt.reset_bit_offset != 0
    {
        return;
    }
    if let Some(address) = crate::mm::syspte::map_io_space(fadt.reset_register & !0xfff, 1) {
        RESET_VALUE.store(fadt.reset_value, Ordering::Relaxed);
        RESET_ADDRESS.store(address + (fadt.reset_register & 0xfff), Ordering::Release);
    }
}

/// Actions understood by `hal_return_to_firmware`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareAction {
    Halt,
    Reboot,
    Shutdown,
}

/// Return control to firmware. This function does not return.
pub fn return_to_firmware(action: FirmwareAction) -> ! {
    #[cfg(target_arch = "x86_64")]
    {
        let action = match action {
            FirmwareAction::Halt => crate::hal::x86_64::halt::firmware_action::HAL_HALT,
            FirmwareAction::Reboot => crate::hal::x86_64::halt::firmware_action::HAL_REBOOT,
            FirmwareAction::Shutdown => crate::hal::x86_64::halt::firmware_action::HAL_SHUTDOWN,
        };
        crate::hal::x86_64::halt::hal_return_to_firmware(u32::from(action));
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        if action == FirmwareAction::Reboot {
            let address = RESET_ADDRESS.load(Ordering::Acquire);
            if address != 0 {
                // SAFETY: init mapped and validated this ACPI reset register.
                unsafe {
                    (address as *mut u8).write_volatile(RESET_VALUE.load(Ordering::Relaxed));
                }
            }
        }
        #[cfg(target_arch = "riscv64")]
        if action != FirmwareAction::Halt {
            let kind = if action == FirmwareAction::Reboot {
                crate::arch::riscv64::sbi::ResetType::ColdReboot
            } else {
                crate::arch::riscv64::sbi::ResetType::Shutdown
            };
            // SAFETY: SBI SRST is invoked at supervisor privilege.
            unsafe {
                let _ = crate::arch::riscv64::sbi::system_reset(kind);
            }
        }
        #[cfg(target_arch = "aarch64")]
        if action != FirmwareAction::Halt {
            let function: u64 = if action == FirmwareAction::Reboot {
                0x8400_0009
            } else {
                0x8400_0008
            };
            // ACPI ARM_BOOT_ARCH declares whether PSCI uses SMC or HVC.
            // Unknown firmware is left halted rather than issuing a trapping call.
            unsafe {
                match PSCI_CONDUIT.load(Ordering::Acquire) {
                    1 => {
                        core::arch::asm!("smc #0", inout("x0") function => _, out("x1") _, out("x2") _, out("x3") _, options(nostack))
                    }
                    2 => {
                        core::arch::asm!("hvc #0", inout("x0") function => _, out("x1") _, out("x2") _, out("x3") _, options(nostack))
                    }
                    _ => {}
                }
            }
        }
        crate::arch::halt_loop();
    }
}

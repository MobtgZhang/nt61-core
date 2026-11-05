use crate::arch::Arch;
use crate::fs::BootFormat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopBootPlan {
    pub arch: Arch,
    pub format: BootFormat,
}

impl DesktopBootPlan {
    pub const ALL: [DesktopBootPlan; 28] = [
        plan(Arch::X86_64, BootFormat::Ramfs),
        plan(Arch::X86_64, BootFormat::Ext4),
        plan(Arch::X86_64, BootFormat::Ntfs),
        plan(Arch::X86_64, BootFormat::Refs),
        plan(Arch::X86_64, BootFormat::Fat32),
        plan(Arch::X86_64, BootFormat::Iso),
        plan(Arch::X86_64, BootFormat::Qcow2),
        plan(Arch::Aarch64, BootFormat::Ramfs),
        plan(Arch::Aarch64, BootFormat::Ext4),
        plan(Arch::Aarch64, BootFormat::Ntfs),
        plan(Arch::Aarch64, BootFormat::Refs),
        plan(Arch::Aarch64, BootFormat::Fat32),
        plan(Arch::Aarch64, BootFormat::Iso),
        plan(Arch::Aarch64, BootFormat::Qcow2),
        plan(Arch::Riscv64, BootFormat::Ramfs),
        plan(Arch::Riscv64, BootFormat::Ext4),
        plan(Arch::Riscv64, BootFormat::Ntfs),
        plan(Arch::Riscv64, BootFormat::Refs),
        plan(Arch::Riscv64, BootFormat::Fat32),
        plan(Arch::Riscv64, BootFormat::Iso),
        plan(Arch::Riscv64, BootFormat::Qcow2),
        plan(Arch::Loongarch64, BootFormat::Ramfs),
        plan(Arch::Loongarch64, BootFormat::Ext4),
        plan(Arch::Loongarch64, BootFormat::Ntfs),
        plan(Arch::Loongarch64, BootFormat::Refs),
        plan(Arch::Loongarch64, BootFormat::Fat32),
        plan(Arch::Loongarch64, BootFormat::Iso),
        plan(Arch::Loongarch64, BootFormat::Qcow2),
    ];

    pub fn is_desktop_capable(&self) -> bool {
        self.arch.supports_desktop() && self.format.supports_desktop()
    }
}

const fn plan(arch: Arch, format: BootFormat) -> DesktopBootPlan {
    DesktopBootPlan { arch, format }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_arch_and_boot_format_combination_is_covered() {
        assert_eq!(DesktopBootPlan::ALL.len(), 28);
        for plan in DesktopBootPlan::ALL {
            assert!(plan.is_desktop_capable());
        }
    }
}

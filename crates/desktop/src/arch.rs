use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Riscv64,
    Loongarch64,
}

impl Arch {
    pub const ALL: [Arch; 4] = [
        Arch::X86_64,
        Arch::Aarch64,
        Arch::Riscv64,
        Arch::Loongarch64,
    ];

    pub fn from_target(target: &str) -> Option<Self> {
        match target {
            "x86_64" => Some(Self::X86_64),
            "aarch64" => Some(Self::Aarch64),
            "riscv64" => Some(Self::Riscv64),
            "loongarch64" => Some(Self::Loongarch64),
            _ => None,
        }
    }

    pub fn pe_machine(&self) -> u16 {
        match self {
            Self::X86_64 => 0x8664,
            Self::Aarch64 => 0xAA64,
            Self::Riscv64 => 0xE42C,
            Self::Loongarch64 => 0x6264,
        }
    }

    pub fn supports_desktop(&self) -> bool {
        true
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::Riscv64 => "riscv64",
            Self::Loongarch64 => "loongarch64",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_architectures_are_recognized() {
        for arch in Arch::ALL {
            assert_eq!(Arch::from_target(&arch.to_string()), Some(arch));
            assert!(arch.supports_desktop());
        }
    }

    #[test]
    fn pe_machine_ids_match_windows_targets() {
        assert_eq!(Arch::X86_64.pe_machine(), 0x8664);
        assert_eq!(Arch::Aarch64.pe_machine(), 0xAA64);
        assert_eq!(Arch::Riscv64.pe_machine(), 0xE42C);
        assert_eq!(Arch::Loongarch64.pe_machine(), 0x6264);
    }
}

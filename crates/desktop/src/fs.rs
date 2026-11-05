use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootFormat {
    Ramfs,
    Ext4,
    Ntfs,
    Refs,
    Fat32,
    Iso,
    Qcow2,
}

impl BootFormat {
    pub const ALL: [BootFormat; 7] = [
        BootFormat::Ramfs,
        BootFormat::Ext4,
        BootFormat::Ntfs,
        BootFormat::Refs,
        BootFormat::Fat32,
        BootFormat::Iso,
        BootFormat::Qcow2,
    ];

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "ramfs" => Some(Self::Ramfs),
            "ext4" => Some(Self::Ext4),
            "ntfs" => Some(Self::Ntfs),
            "refs" => Some(Self::Refs),
            "fat32" => Some(Self::Fat32),
            "iso" => Some(Self::Iso),
            "qcow2" => Some(Self::Qcow2),
            _ => None,
        }
    }

    pub fn supports_desktop(&self) -> bool {
        true
    }

    pub fn is_persistent(&self) -> bool {
        !matches!(self, Self::Ramfs | Self::Iso)
    }
}

impl fmt::Display for BootFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Ramfs => "ramfs",
            Self::Ext4 => "ext4",
            Self::Ntfs => "ntfs",
            Self::Refs => "refs",
            Self::Fat32 => "fat32",
            Self::Iso => "iso",
            Self::Qcow2 => "qcow2",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_boot_format_is_desktop_capable() {
        for format in BootFormat::ALL {
            assert_eq!(BootFormat::from_str(&format.to_string()), Some(format));
            assert!(format.supports_desktop());
        }
    }

    #[test]
    fn non_persistent_formats_are_identified() {
        assert!(!BootFormat::Ramfs.is_persistent());
        assert!(!BootFormat::Iso.is_persistent());
        assert!(BootFormat::Ntfs.is_persistent());
        assert!(BootFormat::Qcow2.is_persistent());
    }
}

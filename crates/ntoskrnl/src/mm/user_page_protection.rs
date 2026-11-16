//! Protection of existing 4 KiB user leaves. Callers serialize mapping changes
//! and keep every page-table allocation alive across validation and update.
//! Large leaves, copy-on-write and PAGE_GUARD need separate MM machinery.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture {
    X64,
    Arm64,
    Riscv64,
    Loong64,
}

#[derive(Clone, Copy)]
pub struct Protection {
    user: bool,
    write: bool,
    execute: bool,
}

impl Protection {
    pub fn from_page(value: u32) -> Option<Self> {
        let (user, write, execute) = match value {
            0x01 => (false, false, false),
            0x02 => (true, false, false),
            0x04 => (true, true, false),
            // Like Win7 x64, execute-only is readable on architectures whose
            // user instruction and data permissions cannot express it.
            0x10 | 0x20 => (true, false, true),
            0x40 => (true, true, true),
            _ => return None,
        };
        Some(Self {
            user,
            write,
            execute,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub va: u64,
    pub table: u64,
    pub index: usize,
    pub value: u64,
}

impl Architecture {
    fn noaccess_marker(self) -> u64 {
        match self {
            // Hardware PTE bit 9 already belongs to copy-on-write.
            Self::X64 => 1 << 58,
            Self::Arm64 => 1 << 55,
            Self::Riscv64 => 1 << 8,
            Self::Loong64 => 1 << 11,
        }
    }

    /// Preserve physical, cache, accessed and dirty state. NOACCESS uses an
    /// owned software marker plus a supervisor-only leaf, so later protection
    /// restoration does not mistake a pre-existing kernel leaf for user memory.
    pub fn protect(self, old: u64, protection: Protection) -> Option<u64> {
        let user = match self {
            Self::X64 => old & 4 != 0,
            Self::Arm64 => old & (1 << 6) != 0,
            Self::Riscv64 => old & (1 << 4) != 0,
            Self::Loong64 => old & 12 == 12,
        };
        // This path cannot make a shared COW frame writable. Global user
        // translations require a global-aware shootdown and are rejected.
        if self == Self::X64 && old & ((1 << 9) | (1 << 8)) != 0 {
            return None;
        }
        let marker = self.noaccess_marker();
        if !user && old & marker == 0 {
            return None;
        }
        let mut value = old & !marker;
        match self {
            Self::X64 => {
                value &= !(2 | 4 | 256 | (1 << 63));
                if protection.user {
                    value |= 4;
                }
                if protection.write {
                    value |= 2;
                }
                if !protection.execute {
                    value |= 1 << 63;
                }
            }
            Self::Arm64 => {
                value &= !((1 << 6) | (1 << 7) | (1 << 54));
                // EL1 must never execute the retained user allocation.
                value |= (1 << 53) | (1 << 11);
                if protection.user {
                    value |= 1 << 6;
                }
                if !protection.write {
                    value |= 1 << 7;
                }
                if !protection.execute {
                    value |= 1 << 54;
                }
            }
            Self::Riscv64 => {
                value &= !(2 | 4 | 8 | 16 | 32);
                // Retain a readable supervisor leaf for NOACCESS. Clearing
                // R/W/X together would turn it into a non-leaf in Sv39.
                value |= 2;
                if protection.user {
                    value |= 16;
                }
                if protection.write {
                    value |= 4 | 128;
                }
                if protection.execute {
                    value |= 8;
                }
            }
            Self::Loong64 => {
                value &= !(2 | 12 | 64 | 256 | (1 << 61) | (1 << 62));
                if protection.user {
                    value |= 12;
                }
                if protection.write {
                    value |= 2 | 256;
                }
                if !protection.execute {
                    value |= 1 << 62;
                }
            }
        }
        if !protection.user {
            value |= marker;
        }
        Some(value)
    }

    /// Locate only ordinary user 4 KiB leaves. Never descend through a block
    /// or huge mapping into its payload, and never widen ancestor permissions.
    pub fn leaf(
        self,
        root: u64,
        va: u64,
        protection: Protection,
        mut read: impl FnMut(u64, usize) -> Option<u64>,
    ) -> Option<Leaf> {
        const ADDRESS: u64 = 0x0000_ffff_ffff_f000;
        if root == 0 || root & 0xfff != 0 {
            return None;
        }
        let limit = if self == Self::Riscv64 {
            1u64 << 38
        } else {
            1u64 << 47
        };
        if va < 0x1000 || va >= limit {
            return None;
        }
        let shifts: &[u32] = match self {
            Self::X64 | Self::Arm64 => &[39, 30, 21, 12],
            Self::Riscv64 => &[30, 21, 12],
            Self::Loong64 => &[48, 39, 30, 21, 12],
        };
        let mut table = root;
        for &shift in shifts {
            let index = ((va >> shift) & 511) as usize;
            let value = read(table, index)?;
            let last = shift == 12;
            match self {
                Self::X64 => {
                    if value & 1 == 0 {
                        return None;
                    }
                    if !last
                        && (value & 128 != 0
                            || value & 4 == 0
                            || (protection.write && value & 2 == 0)
                            || (protection.execute && value & (1 << 63) != 0))
                    {
                        return None;
                    }
                    if !last {
                        table = value & 0x000f_ffff_ffff_f000;
                    }
                }
                Self::Arm64 => {
                    if value & 3 != 3 {
                        return None;
                    }
                    if !last {
                        if value & (1 << 61) != 0
                            || (protection.write && value & (1 << 62) != 0)
                            || (protection.execute && value & (1 << 60) != 0)
                        {
                            return None;
                        }
                        table = value & ADDRESS;
                    }
                }
                Self::Riscv64 => {
                    if value & 1 == 0 || value & 0xffc0_0000_0000_0000 != 0 {
                        return None;
                    }
                    if last {
                        if value & 14 == 0 || value & 6 == 4 {
                            return None;
                        }
                    } else {
                        if value & (14 | 16 | 64 | 128) != 0 {
                            return None;
                        }
                        table = ((value >> 10) & ((1u64 << 44) - 1)) << 12;
                    }
                }
                Self::Loong64 => {
                    if last {
                        if value & 1 == 0 {
                            return None;
                        }
                    } else {
                        // Native directory entries are bare physical pointers.
                        if value == 0 || value & !ADDRESS != 0 {
                            return None;
                        }
                        table = value;
                    }
                }
            }
            if last {
                self.protect(value, protection)?;
                return Some(Leaf {
                    va,
                    table,
                    index,
                    value,
                });
            }
            if table == 0 {
                return None;
            }
        }
        None
    }
}

/// Validate the whole extent before the first store. The update callback must
/// compare the observed leaf before replacing it, and invalidate translations
/// after a successful store. A racing change or store failure can leave a
/// changed prefix; callers requiring transactions must provide that mechanism.
pub fn protect_range(
    architecture: Architecture,
    root: u64,
    base: u64,
    size: u64,
    page_protection: u32,
    mut read: impl FnMut(u64, usize) -> Option<u64>,
    mut update: impl FnMut(Leaf, u64) -> bool,
) -> bool {
    if size == 0 {
        return true;
    }
    let Some(protection) = Protection::from_page(page_protection) else {
        return false;
    };
    let Some(end) = base
        .checked_add(size)
        .and_then(|end| end.checked_add(4095))
        .map(|end| end & !4095)
    else {
        return false;
    };
    let start = base & !4095;
    for validation in [true, false] {
        let mut page = start;
        while page < end {
            let Some(leaf) = architecture.leaf(root, page, protection, &mut read) else {
                return false;
            };
            if !validation {
                let Some(value) = architecture.protect(leaf.value, protection) else {
                    return false;
                };
                if !update(leaf, value) {
                    return false;
                }
            }
            page += 4096;
        }
    }
    true
}

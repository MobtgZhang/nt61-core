//! Architecture-neutral demand-zero state machine. Raw descriptors are decoded
//! below using each ISA's format; NT/x86 software MMPTE states are never used.
use super::{AccessFlags, FaultStatus};

#[derive(Clone, Copy)]
pub enum Architecture {
    Arm64,
    Riscv64,
    LoongArch64,
}

#[derive(Clone, Copy)]
pub struct Region {
    pub committed: bool,
    pub guard: bool,
    pub anonymous: bool,
    pub protection: u8,
}

#[derive(Clone, Copy)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

pub fn permissions(protection: u8) -> Option<Permissions> {
    let (read, write, execute) = match protection {
        1 | 9 => (true, false, false),
        // The portable mapper cannot express execute-only without also
        // granting read access. Decline it until native X-only installation.
        2 => return None,
        3 => (true, false, true),
        4 | 10 => (true, true, false),
        6 => (true, true, true),
        // Write-copy needs a backing/prototype owner before it can be copied.
        _ => return None,
    };
    Some(Permissions {
        read,
        write,
        execute,
    })
}

fn allows(p: Permissions, access: AccessFlags) -> bool {
    (!access.read || p.read) && (!access.write || p.write) && (!access.execute || p.execute)
}

pub fn user_address(architecture: Architecture, va: u64) -> bool {
    let bits = match architecture {
        Architecture::Riscv64 => 38,
        _ => 47,
    };
    va >= 0x1000 && va < (1u64 << bits)
}

/// Descriptors are passed at a verified native leaf level (including blocks).
/// Returns permissions and the access-state bits which may be set safely.
fn present(architecture: Architecture, raw: u64, write: bool) -> Option<(Permissions, bool, u64)> {
    match architecture {
        Architecture::Arm64 if raw & 1 != 0 => Some((
            Permissions {
                read: true,
                write: raw & (1 << 7) == 0,
                execute: raw & (1 << 54) == 0,
            },
            raw & (1 << 6) != 0,
            raw | (1 << 10),
        )),
        Architecture::Riscv64 if raw & 1 != 0 && raw & 0xe != 0 => {
            if raw & 4 != 0 && raw & 2 == 0 {
                return None;
            }
            Some((
                Permissions {
                    read: raw & 2 != 0,
                    write: raw & 4 != 0,
                    execute: raw & 8 != 0,
                },
                raw & 16 != 0,
                raw | 64 | if write { 128 } else { 0 },
            ))
        }
        Architecture::LoongArch64 if raw & 1 != 0 => Some((
            Permissions {
                read: raw & (1 << 61) == 0,
                write: raw & (1 << 8) != 0,
                execute: raw & (1 << 62) == 0,
            },
            raw & 12 == 12,
            raw | if write { 2 } else { 0 },
        )),
        _ => None,
    }
}

pub enum Install {
    Installed,
    Occupied,
    OutOfMemory,
}

pub trait Backend {
    fn leaf(&mut self, va: u64) -> u64;
    fn update_access(&mut self, va: u64, expected: u64, updated: u64) -> bool;
    fn allocate_zeroed(&mut self) -> Option<u64>;
    fn free(&mut self, pa: u64);
    fn install(&mut self, va: u64, pa: u64, permissions: Permissions) -> Install;
}

pub fn resolve<B: Backend>(
    architecture: Architecture,
    va: u64,
    access: AccessFlags,
    region: Option<Region>,
    backend: &mut B,
) -> FaultStatus {
    if !user_address(architecture, va)
        || access.reserved_bit
        || !access.user
        || !(access.read || access.write || access.execute)
    {
        return FaultStatus::AccessViolation;
    }
    let Some(region) = region else {
        return FaultStatus::AccessViolation;
    };
    if region.guard {
        return FaultStatus::CheckVad;
    }
    if !region.committed {
        return FaultStatus::AccessViolation;
    }
    let Some(permissions) = permissions(region.protection) else {
        return FaultStatus::AccessViolation;
    };
    if !allows(permissions, access) {
        return FaultStatus::AccessViolation;
    }
    let page = va & !0xfff;
    let raw = backend.leaf(page);
    if let Some((hardware, user, updated)) = present(architecture, raw, access.write) {
        if !user || !allows(hardware, access) {
            return FaultStatus::AccessViolation;
        }
        // Only a genuine missing A/D bit can explain a present-page fault.
        // Never return Handled for an unchanged descriptor (infinite retry).
        return if updated != raw && backend.update_access(page, raw, updated) {
            FaultStatus::Handled
        } else {
            FaultStatus::AccessViolation
        };
    }
    // Any nonzero native invalid descriptor may encode pagefile/transition
    // ownership. Never replace it with a zero page and lose the backing data.
    if raw != 0 || !region.anonymous {
        return FaultStatus::AccessViolation;
    }
    let Some(pa) = backend.allocate_zeroed() else {
        return FaultStatus::OutOfMemory;
    };
    match backend.install(page, pa, permissions) {
        Install::Installed => FaultStatus::Handled,
        Install::Occupied => {
            backend.free(pa);
            FaultStatus::AccessViolation
        }
        Install::OutOfMemory => {
            backend.free(pa);
            FaultStatus::OutOfMemory
        }
    }
}

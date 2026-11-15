//! Host tests execute the production native fault state machine with a backend
//! that records ownership and installation, without dereferencing guest tables.
#[derive(Clone, Copy)]
pub struct AccessFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user: bool,
    pub reserved_bit: bool,
    pub instruction_fetch: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum FaultStatus {
    Handled,
    CheckVad,
    AccessViolation,
    OutOfMemory,
}
#[path = "../../arch/src/arch/aarch64/leaf_descriptor.rs"]
mod arm_leaf;
#[path = "../../arch/src/arch/loongarch64/leaf_descriptor.rs"]
mod loong_leaf;
#[path = "../src/mm/access_fault/native_policy.rs"]
mod policy;
use policy::{Architecture::*, *};
const ARCHES: [Architecture; 3] = [Arm64, Riscv64, LoongArch64];
const READ: AccessFlags = AccessFlags {
    read: true,
    write: false,
    execute: false,
    user: true,
    reserved_bit: false,
    instruction_fetch: false,
};
const WRITE: AccessFlags = AccessFlags {
    read: false,
    write: true,
    ..READ
};
const EXECUTE: AccessFlags = AccessFlags {
    read: false,
    execute: true,
    instruction_fetch: true,
    ..READ
};
const REGION: Region = Region {
    committed: true,
    guard: false,
    anonymous: true,
    protection: 4,
};
#[derive(Default)]
struct Memory {
    raw: u64,
    allocated: usize,
    freed: usize,
    installed: Option<Permissions>,
    allocation_fails: bool,
    install_fails: bool,
    occupied: bool,
}
impl Backend for Memory {
    fn leaf(&mut self, _: u64) -> u64 {
        self.raw
    }
    fn update_access(&mut self, _: u64, expected: u64, updated: u64) -> bool {
        assert_eq!(self.raw, expected);
        self.raw = updated;
        true
    }
    fn allocate_zeroed(&mut self) -> Option<u64> {
        self.allocated += 1;
        if self.allocation_fails {
            None
        } else {
            Some(0x12345000)
        }
    }
    fn free(&mut self, pa: u64) {
        assert_eq!(pa, 0x12345000);
        self.freed += 1;
    }
    fn install(&mut self, va: u64, pa: u64, p: Permissions) -> Install {
        assert_eq!(va, 0x8000);
        assert_eq!(pa, 0x12345000);
        if self.install_fails {
            Install::OutOfMemory
        } else if self.occupied {
            Install::Occupied
        } else {
            self.installed = Some(p);
            Install::Installed
        }
    }
}
fn run(a: Architecture, access: AccessFlags, r: Region, m: &mut Memory) -> FaultStatus {
    resolve(a, 0x8123, access, Some(r), m)
}
#[test]
fn committed_anonymous_faults_install_exact_permissions() {
    for a in ARCHES {
        for protection in [1, 3, 4, 6, 9, 10] {
            let p = permissions(protection).unwrap();
            let mut m = Memory::default();
            assert_eq!(
                run(
                    a,
                    READ,
                    Region {
                        protection,
                        ..REGION
                    },
                    &mut m
                ),
                FaultStatus::Handled
            );
            let installed = m.installed.unwrap();
            assert_eq!(
                (installed.read, installed.write, installed.execute),
                (p.read, p.write, p.execute)
            );
            assert_eq!((m.allocated, m.freed), (1, 0));
        }
    }
}
#[test]
fn denied_vad_access_cannot_allocate_or_widen_permissions() {
    for a in ARCHES {
        for (protection, access) in [
            (1, WRITE),
            (3, WRITE),
            (4, EXECUTE),
            (9, EXECUTE),
            (0, READ),
            (2, EXECUTE),
            (5, WRITE),
            (7, WRITE),
            (8, READ),
            (255, READ),
        ] {
            let mut m = Memory::default();
            assert_eq!(
                run(
                    a,
                    access,
                    Region {
                        protection,
                        ..REGION
                    },
                    &mut m
                ),
                FaultStatus::AccessViolation
            );
            assert_eq!(m.allocated, 0);
        }
    }
}
#[test]
fn missing_reserved_guard_and_file_backing_are_not_zero_filled() {
    for a in ARCHES {
        let mut m = Memory::default();
        assert_eq!(
            resolve(a, 0x8123, READ, None, &mut m),
            FaultStatus::AccessViolation
        );
        assert_eq!(
            run(
                a,
                READ,
                Region {
                    committed: false,
                    ..REGION
                },
                &mut m
            ),
            FaultStatus::AccessViolation
        );
        assert_eq!(
            run(
                a,
                READ,
                Region {
                    anonymous: false,
                    ..REGION
                },
                &mut m
            ),
            FaultStatus::AccessViolation
        );
        assert_eq!(
            run(
                a,
                READ,
                Region {
                    guard: true,
                    ..REGION
                },
                &mut m
            ),
            FaultStatus::CheckVad
        );
        assert_eq!(m.allocated, 0);
    }
}
#[test]
fn nonzero_invalid_native_backing_is_never_replaced() {
    for a in ARCHES {
        for raw in [2, 0x12345000, 0x8000000000000000] {
            let mut m = Memory {
                raw,
                ..Default::default()
            };
            assert_eq!(run(a, READ, REGION, &mut m), FaultStatus::AccessViolation);
            assert_eq!((m.raw, m.allocated), (raw, 0));
        }
    }
}
#[test]
fn native_access_and_dirty_bits_keep_the_original_page_and_permissions() {
    for (a, raw, access, delta) in [
        (Arm64, 0x12345000 | 3 | 64 | (1 << 54), READ, 1 << 10),
        (
            Riscv64,
            ((0x12345000u64 >> 12) << 10) | 1 | 2 | 4 | 16,
            WRITE,
            64 | 128,
        ),
        (LoongArch64, 0x12345000 | 1 | 12 | 256 | (1 << 62), WRITE, 2),
    ] {
        let mut m = Memory {
            raw,
            ..Default::default()
        };
        assert_eq!(run(a, access, REGION, &mut m), FaultStatus::Handled);
        assert_eq!(m.raw, raw | delta);
        assert_eq!(m.allocated, 0);
        assert_eq!(run(a, access, REGION, &mut m), FaultStatus::AccessViolation);
    }
}
#[test]
fn native_hardware_permissions_override_more_permissive_vads() {
    for (a, raw, access) in [
        (Arm64, 0x12345000 | 3 | 64 | 128, WRITE),
        (Arm64, 0x12345000 | 3 | 64 | (1 << 54), EXECUTE),
        (Riscv64, ((0x12345000u64 >> 12) << 10) | 1 | 2 | 16, WRITE),
        (Riscv64, ((0x12345000u64 >> 12) << 10) | 1 | 2 | 4, READ),
        (Riscv64, ((0x12345000u64 >> 12) << 10) | 1 | 4 | 16, WRITE),
        (LoongArch64, 0x12345000 | 1 | 12, WRITE),
        (LoongArch64, 0x12345000 | 1 | 12 | 256 | (1 << 61), READ),
        (LoongArch64, 0x12345000 | 1 | 12 | 256 | (1 << 62), EXECUTE),
    ] {
        let mut m = Memory {
            raw,
            ..Default::default()
        };
        assert_eq!(
            run(
                a,
                access,
                Region {
                    protection: 6,
                    ..REGION
                },
                &mut m
            ),
            FaultStatus::AccessViolation
        );
        assert_eq!((m.raw, m.allocated), (raw, 0));
    }
}
#[test]
fn failed_installation_releases_its_private_page() {
    for a in ARCHES {
        let mut m = Memory {
            allocation_fails: true,
            ..Default::default()
        };
        assert_eq!(run(a, READ, REGION, &mut m), FaultStatus::OutOfMemory);
        assert_eq!(m.freed, 0);
        for collision in [true, false] {
            let mut m = Memory {
                occupied: collision,
                install_fails: !collision,
                ..Default::default()
            };
            assert_eq!(
                run(a, READ, REGION, &mut m),
                if collision {
                    FaultStatus::AccessViolation
                } else {
                    FaultStatus::OutOfMemory
                }
            );
            assert_eq!((m.allocated, m.freed), (1, 1));
        }
    }
}
#[test]
fn bad_addresses_and_kernel_faults_never_touch_memory() {
    for a in ARCHES {
        let limit = if matches!(a, Riscv64) {
            1 << 38
        } else {
            1 << 47
        };
        for va in [0, 1, 0xfff, limit, u64::MAX] {
            let mut m = Memory::default();
            assert_eq!(
                resolve(a, va, READ, Some(REGION), &mut m),
                FaultStatus::AccessViolation
            );
            assert_eq!(m.allocated, 0);
        }
        assert!(user_address(a, 0x1000));
        assert!(user_address(a, limit - 1));
        for access in [
            AccessFlags {
                user: false,
                ..READ
            },
            AccessFlags {
                reserved_bit: true,
                ..READ
            },
            AccessFlags {
                read: false,
                ..READ
            },
        ] {
            let mut m = Memory::default();
            assert_eq!(run(a, access, REGION, &mut m), FaultStatus::AccessViolation);
            assert_eq!(m.allocated, 0);
        }
    }
}
#[test]
fn arm_descriptors_preserve_execute_never_and_readonly() {
    for user in [false, true] {
        for writable in [false, true] {
            for nx in [false, true] {
                let raw = arm_leaf::encode(0x12345000, arm_leaf::attributes(user, writable, nx));
                assert_eq!(raw & 0x0000fffffffff000, 0x12345000);
                assert_eq!(raw & 64 != 0, user);
                assert_eq!(raw & (1 << 11) != 0, user);
                assert_eq!(raw & 128 == 0, writable);
                assert_eq!(raw & (1 << 53) != 0, user || nx);
                assert_eq!(raw & (1 << 54) != 0, !user || nx);
                assert_ne!(raw & (1 << 10), 0);
            }
        }
    }
}

#[test]
fn loong_descriptors_keep_user_translations_local_to_the_process() {
    for user in [false, true] {
        for writable in [false, true] {
            for nx in [false, true] {
                let raw = loong_leaf::encode(0x12345000, writable, user, nx);
                assert_eq!(raw & 0x0000fffffffff000, 0x12345000);
                assert_eq!(raw & 12, if user { 12 } else { 0 });
                assert_eq!(raw & 64 == 0, user);
                assert_eq!(raw & 256 != 0, writable);
                assert_eq!(raw & 2 != 0, writable);
                assert_eq!(raw & (1 << 62) != 0, nx);
                assert_ne!(raw & 1, 0);
            }
        }
    }
}

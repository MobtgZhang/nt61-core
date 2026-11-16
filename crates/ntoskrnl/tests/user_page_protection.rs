//! Native-format production page walk/encoding, with synthetic retained tables.
#[path = "../src/mm/user_page_protection.rs"]
mod protection;

use protection::{protect_range, Architecture as Arch, Protection};
use std::{cell::RefCell, collections::BTreeMap};

const ARCHES: [Arch; 4] = [Arch::X64, Arch::Arm64, Arch::Riscv64, Arch::Loong64];
type Tables = BTreeMap<(u64, usize), u64>;

fn shifts(arch: Arch) -> &'static [u32] {
    match arch {
        Arch::X64 | Arch::Arm64 => &[39, 30, 21, 12],
        Arch::Riscv64 => &[30, 21, 12],
        Arch::Loong64 => &[48, 39, 30, 21, 12],
    }
}
fn user_leaf(arch: Arch) -> u64 {
    match arch {
        Arch::X64 => 0x12345000 | 7 | 32 | 64 | (1 << 63),
        Arch::Arm64 => {
            0x12345000
                | 3
                | (2 << 2)
                | (3 << 8)
                | (1 << 6)
                | (1 << 10)
                | (1 << 11)
                | (1 << 53)
                | (1 << 54)
        }
        Arch::Riscv64 => (0x12345 << 10) | 1 | 2 | 4 | 16 | 64 | 128,
        Arch::Loong64 => 0x12345000 | 1 | 2 | 12 | 16 | 128 | 256 | (1 << 62),
    }
}
fn tables(arch: Arch, pages: usize) -> (Tables, u64) {
    let mut tables = Tables::new();
    let levels = shifts(arch);
    for level in 0..levels.len() - 1 {
        let next = (level as u64 + 2) * 4096;
        let entry = match arch {
            Arch::X64 => next | 7,
            Arch::Arm64 => next | 3,
            Arch::Riscv64 => (next >> 12 << 10) | 1,
            Arch::Loong64 => next,
        };
        tables.insert(((level as u64 + 1) * 4096, 0), entry);
    }
    let leaf = levels.len() as u64 * 4096;
    for index in 1..=pages {
        tables.insert((leaf, index), user_leaf(arch));
    }
    (tables, leaf)
}
fn change(arch: Arch, map: &RefCell<Tables>, base: u64, size: u64, flags: u32) -> (bool, Vec<u64>) {
    let mut invalidated = vec![];
    let ok = protect_range(
        arch,
        4096,
        base,
        size,
        flags,
        |table, index| map.borrow().get(&(table, index)).copied(),
        |leaf, value| {
            assert_eq!(
                map.borrow_mut().insert((leaf.table, leaf.index), value),
                Some(leaf.value)
            );
            invalidated.push(leaf.va);
            true
        },
    );
    (ok, invalidated)
}
fn accessible(arch: Arch, value: u64) -> bool {
    match arch {
        Arch::X64 => value & 4 != 0,
        Arch::Arm64 => value & 64 != 0,
        Arch::Riscv64 => value & 16 != 0,
        Arch::Loong64 => value & 12 == 12,
    }
}
fn writable(arch: Arch, value: u64) -> bool {
    match arch {
        Arch::X64 => value & 2 != 0,
        Arch::Arm64 => value & 128 == 0,
        Arch::Riscv64 => value & 4 != 0,
        Arch::Loong64 => value & (2 | 256) == (2 | 256),
    }
}
fn executable(arch: Arch, value: u64) -> bool {
    match arch {
        Arch::X64 => value & (1 << 63) == 0,
        Arch::Arm64 => value & (1 << 54) == 0,
        Arch::Riscv64 => value & 8 != 0,
        Arch::Loong64 => value & (1 << 62) == 0,
    }
}

#[test]
fn noaccess_revokes_reads_then_restores_same_frame_on_all_isas() {
    for arch in ARCHES {
        let (initial, leaf) = tables(arch, 1);
        let map = RefCell::new(initial);
        assert_eq!(change(arch, &map, 4096, 4096, 1), (true, vec![4096]));
        let denied = map.borrow()[&(leaf, 1)];
        assert!(!accessible(arch, denied), "{arch:?}");
        assert!(!writable(arch, denied));
        assert!(!executable(arch, denied));
        assert!(change(arch, &map, 4096, 4096, 4).0);
        assert_eq!(map.borrow()[&(leaf, 1)], user_leaf(arch), "{arch:?}");
    }
}

#[test]
fn permission_matrix_preserves_frame_and_cache_state() {
    for arch in ARCHES {
        for (flags, write, execute) in [
            (2, false, false),
            (4, true, false),
            (16, false, true),
            (32, false, true),
            (64, true, true),
        ] {
            let old = user_leaf(arch);
            let new = arch
                .protect(old, Protection::from_page(flags).unwrap())
                .unwrap();
            assert!(accessible(arch, new));
            assert_eq!(writable(arch, new), write, "{arch:?}/{flags}");
            assert_eq!(executable(arch, new), execute, "{arch:?}/{flags}");
            let mask = match arch {
                Arch::X64 => 0x000f_ffff_ffff_f000 | 8 | 16 | 128 | 32 | 64,
                Arch::Arm64 => 0x0000_ffff_ffff_f000 | (7 << 2) | (3 << 8) | (1 << 10),
                Arch::Riscv64 => ((1u64 << 44) - 1) << 10 | 64 | 128,
                Arch::Loong64 => 0x0000_ffff_ffff_f000 | 16 | 32 | 128,
            };
            assert_eq!(new & mask, old & mask);
        }
    }
}

#[test]
fn kernel_leaves_cannot_be_promoted_to_user() {
    for arch in ARCHES {
        let old = user_leaf(arch)
            & !match arch {
                Arch::X64 => 4,
                Arch::Arm64 => 64,
                Arch::Riscv64 => 16,
                Arch::Loong64 => 12,
            };
        assert!(arch
            .protect(old, Protection::from_page(4).unwrap())
            .is_none());
    }
}

#[test]
fn missing_later_page_does_not_change_prefix() {
    for arch in ARCHES {
        let (initial, _) = tables(arch, 1);
        let map = RefCell::new(initial.clone());
        assert_eq!(change(arch, &map, 4096, 8192, 1), (false, vec![]));
        assert_eq!(*map.borrow(), initial);
    }
}

#[test]
fn unaligned_extent_updates_and_invalidates_every_touched_page() {
    for arch in ARCHES {
        let (initial, _) = tables(arch, 2);
        let map = RefCell::new(initial);
        assert_eq!(change(arch, &map, 8191, 2, 2), (true, vec![4096, 8192]));
    }
}

#[test]
fn malformed_ranges_and_unsupported_flags_do_not_touch_page_tables() {
    for arch in ARCHES {
        for (base, size, flags) in [
            (u64::MAX, 1, 4),
            (u64::MAX - 4095, 1, 4),
            (4096, 1, 8),
            (4096, 1, 128),
            (4096, 1, 0x104),
            (0, 1, 4),
            (1 << 47, 1, 4),
        ] {
            assert!(!protect_range(
                arch,
                4096,
                base,
                size,
                flags,
                |_, _| panic!("unexpected read"),
                |_, _| panic!("unexpected write")
            ));
        }
        assert!(protect_range(
            arch,
            0,
            u64::MAX,
            0,
            0,
            |_, _| panic!("empty read"),
            |_, _| panic!("empty write")
        ));
    }
}

#[test]
fn large_leaves_never_read_payload_as_page_tables() {
    for arch in [Arch::X64, Arch::Arm64, Arch::Riscv64] {
        let levels = shifts(arch);
        for level in 0..levels.len() - 1 {
            let (mut map, _) = tables(arch, 1);
            let table = (level as u64 + 1) * 4096;
            map.insert(
                (table, 0),
                match arch {
                    Arch::X64 => 0x200000 | 135,
                    Arch::Arm64 => 0x200000 | 1,
                    Arch::Riscv64 => (0x200000 >> 12 << 10) | 23,
                    _ => unreachable!(),
                },
            );
            let mut reads = 0;
            assert!(arch
                .leaf(4096, 4096, Protection::from_page(4).unwrap(), |t, i| {
                    reads += 1;
                    assert!(reads <= level + 1);
                    map.get(&(t, i)).copied()
                })
                .is_none());
        }
    }
}

#[test]
fn ancestor_permissions_are_never_widened() {
    for (arch, mask) in [
        (Arch::X64, 4),
        (Arch::X64, 2),
        (Arch::Arm64, 1 << 61),
        (Arch::Arm64, 1 << 62),
    ] {
        let (mut map, _) = tables(arch, 1);
        let entry = map.get_mut(&(4096, 0)).unwrap();
        if arch == Arch::X64 {
            *entry &= !mask;
        } else {
            *entry |= mask;
        }
        assert!(arch
            .leaf(4096, 4096, Protection::from_page(4).unwrap(), |t, i| map
                .get(&(t, i))
                .copied())
            .is_none());
    }
}

#[test]
fn failed_atomic_update_is_reported_as_failure() {
    for arch in ARCHES {
        let (map, _) = tables(arch, 1);
        assert!(!protect_range(
            arch,
            4096,
            4096,
            4096,
            1,
            |t, i| map.get(&(t, i)).copied(),
            |_, _| false
        ));
    }
}

#[test]
fn cow_pages_cannot_be_made_writable_or_mistaken_for_noaccess() {
    let cow = user_leaf(Arch::X64) | (1 << 9);
    for flags in [1, 2, 4, 16, 32, 64] {
        assert!(Arch::X64
            .protect(cow, Protection::from_page(flags).unwrap())
            .is_none());
        assert!(Arch::X64
            .protect(cow & !4, Protection::from_page(flags).unwrap())
            .is_none());
    }
}

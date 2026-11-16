#[path = "../src/mm/x64_mapping_presence.rs"]
mod walk;

fn check(entries: &[(u64, usize, u64)], va: u64) -> bool {
    walk::page_is_mapped(0x1000, va, |table, index| {
        entries
            .iter()
            .find(|&&(p, i, _)| p == table && i == index)
            .expect("walker dereferenced a leaf or unexpected page-table entry")
            .2
    })
}

#[test]
fn conventional_four_level_walk_and_missing_entries() {
    let entries = [
        (0x1000, 0, 0x2001),
        (0x2000, 0, 0x3001),
        (0x3000, 0, 0x4001),
        (0x4000, 1, 0x5001),
    ];
    assert!(check(&entries, 0x1000));
    for missing in 0..4 {
        let mut absent = entries;
        absent[missing].2 = 0;
        assert!(!check(&absent, 0x1000));
    }
}

#[test]
fn large_pages_do_not_read_the_backing_data_as_a_table() {
    assert!(check(
        &[(0x1000, 0, 0x2001), (0x2000, 1, 0x8000_1081)],
        0x4567_89ab
    ));
    assert!(check(
        &[
            (0x1000, 0, 0x2001),
            (0x2000, 0, 0x3001),
            (0x3000, 1, 0x601081)
        ],
        0x234567
    ));
}

#[test]
fn reserved_large_leaf_address_bits_are_rejected() {
    assert!(!check(&[(0x1000, 0, 0x2001), (0x2000, 0, 0x2081)], 0));
    assert!(!check(
        &[
            (0x1000, 0, 0x2001),
            (0x2000, 0, 0x3001),
            (0x3000, 0, 0x2081)
        ],
        0
    ));
    assert!(!check(&[(0x1000, 0, 0x81)], 0));
}

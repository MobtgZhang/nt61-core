#[path = "../src/mm/user_mapping_range.rs"]
mod policy;

#[test]
fn presence_checks_reject_overflow_before_touching_page_tables() {
    assert!(!policy::all_pages(u64::MAX - 0xfff, 0x2000, |_| panic!("wrapped walk")));
    assert!(!policy::all_pages(u64::MAX - 1, 1, |_| panic!("rounded walk")));
    assert!(policy::all_pages(u64::MAX, 0, |_| panic!("empty walk")));
}

#[test]
fn presence_checks_visit_both_sides_of_an_unaligned_boundary_and_stop_at_a_hole() {
    let mut visited = Vec::new();
    assert!(!policy::all_pages(0x1fff, 0x2002, |page| {
        visited.push(page);
        page != 0x3000
    }));
    assert_eq!(visited, [0x1000, 0x2000, 0x3000]);
}

#[test]
fn explorer_cannot_replace_the_running_loongarch_stack() {
    assert!(!policy::available(0x1f0000, 0x42bf000, &[(0x433f000, 0x100000)]));
    assert!(policy::available(0x1f0000, 0x42bf000, &[(0x239000000, 0x100000)]));
}

#[test]
fn explorer_bss_cannot_replace_aarch64_bootstrap_mmio() {
    let reserved = &policy::AARCH64_BOOTSTRAP_MMIO;
    assert!(!policy::available(0x001f_0000, 0x0804_2000, reserved));
    assert!(!policy::available(0x0801_000c, 4, reserved));
    assert!(!policy::available(0x0900_0000, 4, reserved));
    assert!(policy::available(0x0003_ffff_0000, 0x0804_2000, reserved));
}

#[test]
fn adjacent_ranges_and_zero_sizes_do_not_overlap() {
    assert!(policy::available(0x1000, 0x1000, &[(0x2000, 0x1000)]));
    assert!(policy::available(0x3000, 0x1000, &[(0x2000, 0x1000)]));
    assert!(policy::available(0x2000, 0, &[(0x2000, 0x1000)]));
    assert!(policy::available(0x2000, 1, &[(0x2000, 0)]));
}

#[test]
fn unaligned_and_containing_ranges_are_checked_at_page_granularity() {
    assert!(!policy::available(0x1fff, 2, &[(0x2001, 1)]));
    assert!(!policy::available(0x1000, 0x5000, &[(0x3000, 1)]));
    assert!(!policy::available(0x3000, 1, &[(0x1000, 0x5000)]));
}

#[test]
fn wrapped_user_and_reserved_extents_fail_closed() {
    assert!(!policy::available(u64::MAX - 0xfff, 0x2000, &[]));
    assert!(!policy::available(u64::MAX, 1, &[]));
    assert!(!policy::available(0x1000, 1, &[(u64::MAX, 1)]));
}

#[test]
fn physical_extent_includes_all_rounded_pages_without_wrapping() {
    assert_eq!(policy::physical_extent(1, 0x1000, 0x7001), Some((0x7000, 0x9000)));
    assert_eq!(policy::physical_extent(1, 0xfff, u64::MAX - 0xfff), None);
    assert_eq!(policy::physical_extent(1, 0x1000, u64::MAX - 0x1fff), None);
}

#[test]
fn high_bit_aliases_and_ranges_crossing_the_user_limit_are_rejected() {
    for bits in [38, 47] {
        let limit = 1u64 << bits;
        assert!(policy::user_extent(limit - 0x1000, 0x1000, limit));
        assert!(!policy::user_extent(limit - 0x1000, 0x1001, limit));
        assert!(!policy::user_extent(limit, 1, limit));
        assert!(!policy::user_extent(0x1_0000_0443_e000, 0x1000, limit));
        assert!(!policy::user_extent(0, 1, limit));
        assert!(policy::user_extent(u64::MAX, 0, limit));
    }
}

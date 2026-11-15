//! Exercise the production fixed-image address policy without kernel state.
#[path = "../src/loader/fixed_base.rs"]
mod fixed_base;

use fixed_base::riscv64_user_image_base;

#[test]
fn rejects_null_page_and_empty_images() {
    for base in [0, 1, 0xfff] {
        assert_eq!(riscv64_user_image_base(base, 0x2000), None);
    }
    assert_eq!(riscv64_user_image_base(0x1f0000, 0), None);
}

#[test]
fn retains_linked_desktop_base_without_a_slide() {
    assert_eq!(riscv64_user_image_base(0x1f0000, 0x4200000), Some(0x1f0000));
    assert_eq!(riscv64_user_image_base(0x1000, 0x1000), Some(0x1000));
}

#[test]
fn complete_extent_must_fit_the_low_sv39_half() {
    let limit = 1u64 << 38;
    assert_eq!(riscv64_user_image_base(limit - 0x1000, 0x1000), Some(limit - 0x1000));
    assert_eq!(riscv64_user_image_base(limit - 0x1000, 0x1001), None);
    assert_eq!(riscv64_user_image_base(limit, 1), None);
    assert_eq!(riscv64_user_image_base(0xffff_ffc0_0000_0000, 0x1000), None);
}

#[test]
fn overflow_cannot_wrap_into_the_user_range() {
    assert_eq!(riscv64_user_image_base(u64::MAX - 0xfff, 0x1000), None);
    assert_eq!(riscv64_user_image_base(0x1000, u64::MAX), None);
}

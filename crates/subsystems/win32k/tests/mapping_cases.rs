//! Mapping changes must affect the production transform and saved DC state.
use super::*;

#[test]
fn virtual_resolution_changes_mapping_metrics_and_is_saved_without_changing_caps() {
    let handle = dc::gre_create_compatible_dc(0);
    use dc::services::nt_gdi_set_virtual_resolution as set;
    assert_eq!(call(set, &[0, 1200, 600, 300, 150]), 0);
    assert_eq!(call(set, &[handle, 1200, 0, 300, 150]), 0);
    assert_eq!(call(set, &[handle, 1200, 600, 300, 150]), 1);
    assert_eq!(extents(handle), ((1, 1), (1, 1)));
    dc::gre_set_map_mode(handle, dc::MM_LOMETRIC);
    assert_eq!(extents(handle), ((1200, -600), (3000, 1500)));
    assert_eq!(call(dc::caps::nt_gdi_get_device_caps, &[handle, 8]), 1024);
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(call(set, &[handle, 800, 400, 100, 50]), 1);
    dc::gre_set_map_mode(handle, dc::MM_TEXT);
    dc::gre_set_map_mode(handle, dc::MM_HIMETRIC);
    assert_eq!(extents(handle), ((800, -400), (10000, 5000)));
    assert!(dc::gre_restore_dc(handle, -1));
    dc::gre_set_map_mode(handle, dc::MM_TEXT);
    dc::gre_set_map_mode(handle, dc::MM_HIMETRIC);
    assert_eq!(extents(handle), ((1200, -600), (30000, 15000)));
    assert_eq!(call(set, &[handle, 0, 0, 0, 0]), 1);
    dc::gre_set_map_mode(handle, dc::MM_TEXT);
    dc::gre_set_map_mode(handle, dc::MM_LOMETRIC);
    assert_eq!(extents(handle).0, (1024, -768));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn invalid_map_modes_fail_without_changing_state() {
    let handle = dc::gre_create_compatible_dc(0);
    for mode in [i32::MIN, -1, 0, 9, i32::MAX] {
        assert_eq!(dc::gre_set_map_mode(handle, mode), 0);
        assert_eq!(dc::gre_get_map_mode(handle), dc::MM_TEXT);
    }
    assert!(dc::gre_delete_dc(handle));
    assert_eq!(dc::gre_set_map_mode(handle, dc::MM_TEXT), 0);
}

#[test]
fn fixed_modes_ignore_extent_setters_even_for_zero_extents() {
    let handle = dc::gre_create_compatible_dc(0);
    let mut old = dc::SizeL::default();
    assert!(dc::gre_set_viewport_ext_ex(handle, 0, 0, Some(&mut old)));
    assert_eq!((old.cx, old.cy), (1, 1));
    assert!(dc::gre_set_window_ext_ex(handle, -40, 20, None));
    let object = dc::get_dc(handle).unwrap();
    assert_eq!((object.viewport_ext.cx, object.viewport_ext.cy), (1, 1));
    assert_eq!((object.window_ext.cx, object.window_ext.cy), (1, 1));
    drop(object);
    assert!(dc::gre_delete_dc(handle));
}

use dc::services::{
    nt_gdi_get_and_set_dc_dword as exchange, nt_gdi_scale_viewport_ext_ex as scale_viewport,
    nt_gdi_scale_window_ext_ex as scale_window,
};

fn exchange_attribute(handle: u64, selector: u64, value: u64) -> (u64, u32) {
    let mut previous = 0xdead_beefu32;
    let pointer = &mut previous as *mut u32 as u64;
    mm::user_copy::allow(pointer, 4, true);
    let result = call(exchange, &[handle, selector, value, pointer]);
    // Read through the same user buffer written by the production handler.
    std::hint::black_box(&mut previous);
    (result, previous)
}

fn extents(handle: u64) -> ((i32, i32), (i32, i32)) {
    let dc = dc::get_dc(handle).unwrap();
    (
        (dc.viewport_ext.cx, dc.viewport_ext.cy),
        (dc.window_ext.cx, dc.window_ext.cy),
    )
}

#[test]
fn physical_modes_map_positive_y_up_and_preserve_origins() {
    let handle = dc::gre_create_compatible_dc(0);
    for mode in dc::MM_LOMETRIC..=dc::MM_TWIPS {
        assert_ne!(dc::gre_set_map_mode(handle, mode), 0);
        let mut dc = dc::get_dc(handle).unwrap();
        dc.viewport_org = dc::PointL::new(10, 20);
        let transform = transforms::page_to_device(&dc).unwrap();
        assert!(transform.m11 > 0.0);
        assert!(transform.m22 < 0.0);
        // Host display: 1024x768 pixels and a 96-DPI estimated monitor size.
        let units_per_inch = match mode {
            dc::MM_LOMETRIC => 254.0,
            dc::MM_HIMETRIC => 2540.0,
            dc::MM_LOENGLISH => 100.0,
            dc::MM_HIENGLISH => 1000.0,
            dc::MM_TWIPS => 1440.0,
            _ => unreachable!(),
        };
        assert!((f64::from(transform.m11) * units_per_inch - 96.0).abs() < 0.1);
        assert!((f64::from(transform.m22) * units_per_inch + 96.0).abs() < 0.1);
        assert_eq!((transform.dx, transform.dy), (10.0, 20.0));
    }
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn anisotropic_mode_preserves_existing_extents_and_rejects_zero() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(
        dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC),
        dc::MM_TEXT
    );
    assert!(dc::gre_set_viewport_ext_ex(handle, -12, 9, None));
    assert!(dc::gre_set_window_ext_ex(handle, 6, -3, None));
    assert!(!dc::gre_set_viewport_ext_ex(handle, 0, 99, None));
    assert!(!dc::gre_set_window_ext_ex(handle, 99, 0, None));
    assert_eq!(extents(handle), ((-12, 9), (6, -3)));
    assert_eq!(
        dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC),
        dc::MM_ANISOTROPIC
    );
    assert_eq!(extents(handle), ((-12, 9), (6, -3)));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn isotropic_transform_reduces_larger_scale_without_losing_signs() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(dc::gre_set_map_mode(handle, dc::MM_ISOTROPIC), dc::MM_TEXT);
    assert!(dc::gre_set_window_ext_ex(handle, 10, -10, None));
    assert!(dc::gre_set_viewport_ext_ex(handle, -100, 40, None));
    let dc = dc::get_dc(handle).unwrap();
    let first = transforms::page_to_device(&dc).unwrap();
    let second = transforms::page_to_device(&dc).unwrap();
    assert_eq!(first.m11, -4.0);
    assert_eq!(first.m22, -4.0);
    assert_eq!(first.m11, second.m11);
    drop(dc);
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn scaling_uses_wrapping_product_and_signed_truncating_division() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC);
    dc::gre_set_viewport_ext_ex(handle, 7, -7, None);
    assert_eq!(call(scale_viewport, &[handle, 2, 3, 2, 3, 0]), 1);
    assert_eq!(extents(handle).0, (4, -4));
    dc::gre_set_window_ext_ex(handle, i32::MAX, 3, None);
    assert_eq!(call(scale_window, &[handle, 2, 1, 2, 1, 0]), 1);
    assert_eq!(extents(handle).1, (-2, 6));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn failed_viewport_scale_writes_old_extent_but_window_does_not() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC);
    dc::gre_set_viewport_ext_ex(handle, 6, 8, None);
    dc::gre_set_window_ext_ex(handle, 3, 4, None);
    let mut output = dc::SizeL::new(-99, -99);
    let pointer = &mut output as *mut dc::SizeL as u64;
    mm::user_copy::allow(pointer, 8, true);
    for factors in [[1, 0, 1, 1], [0, 1, 1, 1], [1, 1, 1, 0], [1, 1, 0, 1]] {
        let args = [
            handle, factors[0], factors[1], factors[2], factors[3], pointer,
        ];
        assert_eq!(call(scale_viewport, &args), 0);
        assert_eq!((output.cx, output.cy), (6, 8));
        output = dc::SizeL::new(-99, -99);
        assert_eq!(call(scale_window, &args), 0);
        assert_eq!((output.cx, output.cy), (-99, -99));
    }
    assert_eq!(extents(handle), ((6, 8), (3, 4)));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn failed_output_before_viewport_scale_and_after_window_scale() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC);
    assert_eq!(call(scale_viewport, &[handle, 2, 1, 3, 1, 1]), 0);
    assert_eq!(extents(handle).0, (1, 1));
    assert_eq!(call(scale_window, &[handle, 2, 1, 3, 1, 1]), 0);
    assert_eq!(extents(handle).1, (2, 3));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn fixed_modes_ignore_bad_factors_and_scaling_preserves_saved_state() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(scale_viewport, &[handle, 0, 0, 0, 0, 0]), 1);
    assert_eq!(call(scale_window, &[handle, 0, 0, 0, 0, 0]), 1);
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(exchange_attribute(handle, 8, 8), (1, 1));
    assert_eq!(call(scale_viewport, &[handle, 2, 1, 3, 1, 0]), 1);
    assert_eq!(extents(handle).0, (2, 3));
    assert!(dc::gre_restore_dc(handle, -1));
    assert_eq!(dc::gre_get_map_mode(handle), dc::MM_TEXT);
    assert_eq!(extents(handle), ((1, 1), (1, 1)));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn unaligned_window_output_is_valid_but_viewport_requires_dword_alignment() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC);
    let mut bytes = [0xabu8; 16];
    let aligned = (bytes.as_mut_ptr() as u64 + 3) & !3;
    let pointer = aligned + 1;
    mm::user_copy::allow(pointer, 8, true);
    assert_eq!(call(scale_viewport, &[handle, 2, 1, 3, 1, pointer]), 0);
    assert_eq!(extents(handle).0, (1, 1));
    assert_eq!(bytes, [0xab; 16]);
    assert_eq!(call(scale_window, &[handle, 2, 1, 3, 1, pointer]), 1);
    let offset = (pointer - bytes.as_ptr() as u64) as usize;
    assert_eq!(&bytes[offset..offset + 8], &[1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(extents(handle).1, (2, 3));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn scale_division_overflow_fails_atomically_without_panicking() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC);
    dc::gre_set_viewport_ext_ex(handle, i32::MIN, 7, None);
    assert_eq!(call(scale_viewport, &[handle, 1, u64::MAX, 2, 1, 0]), 0);
    assert_eq!(extents(handle).0, (i32::MIN, 7));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn exchange_attributes_validate_values_and_truncate_dword_slots() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(
        exchange_attribute(handle, 0x1234_0000_0004, 0x9876_0000_0002),
        (1, 1)
    );
    assert_eq!(exchange_attribute(handle, 4, 3), (1, 2));
    assert_eq!(exchange_attribute(handle, 7, 1), (1, 0));
    assert_eq!(exchange_attribute(handle, 7, 2), (1, u32::MAX));
    assert_eq!(exchange_attribute(handle, 7, 0), (1, 1));
    assert_eq!(exchange_attribute(handle, 9, 2), (1, 1));
    assert_eq!(exchange_attribute(handle, 9, 99), (1, 0));
    assert_eq!(exchange_attribute(handle, 9, 1), (1, 2));
    assert_eq!(exchange_attribute(handle, 8, 99), (1, 0));
    assert_eq!(dc::gre_get_map_mode(handle), dc::MM_TEXT);
    for selector in [0, 3, 5, 10, u32::MAX as u64] {
        assert_eq!(exchange_attribute(handle, selector, 0), (0, 0xdead_beef));
    }
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn exchange_commits_before_output_failure_and_attributes_restore() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(call(exchange, &[handle, 4, 2, 0]), 0);
    assert_eq!(exchange_attribute(handle, 4, 3), (1, 2));
    assert_eq!(exchange_attribute(handle, 7, 1), (1, 0));
    assert_eq!(exchange_attribute(handle, 9, 2), (1, 1));
    assert!(dc::gre_restore_dc(handle, -1));
    let dc = dc::get_dc(handle).unwrap();
    assert_eq!(dc.attributes.relative_mode, 1);
    assert_eq!(dc.attributes.mapper_flags, 0);
    assert_eq!(dc.attributes.arc_direction, 1);
    drop(dc);
    assert_eq!(call(exchange, &[handle, 8, 8, 0]), 0);
    assert_eq!(dc::gre_get_map_mode(handle), dc::MM_ANISOTROPIC);
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn eps_copy_count_and_invalid_dc_special_output() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::get_dc(handle).unwrap().eps_printing = true;
    assert_eq!(exchange_attribute(handle, 1, u64::MAX), (1, 0x400));
    assert_eq!(exchange_attribute(handle, 1, 0), (1, 0));
    assert_eq!(exchange_attribute(handle, 2, 4), (1, 1));
    assert_eq!(exchange_attribute(handle, 2, 2), (1, 4));
    assert_eq!(exchange_attribute(handle, 6, 0xffff), (1, 0));
    assert!(dc::gre_delete_dc(handle));
    assert_eq!(exchange_attribute(handle, 5, 0), (0, 0x8000_0000));
    assert_eq!(exchange_attribute(handle, 8, 0), (0, 0xdead_beef));
    assert_eq!(call(scale_viewport, &[handle, 1, 1, 1, 1, 0]), 0);
    assert_eq!(call(scale_window, &[handle, 1, 1, 1, 1, 0]), 0);
}

#[test]
fn dc_point_uses_win7_bit_selectors_and_current_position() {
    let handle = dc::gre_create_compatible_dc(0);
    {
        let mut dc = dc::get_dc(handle).unwrap();
        dc.viewport_org = dc::PointL::new(4, 5);
        dc.window_org = dc::PointL::new(8, 9);
        dc.cur_x = 16;
        dc.cur_y = 17;
    }
    let mut point = dc::PointL::default();
    let pointer = &mut point as *mut dc::PointL as u64;
    mm::user_copy::allow(pointer, 8, true);
    for (selector, expected) in [
        (4, (4, 5)),
        (8, (8, 9)),
        (16, (16, 17)),
        (32, (0, 0)),
        (0x44, (4, 5)),
    ] {
        assert_eq!(
            call(
                transforms::nt_gdi_get_dc_point,
                &[handle, selector, pointer]
            ),
            1
        );
        assert_eq!((point.x, point.y), expected);
    }
    for selector in [0, 3, 6, 0x60] {
        assert_eq!(
            call(
                transforms::nt_gdi_get_dc_point,
                &[handle, selector, pointer]
            ),
            0
        );
        assert_eq!((point.x, point.y), (4, 5));
    }
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn viewport_query_realizes_isotropic_extent_before_subsequent_scaling() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::gre_set_map_mode(handle, dc::MM_ISOTROPIC);
    dc::gre_set_window_ext_ex(handle, 10, 10, None);
    dc::gre_set_viewport_ext_ex(handle, 100, 40, None);
    let mut point = dc::PointL::default();
    let pointer = &mut point as *mut dc::PointL as u64;
    mm::user_copy::allow(pointer, 8, true);
    assert_eq!(
        call(transforms::nt_gdi_get_dc_point, &[handle, 1, pointer]),
        1
    );
    assert_eq!((point.x, point.y), (40, 40));
    assert_eq!(extents(handle).0, (40, 40));
    assert_eq!(call(scale_viewport, &[handle, 2, 1, 2, 1, pointer]), 1);
    assert_eq!((point.x, point.y), (40, 40));
    assert_eq!(extents(handle).0, (80, 80));
    assert!(dc::gre_delete_dc(handle));
}

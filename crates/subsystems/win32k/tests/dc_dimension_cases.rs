//! DC device dimensions and the original Win7 isotropic consumer contract.
use super::*;
use dc::services::{nt_gdi_get_device_width as width, nt_gdi_set_size_device as set_size};

#[test]
fn device_width_tracks_creation_selection_restore_and_boundary_wraparound() {
    let display = dc::gre_create_display_dc();
    assert_eq!(call(width, &[display]), 1024);
    let handle = dc::gre_create_compatible_dc(display);
    assert_eq!(call(width, &[handle]), 1);
    let first = objects::gdi_create_bitmap(37, -19, 1);
    let second = objects::gdi_create_bitmap(13, 7, 32);
    dc::gre_select_object(handle, first);
    assert_eq!(call(width, &[handle]), 37);
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(dc::gre_select_object(handle, second), first);
    assert_eq!(call(width, &[handle]), 13);
    assert!(dc::gre_restore_dc(handle, -1));
    assert_eq!(call(width, &[handle]), 37);
    // Neither virtual-device metrics, mapping nor clipping redefine width.
    assert_eq!(call(set_size, &[handle, 100, 200]), 1);
    dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC);
    dc::gre_set_viewport_ext_ex(handle, 9, 3, None);
    assert_eq!(call(width, &[handle]), 37);
    for (left, right) in [(i32::MIN, i32::MAX), (i32::MAX, i32::MIN), (100, -3)] {
        dc::gre_set_dcbounds(handle, &objects::Rect::new(left, 0, right, 1));
        assert_eq!(call(width, &[handle]), right.wrapping_sub(left) as u32 as u64);
    }
    assert!(dc::gre_delete_dc(handle));
    assert_eq!(call(width, &[handle]), u32::MAX as u64);
    assert_eq!(call(width, &[0]), u32::MAX as u64);
    assert!(objects::gdi_delete_object(first));
    assert!(objects::gdi_delete_object(second));
    assert!(dc::gre_delete_dc(display));
}

#[test]
fn size_device_truncates_dwords_rejects_zero_and_restores_saved_metrics() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(set_size, &[0, 1, 1]), 0);
    assert_eq!(call(set_size, &[handle, 1, 2]), 1);
    for (x, y) in [(0, 2), (1, 0), (1 << 32, 2), (1, 1 << 32)] {
        assert_eq!(call(set_size, &[handle, x, y]), 0);
        let dc = dc::get_dc(handle).unwrap();
        assert_eq!((dc.attributes.size_device.cx, dc.attributes.size_device.cy), (1, 2));
    }
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(call(set_size, &[handle, u64::MAX, 0x1_8000_0000]), 1);
    {
        let dc = dc::get_dc(handle).unwrap();
        assert_eq!((dc.attributes.size_device.cx, dc.attributes.size_device.cy), (-1, i32::MIN));
    }
    assert!(dc::gre_restore_dc(handle, -1));
    {
        let dc = dc::get_dc(handle).unwrap();
        assert_eq!((dc.attributes.size_device.cx, dc.attributes.size_device.cy), (1, 2));
    }
    assert_eq!(call(width, &[handle]), 1);
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn size_device_changes_real_transform_and_native_query_can_realize_zero() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(dc::services::nt_gdi_set_virtual_resolution, &[handle, 100, 100, 100, 100]), 1);
    dc::gre_set_map_mode(handle, dc::MM_ISOTROPIC);
    dc::gre_set_window_ext_ex(handle, 100, 100, None);
    dc::gre_set_viewport_ext_ex(handle, 100, 100, None);
    assert_eq!(call(set_size, &[handle, 100, 200]), 1);
    {
        let dc = dc::get_dc(handle).unwrap();
        let transform = transforms::page_to_device(&dc).unwrap();
        assert_eq!((transform.m11, transform.m22), (1.0, 0.5));
    }
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(call(set_size, &[handle, 100, 100_000]), 1);
    let mut point = dc::PointL::new(-1, -1);
    let pointer = &mut point as *mut dc::PointL as u64;
    mm::user_copy::allow(pointer, 8, true);
    assert_eq!(call(transforms::nt_gdi_get_dc_point, &[handle, 1, pointer]), 1);
    assert_eq!((point.x, point.y), (100, 0));
    {
        let dc = dc::get_dc(handle).unwrap();
        let transform = transforms::page_to_device(&dc).unwrap();
        assert_eq!((dc.viewport_ext.cx, dc.viewport_ext.cy), (100, 0));
        assert_eq!((transform.m11, transform.m22), (1.0, 0.0));
    }
    assert!(dc::gre_restore_dc(handle, -1));
    {
        let dc = dc::get_dc(handle).unwrap();
        let transform = transforms::page_to_device(&dc).unwrap();
        assert_eq!((transform.m11, transform.m22), (1.0, 0.5));
    }
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn isotropic_extent_matches_2528_original_win7_instruction_results() {
    // SHA256 335f286e3b73a4111d459aa1114c9301752d96f6ba784954112d5df27e9f9ac8.
    // Original vMakeIso, EngMulDiv and bFToL instructions, synthetic DC_ATTR.
    // Equal 96-DPI axes are supplied for non-virtual display cases.
    let fixture = include_bytes!("data/win7-dc-dimensions-x64.bin");
    assert_eq!(fixture.len(), 2528 * 48);
    let mut dc = dc::DcObject::new();
    dc.map_mode = dc::MM_ISOTROPIC;
    for (index, record) in fixture.chunks_exact(48).enumerate() {
        let values: [i32; 12] = core::array::from_fn(|i| {
            i32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap())
        });
        dc.attributes.virtual_resolution = core::array::from_fn(|i| values[i] as u32);
        dc.attributes.size_device = dc::SizeL::new(values[4], values[5]);
        dc.window_ext = dc::SizeL::new(values[6], values[7]);
        dc.viewport_ext = dc::SizeL::new(values[8], values[9]);
        let actual = dc::mapping::effective_viewport(&dc).unwrap();
        assert_eq!((actual.cx, actual.cy), (values[10], values[11]), "fixture {index}: {values:?}");
    }
}

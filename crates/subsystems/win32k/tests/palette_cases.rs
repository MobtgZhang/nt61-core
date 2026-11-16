use super::*;
use dc::caps::*;
use palette::*;

#[test]
fn system_palette_policy_respects_the_true_color_device_capability() {
    let dc = dc::gre_create_compatible_dc(0);
    assert_eq!(call(nt_gdi_get_system_palette_use, &[0]), 1);
    assert_eq!(call(nt_gdi_get_system_palette_use, &[u64::MAX]), 1);
    assert_eq!(call(nt_gdi_get_device_caps, &[dc, 38]) & 0x100, 0);
    for handle in [dc, 0, u64::MAX] {
        for policy in [0, 1, 2, 3, 4, u32::MAX as u64] {
            assert_eq!(call(nt_gdi_set_system_palette_use, &[handle, policy]), 0);
            assert_eq!(call(nt_gdi_get_system_palette_use, &[handle]), 1);
        }
    }
    assert!(dc::gre_delete_dc(dc));
}

#[test]
fn halftone_palette_contains_reserved_colors_and_rgb_cube() {
    let handle = call(nt_gdi_create_halftone_palette, &[0]);
    assert_ne!(handle, 0);
    assert_eq!(palette_size(handle), 256);
    let system = objects::gdi_get_stock_object(15);
    assert_eq!(&entries(handle, 0, 10).1[..40], &entries(system, 0, 10).1[..40]);
    assert_eq!(&entries(handle, 246, 10).1[..40], &entries(system, 10, 10).1[..40]);
    for (index, rgb) in [(10, [64, 32, 0, 0]), (64, [0, 0, 64, 0]),
        (127, [224, 224, 64, 0]), (245, [160, 192, 192, 0])] {
        assert_eq!(&entries(handle, index, 1).1[..4], &rgb);
    }
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn selected_object_query_returns_owned_handles_and_rejects_public_ordinals() {
    let dc = dc::gre_create_compatible_dc(0);
    for (selector, stock) in [
        (0x0030_0000, 7),
        (0x0050_0000, 7),
        (0x0010_0000, 0),
        (0x0008_0000, 15),
        (0x000a_0000, 13),
    ] {
        assert_eq!(
            call(selection::nt_gdi_get_dc_object, &[dc, selector]),
            objects::gdi_get_stock_object(stock)
        );
    }
    for selector in [0, 1, 2, 3, 4, 5, 6, 7, 8, 0x0009_0000, u32::MAX as u64] {
        assert_eq!(call(selection::nt_gdi_get_dc_object, &[dc, selector]), 0);
    }
    assert_eq!(call(selection::nt_gdi_get_dc_object, &[0, 0x0030_0000]), 0);
    let palette = create_palette(&[[3, 4, 5, 0]]);
    call(nt_user_select_palette, &[dc, palette, 0]);
    assert_eq!(
        call(selection::nt_gdi_get_dc_object, &[dc, 0x0008_0000]),
        palette
    );
    assert!(dc::gre_delete_dc(dc));
    assert_eq!(call(selection::nt_gdi_get_dc_object, &[dc, 0x0008_0000]), 0);
    assert!(objects::gdi_delete_object(palette));
}

fn create_palette(entries: &[[u8; 4]]) -> u64 {
    let mut bytes = vec![0, 3, entries.len() as u8, (entries.len() >> 8) as u8];
    bytes.extend(entries.iter().flatten());
    mm::user_copy::allow(bytes.as_ptr() as u64, bytes.len(), false);
    call(
        nt_gdi_create_palette_internal,
        &[bytes.as_ptr() as u64, entries.len() as u64],
    )
}
fn entries(handle: u64, start: u64, count: usize) -> (u64, Vec<u8>) {
    let mut bytes = vec![0xab; count * 4 + 4];
    mm::user_copy::allow(bytes.as_mut_ptr() as u64, count * 4, true);
    let result = call(
        nt_gdi_do_palette,
        &[handle, start, count as u64, bytes.as_mut_ptr() as u64, 2, 0],
    );
    (result, bytes)
}
fn palette_size(handle: u64) -> u64 {
    call(nt_gdi_do_palette, &[handle, 0, 0, 0, 2, 0])
}
#[test]
fn stock_handles_have_correct_types_identity_and_immutable_lifetimes() {
    for index in 0..18 {
        let handle = call(objects::nt_gdi_get_stock_object, &[index]);
        if index == 9 {
            assert_eq!(handle, 0);
            continue;
        }
        let expected = match index {
            0..=5 => objects::GdiObjectType::Brush,
            6..=8 => objects::GdiObjectType::Pen,
            15 => objects::GdiObjectType::Palette,
            _ => objects::GdiObjectType::Font,
        };
        assert_eq!(objects::gdi_get_object_type(handle), Some(expected));
        assert_eq!(objects::gdi_get_stock_object(index as i32), handle);
        assert!(objects::gdi_delete_object(handle));
        assert!(objects::gdi_is_handle_valid(handle));
    }
    assert_eq!(objects::gdi_get_stock_object(-1), 0);
    assert_eq!(objects::gdi_get_stock_object(20), 0);
    assert_eq!(
        objects::gdi_get_brush_color(objects::gdi_get_stock_object(0)),
        0xffffff
    );
    assert_eq!(
        objects::gdi_get_brush_color(objects::gdi_get_stock_object(4)),
        0
    );
}
#[test]
fn dc_defaults_are_real_stock_objects_and_survive_save_restore() {
    let handle = dc::gre_create_compatible_dc(0);
    {
        let dc = dc::get_dc(handle).unwrap();
        assert_eq!(dc.pen, objects::gdi_get_stock_object(7));
        assert_eq!(dc.brush, objects::gdi_get_stock_object(0));
        assert_eq!(dc.font, objects::gdi_get_stock_object(13));
        assert_eq!(dc.palette_handle, objects::gdi_get_stock_object(15));
    }
    let palette = create_palette(&[[10, 20, 30, 0]]);
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(
        call(nt_user_select_palette, &[handle, palette, 1]),
        objects::gdi_get_stock_object(15)
    );
    assert!(!objects::gdi_delete_object(palette));
    assert!(dc::gre_restore_dc(handle, -1));
    assert!(objects::gdi_delete_object(palette));
    assert!(dc::gre_delete_dc(handle));
}
#[test]
fn palette_creation_captures_input_and_returns_distinct_owned_objects() {
    let first = create_palette(&[[1, 2, 3, 0xff], [4, 5, 6, 0]]);
    let second = create_palette(&[[7, 8, 9, 0]]);
    assert_ne!(first, 0);
    assert_ne!(first, second);
    let (count, bytes) = entries(first, 0, 3);
    assert_eq!(count, 2);
    assert_eq!(&bytes[..8], &[1, 2, 3, 7, 4, 5, 6, 0]);
    assert!(bytes[8..].iter().all(|&byte| byte == 0xab));
    assert_eq!(palette_size(first), 2);
    assert!(objects::gdi_delete_object(first));
    assert_eq!(palette_size(first), 0);
    assert!(objects::gdi_delete_object(second));
}
#[test]
fn palette_user_pointer_failures_do_not_create_or_mutate_objects() {
    assert_eq!(call(nt_gdi_create_palette_internal, &[0x1000, 5]), 0);
    assert_eq!(call(nt_gdi_create_palette_internal, &[0, 0]), 0);
    assert_eq!(
        call(nt_gdi_create_palette_internal, &[0, u32::MAX as u64]),
        0
    );
    let handle = create_palette(&[[1, 2, 3, 0]]);
    assert_eq!(call(nt_gdi_do_palette, &[handle, 0, 1, 0x1000, 1, 1]), 0);
    assert_eq!(call(nt_gdi_do_palette, &[handle, 0, 1, 0x1000, 2, 0]), 0);
    assert_eq!(&entries(handle, 0, 1).1[..4], &[1, 2, 3, 0]);
    assert!(objects::gdi_delete_object(handle));
}
#[test]
fn palette_set_entries_and_animation_update_only_reserved_colors() {
    let handle = create_palette(&[[1, 2, 3, 0], [4, 5, 6, 1]]);
    let update = [30u8, 31, 32, 0, 40, 41, 42, 1];
    mm::user_copy::allow(update.as_ptr() as u64, update.len(), false);
    assert_eq!(
        call(
            nt_gdi_do_palette,
            &[handle, 0, 2, update.as_ptr() as u64, 0, 1]
        ),
        1
    );
    assert_eq!(&entries(handle, 0, 2).1[..8], &[1, 2, 3, 0, 40, 41, 42, 1]);
    assert_eq!(
        call(
            nt_gdi_do_palette,
            &[handle, 0, 2, update.as_ptr() as u64, 1, 1]
        ),
        2
    );
    assert_eq!(&entries(handle, 0, 2).1[..8], &update);
    assert!(objects::gdi_delete_object(handle));
}
#[test]
fn palette_resize_preserves_prefix_and_zeroes_new_entries_including_regrowth() {
    let handle = create_palette(&[[1, 2, 3, 0], [4, 5, 6, 0]]);
    assert_eq!(call(nt_gdi_resize_palette, &[handle, 300]), 1);
    assert_eq!(palette_size(handle), 300);
    assert_eq!(&entries(handle, 299, 1).1[..4], &[0; 4]);
    assert_eq!(call(nt_gdi_resize_palette, &[handle, 1]), 1);
    assert_eq!(call(nt_gdi_resize_palette, &[handle, 2]), 1);
    assert_eq!(&entries(handle, 0, 2).1[..8], &[1, 2, 3, 0, 0, 0, 0, 0]);
    assert_eq!(call(nt_gdi_resize_palette, &[handle, u32::MAX as u64]), 0);
    assert_eq!(palette_size(handle), 2);
    assert!(objects::gdi_delete_object(handle));
}
#[test]
fn logical_palettes_can_exceed_the_hardware_256_color_limit() {
    let handle = create_palette(&vec![[100, 101, 102, 0]; 300]);
    assert_ne!(handle, 0);
    assert_eq!(palette_size(handle), 300);
    assert_eq!(&entries(handle, 299, 1).1[..4], &[100, 101, 102, 0]);
    assert!(objects::gdi_delete_object(handle));
}
#[test]
fn nearest_palette_color_uses_rgb_distance_with_first_entry_tie_breaking() {
    let handle = create_palette(&[[10, 0, 0, 0], [20, 0, 0, 0], [0, 0, 255, 0]]);
    assert_eq!(call(nt_gdi_get_nearest_palette_index, &[handle, 15]), 0);
    assert_eq!(call(nt_gdi_get_nearest_palette_index, &[handle, 19]), 1);
    assert_eq!(
        call(nt_gdi_get_nearest_palette_index, &[handle, 0x00fe0000]),
        2
    );
    assert!(objects::gdi_delete_object(handle));
}
#[test]
fn default_palette_is_twenty_system_colors_and_rejects_mutation() {
    let handle = objects::gdi_get_stock_object(15);
    assert_eq!(palette_size(handle), 20);
    assert_eq!(
        &entries(handle, 8, 2).1[..8],
        &[192, 220, 192, 0, 166, 202, 240, 0]
    );
    assert_eq!(&entries(handle, 19, 1).1[..4], &[255, 255, 255, 0]);
    assert_eq!(call(nt_gdi_resize_palette, &[handle, 10]), 0);
    let update = [1u8, 2, 3, 0];
    mm::user_copy::allow(update.as_ptr() as u64, 4, false);
    assert_eq!(
        call(
            nt_gdi_do_palette,
            &[handle, 0, 1, update.as_ptr() as u64, 1, 1]
        ),
        0
    );
}
#[test]
fn palette_word_parameters_truncate_and_selection_rejects_wrong_object_types() {
    let handle = create_palette(&[[1, 2, 3, 0], [4, 5, 6, 0]]);
    assert_eq!(&entries(handle, 0x10001, 1).1[..4], &[4, 5, 6, 0]);
    let dc = dc::gre_create_compatible_dc(0);
    assert_eq!(
        call(
            nt_user_select_palette,
            &[dc, objects::gdi_get_stock_object(0), 0]
        ),
        0
    );
    assert_eq!(
        call(nt_user_select_palette, &[dc, handle, 0]),
        objects::gdi_get_stock_object(15)
    );
    assert!(dc::gre_delete_dc(dc));
    assert!(objects::gdi_delete_object(handle));
}
#[test]
fn device_cap_queries_agree_with_bulk_layout_and_dc_mapping_model() {
    let handle = dc::gre_create_compatible_dc(0);
    let mut caps = [0xdeadbeefu32; 37];
    mm::user_copy::allow(caps.as_mut_ptr() as u64, 144, true);
    assert_eq!(
        call(
            nt_gdi_get_device_caps_all,
            &[handle, caps.as_mut_ptr() as u64]
        ),
        1
    );
    for (index, slot) in [
        (0, 0),
        (2, 1),
        (4, 4),
        (6, 5),
        (8, 6),
        (10, 7),
        (12, 8),
        (38, 13),
        (88, 17),
        (90, 18),
        (104, 19),
        (108, 20),
        (118, 27),
        (117, 28),
        (120, 34),
        (121, 35),
    ] {
        assert_eq!(
            call(nt_gdi_get_device_caps, &[handle, index]),
            caps[slot] as u64
        );
    }
    assert_eq!(
        (caps[6], caps[7], caps[8], caps[17], caps[18]),
        (1024, 768, 32, 96, 96)
    );
    assert_eq!(caps[19], 0);
    assert_eq!(caps[13] & 0x100, 0); // no RC_PALETTE
    assert_eq!(caps[36], 0xdeadbeef);
    assert!(dc::gre_delete_dc(handle));
}
#[test]
fn invalid_device_cap_queries_preserve_outputs() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(nt_gdi_get_device_caps, &[0, 8]), 0);
    assert_eq!(call(nt_gdi_get_device_caps, &[handle, u32::MAX as u64]), 0);
    assert_eq!(call(nt_gdi_get_device_caps_all, &[handle, 0x1000]), 0);
    assert!(dc::gre_delete_dc(handle));
    assert_eq!(call(nt_gdi_get_device_caps, &[handle, 8]), 0);
}

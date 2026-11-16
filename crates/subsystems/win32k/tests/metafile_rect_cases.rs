use super::*;
use transforms::metafile::nt_gdi_convert_metafile_rect as convert;

fn convert_rect(handle: u64, input: [i32; 4]) -> (u64, [i32; 4]) {
    let mut rect = input;
    let pointer = rect.as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, 16, true);
    let result = call(convert, &[handle, pointer]);
    std::hint::black_box(&mut rect);
    (result, rect)
}

#[test]
fn metafile_rect_excludes_last_device_pixel_and_reports_empty_bounds() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(
        convert_rect(handle, [10, 20, 30, 40]),
        (2, [10, 20, 29, 39])
    );
    assert_eq!(
        convert_rect(handle, [30, 40, 10, 20]),
        (2, [10, 20, 29, 39])
    );
    assert_eq!(convert_rect(handle, [10, 20, 10, 20]), (1, [10, 20, 9, 19]));
    assert_eq!(
        convert_rect(handle, [10, 20, 11, 21]),
        (2, [10, 20, 10, 20])
    );
    // Identity conversion must preserve integer precision beyond f32's 24-bit
    // mantissa; the original shifts LONG coordinates directly to POINTFIX.
    assert_eq!(
        convert_rect(handle, [20_000_001, -20_000_001, 20_000_011, -19_999_991]),
        (2, [20_000_001, -20_000_001, 20_000_010, -19_999_992])
    );
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn metafile_rect_uses_device_pixels_under_scaled_and_reflected_mapping() {
    let handle = dc::gre_create_compatible_dc(0);
    dc::gre_set_map_mode(handle, dc::MM_ANISOTROPIC);
    dc::gre_set_window_ext_ex(handle, 2, 2, None);
    dc::gre_set_viewport_ext_ex(handle, 1, -1, None);
    assert_eq!(
        convert_rect(handle, [-10, -20, 30, 40]),
        (2, [-10, 40, 28, -18])
    );
    assert_eq!(dc::gre_save_dc(handle), 1);
    dc::gre_set_viewport_ext_ex(handle, 2, 2, None);
    assert_eq!(
        convert_rect(handle, [-10, -20, 30, 40]),
        (2, [-10, -20, 29, 39])
    );
    assert!(dc::gre_restore_dc(handle, -1));
    assert_eq!(
        convert_rect(handle, [-10, -20, 30, 40]),
        (2, [-10, 40, 28, -18])
    );
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn metafile_rect_rejects_bad_buffers_handles_and_singular_transform_without_write() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(convert, &[handle, 1]), 0);
    assert_eq!(convert_rect(0, [10, 20, 30, 40]), (0, [10, 20, 30, 40]));
    let read_only = [10i32, 20, 30, 40];
    let pointer = read_only.as_ptr() as u64;
    mm::user_copy::allow(pointer, 16, false);
    assert_eq!(call(convert, &[handle, pointer]), 0);
    assert_eq!(read_only, [10, 20, 30, 40]);
    {
        let mut object = dc::get_dc(handle).unwrap();
        object.world_transform.m11 = 0.0;
    }
    assert_eq!(
        convert_rect(handle, [10, 20, 30, 40]),
        (0, [10, 20, 30, 40])
    );
    assert!(dc::gre_delete_dc(handle));
}

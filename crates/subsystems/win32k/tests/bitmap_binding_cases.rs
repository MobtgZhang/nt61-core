//! Exercise the memory-DC creation/selection/rendering chain with a live device
//! surface. A selected bitmap must replace that device binding, not alias it.
use super::*;

#[test]
fn selected_memory_bitmap_pixels_do_not_modify_the_compatible_device() {
    let mut screen = surface::GdiSurface::new(4, 4, 32).unwrap();
    let display = dc::get_dc_table().create(1, (&mut screen as *mut _) as u64);
    let memory = dc::gre_create_compatible_dc(display);
    let bitmap = objects::gdi_create_bitmap(4, 4, 32);
    assert_ne!(bitmap, 0);
    dc::gre_select_object(memory, bitmap);
    assert_eq!(
        call(raster::nt_gdi_set_pixel, &[display, 1, 1, 0x123456]),
        0x123456
    );
    assert_eq!(
        call(raster::nt_gdi_set_pixel, &[memory, 1, 1, 0x654321]),
        0x654321
    );
    let actual = call(raster::nt_gdi_get_pixel, &[display, 1, 1]);
    assert!(dc::gre_delete_dc(memory));
    assert!(dc::gre_delete_dc(display));
    assert!(objects::gdi_delete_object(bitmap));
    assert_eq!(
        actual, 0x123456,
        "a selected bitmap must own its drawing pixels"
    );
}

#[test]
fn restoring_a_memory_dc_restores_pixels_and_keeps_saved_bitmaps_alive() {
    let memory = dc::gre_create_compatible_dc(0);
    let first = objects::gdi_create_bitmap(4, 4, 32);
    let second = objects::gdi_create_bitmap(2, 2, 32);
    dc::gre_select_object(memory, first);
    assert_eq!(
        call(raster::nt_gdi_set_pixel, &[memory, 1, 1, 0x123456]),
        0x123456
    );
    assert_eq!(dc::gre_save_dc(memory), 1);
    assert_eq!(dc::gre_select_object(memory, second), first);
    assert_eq!(
        call(raster::nt_gdi_set_pixel, &[memory, 1, 1, 0x654321]),
        0x654321
    );
    assert!(!objects::gdi_delete_object(first));
    assert!(!objects::gdi_delete_object(second));
    assert!(dc::gre_restore_dc(memory, -1));
    assert_eq!(call(raster::nt_gdi_get_pixel, &[memory, 1, 1]), 0x123456);
    assert!(objects::gdi_delete_object(second));
    assert_eq!(dc::get_dc(memory).unwrap().bounds, Rect::new(0, 0, 4, 4));
    assert!(dc::gre_delete_dc(memory));
    assert!(objects::gdi_delete_object(first));
}

#[test]
fn selected_bitmap_raster_and_native_bit_transfers_share_the_same_storage() {
    let memory = dc::gre_create_compatible_dc(0);
    let bitmap = objects::gdi_create_bitmap(2, 2, 32);
    dc::gre_select_object(memory, bitmap);
    let mut bytes = [0u8; 16];
    bytes[12..16].copy_from_slice(&0x00563412u32.to_le_bytes());
    let address = bytes.as_mut_ptr() as u64;
    mm::user_copy::allow(address, bytes.len(), true);
    assert_eq!(
        call(
            bitmap::storage::nt_gdi_set_bitmap_bits,
            &[bitmap, 16, address]
        ),
        16
    );
    assert_eq!(call(raster::nt_gdi_get_pixel, &[memory, 1, 1]), 0x123456);
    assert_eq!(
        call(raster::nt_gdi_set_pixel, &[memory, 1, 1, 0xabcdef]),
        0xabcdef
    );
    assert_eq!(
        call(
            bitmap::storage::nt_gdi_get_bitmap_bits,
            &[bitmap, 16, address]
        ),
        16
    );
    std::hint::black_box(&mut bytes);
    assert_eq!(
        u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        0x00efcdab
    );
    assert!(dc::gre_delete_dc(memory));
    assert!(objects::gdi_delete_object(bitmap));
}

#[test]
fn information_transition_and_source_free_draw_preserve_device_pixels() {
    let mut screen = surface::GdiSurface::new(4, 4, 32).unwrap();
    let display = dc::get_dc_table().create(1, (&mut screen as *mut _) as u64);
    let memory = dc::gre_create_compatible_dc(display);
    let bitmap = objects::gdi_create_bitmap(4, 4, 32);
    dc::gre_select_object(memory, bitmap);
    call(raster::nt_gdi_set_pixel, &[display, 1, 1, 0x123456]);
    assert!(dc::info::make_information_dc(display, true));
    assert_eq!(
        call(raster::nt_gdi_get_pixel, &[display, 1, 1]),
        u32::MAX as u64
    );
    assert!(gdi_ops::gre_bit_blt(
        &mut dc::get_dc(memory).unwrap(),
        1,
        1,
        1,
        1,
        None,
        1,
        1,
        0x42,
    ));
    assert_eq!(dc::gre_select_object(display, bitmap), 0);
    assert!(dc::info::make_information_dc(display, false));
    assert_eq!(call(raster::nt_gdi_get_pixel, &[display, 1, 1]), 0x123456);
    assert_eq!(call(raster::nt_gdi_get_pixel, &[memory, 1, 1]), 0);
    assert!(dc::gre_delete_dc(memory));
    assert!(dc::gre_delete_dc(display));
    assert!(objects::gdi_delete_object(bitmap));
}

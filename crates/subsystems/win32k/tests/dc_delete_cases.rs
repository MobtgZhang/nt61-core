use super::*;

#[test]
fn delete_object_routes_dc_and_releases_live_and_saved_selections() {
    let handle = dc::gre_create_compatible_dc(0);
    let first = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0x123456);
    let second = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0x654321);
    assert_ne!(call(selection::nt_gdi_select_pen, &[handle, first]), 0);
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(call(selection::nt_gdi_select_pen, &[handle, second]), first);
    assert!(!objects::gdi_delete_object(first));
    assert!(!objects::gdi_delete_object(second));

    assert!(objects::gdi_delete_object(handle));
    assert!(dc::get_dc(handle).is_none());
    assert!(!objects::gdi_delete_object(handle));
    assert!(!objects::gdi_delete_object(handle | (1 << 40)));
    assert!(objects::gdi_delete_object(first));
    assert!(objects::gdi_delete_object(second));
}

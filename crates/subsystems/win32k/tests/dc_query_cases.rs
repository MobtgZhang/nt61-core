//! Query the same DC state used by SaveDC/RestoreDC and rendering.
use super::*;
use dc::services::nt_gdi_get_dc_dword;

fn query(handle: u64, selector: u64) -> (u64, u32) {
    let mut output = 0xdead_beef;
    let pointer = &mut output as *mut u32 as u64;
    mm::user_copy::allow(pointer, 4, true);
    let success = call(nt_gdi_get_dc_dword, &[handle, selector, pointer]);
    (success, output)
}

#[test]
fn save_depth_tracks_positive_and_negative_restores() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(query(handle, 5), (1, 1));
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(dc::gre_save_dc(handle), 2);
    assert_eq!(query(handle, 5), (1, 3));
    assert!(dc::gre_restore_dc(handle, -1));
    assert_eq!(query(handle, 5), (1, 2));
    assert!(dc::gre_restore_dc(handle, 1));
    assert_eq!(query(handle, 5), (1, 1));
    assert!(!dc::gre_restore_dc(handle, 1));
    assert_eq!(query(handle, 5), (1, 1));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn memory_type_and_mapping_queries_return_bool_and_write_real_state() {
    let memory = dc::gre_create_compatible_dc(0);
    let display = dc::gre_create_display_dc();
    assert_eq!(query(memory, 7), (1, 1));
    assert_eq!(query(display, 7), (1, 0));
    assert_eq!(query(memory, 8), (1, dc::MM_TEXT as u32));
    assert_eq!(dc::gre_save_dc(memory), 1);
    dc::get_dc(memory).unwrap().map_mode = dc::MM_ANISOTROPIC;
    assert_eq!(query(memory, 8), (1, dc::MM_ANISOTROPIC as u32));
    // Selector is DWORD, so high argument bits are ignored.
    assert_eq!(query(memory, 0x1234_0000_0008), (1, 8));
    assert!(dc::gre_restore_dc(memory, -1));
    assert_eq!(query(memory, 8), (1, dc::MM_TEXT as u32));
    assert!(dc::gre_delete_dc(memory));
    assert!(dc::gre_delete_dc(display));
}

#[test]
fn invalid_handles_and_unsupported_selectors_leave_output_untouched() {
    let handle = dc::gre_create_compatible_dc(0);
    for selector in [0, 2, 3, 6, 9, u32::MAX as u64] {
        assert_eq!(query(handle, selector), (0, 0xdead_beef));
    }
    assert_eq!(query(0, 3), (0, 0xdead_beef));
    assert_eq!(query(u64::MAX, 8), (0, 0xdead_beef));
    assert!(dc::gre_delete_dc(handle));
    assert_eq!(query(handle, 5), (0, 0xdead_beef));
}

#[test]
fn bad_output_fails_and_unaligned_dword_output_is_copied_safely() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(nt_gdi_get_dc_dword, &[handle, 5, 0]), 0);
    let mut bytes = [0xabu8; 6];
    let pointer = bytes.as_mut_ptr() as u64 + 1;
    mm::user_copy::allow(pointer, 4, false);
    assert_eq!(call(nt_gdi_get_dc_dword, &[handle, 5, pointer]), 0);
    assert_eq!(bytes, [0xab; 6]);
    mm::user_copy::allow(pointer, 4, true);
    assert_eq!(call(nt_gdi_get_dc_dword, &[handle, 5, pointer]), 1);
    assert_eq!(bytes, [0xab, 1, 0, 0, 0, 0xab]);
    assert_eq!(query(handle, 5), (1, 1));
    assert!(dc::gre_delete_dc(handle));
}

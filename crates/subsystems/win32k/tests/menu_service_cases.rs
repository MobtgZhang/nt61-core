use super::*;
use user_state::menu_services::*;

#[test]
fn menu_identity_is_immediate_and_help_ids_reject_dead_handles() {
    user_state::with_user_state(|state| {
        assert!(state.create_menu(0x9000, false));
        assert!(state.create_menu(0x9001, true));
        assert!(state.append_menu_item(0x9000, user_state::MF_POPUP, 0x9001, &[]));
    });
    assert_eq!(call(nt_user_get_menu_index, &[0x9000, 0x9001]), 0);
    assert_eq!(call(nt_user_get_menu_index, &[0x9001, 0x9000]), u32::MAX as u64);
    assert_eq!(call(nt_user_set_menu_context_help_id, &[0x9000, 0x12345678]), 1);
    user_state::with_user_state(|global| {
        assert!(global.destroy_menu(0x9000));
        assert!(global.destroy_menu(0x9001));
    });
    assert_eq!(call(nt_user_set_menu_context_help_id, &[0x9000, 0]), 0);
}

#[test]
fn dpi_identity_conversion_includes_edges_and_probes_both_directions() {
    use window::coordinates::*;
    let hwnd = window::WINDOW_MANAGER.lock()
        .create_window(&[], &[], 0, 0, -20, 10, 100, 80, None, 0, 0).unwrap();
    for service in [nt_user_logical_to_physical_point, nt_user_physical_to_logical_point] {
        for point in [[-20, 10], [80, 90], [1, 20]] {
            let mut output = point;
            let address = output.as_mut_ptr() as u64;
            mm::user_copy::allow(address, 8, true);
            assert_eq!(call(service, &[hwnd, address]), 1);
            assert_eq!(output, point);
        }
        let mut outside = [81i32, 90];
        let address = outside.as_mut_ptr() as u64;
        mm::user_copy::allow(address, 8, true);
        assert_eq!(call(service, &[hwnd, address]), 0);
        assert_eq!(outside, [81, 90]);
        assert_eq!(call(service, &[hwnd, 1]), 0);
        let mut readonly = [1i32, 20];
        let address = readonly.as_mut_ptr() as u64;
        mm::user_copy::allow(address, 8, false);
        assert_eq!(call(service, &[hwnd, address]), 0);
    }
    assert!(window::destroy_window_internal(hwnd));
}

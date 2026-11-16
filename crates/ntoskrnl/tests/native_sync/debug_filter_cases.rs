use super::{ntdll, state_guard, STATUS_ACCESS_DENIED, STATUS_SUCCESS};
use ntdll::debug_filter::{nt_query_debug_filter_state as query, nt_set_debug_filter_state as set};

#[test]
fn debug_masks_enforce_privilege_and_win2000_override() {
    let _guard = state_guard();
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(false));
    assert_eq!(set(20, 6, 1), STATUS_ACCESS_DENIED);
    assert_eq!(query(20, 6), 0);
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(true));
    assert_eq!(set(20, 6, 1), STATUS_SUCCESS);
    assert_eq!(query(20, 6), 1);
    assert_eq!(query(21, 6), 0);
    assert_eq!(set(20, 6, 0), STATUS_SUCCESS);
    assert_eq!(set(u32::MAX, 6, 1), STATUS_SUCCESS);
    assert_eq!(query(21, 6), 1);
    assert_eq!(set(21, 6, 0), STATUS_SUCCESS);
    assert_eq!(query(21, 6), 1);
    assert_eq!(set(u32::MAX, 6, 0), STATUS_SUCCESS);
    assert_eq!(query(21, 6), 0);
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(false));
}

#[test]
fn debug_direct_masks_default_component_and_reserved_bit_match_nt() {
    let _guard = state_guard();
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(true));
    assert_eq!(set(101, 0x8000_0600, 1), STATUS_SUCCESS);
    assert_eq!(query(500, 9), 1);
    assert_eq!(query(500, 10), 1);
    assert_eq!(query(500, 11), 0);
    assert_eq!(set(500, 9, 0), STATUS_SUCCESS);
    assert_eq!(query(101, 9), 0);
    assert_eq!(query(101, 10), 1);
    assert_eq!(set(101, 10, 0), STATUS_SUCCESS);
    assert_eq!(set(20, 31, 1), STATUS_SUCCESS);
    assert_eq!(query(20, 31), 0);
    assert_eq!(query(20, 0x8000_0000), 0);
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(false));
}

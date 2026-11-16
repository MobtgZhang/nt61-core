use super::*;
use ntdll::{locale::*, profile_interval::*};

#[test]
fn locale_defaults_capture_output_and_gate_system_changes_by_privilege() {
    let _guard = state_guard();
    ntdll::privilege::ENABLED.with(|value| value.set(false));
    let mut original = 0u32;
    assert_eq!(nt_query_default_locale(1, &mut original), STATUS_SUCCESS);
    assert_eq!(nt_set_default_locale(1, 0x804), STATUS_SUCCESS);
    let mut output = 0u32;
    assert_eq!(nt_query_default_locale(1, &mut output), STATUS_SUCCESS);
    assert_eq!(output, 0x804);
    assert_eq!(
        nt_set_default_locale(1, 0x10000409),
        STATUS_INVALID_PARAMETER
    );
    assert_eq!(nt_set_default_locale(1, 0), STATUS_NOT_SUPPORTED);
    assert_eq!(nt_set_default_locale(0, 0x804), STATUS_PRIVILEGE_NOT_HELD);
    assert_eq!(
        nt_query_default_locale(1, core::ptr::null_mut()),
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(nt_set_default_locale(1, original), STATUS_SUCCESS);
}

#[test]
fn ui_language_stages_before_commit_and_keeps_install_language_immutable() {
    let _guard = state_guard();
    let mut original = 0u16;
    assert_eq!(nt_query_default_ui_language(&mut original), STATUS_SUCCESS);
    assert_eq!(nt_set_default_ui_language(0x804), STATUS_SUCCESS);
    let mut language = 0u16;
    assert_eq!(nt_query_default_ui_language(&mut language), STATUS_SUCCESS);
    assert_eq!(language, original);
    assert_eq!(nt_set_default_ui_language(0), STATUS_SUCCESS);
    assert_eq!(nt_query_default_ui_language(&mut language), STATUS_SUCCESS);
    assert_eq!(language, 0x804);
    assert_eq!(nt_query_install_ui_language(&mut language), STATUS_SUCCESS);
    assert_eq!(language, 0x409);
    assert_eq!(nt_set_default_ui_language(original), STATUS_SUCCESS);
    assert_eq!(nt_set_default_ui_language(0), STATUS_SUCCESS);
}

#[test]
fn interval_profile_validates_privilege_source_and_timer_quantization() {
    let _guard = state_guard();
    ntdll::privilege::ENABLED.with(|value| value.set(false));
    assert_eq!(nt_set_interval_profile(1, 0), STATUS_PRIVILEGE_NOT_HELD);
    ntdll::privilege::ENABLED.with(|value| value.set(true));
    assert_eq!(nt_set_interval_profile(1, 1), STATUS_NOT_SUPPORTED);
    let mut original = 0u32;
    assert_eq!(nt_query_interval_profile(0, &mut original), STATUS_SUCCESS);
    assert_eq!(nt_set_interval_profile(1, 0), STATUS_SUCCESS);
    let mut interval = 0u32;
    assert_eq!(nt_query_interval_profile(0, &mut interval), STATUS_SUCCESS);
    assert_eq!(interval, 10000);
    assert_eq!(nt_set_interval_profile(50000, 0), STATUS_SUCCESS);
    assert_eq!(nt_query_interval_profile(0, &mut interval), STATUS_SUCCESS);
    assert_eq!(interval, 50000);
    assert_eq!(
        nt_query_interval_profile(1, &mut interval),
        STATUS_NOT_SUPPORTED
    );
    assert_eq!(
        nt_query_interval_profile(0, core::ptr::null_mut()),
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(nt_set_interval_profile(original, 0), STATUS_SUCCESS);
    ntdll::privilege::ENABLED.with(|value| value.set(false));
}

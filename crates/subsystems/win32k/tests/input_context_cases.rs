use super::*;
use window::{input_context::*, WINDOW_MANAGER};

#[test]
fn contexts_enforce_ownership_preserve_full_client_pointers_and_restore_default_on_destroy() {
    ps::thread::IDENTITY.with(|id| id.set((810, 811)));
    let hwnd = WINDOW_MANAGER.lock().create_window(&[], &[], 0, 0, 0, 0, 100, 100, None, 0, 0).unwrap();
    let default = WINDOW_MANAGER.lock().get_window(hwnd).unwrap().input_context;
    let context = call(nt_user_create_input_context, &[0xfedc_ba98_7654_3210]);
    assert_ne!(context, 0);
    assert_ne!(context, default);
    assert_eq!(call(nt_user_query_input_context, &[context, 0]), 810);
    assert_eq!(call(nt_user_query_input_context, &[context, 1]), 811);
    assert_eq!(call(nt_user_query_input_context, &[context, 3]), default);
    assert_eq!(call(nt_user_update_input_context, &[context, 0, 1]), 0);
    assert_eq!(call(nt_user_update_input_context, &[context, 1, hwnd]), 1);
    assert_eq!(call(nt_user_associate_input_context, &[hwnd, context, 0]), 0);
    assert_eq!(WINDOW_MANAGER.lock().get_window(hwnd).unwrap().input_context, context);
    ps::thread::IDENTITY.with(|id| id.set((812, 813)));
    assert_eq!(call(nt_user_associate_input_context, &[hwnd, 0, 0]), 2);
    assert_eq!(call(nt_user_destroy_input_context, &[context]), 0);
    assert_eq!(call(nt_user_set_app_ime_level, &[hwnd, 3]), 0);
    ps::thread::IDENTITY.with(|id| id.set((810, 811)));
    assert_eq!(call(nt_user_set_app_ime_level, &[hwnd, 3]), 1);
    assert_eq!(call(nt_user_get_app_ime_level, &[hwnd]), 3);
    assert_eq!(call(nt_user_destroy_input_context, &[default]), 0);
    assert_eq!(call(nt_user_destroy_input_context, &[context]), 1);
    assert_eq!(call(nt_user_query_input_context, &[context, 0]), 0);
    assert_eq!(WINDOW_MANAGER.lock().get_window(hwnd).unwrap().input_context, default);
    assert!(window::destroy_window_internal(hwnd));
    ps::thread::IDENTITY.with(|id| id.set((1, 1)));
}

#[test]
fn context_enumeration_reports_required_count_and_thread_disable_prevents_new_objects() {
    ps::thread::IDENTITY.with(|id| id.set((820, 821)));
    let context = call(nt_user_create_input_context, &[42]);
    assert_ne!(context, 0);
    let mut required = 0u32;
    let count = &mut required as *mut u32 as u64;
    mm::user_copy::allow(count, 4, true);
    assert_eq!(call(nt_user_build_himc_list, &[0, 0, 0, count]), 0xc0000023);
    assert_eq!(required, 2);
    let mut output = [0xdeadbeefu64; 3];
    let address = output.as_mut_ptr() as u64;
    mm::user_copy::allow(address, 24, true);
    assert_eq!(call(nt_user_build_himc_list, &[0, 1, address, count]), 0xc0000023);
    assert_eq!(output[1], 0xdeadbeef);
    assert_eq!(call(nt_user_build_himc_list, &[0, 3, address, count]), 0);
    assert_eq!(output[1], context);
    assert_eq!(call(nt_user_build_himc_list, &[0, 3, 1, count]), 0xc0000005);
    assert_eq!(call(nt_user_disable_thread_ime, &[0]), 1);
    assert_eq!(call(nt_user_create_input_context, &[1]), 0);
    assert_eq!(call(nt_user_destroy_input_context, &[context]), 1);
    ps::thread::IDENTITY.with(|id| id.set((1, 1)));
}

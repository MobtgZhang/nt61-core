use crate::{call, clipboard::*, mm::user_copy::allow};
use services::{nt_user_get_priority_clipboard_format as priority, nt_user_get_updated_clipboard_formats as updated};

#[test]
fn native_format_queries_use_storage_and_validate_caller_buffers() {
    let _guard = TEST_LOCK.lock().unwrap();
    if is_open() {
        close_clipboard();
    }
    assert!(open_clipboard(0));
    assert!(empty_clipboard());
    assert_eq!(call(priority, &[0, 3]), 0);
    let mut required = 99u32;
    let count = &mut required as *mut u32 as u64;
    allow(count, 4, true);
    assert_eq!(call(updated, &[0, 0, count]), 1);
    assert_eq!(required, 0);

    assert!(set_clipboard_data(CF_DIB, &[1, 2]));
    assert!(set_clipboard_data(CF_UNICODETEXT, b"text"));
    assert!(close_clipboard());
    // Format queries work without opening and follow caller priority order.
    let order = [CF_TIFF, CF_UNICODETEXT, CF_DIB];
    let address = order.as_ptr() as u64;
    allow(address, 12, false);
    assert_eq!(call(priority, &[address, 3]), CF_UNICODETEXT as u64);
    assert_eq!(call(priority, &[address, 1]), u32::MAX as u64);
    assert_eq!(call(priority, &[0, 0]), u32::MAX as u64);
    assert_eq!(call(priority, &[0, 1]), u32::MAX as u64);
    assert_eq!(call(priority, &[address, u32::MAX as u64]), u32::MAX as u64);
    // An unreadable tail is rejected even if the first entry matches.
    assert_eq!(call(priority, &[address + 4, 3]), u32::MAX as u64);

    let mut formats = [0xdeadbeefu32; 3];
    let output = formats.as_mut_ptr() as u64;
    allow(output, 12, true);
    assert_eq!(call(updated, &[output, 1, count]), 0);
    assert_eq!(required, 2);
    assert_eq!(formats, [0xdeadbeef; 3]);
    assert_eq!(call(updated, &[output, 3, count]), 1);
    assert_eq!(formats, [CF_DIB, CF_UNICODETEXT, 0xdeadbeef]);
    assert_eq!(call(updated, &[output, 3, 0]), 0);
    assert_eq!(call(updated, &[0, 3, count]), 0);
    assert!(open_clipboard(0));
    assert!(empty_clipboard());
    assert!(close_clipboard());
}

#[test]
fn ownership_survives_readonly_opens_and_only_modified_close_notifies_live_listeners() {
    use crate::{window::{WindowStyle, WINDOW_MANAGER}, libs::win32k::message::POSTED};
    let _guard = TEST_LOCK.lock().unwrap();
    if is_open() { close_clipboard(); }
    let owner = WINDOW_MANAGER.lock().create_window(&[], &[], WindowStyle::VISIBLE, 0, 0, 0, 100, 100, None, 0, 0).unwrap();
    let listener = WINDOW_MANAGER.lock().create_window(&[], &[], 0, 0, 0, 0, 50, 50, None, 0, 0).unwrap();
    assert_eq!(call(services::nt_user_add_clipboard_format_listener, &[listener]), 1);
    assert_eq!(call(services::nt_user_add_clipboard_format_listener, &[listener]), 0);
    assert!(open_clipboard(owner));
    assert_eq!(get_open_window(), owner);
    assert!(empty_clipboard());
    assert_eq!(get_owner(), owner);
    assert!(set_clipboard_data(CF_UNICODETEXT, &[65, 0, 0, 0]));
    let sequence = get_sequence_number();
    assert!(close_clipboard());
    assert_eq!(get_sequence_number(), sequence);
    let notifications = || POSTED.lock().unwrap().iter().filter(|entry| entry.0 == listener && entry.1 == 0x31d).count();
    assert_eq!(notifications(), 1);
    assert!(open_clipboard(0));
    assert_eq!(get_owner(), owner);
    assert!(close_clipboard());
    assert_eq!(notifications(), 1);
    assert!(crate::window::destroy_window_internal(listener));
    assert!(crate::window::destroy_window_internal(owner));
    assert_eq!(get_owner(), 0);
    assert_eq!(call(services::nt_user_remove_clipboard_format_listener, &[listener]), 0);
}

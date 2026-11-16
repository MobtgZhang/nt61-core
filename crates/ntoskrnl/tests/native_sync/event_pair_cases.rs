use super::*;
use ntdll::{event_pair::*, file};

fn pair(access: u32) -> HANDLE {
    let mut handle = core::ptr::null_mut();
    unsafe {
        assert_eq!(nt_create_event_pair(&mut handle, access, core::ptr::null_mut()), STATUS_SUCCESS);
    }
    handle
}

fn operate(handle: HANDLE, signal: u64, wait: u64) -> file::WaitStart {
    file::operate_event_pair(handle, signal, wait, 100, 701).unwrap()
}

fn completed(start: file::WaitStart) {
    assert!(matches!(start, file::WaitStart::Complete(STATUS_SUCCESS)));
}

#[test]
fn independent_halves_coalesce_signals_and_release_one_waiter() {
    let _guard = state_guard();
    let handle = pair(0x1f0000);
    assert_eq!(nt_set_low_event_pair(handle), STATUS_SUCCESS);
    assert_eq!(nt_set_low_event_pair(handle), STATUS_SUCCESS);
    completed(operate(handle, 0, 1));
    let file::WaitStart::Pending(low) = operate(handle, 0, 1) else {
        panic!("low signal must be consumed once");
    };
    let file::WaitStart::Pending(high) = operate(handle, 0, 2) else {
        panic!("high half must remain unsignaled");
    };
    assert_eq!(nt_set_high_event_pair(handle), STATUS_SUCCESS);
    assert_eq!(high.poll(), Some(STATUS_SUCCESS));
    assert_eq!(low.poll(), None);
    assert_eq!(nt_set_low_event_pair(handle), STATUS_SUCCESS);
    assert_eq!(low.poll(), Some(STATUS_SUCCESS));
    assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
}

#[test]
fn atomic_set_and_wait_hands_off_to_opposite_half() {
    let _guard = state_guard();
    let handle = pair(0x100000);
    let file::WaitStart::Pending(low) = operate(handle, 0, 1) else {
        panic!("expected queued waiter");
    };
    let file::WaitStart::Pending(high) = operate(handle, 1, 2) else {
        panic!("expected opposite-half waiter");
    };
    assert_eq!(low.poll(), Some(STATUS_SUCCESS));
    assert_eq!(high.poll(), None);
    completed(operate(handle, 2, 0));
    assert_eq!(high.poll(), Some(STATUS_SUCCESS));
    // A signal already present on the opposite half completes immediately.
    completed(operate(handle, 2, 0));
    assert_eq!(nt_set_low_wait_high_event_pair(handle), STATUS_SUCCESS);
    assert_eq!(nt_wait_low_event_pair(handle), STATUS_SUCCESS);
    completed(operate(handle, 1, 0));
    assert_eq!(nt_set_high_wait_low_event_pair(handle), STATUS_SUCCESS);
    assert_eq!(nt_wait_high_event_pair(handle), STATUS_SUCCESS);
    assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
}

#[test]
fn pair_access_and_object_type_are_checked_before_signaling() {
    let _guard = state_guard();
    let denied = pair(0x20000);
    for operation in [nt_set_low_event_pair, nt_set_high_event_pair,
        nt_wait_low_event_pair, nt_wait_high_event_pair,
        nt_set_low_wait_high_event_pair, nt_set_high_wait_low_event_pair] {
        assert_eq!(operation(denied), STATUS_ACCESS_DENIED);
        assert_eq!(operation(core::ptr::null_mut()), STATUS_INVALID_HANDLE);
    }
    let event = create_event_kernel(1, false).unwrap();
    assert_eq!(nt_set_low_event_pair(event), STATUS_OBJECT_TYPE_MISMATCH);
    assert_eq!(nt_wait_for_single_object_kernel(denied, 0, Some(0)), STATUS_ACCESS_DENIED);
    let handle = pair(0x100000);
    assert_eq!(nt_wait_for_single_object_kernel(handle, 0, Some(0)), STATUS_OBJECT_TYPE_MISMATCH);
    for handle in [denied, event, handle] {
        assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
    }
}

#[test]
fn queued_pair_wait_pins_object_across_final_close_and_slot_reuse() {
    let _guard = state_guard();
    let handle = pair(0x100000);
    let alias = file::duplicate_sync_handle(handle).unwrap();
    let file::WaitStart::Pending(wait) = operate(handle, 0, 1) else {
        panic!("expected waiter");
    };
    assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
    assert_eq!(nt_set_low_event_pair(alias), STATUS_SUCCESS);
    assert_eq!(wait.poll(), Some(STATUS_SUCCESS));
    drop(wait);
    let file::WaitStart::Pending(wait) = operate(alias, 0, 2) else {
        panic!("expected waiter");
    };
    assert_eq!(unsafe { nt_close_sync(alias) }, STATUS_SUCCESS);
    let replacement = pair(0x100000);
    assert_eq!(nt_set_high_event_pair(replacement), STATUS_SUCCESS);
    assert_eq!(wait.poll(), None);
    drop(wait);
    assert_eq!(unsafe { nt_close_sync(replacement) }, STATUS_SUCCESS);
}

#[test]
fn wow64_wait_captures_four_byte_elements_and_validates_types() {
    let _guard = state_guard();
    let first = create_event_kernel(1, false).unwrap();
    let second = create_event_kernel(1, true).unwrap();
    let handles = [first as u32, second as u32];
    let timeout = 0i64;
    unsafe {
        assert_eq!(nt_wait_for_multiple_objects32(2, handles.as_ptr(), 1, 0, &timeout), 1);
        assert_eq!(nt_wait_for_multiple_objects32(2, handles.as_ptr(), 0, 0, &timeout), STATUS_TIMEOUT);
        assert_eq!(nt_wait_for_multiple_objects32(65, handles.as_ptr(), 1, 0, &timeout), STATUS_INVALID_PARAMETER);
        assert_eq!(nt_wait_for_multiple_objects32(1, core::ptr::null(), 1, 0, &timeout), STATUS_ACCESS_VIOLATION);
        assert_eq!(nt_wait_for_multiple_objects32(1, handles.as_ptr(), 2, 0, &timeout), STATUS_INVALID_PARAMETER);
        assert_eq!(nt_wait_for_multiple_objects32(1, [u32::MAX].as_ptr(), 1, 0, &timeout), STATUS_INVALID_HANDLE);
    }
    for handle in [first, second] {
        assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
    }
}

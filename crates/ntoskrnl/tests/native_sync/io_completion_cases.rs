use super::*;
use ntdll::io_completion::*;

fn create(access: u32) -> HANDLE {
    let mut handle = core::ptr::null_mut();
    assert_eq!(unsafe { nt_create_io_completion(&mut handle, access, core::ptr::null_mut(), 0) }, STATUS_SUCCESS);
    handle
}

fn depth(handle: HANDLE) -> u32 {
    let mut depth = u32::MAX;
    let mut returned = 0;
    assert_eq!(unsafe { nt_query_io_completion(handle, 0, &mut depth as *mut _ as PVOID, 4, &mut returned) }, STATUS_SUCCESS);
    assert_eq!(returned, 4);
    depth
}

#[test]
fn completion_packets_are_fifo_and_failed_copies_preserve_the_front() {
    let _guard = state_guard();
    let handle = create(3);
    for key in 1..=3 {
        assert_eq!(nt_set_io_completion(handle, key, key + 10, STATUS_ACCESS_DENIED, key + 20), STATUS_SUCCESS);
    }
    assert_eq!(depth(handle), 3);
    let mut key = 0;
    let mut context = 0;
    let mut iosb = IoStatusBlock::default();
    let mut timeout = 0;
    // Simulate the mapping disappearing after the initial output probes.
    mm::user_copy::FAIL_WRITE.with(|value| value.set(&mut context as *mut _ as u64));
    assert_eq!(unsafe { nt_remove_io_completion(handle, &mut key, &mut context, &mut iosb, &mut timeout) }, STATUS_ACCESS_VIOLATION);
    mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
    assert_eq!(depth(handle), 3);
    for expected in 1..=3 {
        assert_eq!(unsafe { nt_remove_io_completion(handle, &mut key, &mut context, &mut iosb, &mut timeout) }, STATUS_SUCCESS);
        assert_eq!((key, context, iosb.status, iosb.information), (expected, expected + 10, STATUS_ACCESS_DENIED, expected as usize + 20));
    }
    assert_eq!(unsafe { nt_remove_io_completion(handle, &mut key, &mut context, &mut iosb, &mut timeout) }, STATUS_TIMEOUT);
    assert_eq!(depth(handle), 0);
    assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
}

#[test]
fn completion_named_opens_alias_queue_and_enforce_access() {
    let _guard = state_guard();
    named_cases::named("\\BaseNamedObjects\\CompletionQueue", 0, |attributes| {
        let mut handle = core::ptr::null_mut();
        let mut query = core::ptr::null_mut();
        assert_eq!(unsafe { nt_create_io_completion(&mut handle, 3, attributes, 1) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_open_io_completion(&mut query, 1, attributes) }, STATUS_SUCCESS);
        assert_eq!(nt_set_io_completion(query, 0, 0, 0, 0), STATUS_ACCESS_DENIED);
        let alias = ntdll::file::duplicate_sync_handle(handle).unwrap();
        assert_eq!(nt_set_io_completion(alias, 19, 0, 0, 0), STATUS_SUCCESS);
        assert_eq!(depth(query), 1);
        assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
        assert_eq!(depth(query), 1);
        assert_eq!(unsafe { nt_close_sync(alias) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_close_sync(query) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_open_io_completion(&mut query, 1, attributes) }, STATUS_OBJECT_NAME_NOT_FOUND);
    });
}

#[test]
fn completion_ex_copies_native_layout_and_retains_unconsumed_packets() {
    let _guard = state_guard();
    let handle = create(3);
    for key in 1..=3 {
        assert_eq!(nt_set_io_completion(handle, key, key + 100, STATUS_INVALID_PARAMETER, key + 200), STATUS_SUCCESS);
    }
    let mut packets = [u64::MAX; 8];
    let mut removed = 0;
    let mut timeout = 0;
    assert_eq!(unsafe { nt_remove_io_completion_ex(handle, packets.as_mut_ptr() as PVOID, 2, &mut removed, &mut timeout, 0) }, STATUS_SUCCESS);
    assert_eq!(removed, 2);
    assert_eq!(&packets[..4], &[1, 101, STATUS_INVALID_PARAMETER as u32 as u64, 201]);
    assert_eq!(&packets[4..], &[2, 102, STATUS_INVALID_PARAMETER as u32 as u64, 202]);
    assert_eq!(depth(handle), 1);
    assert_eq!(unsafe { nt_remove_io_completion_ex(handle, packets.as_mut_ptr() as PVOID, 0, &mut removed, &mut timeout, 0) }, STATUS_INVALID_PARAMETER);
    assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
}

#[test]
fn completion_relative_timeout_and_close_do_not_follow_recycled_handles() {
    let _guard = state_guard();
    let handle = create(3);
    let mut packets = [0u64; 4];
    let mut removed = 77;
    let mut timeout = -30;
    ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(10));
    assert_eq!(unsafe { nt_remove_io_completion_ex(handle, packets.as_mut_ptr() as PVOID, 1, &mut removed, &mut timeout, 0) }, STATUS_TIMEOUT);
    assert_eq!(removed, 77);
    let handle_value = handle as usize;
    ke::scheduler::YIELD_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(move || {
        assert_eq!(unsafe { nt_close_sync(handle_value as HANDLE) }, STATUS_SUCCESS);
        let replacement = create(3);
        assert_eq!(nt_set_io_completion(replacement, 999, 0, 0, 0), STATUS_SUCCESS);
        assert_eq!(unsafe { nt_close_sync(replacement) }, STATUS_SUCCESS);
    })));
    assert_eq!(unsafe { nt_remove_io_completion_ex(handle, packets.as_mut_ptr() as PVOID, 1, &mut removed, core::ptr::null_mut(), 0) }, 0x80);
    assert_eq!(removed, 77);
}

#[test]
fn completion_validates_classes_types_and_outputs_before_mutating_queue() {
    let _guard = state_guard();
    let handle = create(3);
    let mut output = 0u32;
    assert_eq!(unsafe { nt_query_io_completion(handle, 1, &mut output as *mut _ as PVOID, 4, core::ptr::null_mut()) }, STATUS_INVALID_INFO_CLASS);
    assert_eq!(unsafe { nt_query_io_completion(handle, 0, &mut output as *mut _ as PVOID, 3, core::ptr::null_mut()) }, STATUS_INFO_LENGTH_MISMATCH);
    assert_eq!(unsafe { nt_remove_io_completion_ex(handle, 1usize as PVOID, 1, &mut output, core::ptr::null_mut(), 0) }, STATUS_ACCESS_VIOLATION);
    let event = create_event_kernel(1, true).unwrap();
    assert_eq!(nt_set_io_completion(event, 0, 0, 0, 0), STATUS_OBJECT_TYPE_MISMATCH);
    assert_eq!(unsafe { nt_close_sync(event) }, STATUS_SUCCESS);
    assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
}

fn reserve() -> HANDLE {
    let mut handle = core::ptr::null_mut();
    assert_eq!(unsafe { nt_allocate_reserve_object(&mut handle, core::ptr::null_mut(), 1) }, 0);
    handle
}

#[test]
fn completion_reserves_allow_only_one_outstanding_packet_and_are_reusable() {
    let _guard = state_guard();
    let port = create(3);
    let reserved = reserve();
    let alias = ntdll::file::duplicate_sync_handle(reserved).unwrap();
    assert_eq!(nt_set_io_completion_ex(port, reserved, 10, 20, -1, 30), 0);
    assert_eq!(nt_set_io_completion_ex(port, alias, 11, 21, 0, 31), STATUS_INVALID_PARAMETER);
    assert_eq!(depth(port), 1);
    let mut packet = [0u64; 4];
    let mut removed = 0;
    let mut timeout = 0;
    assert_eq!(unsafe { nt_remove_io_completion_ex(port, packet.as_mut_ptr().cast(), 1,
        &mut removed, &mut timeout, 0) }, 0);
    assert_eq!(packet, [10, 20, u32::MAX as u64, 30]);
    assert_eq!(nt_set_io_completion_ex(port, alias, 11, 21, 0, 31), 0);
    assert_eq!(unsafe { nt_close_sync(reserved) }, 0);
    assert_eq!(unsafe { nt_close_sync(alias) }, 0);
    // The queued node owns its storage after the last reserve handle closes.
    assert_eq!(unsafe { nt_remove_io_completion_ex(port, packet.as_mut_ptr().cast(), 1,
        &mut removed, &mut timeout, 0) }, 0);
    assert_eq!(packet, [11, 21, 0, 31]);
    assert_eq!(unsafe { nt_close_sync(port) }, 0);
}

#[test]
fn closing_port_releases_reserve_and_failed_copy_keeps_it_busy() {
    let _guard = state_guard();
    let first = create(3);
    let second = create(3);
    let reserved = reserve();
    assert_eq!(nt_set_io_completion_ex(first, reserved, 1, 2, 0, 3), 0);
    let mut key = 0;
    let mut context = 0;
    let mut iosb = IoStatusBlock::default();
    let mut timeout = 0;
    mm::user_copy::FAIL_WRITE.with(|value| value.set(&mut key as *mut _ as u64));
    assert_eq!(unsafe { nt_remove_io_completion(first, &mut key, &mut context,
        &mut iosb, &mut timeout) }, STATUS_ACCESS_VIOLATION);
    mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
    assert_eq!(nt_set_io_completion_ex(second, reserved, 4, 5, 0, 6), STATUS_INVALID_PARAMETER);
    assert_eq!(unsafe { nt_close_sync(first) }, 0);
    assert_eq!(nt_set_io_completion_ex(second, reserved, 4, 5, 0, 6), 0);
    assert_eq!(unsafe { nt_close_sync(second) }, 0);
    assert_eq!(unsafe { nt_close_sync(reserved) }, 0);
}

#[test]
fn reserve_outputs_and_rights_are_checked_without_consuming_a_slot() {
    let _guard = state_guard();
    let port = create(3);
    let reserved = reserve();
    let query = ntdll::object::duplicate_object_kernel((-1isize) as HANDLE, reserved,
        (-1isize) as HANDLE, 1, 0, 0).unwrap();
    assert_eq!(nt_set_io_completion_ex(port, query, 0, 0, 0, 0), STATUS_ACCESS_DENIED);
    assert_eq!(nt_set_io_completion_ex(port, port, 0, 0, 0, 0), STATUS_OBJECT_TYPE_MISMATCH);
    assert_eq!(unsafe { nt_allocate_reserve_object(1usize as *mut HANDLE, core::ptr::null_mut(), 1) }, STATUS_ACCESS_VIOLATION);
    assert_eq!(unsafe { nt_allocate_reserve_object(core::ptr::null_mut(), core::ptr::null_mut(), 2) }, STATUS_INVALID_PARAMETER);
    for handle in [query, reserved, port] { assert_eq!(unsafe { nt_close_sync(handle) }, 0); }
}

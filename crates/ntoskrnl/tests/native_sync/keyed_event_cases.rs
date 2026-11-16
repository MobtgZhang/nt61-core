use super::*;
use ntdll::{file, keyed_event::*};

fn create(access: u32) -> HANDLE {
    let mut handle = core::ptr::null_mut();
    assert_eq!(unsafe {
        nt_create_keyed_event(&mut handle, access, core::ptr::null_mut(), 0)
    }, STATUS_SUCCESS);
    handle
}

fn begin(handle: HANDLE, key: u64, release: bool, pid: u64) -> file::WaitStart {
    file::begin_keyed_wait(handle, key, release, pid, 701, None, None).unwrap()
}

fn pending(start: file::WaitStart) -> impl Fn() -> Option<i32> {
    let file::WaitStart::Pending(wait) = start else {
        panic!("expected a queued rendezvous");
    };
    move || wait.poll()
}

fn success(start: file::WaitStart) {
    assert!(matches!(start, file::WaitStart::Complete(STATUS_SUCCESS)));
}

#[test]
fn keyed_rendezvous_works_in_both_orders_without_storing_signals() {
    let _guard = state_guard();
    for handle in [create(3), core::ptr::null_mut()] {
        for release in [true, false] {
            let first = pending(begin(handle, 0x1234, release, 100));
            let second = pending(begin(handle, 0x1234, release, 100));
            assert_eq!(first(), None);
            success(begin(handle, 0x1234, !release, 100));
            assert_eq!(first(), Some(STATUS_SUCCESS));
            assert_eq!(second(), None);
            success(begin(handle, 0x1234, !release, 100));
            assert_eq!(second(), Some(STATUS_SUCCESS));
        }
        assert!(matches!(file::begin_keyed_wait(handle, 0x1234, false, 100, 701, Some(0), None),
            Ok(file::WaitStart::Complete(STATUS_TIMEOUT))));
        if !handle.is_null() { assert!(file::free_handle(handle)); }
    }
}

#[test]
fn keys_processes_objects_and_queue_order_are_independent() {
    let _guard = state_guard();
    let handle = create(3);
    let other = create(3);
    let first = pending(begin(handle, 2, false, 100));
    let wrong_key = pending(begin(handle, 4, true, 100));
    let wrong_pid = pending(begin(handle, 2, true, 101));
    let wrong_object = pending(begin(other, 2, true, 100));
    success(begin(handle, 2, true, 100));
    assert_eq!(first(), Some(STATUS_SUCCESS));
    drop(first); // Recycle the lowest slot while an older request is queued.
    let younger = pending(begin(handle, 4, true, 100));
    success(begin(handle, 4, false, 100));
    assert_eq!(wrong_key(), Some(STATUS_SUCCESS));
    assert_eq!(younger(), None);
    assert_eq!(wrong_pid(), None);
    assert_eq!(wrong_object(), None);
    for handle in [handle, other] { assert!(file::free_handle(handle)); }
}

#[test]
fn keyed_wait_retains_object_across_close_and_handle_reuse() {
    let _guard = state_guard();
    let handle = create(3);
    let alias = file::duplicate_sync_handle(handle).unwrap();
    let wait = pending(begin(handle, 2, false, 100));
    assert!(file::free_handle(handle));
    success(begin(alias, 2, true, 100));
    assert_eq!(wait(), Some(STATUS_SUCCESS));
    drop(wait);
    let wait = pending(begin(alias, 4, false, 100));
    assert!(file::free_handle(alias));
    let replacement = create(3);
    let unrelated = pending(begin(replacement, 4, true, 100));
    assert_eq!(wait(), None);
    assert_eq!(unrelated(), None);
    assert!(file::free_handle(replacement));
}

#[test]
fn keyed_native_wrappers_check_access_alignment_and_timeout() {
    let _guard = state_guard();
    let handle = create(3);
    let wait_only = create(0x80000000); // GENERIC_READ -> WAIT | READ_CONTROL.
    let wake_only = create(0x40000000);
    let execute = create(0x20000000);
    let zero = 0;
    unsafe {
        assert_eq!(nt_wait_for_keyed_event(handle, 1, 0, &zero), 0xc00000ef_u32 as i32);
        assert_eq!(nt_release_keyed_event(wait_only, 2, 0, &zero), STATUS_ACCESS_DENIED);
        assert_eq!(nt_wait_for_keyed_event(wake_only, 2, 0, &zero), STATUS_ACCESS_DENIED);
        assert_eq!(nt_wait_for_keyed_event(execute, 2, 0, &zero), STATUS_ACCESS_DENIED);
        assert_eq!(nt_wait_for_keyed_event(wait_only, 2, 0, &zero), STATUS_TIMEOUT);
        assert_eq!(nt_release_keyed_event(wake_only, 2, 0, &zero), STATUS_TIMEOUT);
        assert_eq!(nt_wait_for_keyed_event(handle, 2, 0, 1usize as *const i64), STATUS_ACCESS_VIOLATION);
        let event = create_event_kernel(1, false).unwrap();
        assert_eq!(nt_wait_for_keyed_event(event, 2, 0, &zero), STATUS_OBJECT_TYPE_MISMATCH);
        assert!(file::free_handle(event));
        let relative = -100;
        ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(100));
        assert_eq!(nt_release_keyed_event(handle, 2, 0, &relative), STATUS_TIMEOUT);
        ke::scheduler::ADVANCE_ON_YIELD.with(|delta| delta.set(0));
        let expired = 1;
        assert_eq!(nt_wait_for_keyed_event(handle, 2, 0, &expired), STATUS_TIMEOUT);
    }
    for handle in [handle, wait_only, wake_only, execute] { assert!(file::free_handle(handle)); }
}

#[test]
fn alerts_and_timeouts_remove_keyed_rendezvous_candidates() {
    let _guard = state_guard();
    let handle = create(3);
    let state = ntdll::alert_state::UserAlertState::new();
    let wait = pending(file::begin_keyed_wait(handle, 2, false, 100, 701, None, Some(&state)).unwrap());
    assert_eq!(file::alert_user_thread(701, &state), STATUS_SUCCESS);
    assert_eq!(wait(), Some(STATUS_ALERTED));
    assert!(matches!(file::begin_keyed_wait(handle, 2, true, 100, 701, Some(0), None),
        Ok(file::WaitStart::Complete(STATUS_TIMEOUT))));
    let expired = pending(file::begin_keyed_wait(handle, 4, false, 100, 701, Some(-1), None).unwrap());
    ke::time::advance(1);
    assert!(matches!(file::begin_keyed_wait(handle, 4, true, 100, 701, Some(0), None),
        Ok(file::WaitStart::Complete(STATUS_TIMEOUT))));
    assert_eq!(expired(), Some(STATUS_TIMEOUT));
    assert!(file::free_handle(handle));
}

#[test]
fn named_keyed_events_and_event_pairs_share_the_object_namespace() {
    let _guard = state_guard();
    super::named_cases::named("\\BaseNamedObjects\\KeyedRendezvous", 0, |oa| unsafe {
        let mut handle = core::ptr::null_mut();
        let mut alias = core::ptr::null_mut();
        assert_eq!(nt_create_keyed_event(&mut handle, 3, oa, 0), STATUS_SUCCESS);
        assert_eq!(nt_open_keyed_event(&mut alias, 3, oa), STATUS_SUCCESS);
        let wait = pending(begin(handle, 2, false, 100));
        success(begin(alias, 2, true, 100));
        assert_eq!(wait(), Some(STATUS_SUCCESS));
        assert_eq!(ntdll::event_pair::nt_open_event_pair(&mut alias, 0x100000, oa), STATUS_OBJECT_TYPE_MISMATCH);
        assert!(file::free_handle(handle));
        assert!(file::free_handle(alias));
    });
    super::named_cases::named("\\BaseNamedObjects\\NamedPair", 0, |oa| unsafe {
        let mut handle = core::ptr::null_mut();
        let mut alias = core::ptr::null_mut();
        assert_eq!(ntdll::event_pair::nt_create_event_pair(&mut handle, 0x100000, oa), STATUS_SUCCESS);
        assert_eq!(ntdll::event_pair::nt_open_event_pair(&mut alias, 0x100000, oa), STATUS_SUCCESS);
        assert_eq!(ntdll::event_pair::nt_set_low_event_pair(handle), STATUS_SUCCESS);
        assert_eq!(ntdll::event_pair::nt_wait_low_event_pair(alias), STATUS_SUCCESS);
        assert!(file::free_handle(handle));
        assert!(file::free_handle(alias));
    });
}

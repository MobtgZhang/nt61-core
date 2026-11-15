use super::*;
use ntdll::{alert, alert_state::UserAlertState, file};

fn alert_self() {
    // SAFETY: NtCurrentThread is a scalar pseudo handle, no pointers involved.
    assert_eq!(
        unsafe { alert::nt_alert_thread((-2isize) as HANDLE) },
        STATUS_SUCCESS
    );
}

#[test]
fn user_alerts_coalesce_and_are_consumed_once() {
    let _guard = state_guard();
    let thread = file::alloc_handle(file::HandleKind::Thread, 701);
    unsafe {
        assert_eq!(alert::nt_alert_thread(thread), STATUS_SUCCESS);
        assert_eq!(alert::nt_alert_thread(thread), STATUS_SUCCESS);
        assert_eq!(alert::nt_test_alert(), STATUS_ALERTED);
        assert_eq!(alert::nt_test_alert(), STATUS_SUCCESS);
        assert_eq!(
            alert::nt_alert_thread(core::ptr::null_mut()),
            STATUS_INVALID_HANDLE
        );
    }
    let event = create_event_kernel(1, false).unwrap();
    assert_eq!(
        unsafe { alert::nt_alert_thread(event) },
        STATUS_OBJECT_TYPE_MISMATCH
    );
    let stale = file::alloc_handle(file::HandleKind::Thread, 999);
    assert_eq!(
        unsafe { alert::nt_alert_thread(stale) },
        STATUS_THREAD_IS_TERMINATING
    );
    for handle in [thread, stale, event] {
        assert!(file::free_handle(handle));
    }
}

#[test]
fn ready_object_precedes_pending_alert_and_invalid_wait_preserves_it() {
    let _guard = state_guard();
    let event = create_event_kernel(1, true).unwrap();
    alert_self();
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 1, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(core::ptr::null_mut(), 1, Some(0)),
        STATUS_INVALID_HANDLE
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 1, Some(0)),
        STATUS_ALERTED
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 1, Some(0)),
        STATUS_TIMEOUT
    );
    assert!(file::free_handle(event));
}

#[test]
fn nonalertable_wait_and_delay_leave_user_alert_pending() {
    let _guard = state_guard();
    let event = create_event_kernel(1, false).unwrap();
    alert_self();
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert_eq!(nt_delay_execution_kernel(0, 0), STATUS_SUCCESS);
    assert_eq!(nt_delay_execution_kernel(255, -1_000_000), STATUS_ALERTED);
    assert_eq!(unsafe { alert::nt_test_alert() }, STATUS_SUCCESS);
    assert!(file::free_handle(event));
}

#[test]
fn queued_alert_finishes_without_consuming_later_event_signal() {
    let _guard = state_guard();
    let event = create_event_kernel(1, false).unwrap();
    let state = UserAlertState::new();
    let wait = match file::begin_wait(&[event], false, 100, 701, None, Some(&state)).unwrap() {
        file::WaitStart::Pending(wait) => wait,
        _ => panic!("wait should be queued"),
    };
    assert_eq!(file::alert_user_thread(701, &state), STATUS_SUCCESS);
    assert_eq!(
        unsafe { nt_set_event(event, core::ptr::null_mut()) },
        STATUS_SUCCESS
    );
    assert_eq!(wait.poll(), Some(STATUS_ALERTED));
    assert!(!file::take_user_alert(&state));
    drop(wait);
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert!(file::free_handle(event));
}

#[test]
fn completed_wait_preserves_subsequent_alert_and_alert_cannot_change_timeout() {
    let _guard = state_guard();
    let event = create_event_kernel(1, false).unwrap();
    let state = UserAlertState::new();
    let wait = match file::begin_wait(&[event], false, 100, 701, Some(-10), Some(&state)).unwrap() {
        file::WaitStart::Pending(wait) => wait,
        _ => panic!("wait should be queued"),
    };
    ke::time::advance(10);
    assert_eq!(wait.poll(), Some(STATUS_TIMEOUT));
    assert_eq!(file::alert_user_thread(701, &state), STATUS_SUCCESS);
    assert_eq!(wait.poll(), Some(STATUS_TIMEOUT));
    assert!(file::take_user_alert(&state));
    drop(wait);
    assert!(file::free_handle(event));
}

#[test]
fn alert_state_does_not_alias_modulo_tid_and_termination_clears_it() {
    let _guard = state_guard();
    let first = UserAlertState::new();
    let second = UserAlertState::new();
    assert_eq!(file::alert_user_thread(701, &first), STATUS_SUCCESS);
    assert!(!file::take_user_alert(&second));
    assert_eq!(file::alert_user_thread(957, &second), STATUS_SUCCESS);
    assert!(file::take_user_alert(&first));
    file::terminate_user_alert(&second);
    assert!(!file::take_user_alert(&second));
    assert_eq!(
        file::alert_user_thread(957, &second),
        STATUS_THREAD_IS_TERMINATING
    );
}

#[test]
fn signal_and_wait_signals_even_when_pending_alert_interrupts_wait() {
    let _guard = state_guard();
    let signal = create_event_kernel(1, false).unwrap();
    let wait = create_event_kernel(1, false).unwrap();
    alert_self();
    assert_eq!(
        signal_and_wait_kernel(signal, wait, 1, Some(0)),
        STATUS_ALERTED
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(signal, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(wait, 0, Some(0)),
        STATUS_TIMEOUT
    );
    assert!(file::free_handle(signal));
    assert!(file::free_handle(wait));
}

#[test]
fn user_alert_interrupts_running_delay_and_wait_after_yield() {
    let _guard = state_guard();
    let event = create_event_kernel(1, false).unwrap();
    ke::scheduler::YIELD_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(alert_self)));
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 1, None),
        STATUS_ALERTED
    );
    ke::scheduler::YIELD_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(alert_self)));
    assert_eq!(nt_delay_execution_kernel(1, -100_000), STATUS_ALERTED);
    assert_eq!(unsafe { alert::nt_test_alert() }, STATUS_SUCCESS);
    assert!(file::free_handle(event));
}

#[test]
fn alert_waitall_does_not_consume_partial_signals_and_releases_pins() {
    let _guard = state_guard();
    let event = create_event_kernel(1, true).unwrap();
    let sem = create_semaphore_kernel(0, 1).unwrap();
    alert_self();
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[event, sem], 0, 1, None),
        STATUS_ALERTED
    );
    assert_eq!(
        nt_wait_for_single_object_kernel(event, 0, Some(0)),
        STATUS_SUCCESS
    );
    let before = file::handle_count();
    ke::scheduler::YIELD_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(alert_self)));
    assert_eq!(
        nt_wait_for_multiple_objects_kernel(2, &[event, sem], 0, 1, None),
        STATUS_ALERTED
    );
    assert!(file::free_handle(event));
    assert!(file::free_handle(sem));
    assert_eq!(file::handle_count(), before - 2);
}

#[test]
fn concurrent_signal_and_alert_choose_one_wait_completion() {
    let _guard = state_guard();
    for _ in 0..64 {
        let event = create_event_kernel(1, false).unwrap();
        let state = std::sync::Arc::new(UserAlertState::new());
        let wait = match file::begin_wait(&[event], false, 100, 701, None, Some(&state)).unwrap() {
            file::WaitStart::Pending(wait) => wait,
            _ => panic!("wait should be queued"),
        };
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let signal = {
            let barrier = barrier.clone();
            let raw = event as usize;
            std::thread::spawn(move || {
                barrier.wait();
                unsafe { nt_set_event(raw as HANDLE, core::ptr::null_mut()) }
            })
        };
        let alert = {
            let barrier = barrier.clone();
            let state = state.clone();
            std::thread::spawn(move || {
                barrier.wait();
                file::alert_user_thread(701, &state)
            })
        };
        barrier.wait();
        assert_eq!(signal.join().unwrap(), STATUS_SUCCESS);
        assert_eq!(alert.join().unwrap(), STATUS_SUCCESS);
        let result = wait.poll().unwrap();
        drop(wait);
        let available = nt_wait_for_single_object_kernel(event, 0, Some(0));
        match result {
            STATUS_ALERTED => {
                assert_eq!(available, STATUS_SUCCESS);
                assert!(!file::take_user_alert(&state));
            }
            STATUS_SUCCESS => {
                assert_eq!(available, STATUS_TIMEOUT);
                assert!(file::take_user_alert(&state));
            }
            status => panic!("unexpected wait status {status:#x}"),
        }
        assert!(file::free_handle(event));
    }
}

#[test]
fn unavailable_apc_and_alert_resume_do_not_claim_or_execute_side_effects() {
    let _guard = state_guard();
    let mut count = 77u32;
    let current = (-2isize) as HANDLE;
    unsafe {
        assert_eq!(
            ntdll::apc::nt_queue_apc_thread(
                current,
                1usize as _,
                2usize as _,
                3usize as _,
                4usize as _
            ),
            STATUS_NOT_IMPLEMENTED
        );
        assert_eq!(
            ntdll::apc::nt_alert_resume_thread(current, &mut count),
            STATUS_NOT_IMPLEMENTED
        );
        assert_eq!(count, 77);
        assert_eq!(
            ntdll::apc::nt_alert_resume_thread(current, 1usize as _),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(alert::nt_test_alert(), STATUS_SUCCESS);
    }
}

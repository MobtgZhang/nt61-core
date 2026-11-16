use super::*;
use ntdll::timer::{nt_set_timer_ex, TimerSetCoalescableTimerInfo};

fn information(previous: *mut u8) -> TimerSetCoalescableTimerInfo {
    TimerSetCoalescableTimerInfo {
        due_time: -20_000,
        apc_routine: core::ptr::null_mut(),
        context: core::ptr::null_mut(),
        wake_context: core::ptr::null_mut(),
        period: 3,
        tolerable_delay: 10,
        previous_state: previous,
    }
}

#[test]
fn coalescable_timer_expires_and_rearms_through_the_shared_dispatcher() {
    let _guard = state_guard();
    let timer = ntdll::timer::create_timer_kernel(1).unwrap();
    let mut previous = 123;
    let mut info = information(&mut previous);
    unsafe {
        assert_eq!(nt_set_timer_ex(timer, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_SUCCESS);
    }
    assert_eq!(previous, 0);
    assert_eq!(nt_wait_for_single_object_kernel(timer, 0, Some(0)), STATUS_TIMEOUT);
    ke::time::advance(20_000);
    assert_eq!(nt_wait_for_single_object_kernel(timer, 0, Some(0)), STATUS_SUCCESS);
    assert_eq!(nt_wait_for_single_object_kernel(timer, 0, Some(0)), STATUS_TIMEOUT);
    ke::time::advance(30_000);
    // Rearming reports the signal before resetting it, independently of waits.
    unsafe {
        assert_eq!(nt_set_timer_ex(timer, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_SUCCESS);
    }
    assert_eq!(previous, 1);
    assert_eq!(nt_wait_for_single_object_kernel(timer, 0, Some(0)), STATUS_TIMEOUT);
    assert_eq!(unsafe { nt_close_sync(timer) }, STATUS_SUCCESS);
}

#[test]
fn coalescable_timer_rejects_bad_buffers_and_unsupported_requests_without_rearming() {
    let _guard = state_guard();
    let timer = ntdll::timer::create_timer_kernel(0).unwrap();
    let due = 1i64;
    unsafe {
        assert_eq!(nt_set_timer(timer, &due, core::ptr::null_mut(), core::ptr::null_mut(), 0, 0, core::ptr::null_mut()), STATUS_SUCCESS);
    }
    let mut previous = 123;
    let mut info = information(&mut previous);
    unsafe {
        assert_eq!(nt_set_timer_ex(timer, 1, core::ptr::null_mut(), 48), STATUS_INVALID_INFO_CLASS);
        for length in [0, 47, 49, u32::MAX] {
            assert_eq!(nt_set_timer_ex(timer, 0, core::ptr::null_mut(), length), STATUS_INFO_LENGTH_MISMATCH);
        }
        assert_eq!(nt_set_timer_ex(timer, 0, core::ptr::null_mut(), 48), STATUS_ACCESS_VIOLATION);
        info.wake_context = 1usize as PVOID;
        assert_eq!(nt_set_timer_ex(timer, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_NOT_SUPPORTED);
        info.wake_context = core::ptr::null_mut();
        info.apc_routine = 1usize as PVOID;
        assert_eq!(nt_set_timer_ex(timer, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_NOT_SUPPORTED);
        info.apc_routine = core::ptr::null_mut();
        info.period = u32::MAX;
        assert_eq!(nt_set_timer_ex(timer, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_INVALID_PARAMETER);
        info.period = 0;
        mm::user_copy::FAIL_WRITE.with(|slot| slot.set(&mut previous as *mut u8 as u64));
        assert_eq!(nt_set_timer_ex(timer, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_ACCESS_VIOLATION);
        mm::user_copy::FAIL_WRITE.with(|slot| slot.set(0));
    }
    assert_eq!(previous, 123);
    assert_eq!(nt_wait_for_single_object_kernel(timer, 0, Some(0)), STATUS_SUCCESS);
    assert_eq!(unsafe { nt_close_sync(timer) }, STATUS_SUCCESS);
}

#[test]
fn coalescable_timer_enforces_modify_right_and_object_type() {
    let _guard = state_guard();
    let timer = ntdll::timer::create_timer_kernel(0).unwrap();
    let limited = ntdll::file::duplicate_sync_handle_with_access(timer, 1, 0, 0).unwrap();
    let event = create_event_kernel(0, false).unwrap();
    let mut info = information(core::ptr::null_mut());
    unsafe {
        assert_eq!(nt_set_timer_ex(limited, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_ACCESS_DENIED);
        assert_eq!(nt_set_timer_ex(event, 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_OBJECT_TYPE_MISMATCH);
        assert_eq!(nt_set_timer_ex(core::ptr::null_mut(), 0, (&mut info as *mut TimerSetCoalescableTimerInfo).cast(), 48), STATUS_INVALID_HANDLE);
    }
    for handle in [limited, timer, event] {
        assert_eq!(unsafe { nt_close_sync(handle) }, STATUS_SUCCESS);
    }
}

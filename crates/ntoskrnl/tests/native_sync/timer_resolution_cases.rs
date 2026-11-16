use super::*;
use ntdll::timer_resolution::*;

#[test]
fn timer_resolution_reports_real_hal_limits_and_process_request_state() {
    let _guard = state_guard();
    let (mut minimum, mut maximum, mut actual) = (0, 0, 0);
    unsafe {
        assert_eq!(nt_query_timer_resolution(&mut minimum, &mut maximum, &mut actual), STATUS_SUCCESS);
        assert_eq!((minimum, maximum, actual), (10_000, 10_000, 10_000));
        assert_eq!(nt_set_timer_resolution(5000, 0, &mut actual), 0xc0000245u32 as i32);
        assert_eq!(nt_set_timer_resolution(5000, 1, &mut actual), STATUS_SUCCESS);
        assert_eq!(nt_set_timer_resolution(1, 1, &mut actual), STATUS_SUCCESS);
        assert_eq!(actual, 10_000);
        assert_eq!(nt_set_timer_resolution(0, 0, &mut actual), STATUS_SUCCESS);
        assert_eq!(nt_set_timer_resolution(0, 0, &mut actual), 0xc0000245u32 as i32);
        assert_eq!(nt_set_timer_resolution(1, 1, &mut actual), STATUS_SUCCESS);
        process_exit(100);
        assert_eq!(nt_set_timer_resolution(0, 0, &mut actual), 0xc0000245u32 as i32);
    }
}

#[test]
fn timer_resolution_rejects_output_fault_without_retaining_request() {
    let _guard = state_guard();
    let mut actual = 0;
    mm::user_copy::FAIL_WRITE.with(|value| value.set(&mut actual as *mut u32 as u64));
    unsafe {
        assert_eq!(nt_set_timer_resolution(1, 1, &mut actual), STATUS_ACCESS_VIOLATION);
        mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
        assert_eq!(nt_set_timer_resolution(0, 0, &mut actual), 0xc0000245u32 as i32);
        assert_eq!(nt_query_timer_resolution(&mut actual, 1 as *mut u32, &mut actual), STATUS_ACCESS_VIOLATION);
    }
    assert_eq!(actual, 10_000);
}

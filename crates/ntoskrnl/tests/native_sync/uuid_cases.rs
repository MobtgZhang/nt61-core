use super::*;
use ntdll::uuid::{nt_allocate_uuids, nt_set_uuid_seed};

fn allocate() -> (i32, u64, u32, u32, [u8; 6]) {
    let (mut time, mut range, mut sequence, mut seed) = (0, 0, 0, [0u8; 6]);
    let status =
        unsafe { nt_allocate_uuids(&mut time, &mut range, &mut sequence, seed.as_mut_ptr()) };
    (status, time, range, sequence, seed)
}

#[test]
fn uuid_ranges_are_disjoint_and_clock_rollback_changes_sequence() {
    let _guard = state_guard();
    ke::time::CLOCK.with(|clock| clock.set((10_000_000, 0)));
    let first = allocate();
    assert_eq!(first.0, STATUS_SUCCESS);
    assert!(first.2 > 0);
    ke::time::advance(20_000);
    let second = allocate();
    assert_eq!(second.0, STATUS_SUCCESS);
    assert!(second.1 >= first.1 + u64::from(first.2));
    assert_eq!(second.3, first.3);
    assert_eq!(second.4, first.4);
    ke::time::CLOCK.with(|clock| clock.set((1_000_000, 0)));
    let rollback = allocate();
    assert_eq!(rollback.0, STATUS_SUCCESS);
    assert_ne!(rollback.3 & 0x3fff, first.3 & 0x3fff);
    assert_eq!(allocate().0, 0xc000022du32 as i32);
}

#[test]
fn uuid_seed_requires_system_logon_and_captures_exactly_six_bytes() {
    let _guard = state_guard();
    let seed = [0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xff];
    unsafe {
        ntdll::privilege::ENABLED.with(|enabled| enabled.set(false));
        assert_eq!(nt_set_uuid_seed(seed.as_ptr()), STATUS_ACCESS_DENIED);
        ntdll::privilege::ENABLED.with(|enabled| enabled.set(true));
        assert_eq!(nt_set_uuid_seed(1 as *const u8), STATUS_ACCESS_VIOLATION);
        assert_eq!(nt_set_uuid_seed(seed.as_ptr()), STATUS_SUCCESS);
    }
    ke::time::CLOCK.with(|clock| clock.set((500_000_000, 0)));
    let values = allocate();
    assert_eq!(values.0, STATUS_SUCCESS);
    assert_eq!(values.4, seed[..6]);
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(false));
}

#[test]
fn uuid_outputs_are_probed_and_misalignment_is_reported_before_allocation() {
    let _guard = state_guard();
    let (mut time, mut range, mut sequence, mut seed) = (0, 0, 0, [0u8; 6]);
    unsafe {
        assert_eq!(
            nt_allocate_uuids(3 as *mut u64, &mut range, &mut sequence, seed.as_mut_ptr()),
            0x80000002u32 as i32
        );
        assert_eq!(
            nt_allocate_uuids(&mut time, &mut range, &mut sequence, 1 as *mut u8),
            STATUS_ACCESS_VIOLATION
        );
    }
    assert_eq!((time, range, sequence, seed), (0, 0, 0, [0; 6]));
}

#[test]
fn uuid_failed_copy_does_not_reissue_reserved_timestamp_range() {
    let _guard = state_guard();
    ke::time::CLOCK.with(|clock| clock.set((900_000_000, 0)));
    let before = allocate();
    assert_eq!(before.0, STATUS_SUCCESS);
    ke::time::advance(20_000);
    let (mut time, mut range, mut sequence, mut seed) = (0, 0, 0, [0u8; 6]);
    mm::user_copy::FAIL_WRITE.with(|fault| fault.set(&mut range as *mut u32 as u64));
    let status =
        unsafe { nt_allocate_uuids(&mut time, &mut range, &mut sequence, seed.as_mut_ptr()) };
    mm::user_copy::FAIL_WRITE.with(|fault| fault.set(0));
    assert_eq!(status, STATUS_ACCESS_VIOLATION);
    ke::time::advance(20_000);
    let after = allocate();
    assert_eq!(after.0, STATUS_SUCCESS);
    assert!(after.1 >= before.1 + u64::from(before.2));
    assert!(after.1 > time);
}

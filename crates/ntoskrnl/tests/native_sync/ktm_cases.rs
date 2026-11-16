use super::*;
use ntdll::ktm::*;
use core::ptr::{null, null_mut};

fn manager() -> HANDLE {
    let mut handle = null_mut();
    assert_eq!(unsafe { nt_create_transaction_manager(&mut handle, 0xf003f, null_mut(), null(), 1, 0) }, 0);
    handle
}

fn transaction(manager: HANDLE, timeout: Option<i64>) -> HANDLE {
    let mut handle = null_mut();
    assert_eq!(unsafe { nt_create_transaction(&mut handle, 0x1f003f, null_mut(), null(), manager,
        0, 0, 0, timeout.as_ref().map_or(null(), |timeout| timeout), null()) }, 0);
    handle
}

fn basic(handle: HANDLE) -> [u8; 24] {
    let mut bytes = [0; 24];
    let mut returned = 0;
    assert_eq!(unsafe { nt_query_information_transaction(handle, 0, bytes.as_mut_ptr().cast(), 24, &mut returned) }, 0);
    assert_eq!(returned, 24);
    bytes
}

fn close(handle: HANDLE) { assert_eq!(unsafe { ntdll::file::nt_close(handle) }, 0); }

#[test]
fn transaction_commit_and_rollback_are_shared_across_guid_opens() {
    let _guard = state_guard();
    let tm = manager();
    let tx = transaction(tm, None);
    let identity: [u8; 16] = basic(tx)[..16].try_into().unwrap();
    let mut reopened = null_mut();
    assert_eq!(unsafe { nt_open_transaction(&mut reopened, 0x1f003f, null_mut(), &identity, tm) }, 0);
    assert_eq!(nt_commit_transaction(reopened, 1), 0);
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 2);
    assert_eq!(nt_rollback_transaction(tx, 1), 0xc0190016u32 as i32);
    close(tx);
    assert_eq!(u32::from_le_bytes(basic(reopened)[20..24].try_into().unwrap()), 2);
    close(reopened);
    assert_eq!(unsafe { nt_open_transaction(&mut reopened, 1, null_mut(), &identity, tm) }, 0xc019004eu32 as i32);
    close(tm);
}

#[test]
fn transaction_parent_lives_until_all_children_and_guid_opens_close() {
    let _guard = state_guard();
    let tm = manager();
    let mut info = [0; 24];
    assert_eq!(unsafe { nt_query_information_transaction_manager(tm, 0, info.as_mut_ptr().cast(), 24, null_mut()) }, 0);
    let identity: [u8; 16] = info[..16].try_into().unwrap();
    let tx = transaction(tm, None);
    close(tm);
    let mut reopened = null_mut();
    assert_eq!(unsafe { nt_open_transaction_manager(&mut reopened, 1, null_mut(), null(), &identity, 0) }, 0);
    close(reopened);
    close(tx);
    assert_eq!(unsafe { nt_open_transaction_manager(&mut reopened, 1, null_mut(), null(), &identity, 0) }, 0xc0190051u32 as i32);
}

#[test]
fn transaction_timeout_uses_monotonic_time_for_relative_deadlines() {
    let _guard = state_guard();
    let tx = transaction(null_mut(), Some(-20));
    ke::time::advance(19);
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 1);
    ke::time::CLOCK.with(|clock| clock.set((50_000, 20)));
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 3);
    assert_eq!(nt_commit_transaction(tx, 1), 0xc0000210u32 as i32);
    close(tx);
    let tx = transaction(null_mut(), Some(50_010));
    ke::time::advance(10);
    assert_eq!(nt_commit_transaction(tx, 0), 0xc0000210u32 as i32);
    close(tx);
}

#[test]
fn transaction_rights_and_type_checks_prevent_mutation() {
    let _guard = state_guard();
    let tx = transaction(null_mut(), None);
    let alias = ntdll::object::duplicate_object_kernel((-1isize) as HANDLE, tx,
        (-1isize) as HANDLE, 1, 0, 0).unwrap();
    assert_eq!(nt_commit_transaction(alias, 1), STATUS_ACCESS_DENIED);
    let tm = manager();
    assert_eq!(nt_commit_transaction(tm, 1), STATUS_OBJECT_TYPE_MISMATCH);
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 1);
    assert_eq!(nt_rollback_transaction(tx, 0), 0);
    assert_eq!(nt_commit_transaction(tx, 0), 0xc0190015u32 as i32);
    close(alias); close(tx); close(tm);
}

#[test]
fn transaction_properties_capture_unicode_and_validate_lengths() {
    let _guard = state_guard();
    let tx = transaction(null_mut(), None);
    let mut input = [0u8; 30];
    input[20..24].copy_from_slice(&6u32.to_le_bytes());
    input[24..].copy_from_slice(&[0x2d, 0x4e, 0x87, 0x65, 0, 0]);
    assert_eq!(unsafe { nt_set_information_transaction(tx, 1, input.as_mut_ptr().cast(), 29) }, STATUS_INVALID_PARAMETER);
    assert_eq!(unsafe { nt_set_information_transaction(tx, 1, input.as_mut_ptr().cast(), 30) }, 0);
    input[24] = 0;
    let mut output = [0u8; 30];
    let mut returned = 0;
    assert_eq!(unsafe { nt_query_information_transaction(tx, 1, output.as_mut_ptr().cast(), 29, &mut returned) }, STATUS_INFO_LENGTH_MISMATCH);
    assert_eq!(returned, 30);
    assert_eq!(unsafe { nt_query_information_transaction(tx, 1, output.as_mut_ptr().cast(), 30, &mut returned) }, 0);
    assert_eq!(output[24], 0x2d);
    assert_eq!(nt_commit_transaction(tx, 1), 0);
    assert_eq!(unsafe { nt_set_information_transaction(tx, 1, input.as_mut_ptr().cast(), 30) }, 0xc0190003u32 as i32);
    close(tx);
}

#[test]
fn transaction_output_fault_releases_guid_and_parent_references() {
    let _guard = state_guard();
    let tm = manager();
    let identity = [0xa5; 16];
    let mut tx = null_mut();
    mm::user_copy::FAIL_WRITE.with(|value| value.set(&mut tx as *mut HANDLE as u64));
    assert_eq!(unsafe { nt_create_transaction(&mut tx, 1, null_mut(), &identity, tm,
        0, 0, 0, null(), null()) }, STATUS_ACCESS_VIOLATION);
    mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
    assert_eq!(unsafe { nt_open_transaction(&mut tx, 1, null_mut(), &identity, tm) }, 0xc019004eu32 as i32);
    close(tm);
}

#[test]
fn resource_manager_guid_is_scoped_to_manager_and_pins_parent() {
    let _guard = state_guard();
    let tm = manager();
    let other = manager();
    let identity = [0x45; 16];
    let mut rm = null_mut();
    assert_eq!(unsafe { nt_create_resource_manager(&mut rm, 0x1f007f, tm, &identity, null_mut(), 1, null()) }, 0);
    let mut reopened = null_mut();
    assert_eq!(unsafe { nt_open_resource_manager(&mut reopened, 1, other, &identity, null_mut()) }, 0xc019004fu32 as i32);
    assert_eq!(unsafe { nt_open_resource_manager(&mut reopened, 1, tm, &identity, null_mut()) }, 0);
    close(tm); close(rm);
    let mut info = [0u8; 20];
    assert_eq!(unsafe { nt_query_information_resource_manager(reopened, 0, info.as_mut_ptr().cast(), 20, null_mut()) }, 0);
    assert_eq!(&info[..16], &identity);
    close(reopened); close(other);
}

#[test]
fn duplicate_transaction_guid_rejected_without_leaking_new_handle() {
    let _guard = state_guard();
    let identity = [0xb5; 16];
    let mut tx = null_mut();
    let mut other = null_mut();
    assert_eq!(unsafe { nt_create_transaction(&mut tx, 1, null_mut(), &identity, null_mut(), 0, 0, 0, null(), null()) }, 0);
    assert_eq!(unsafe { nt_create_transaction(&mut other, 1, null_mut(), &identity, null_mut(), 0, 0, 0, null(), null()) }, 0xc000022bu32 as i32);
    assert!(other.is_null());
    close(tx);
    assert_eq!(unsafe { nt_create_transaction(&mut other, 1, null_mut(), &identity, null_mut(), 0, 0, 0, null(), null()) }, 0);
    close(other);
}

#[test]
fn transaction_enumeration_pages_are_scoped_and_do_not_return_other_types() {
    let _guard = state_guard();
    let tm = manager();
    let other = manager();
    let tx1 = transaction(tm, None);
    let tx2 = transaction(tm, None);
    let tx3 = transaction(other, None);
    let mut cursor = [0u8; 36];
    let mut returned = 0;
    assert_eq!(unsafe { nt_enumerate_transaction_object(tm, 0, cursor.as_mut_ptr().cast(), 20, &mut returned) }, STATUS_BUFFER_TOO_SMALL);
    assert_eq!(returned, 36);
    assert_eq!(unsafe { nt_enumerate_transaction_object(tm, 0, cursor.as_mut_ptr().cast(), 36, &mut returned) }, 0);
    assert_eq!(&cursor[20..36], &basic(tx1)[..16]);
    assert_eq!(&cursor[..16], &basic(tx1)[..16]);
    assert_eq!(unsafe { nt_enumerate_transaction_object(tm, 0, cursor.as_mut_ptr().cast(), 36, &mut returned) }, 0);
    assert_eq!(&cursor[20..36], &basic(tx2)[..16]);
    assert_eq!(unsafe { nt_enumerate_transaction_object(tm, 0, cursor.as_mut_ptr().cast(), 36, &mut returned) }, STATUS_NO_MORE_ENTRIES);
    close(tx1); close(tx2); close(tx3); close(tm); close(other);
}

#[test]
fn guid_open_never_aliases_a_different_named_object() {
    let _guard = state_guard();
    let tx = transaction(null_mut(), None);
    let identity: [u8; 16] = basic(tx)[..16].try_into().unwrap();
    let mut name: Vec<u16> = "\\BaseNamedObjects\\wrong-transaction".encode_utf16().collect();
    let mut text = UnicodeString { length: (name.len()*2) as u16,
        maximum_length: (name.len()*2) as u16, buffer: name.as_mut_ptr() };
    let mut attributes: ObjectAttributes = unsafe { core::mem::zeroed() };
    attributes.length = core::mem::size_of::<ObjectAttributes>() as u32;
    attributes.object_name = &mut text;
    attributes.attributes = 0x80;
    let mut output = null_mut();
    assert_eq!(unsafe { nt_open_transaction(&mut output, 1, &mut attributes, &identity, null_mut()) }, STATUS_NOT_SUPPORTED);
    assert!(output.is_null());
    close(tx);
}

fn resource(tm: HANDLE, identity: u8) -> HANDLE {
    let mut handle = null_mut();
    assert_eq!(unsafe { nt_create_resource_manager(&mut handle, 0x1f007f, tm,
        &[identity; 16], null_mut(), 1, null()) }, 0);
    handle
}

fn enlist(rm: HANDLE, tx: HANDLE, options: u32, mask: u32, key: usize) -> HANDLE {
    let mut handle = null_mut();
    assert_eq!(unsafe { nt_create_enlistment(&mut handle, 0xf001f, rm, tx,
        null_mut(), options, mask, key as PVOID) }, 0);
    handle
}

fn notification(rm: HANDLE, key: u64, mask: u32) {
    let mut packet = [0xa5u8; 32];
    let mut returned = 0;
    assert_eq!(unsafe { nt_get_notification_resource_manager(rm, packet.as_mut_ptr().cast(),
        32, &0, &mut returned, 0, 0) }, 0);
    assert_eq!(returned, 32);
    assert_eq!(u64::from_le_bytes(packet[..8].try_into().unwrap()), key);
    assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), mask);
    assert_eq!(&packet[12..16], &[0; 4]);
    assert_eq!(&packet[24..32], &[0; 8]);
}

#[test]
fn enlisted_commit_waits_for_votes_and_commit_acknowledgements() {
    let _guard = state_guard();
    let tm = manager();
    let rm = resource(tm, 0xc1);
    let tx = transaction(tm, Some(-100));
    let en = enlist(rm, tx, 0, 0xf, 0x1234);
    assert_eq!(unsafe { nt_prepare_complete(en, null()) }, 0xc0190013u32 as i32);
    assert_eq!(nt_commit_transaction(tx, 0), 0);
    notification(rm, 0x1234, 1);
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 1);
    assert_eq!(unsafe { nt_pre_prepare_complete(en, null()) }, 0);
    notification(rm, 0x1234, 2);
    assert_eq!(unsafe { nt_prepare_complete(en, null()) }, 0);
    notification(rm, 0x1234, 4);
    // Once prepared and committed, neither a timeout nor rollback can undo the decision.
    ke::time::advance(200);
    assert_eq!(nt_rollback_transaction(tx, 0), 0xc0190016u32 as i32);
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 2);
    close(tx);
    assert_eq!(unsafe { nt_commit_complete(en, null()) }, 0);
    assert_eq!(unsafe { nt_commit_complete(en, null()) }, 0xc0190013u32 as i32);
    close(en); close(rm); close(tm);
}

#[test]
fn notification_faults_do_not_consume_packets_and_guid_opens_share_state() {
    let _guard = state_guard();
    let tm = manager();
    let rm = resource(tm, 0xc2);
    let tx = transaction(tm, None);
    let en = enlist(rm, tx, 0, 0xe, 55);
    let mut info = [0u8; 48];
    assert_eq!(unsafe { nt_query_information_enlistment(en, 0, info.as_mut_ptr().cast(), 48, null_mut()) }, 0);
    let identity = info[..16].try_into().unwrap();
    let mut alias = null_mut();
    assert_eq!(unsafe { nt_open_enlistment(&mut alias, 0xf001f, rm, &identity, null_mut()) }, 0);
    let mut cursor = [0u8; 36];
    assert_eq!(unsafe { nt_enumerate_transaction_object(rm, 3, cursor.as_mut_ptr().cast(), 36, null_mut()) }, 0);
    assert_eq!(&cursor[20..36], &identity);
    assert_eq!(nt_commit_transaction(tx, 0), 0);
    let mut packet = [0u8; 32];
    let mut returned = 0;
    assert_eq!(unsafe { nt_get_notification_resource_manager(rm, packet.as_mut_ptr().cast(), 31,
        &0, &mut returned, 0, 0) }, STATUS_BUFFER_TOO_SMALL);
    assert_eq!(returned, 32);
    mm::user_copy::FAIL_WRITE.with(|value| value.set(packet.as_mut_ptr() as u64));
    assert_eq!(unsafe { nt_get_notification_resource_manager(rm, packet.as_mut_ptr().cast(), 32,
        &0, &mut returned, 0, 0) }, STATUS_ACCESS_VIOLATION);
    mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
    notification(rm, 55, 2);
    close(en);
    assert_eq!(unsafe { nt_read_only_enlistment(alias, null()) }, 0);
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 2);
    assert_eq!(unsafe { nt_get_notification_resource_manager(rm, packet.as_mut_ptr().cast(), 32,
        &0, &mut returned, 0, 0) }, STATUS_TIMEOUT);
    close(alias); close(tx); close(rm); close(tm);
}

#[test]
fn single_phase_reject_falls_back_to_prepare_and_can_roll_back() {
    let _guard = state_guard();
    let tm = manager();
    let rm = resource(tm, 0xc3);
    let tx = transaction(tm, None);
    let en = enlist(rm, tx, 0, 0x20e, 77);
    assert_eq!(nt_commit_transaction(tx, 0), 0);
    notification(rm, 77, 0x200);
    assert_eq!(unsafe { nt_single_phase_reject(en, null()) }, 0);
    notification(rm, 77, 2);
    assert_eq!(nt_rollback_transaction(tx, 0), 0);
    notification(rm, 77, 8);
    assert_eq!(unsafe { nt_prepare_complete(en, null()) }, 0xc0190013u32 as i32);
    assert_eq!(unsafe { nt_rollback_complete(en, null()) }, 0);
    close(en); close(tx); close(rm); close(tm);
}

#[test]
fn superior_controls_each_phase_and_receives_completion_notifications() {
    let _guard = state_guard();
    let tm = manager();
    let rm = resource(tm, 0xc4);
    let superior_rm = resource(tm, 0xc5);
    let tx = transaction(tm, None);
    let en = enlist(rm, tx, 0, 0xf, 88);
    let sup = enlist(superior_rm, tx, 1, 0x040000f0, 99);
    assert_eq!(nt_commit_transaction(tx, 0), 0);
    notification(superior_rm, 99, 0x04000000);
    assert_eq!(unsafe { nt_commit_enlistment(sup, null()) }, 0xc0190013u32 as i32);
    assert_eq!(unsafe { nt_pre_prepare_enlistment(sup, null()) }, 0);
    notification(rm, 88, 1);
    assert_eq!(unsafe { nt_pre_prepare_complete(en, null()) }, 0);
    notification(superior_rm, 99, 0x10);
    // Preprepare completion must not cause autonomous prepare or commit.
    assert_eq!(u32::from_le_bytes(basic(tx)[20..24].try_into().unwrap()), 1);
    assert_eq!(unsafe { nt_prepare_enlistment(sup, null()) }, 0);
    notification(rm, 88, 2);
    assert_eq!(unsafe { nt_prepare_complete(en, null()) }, 0);
    notification(superior_rm, 99, 0x20);
    assert_eq!(unsafe { nt_commit_enlistment(sup, null()) }, 0);
    notification(rm, 88, 4);
    assert_eq!(unsafe { nt_commit_complete(en, null()) }, 0);
    notification(superior_rm, 99, 0x40);
    close(sup); close(en); close(tx); close(superior_rm); close(rm); close(tm);
}

#[test]
fn resource_manager_description_update_preserves_guid_and_checks_mutation_rights() {
    let _guard = state_guard();
    let tm = manager();
    let identity = [0xabu8; 16];
    let mut rm = null_mut();
    assert_eq!(unsafe { nt_create_resource_manager(&mut rm, 3, tm, &identity, null_mut(), 1, null()) }, 0);
    let mut input = [0u8; 24];
    input[..16].fill(0xff); // The setter must not replace the identity.
    input[16..20].copy_from_slice(&4u32.to_le_bytes());
    input[20..].copy_from_slice(&[b'H', 0, b'i', 0]);
    let alias = ntdll::object::duplicate_object_kernel((-1isize) as HANDLE, rm,
        (-1isize) as HANDLE, 1, 0, 0).unwrap();
    assert_eq!(unsafe { nt_set_information_resource_manager(alias, 0, input.as_mut_ptr().cast(), 24) }, STATUS_ACCESS_DENIED);
    assert_eq!(unsafe { nt_set_information_resource_manager(rm, 0, input.as_mut_ptr().cast(), 24) }, 0);
    let mut output = [0u8; 24];
    let mut returned = 0;
    assert_eq!(unsafe { nt_query_information_resource_manager(rm, 0, output.as_mut_ptr().cast(), 24, &mut returned) }, 0);
    assert_eq!(returned, 24);
    assert_eq!(&output[..16], &identity);
    assert_eq!(&output[16..], &input[16..]);
    input[16..20].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(unsafe { nt_set_information_resource_manager(rm, 0, input.as_mut_ptr().cast(), 24) }, STATUS_INVALID_PARAMETER);
    assert_eq!(unsafe { nt_set_information_resource_manager(rm, 0, input.as_mut_ptr().cast(), 19) }, STATUS_INFO_LENGTH_MISMATCH);
    close(alias);
    close(rm);
    close(tm);
}

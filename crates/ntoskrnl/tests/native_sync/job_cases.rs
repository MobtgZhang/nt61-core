use super::*;
use ntdll::job::*;

fn create() -> HANDLE {
    let mut handle = core::ptr::null_mut();
    assert_eq!(unsafe { nt_create_job_object(&mut handle, 0x1f003f, core::ptr::null_mut()) }, 0);
    handle
}

#[test]
fn job_membership_limits_aliases_and_last_close_keep_process_membership() {
    let _guard = state_guard();
    let handle = create();
    let other = create();
    let process = (-1isize) as HANDLE;
    assert_eq!(nt_is_process_in_job(process, core::ptr::null_mut()), 0x123);
    let mut limits = [0u8; 64];
    limits[16..20].copy_from_slice(&8u32.to_le_bytes());
    limits[40..44].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(unsafe { nt_set_information_job_object(handle, 2, limits.as_mut_ptr().cast(), 64) }, 0);
    assert_eq!(nt_assign_process_to_job_object(handle, process), 0);
    assert_eq!(nt_is_process_in_job(process, handle), 0x124);
    assert_eq!(nt_assign_process_to_job_object(other, process), STATUS_ACCESS_DENIED);
    let second = ntdll::file::alloc_handle(ntdll::file::HandleKind::Process, 101);
    assert_eq!(nt_assign_process_to_job_object(handle, second), 0xc0000044u32 as i32);
    let alias = ntdll::object::duplicate_object_kernel(process, handle, process, 4, 0, 0).unwrap();
    assert_eq!(unsafe { nt_set_information_job_object(alias, 2, limits.as_mut_ptr().cast(), 64) }, STATUS_ACCESS_DENIED);
    let mut output = [0u8; 16];
    let mut length = 0;
    assert_eq!(unsafe { nt_query_information_job_object(alias, 3, output.as_mut_ptr().cast(), 16, &mut length) }, 0);
    assert_eq!(length, 16);
    assert_eq!(&output[..8], &[1,0,0,0,1,0,0,0]);
    assert_eq!(u64::from_le_bytes(output[8..].try_into().unwrap()), 100);
    unsafe { ntdll::file::nt_close(handle); ntdll::file::nt_close(alias); }
    assert_eq!(nt_is_process_in_job(process, core::ptr::null_mut()), 0x124);
    ps::job::remove_process_from_all_jobs(100);
    assert_eq!(nt_is_process_in_job(process, core::ptr::null_mut()), 0x123);
    unsafe { ntdll::file::nt_close(other); ntdll::file::nt_close(second); }
}

#[test]
fn job_named_opens_share_identity_and_output_fault_retires_new_object() {
    let _guard = state_guard();
    let mut name: Vec<u16> = "\\BaseNamedObjects\\native-job".encode_utf16().collect();
    let mut text = UnicodeString { length: (name.len()*2) as u16,
        maximum_length: (name.len()*2) as u16, buffer: name.as_mut_ptr() };
    let mut attributes: ObjectAttributes = unsafe { core::mem::zeroed() };
    attributes.length = core::mem::size_of::<ObjectAttributes>() as u32;
    attributes.object_name = &mut text;
    let mut handle = core::ptr::null_mut();
    let mut alias = core::ptr::null_mut();
    assert_eq!(unsafe { nt_create_job_object(&mut handle, 0x1f003f, &mut attributes) }, 0);
    assert_eq!(unsafe { nt_open_job_object(&mut alias, 4, &mut attributes) }, 0);
    assert_eq!(ntdll::file::lookup_handle(handle).unwrap().target, ntdll::file::lookup_handle(alias).unwrap().target);
    unsafe { ntdll::file::nt_close(handle); ntdll::file::nt_close(alias); }
    let before = ps::job::JOB_TABLE.lock().len();
    mm::user_copy::FAIL_WRITE.with(|value| value.set(&mut handle as *mut HANDLE as u64));
    assert_eq!(unsafe { nt_create_job_object(&mut handle, 4, &mut attributes) }, STATUS_ACCESS_VIOLATION);
    mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
    assert_eq!(ps::job::JOB_TABLE.lock().len(), before);
    assert_eq!(unsafe { nt_open_job_object(&mut alias, 4, &mut attributes) }, STATUS_OBJECT_NAME_NOT_FOUND);
}

#[test]
fn job_process_list_short_buffer_reports_overflow_and_policy_bits_are_not_faked() {
    let _guard = state_guard();
    let handle = create();
    assert_eq!(nt_assign_process_to_job_object(handle, (-1isize) as HANDLE), 0);
    let mut output = [0u8; 8];
    let mut length = 0;
    assert_eq!(unsafe { nt_query_information_job_object(handle, 3, output.as_mut_ptr().cast(), 8, &mut length) }, STATUS_BUFFER_OVERFLOW);
    assert_eq!(length, 8);
    assert_eq!(&output[..4], &1u32.to_le_bytes());
    assert_eq!(&output[4..], &0u32.to_le_bytes());
    let mut limits = [0u8;64];
    limits[16..20].copy_from_slice(&0x2000u32.to_le_bytes()); // KILL_ON_JOB_CLOSE
    assert_eq!(unsafe { nt_set_information_job_object(handle, 2, limits.as_mut_ptr().cast(), 64) }, STATUS_NOT_SUPPORTED);
    ps::job::remove_process_from_all_jobs(100);
    unsafe { ntdll::file::nt_close(handle); }
}

#[test]
fn kernel_job_termination_drops_table_lock_before_process_exit_cleanup() {
    let _guard = state_guard();
    let id = ps::job::create_job().unwrap();
    assert_eq!(ps::job::assign_process_to_job(id, 102), 0);
    assert_eq!(ps::job::terminate_job(id, 123), 0);
    assert_eq!(ps::job::process_job(102), None);
    ps::job::close_native_job(id);
}

#[test]
fn native_job_termination_checks_access_and_preserves_the_job_handle() {
    let _guard = state_guard();
    let handle = create();
    assert_eq!(nt_assign_process_to_job_object(handle, (-1isize) as HANDLE), 0);
    let query_only = ntdll::file::duplicate_sync_handle_with_access(handle, 4, 0, 0).unwrap();
    let count = ps::job::JOB_TABLE.lock().len();
    assert_eq!(nt_terminate_job_object(query_only, -123), STATUS_ACCESS_DENIED);
    assert_eq!(ps::job::JOB_TABLE.lock().len(), count);
    assert_eq!(nt_is_process_in_job((-1isize) as HANDLE, handle), 0x124);
    assert_eq!(nt_terminate_job_object(handle, -123), STATUS_SUCCESS);
    assert_eq!(nt_is_process_in_job((-1isize) as HANDLE, handle), 0x123);
    assert_eq!(nt_terminate_job_object(handle, 0), STATUS_SUCCESS);
    assert_eq!(unsafe { ntdll::file::nt_close(query_only) }, STATUS_SUCCESS);
    assert_eq!(unsafe { ntdll::file::nt_close(handle) }, STATUS_SUCCESS);
    assert_eq!(nt_terminate_job_object(handle, 0), STATUS_INVALID_HANDLE);
}

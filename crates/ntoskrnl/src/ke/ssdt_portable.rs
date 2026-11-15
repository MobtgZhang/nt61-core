//! Portable adapters for existing NT library services.
//!
//! These registrations expose the same implementation on every ISA; registration
//! does not assert that a library implementation is fully Windows compatible.
//! Service numbers are resolved by name against the shared Windows 7 SP1 table.

use super::*;

macro_rules! portable_services {
    ($(($name:literal, $adapter:ident, $module:ident::$function:ident, [$($index:literal),*]));* $(;)?) => {
        $(#[allow(unused_unsafe)]
        unsafe extern "C" fn $adapter(arguments: &SyscallArguments) -> u32 {
            let _ = arguments;
            // SAFETY: dispatch captured the registered argument slots. The target
            // routine retains responsibility for probing its pointer arguments.
            unsafe {
                crate::libs::ntdll::$module::$function($(match read_argument(arguments, $index) {
                    Ok(value) => value as _,
                    Err(status) => return status,
                }),*) as u32
            }
        })*

        pub(super) fn register_portable_services() {
            $(if let Some(number) = nt61_abi::syscalls::native_number($name) {
                let count = <[usize]>::len(&[$($index),*]) as u8;
                let metadata = &nt61_abi::syscall_metadata::NATIVE_SERVICE_METADATA[number as usize];
                // Never expose a helper whose arity differs from the shipped ABI.
                if metadata.arguments == count {
                    // SAFETY: the adapter has the common SSDT signature.
                    unsafe {
                        let _ = register_syscall(number as usize, $adapter, count, $name);
                    }
                }
            })*
        }
    };
}

portable_services! {
    ("NtQuerySystemTime", portable_nt_query_system_time, clock::nt_query_system_time, [0]);
    ("NtQueryPerformanceCounter", portable_nt_query_performance_counter, clock::nt_query_performance_counter, [0, 1]);
    ("NtAccessCheck", portable_nt_access_check, security::nt_access_check, [0, 1, 2, 3, 4, 5, 6, 7]);
    ("NtAdjustPrivilegesToken", portable_nt_adjust_privileges_token, security::nt_adjust_privileges_token, [0, 1, 2, 3, 4, 5]);
    ("NtAllocateVirtualMemory", portable_nt_allocate_virtual_memory, virtual_mem::nt_allocate_virtual_memory, [0, 1, 2, 3, 4, 5]);
    ("NtCancelTimer", portable_nt_cancel_timer, timer::nt_cancel_timer, [0, 1]);
    ("NtClearEvent", portable_nt_clear_event, sync::nt_clear_event, [0]);
    ("NtClose", portable_nt_close, file::nt_close, [0]);
    ("NtContinue", portable_nt_continue, exception::nt_continue, [0, 1]);
    ("NtCreateEvent", portable_nt_create_event, sync::nt_create_event, [0, 1, 2, 3, 4]);
    ("NtCreateFile", portable_nt_create_file, file::nt_create_file, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    ("NtCreateKey", portable_nt_create_key, registry::nt_create_key, [0, 1, 2, 3, 4, 5, 6]);
    ("NtCreateMutant", portable_nt_create_mutant, sync::nt_create_mutant, [0, 1, 2, 3]);
    ("NtCreateProcess", portable_nt_create_process, process::nt_create_process, [0, 1, 2, 3, 4, 5, 6, 7]);
    ("NtCreateProcessEx", portable_nt_create_process_ex, process::nt_create_process_ex, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    ("NtCreateSection", portable_nt_create_section, section::nt_create_section, [0, 1, 2, 3, 4, 5, 6]);
    ("NtCreateSemaphore", portable_nt_create_semaphore, sync::nt_create_semaphore, [0, 1, 2, 3, 4]);
    ("NtCreateThreadEx", portable_nt_create_thread_ex, thread::nt_create_thread_ex, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    ("NtCreateTimer", portable_nt_create_timer, timer::nt_create_timer, [0, 1, 2, 3]);
    ("NtDelayExecution", portable_nt_delay_execution, sync::nt_delay_execution, [0, 1]);
    ("NtDeleteFile", portable_nt_delete_file, file::nt_delete_file, [0]);
    ("NtDeleteKey", portable_nt_delete_key, registry::nt_delete_key, [0]);
    ("NtDeleteValueKey", portable_nt_delete_value_key, registry::nt_delete_value_key, [0, 1]);
    ("NtDeviceIoControlFile", portable_nt_device_io_control_file, nt_extended::nt_device_io_control_file, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    ("NtEnumerateKey", portable_nt_enumerate_key, registry::nt_enumerate_key, [0, 1, 2, 3, 4, 5]);
    ("NtEnumerateValueKey", portable_nt_enumerate_value_key, registry::nt_enumerate_value_key, [0, 1, 2, 3, 4, 5]);
    ("NtFlushBuffersFile", portable_nt_flush_buffers_file, file::nt_flush_buffers_file, [0, 1]);
    ("NtFlushKey", portable_nt_flush_key, registry::nt_flush_key, [0]);
    ("NtFreeVirtualMemory", portable_nt_free_virtual_memory, virtual_mem::nt_free_virtual_memory, [0, 1, 2, 3]);
    ("NtFsControlFile", portable_nt_fs_control_file, nt_extended::nt_fs_control_file, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    ("NtGetContextThread", portable_nt_get_context_thread, thread::nt_get_context_thread, [0, 1]);
    ("NtImpersonateThread", portable_nt_impersonate_thread, security::nt_impersonate_thread, [0, 1, 2]);
    ("NtLockFile", portable_nt_lock_file, nt_extended::nt_lock_file, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    ("NtMapViewOfSection", portable_nt_map_view_of_section, section::nt_map_view_of_section, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    ("NtNotifyChangeDirectoryFile", portable_nt_notify_change_directory_file, nt_extended::nt_notify_change_directory_file, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    ("NtNotifyChangeKey", portable_nt_notify_change_key, registry::nt_notify_change_key, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    ("NtDuplicateObject", portable_nt_duplicate_object, object::nt_duplicate_object, [0, 1, 2, 3, 4, 5, 6]);
    ("NtOpenEvent", portable_nt_open_event, sync::nt_open_event, [0, 1, 2]);
    ("NtOpenMutant", portable_nt_open_mutant, sync::nt_open_mutant, [0, 1, 2]);
    ("NtOpenSemaphore", portable_nt_open_semaphore, sync::nt_open_semaphore, [0, 1, 2]);
    ("NtOpenFile", portable_nt_open_file, file::nt_open_file, [0, 1, 2, 3, 4, 5]);
    ("NtOpenKey", portable_nt_open_key, registry::nt_open_key, [0, 1, 2]);
    ("NtOpenProcess", portable_nt_open_process, process::nt_open_process, [0, 1, 2, 3]);
    ("NtOpenProcessToken", portable_nt_open_process_token, security::nt_open_process_token, [0, 1, 2]);
    ("NtOpenThread", portable_nt_open_thread, thread::nt_open_thread, [0, 1, 2, 3]);
    ("NtOpenThreadToken", portable_nt_open_thread_token, security::nt_open_thread_token, [0, 1, 2, 3]);
    ("NtOpenTimer", portable_nt_open_timer, timer::nt_open_timer, [0, 1, 2]);
    ("NtPrivilegeCheck", portable_nt_privilege_check, security::nt_privilege_check, [0, 1, 2]);
    ("NtProtectVirtualMemory", portable_nt_protect_virtual_memory, virtual_mem::nt_protect_virtual_memory, [0, 1, 2, 3, 4]);
    ("NtPulseEvent", portable_nt_pulse_event, sync::nt_pulse_event, [0, 1]);
    ("NtQueryDirectoryFile", portable_nt_query_directory_file, file::nt_query_directory_file, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    ("NtQueryEvent", portable_nt_query_event, sync::nt_query_event, [0, 1, 2, 3, 4]);
    ("NtQueryMutant", portable_nt_query_mutant, sync::nt_query_mutant, [0, 1, 2, 3, 4]);
    ("NtQueryObject", portable_nt_query_object, object::nt_query_object, [0, 1, 2, 3, 4]);
    ("NtQuerySemaphore", portable_nt_query_semaphore, sync::nt_query_semaphore, [0, 1, 2, 3, 4]);
    ("NtQueryEaFile", portable_nt_query_ea_file, nt_extended::nt_query_ea_file, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    ("NtQueryInformationFile", portable_nt_query_information_file, file::nt_query_information_file, [0, 1, 2, 3, 4]);
    ("NtQueryInformationProcess", portable_nt_query_information_process, process::nt_query_information_process, [0, 1, 2, 3, 4]);
    ("NtQueryInformationThread", portable_nt_query_information_thread, thread::nt_query_information_thread, [0, 1, 2, 3, 4]);
    ("NtQueryInformationToken", portable_nt_query_information_token, security::nt_query_information_token, [0, 1, 2, 3, 4]);
    ("NtQueryKey", portable_nt_query_key, registry::nt_query_key, [0, 1, 2, 3, 4]);
    ("NtQuerySection", portable_nt_query_section, section::nt_query_section, [0, 1, 2, 3, 4]);
    ("NtQuerySecurityObject", portable_nt_query_security_object, security::nt_query_security_object, [0, 1, 2, 3, 4]);
    ("NtQuerySystemInformation", portable_nt_query_system_information, info::nt_query_system_information, [0, 1, 2, 3]);
    ("NtQueryTimer", portable_nt_query_timer, timer::nt_query_timer, [0, 1, 2, 3, 4]);
    ("NtQueryValueKey", portable_nt_query_value_key, registry::nt_query_value_key, [0, 1, 2, 3, 4, 5]);
    ("NtQueryVirtualMemory", portable_nt_query_virtual_memory, virtual_mem::nt_query_virtual_memory, [0, 1, 2, 3, 4, 5]);
    ("NtQueryVolumeInformationFile", portable_nt_query_volume_information_file, file::nt_query_volume_information_file, [0, 1, 2, 3, 4]);
    ("NtQueueApcThread", portable_nt_queue_apc_thread, thread::nt_queue_apc_thread, [0, 1, 2, 3, 4]);
    ("NtRaiseException", portable_nt_raise_exception, exception::nt_raise_exception, [0, 1, 2]);
    ("NtReadFile", portable_nt_read_file, file::nt_read_file, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    ("NtReadVirtualMemory", portable_nt_read_virtual_memory, virtual_mem::nt_read_virtual_memory, [0, 1, 2, 3, 4]);
    ("NtReleaseMutant", portable_nt_release_mutant, sync::nt_release_mutant, [0, 1]);
    ("NtReleaseSemaphore", portable_nt_release_semaphore, sync::nt_release_semaphore, [0, 1, 2]);
    ("NtResetEvent", portable_nt_reset_event, sync::nt_reset_event, [0, 1]);
    ("NtResumeThread", portable_nt_resume_thread, thread::nt_resume_thread, [0, 1]);
    ("NtSetContextThread", portable_nt_set_context_thread, thread::nt_set_context_thread, [0, 1]);
    ("NtSetEaFile", portable_nt_set_ea_file, nt_extended::nt_set_ea_file, [0, 1, 2, 3]);
    ("NtSetEvent", portable_nt_set_event, sync::nt_set_event, [0, 1]);
    ("NtSetInformationFile", portable_nt_set_information_file, file::nt_set_information_file, [0, 1, 2, 3, 4]);
    ("NtSetInformationObject", portable_nt_set_information_object, object::nt_set_information_object, [0, 1, 2, 3]);
    ("NtSetInformationProcess", portable_nt_set_information_process, process::nt_set_information_process, [0, 1, 2, 3]);
    ("NtSetInformationThread", portable_nt_set_information_thread, thread::nt_set_information_thread, [0, 1, 2, 3]);
    ("NtSetInformationToken", portable_nt_set_information_token, security::nt_set_information_token, [0, 1, 2, 3]);
    ("NtSetSecurityObject", portable_nt_set_security_object, security::nt_set_security_object, [0, 1, 2]);
    ("NtSetSystemInformation", portable_nt_set_system_information, info::nt_set_system_information, [0, 1, 2]);
    ("NtSetTimer", portable_nt_set_timer, timer::nt_set_timer, [0, 1, 2, 3, 4, 5, 6]);
    ("NtSetValueKey", portable_nt_set_value_key, registry::nt_set_value_key, [0, 1, 2, 3, 4, 5]);
    ("NtSetVolumeInformationFile", portable_nt_set_volume_information_file, file::nt_set_volume_information_file, [0, 1, 2, 3, 4]);
    ("NtSignalAndWaitForSingleObject", portable_nt_signal_and_wait_for_single_object, sync::nt_signal_and_wait_for_single_object, [0, 1, 2, 3]);
    ("NtSuspendThread", portable_nt_suspend_thread, thread::nt_suspend_thread, [0, 1]);
    ("NtTerminateProcess", portable_nt_terminate_process, process::nt_terminate_process, [0, 1]);
    ("NtTerminateThread", portable_nt_terminate_thread, thread::nt_terminate_thread, [0, 1]);
    ("NtUnlockFile", portable_nt_unlock_file, nt_extended::nt_unlock_file, [0, 1, 2, 3, 4]);
    ("NtUnmapViewOfSection", portable_nt_unmap_view_of_section, section::nt_unmap_view_of_section, [0, 1]);
    ("NtWaitForMultipleObjects", portable_nt_wait_for_multiple_objects, sync::nt_wait_for_multiple_objects, [0, 1, 2, 3, 4]);
    ("NtWaitForSingleObject", portable_nt_wait_for_single_object, sync::nt_wait_for_single_object, [0, 1, 2]);
    ("NtWriteFile", portable_nt_write_file, file::nt_write_file, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    ("NtWriteVirtualMemory", portable_nt_write_virtual_memory, virtual_mem::nt_write_virtual_memory, [0, 1, 2, 3, 4]);
    ("NtYieldExecution", portable_nt_yield_execution, thread::nt_yield_execution, []);
    ("NtTestAlert", portable_nt_test_alert, apc::nt_test_alert, []);
    ("NtAlertThread", portable_nt_alert_thread, apc::nt_alert_thread, [0]);
    ("NtAlertResumeThread", portable_nt_alert_resume_thread, apc::nt_alert_resume_thread, [0, 1]);
}

//! Portable adapters for existing NT library services.
//!
//! These registrations expose the same implementation on every ISA; registration
//! does not assert that a library implementation is fully Windows compatible.
//! Service numbers are resolved by name against the shared Windows 7 SP1 table.

use super::*;

macro_rules! portable_services {
    (@argument_type $index:literal) => { _ };
    ($(($name:literal, $adapter:ident, $module:ident::$function:ident, [$($index:literal),*]));* $(;)?) => {
        $(unsafe extern "C" fn $adapter(arguments: &SyscallArguments) -> u32 {
            let _ = arguments;
            // Safe C functions can also coerce to unsafe C function pointers.
            // One explicitly typed call keeps both kinds of target compatible
            // without suppressing unnecessary-unsafe diagnostics in adapters.
            let service: unsafe extern "C" fn($(portable_services!(@argument_type $index)),*) -> _ =
                crate::libs::ntdll::$module::$function;
            // SAFETY: dispatch captured the registered argument slots. The target
            // routine retains responsibility for probing its pointer arguments.
            unsafe {
                service($(match read_argument(arguments, $index) {
                    Ok(value) => value as _,
                    Err(status) => return status,
                }),*) as u32
            }
        })*

        pub(super) fn register_portable_services() {
            $(let number = nt61_abi::syscalls::native_number($name)
                .expect("portable service must exist in the Windows 7 SP1 table");
                let count = <[usize]>::len(&[$($index),*]) as u8;
                let metadata = &nt61_abi::syscall_metadata::NATIVE_SERVICE_METADATA[number as usize];
                // Never expose a helper whose arity differs from the shipped ABI.
                assert_eq!(metadata.arguments, count, "incorrect ABI arity for {}", $name);
                    // SAFETY: the adapter has the common SSDT signature.
                    unsafe {
                        let _ = register_syscall(number as usize, $adapter, count, $name);
                    }
            )*
        }
    };
}

portable_services! {
    ("NtRenameKey", portable_nt_rename_key, registry::nt_rename_key, [0, 1]);
    ("NtSetInformationKey", portable_nt_set_information_key, registry::nt_set_information_key, [0, 1, 2, 3]);
    ("NtDuplicateToken", portable_nt_duplicate_token, security::nt_duplicate_token, [0, 1, 2, 3, 4, 5]);
    ("NtQueryDefaultLocale", portable_nt_query_default_locale, locale::nt_query_default_locale, [0, 1]);
    ("NtSetDefaultLocale", portable_nt_set_default_locale, locale::nt_set_default_locale, [0, 1]);
    ("NtQueryDefaultUILanguage", portable_nt_query_default_ui_language, locale::nt_query_default_ui_language, [0]);
    ("NtSetDefaultUILanguage", portable_nt_set_default_ui_language, locale::nt_set_default_ui_language, [0]);
    ("NtQueryInstallUILanguage", portable_nt_query_install_ui_language, locale::nt_query_install_ui_language, [0]);
    ("NtQueryIntervalProfile", portable_nt_query_interval_profile, profile_interval::nt_query_interval_profile, [0, 1]);
    ("NtSetIntervalProfile", portable_nt_set_interval_profile, profile_interval::nt_set_interval_profile, [0, 1]);
    ("NtSetInformationResourceManager", portable_nt_set_information_resource_manager, ktm::nt_set_information_resource_manager, [0, 1, 2, 3]);
    ("NtQueryAttributesFile", portable_nt_query_attributes_file, file::nt_query_attributes_file, [0, 1]);
    ("NtQueryFullAttributesFile", portable_nt_query_full_attributes_file, file::nt_query_full_attributes_file, [0, 1]);
    ("NtOpenProcessTokenEx", portable_nt_open_process_token_ex, security::nt_open_process_token_ex, [0, 1, 2, 3]);
    ("NtOpenThreadTokenEx", portable_nt_open_thread_token_ex, security::nt_open_thread_token_ex, [0, 1, 2, 3, 4]);
    ("NtCompareTokens", portable_nt_compare_tokens, security::nt_compare_tokens, [0, 1, 2]);
    ("NtDisplayString", portable_nt_display_string, debug::nt_display_string, [0]);
    ("NtOpenKeyEx", portable_nt_open_key_ex, registry::nt_open_key_ex, [0, 1, 2, 3]);
    ("NtQueryMultipleValueKey", portable_nt_query_multiple_value_key, registry::nt_query_multiple_value_key, [0, 1, 2, 3, 4, 5]);
    ("NtQueryOpenSubKeys", portable_nt_query_open_sub_keys, registry::nt_query_open_sub_keys, [0, 1]);

    ("NtAllocateReserveObject", portable_nt_allocate_reserve_object, io_completion::nt_allocate_reserve_object, [0, 1, 2]);
    ("NtSetIoCompletionEx", portable_nt_set_io_completion_ex, io_completion::nt_set_io_completion_ex, [0, 1, 2, 3, 4, 5]);
    ("NtCreateEnlistment", portable_nt_create_enlistment, ktm::nt_create_enlistment, [0, 1, 2, 3, 4, 5, 6, 7]);
    ("NtOpenEnlistment", portable_nt_open_enlistment, ktm::nt_open_enlistment, [0, 1, 2, 3, 4]);
    ("NtQueryInformationEnlistment", portable_nt_query_information_enlistment, ktm::nt_query_information_enlistment, [0, 1, 2, 3, 4]);
    ("NtSetInformationEnlistment", portable_nt_set_information_enlistment, ktm::nt_set_information_enlistment, [0, 1, 2, 3]);
    ("NtPrePrepareComplete", portable_nt_pre_prepare_complete, ktm::nt_pre_prepare_complete, [0, 1]);
    ("NtPrepareComplete", portable_nt_prepare_complete, ktm::nt_prepare_complete, [0, 1]);
    ("NtCommitComplete", portable_nt_commit_complete, ktm::nt_commit_complete, [0, 1]);
    ("NtRollbackComplete", portable_nt_rollback_complete, ktm::nt_rollback_complete, [0, 1]);
    ("NtReadOnlyEnlistment", portable_nt_read_only_enlistment, ktm::nt_read_only_enlistment, [0, 1]);
    ("NtSinglePhaseReject", portable_nt_single_phase_reject, ktm::nt_single_phase_reject, [0, 1]);
    ("NtPrePrepareEnlistment", portable_nt_pre_prepare_enlistment, ktm::nt_pre_prepare_enlistment, [0, 1]);
    ("NtPrepareEnlistment", portable_nt_prepare_enlistment, ktm::nt_prepare_enlistment, [0, 1]);
    ("NtCommitEnlistment", portable_nt_commit_enlistment, ktm::nt_commit_enlistment, [0, 1]);
    ("NtRollbackEnlistment", portable_nt_rollback_enlistment, ktm::nt_rollback_enlistment, [0, 1]);
    ("NtGetNotificationResourceManager", portable_nt_get_notification_resource_manager, ktm::nt_get_notification_resource_manager, [0, 1, 2, 3, 4, 5, 6]);
    ("NtCreateTransactionManager", portable_nt_create_transaction_manager, ktm::nt_create_transaction_manager, [0, 1, 2, 3, 4, 5]);
    ("NtOpenTransactionManager", portable_nt_open_transaction_manager, ktm::nt_open_transaction_manager, [0, 1, 2, 3, 4, 5]);
    ("NtQueryInformationTransactionManager", portable_nt_query_information_transaction_manager, ktm::nt_query_information_transaction_manager, [0, 1, 2, 3, 4]);
    ("NtCreateTransaction", portable_nt_create_transaction, ktm::nt_create_transaction, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    ("NtOpenTransaction", portable_nt_open_transaction, ktm::nt_open_transaction, [0, 1, 2, 3, 4]);
    ("NtQueryInformationTransaction", portable_nt_query_information_transaction, ktm::nt_query_information_transaction, [0, 1, 2, 3, 4]);
    ("NtSetInformationTransaction", portable_nt_set_information_transaction, ktm::nt_set_information_transaction, [0, 1, 2, 3]);
    ("NtCommitTransaction", portable_nt_commit_transaction, ktm::nt_commit_transaction, [0, 1]);
    ("NtRollbackTransaction", portable_nt_rollback_transaction, ktm::nt_rollback_transaction, [0, 1]);
    ("NtCreateResourceManager", portable_nt_create_resource_manager, ktm::nt_create_resource_manager, [0, 1, 2, 3, 4, 5, 6]);
    ("NtOpenResourceManager", portable_nt_open_resource_manager, ktm::nt_open_resource_manager, [0, 1, 2, 3, 4]);
    ("NtQueryInformationResourceManager", portable_nt_query_information_resource_manager, ktm::nt_query_information_resource_manager, [0, 1, 2, 3, 4]);
    ("NtEnumerateTransactionObject", portable_nt_enumerate_transaction_object, ktm::nt_enumerate_transaction_object, [0, 1, 2, 3, 4]);
    ("NtSetSystemTime", portable_nt_set_system_time, wall_clock::nt_set_system_time, [0, 1]);
    ("NtAllocateUuids", portable_nt_allocate_uuids, uuid::nt_allocate_uuids, [0, 1, 2, 3]);
    ("NtSetUuidSeed", portable_nt_set_uuid_seed, uuid::nt_set_uuid_seed, [0]);
    ("NtFlushProcessWriteBuffers", portable_nt_flush_process_write_buffers, system_services::nt_flush_process_write_buffers, []);
    ("NtTerminateJobObject", portable_nt_terminate_job_object, job::nt_terminate_job_object, [0, 1]);
    ("NtMakePermanentObject", portable_nt_make_permanent_object, namespace::nt_make_permanent_object, [0]);
    ("NtMakeTemporaryObject", portable_nt_make_temporary_object, namespace::nt_make_temporary_object, [0]);
    ("NtCreateDirectoryObject", portable_nt_create_directory_object, namespace::nt_create_directory_object, [0, 1, 2]);
    ("NtOpenDirectoryObject", portable_nt_open_directory_object, namespace::nt_open_directory_object, [0, 1, 2]);
    ("NtQueryDirectoryObject", portable_nt_query_directory_object, namespace::nt_query_directory_object, [0, 1, 2, 3, 4, 5, 6]);
    ("NtCreateSymbolicLinkObject", portable_nt_create_symbolic_link_object, namespace::nt_create_symbolic_link_object, [0, 1, 2, 3]);
    ("NtOpenSymbolicLinkObject", portable_nt_open_symbolic_link_object, namespace::nt_open_symbolic_link_object, [0, 1, 2]);
    ("NtQuerySymbolicLinkObject", portable_nt_query_symbolic_link_object, namespace::nt_query_symbolic_link_object, [0, 1, 2]);
    ("NtRemoveIoCompletionEx", portable_nt_remove_io_completion_ex, io_completion::nt_remove_io_completion_ex, [0, 1, 2, 3, 4, 5]);
    ("NtRemoveIoCompletion", portable_nt_remove_io_completion, io_completion::nt_remove_io_completion, [0, 1, 2, 3, 4]);
    ("NtQueryIoCompletion", portable_nt_query_io_completion, io_completion::nt_query_io_completion, [0, 1, 2, 3, 4]);
    ("NtSetIoCompletion", portable_nt_set_io_completion, io_completion::nt_set_io_completion, [0, 1, 2, 3, 4]);
    ("NtOpenIoCompletion", portable_nt_open_io_completion, io_completion::nt_open_io_completion, [0, 1, 2]);
    ("NtCreateIoCompletion", portable_nt_create_io_completion, io_completion::nt_create_io_completion, [0, 1, 2, 3]);
    ("NtCreateJobObject", portable_nt_create_job_object, job::nt_create_job_object, [0, 1, 2]);
    ("NtOpenJobObject", portable_nt_open_job_object, job::nt_open_job_object, [0, 1, 2]);
    ("NtAssignProcessToJobObject", portable_nt_assign_process_to_job_object, job::nt_assign_process_to_job_object, [0, 1]);
    ("NtIsProcessInJob", portable_nt_is_process_in_job, job::nt_is_process_in_job, [0, 1]);
    ("NtQueryInformationJobObject", portable_nt_query_information_job_object, job::nt_query_information_job_object, [0, 1, 2, 3, 4]);
    ("NtSetInformationJobObject", portable_nt_set_information_job_object, job::nt_set_information_job_object, [0, 1, 2, 3]);
    ("NtQueryTimerResolution", portable_nt_query_timer_resolution, timer_resolution::nt_query_timer_resolution, [0, 1, 2]);
    ("NtSetTimerResolution", portable_nt_set_timer_resolution, timer_resolution::nt_set_timer_resolution, [0, 1, 2]);
    ("NtQueryDebugFilterState", portable_nt_query_debug_filter_state, debug_filter::nt_query_debug_filter_state, [0, 1]);
    ("NtSetDebugFilterState", portable_nt_set_debug_filter_state, debug_filter::nt_set_debug_filter_state, [0, 1, 2]);
    ("NtCreateKeyedEvent", portable_nt_create_keyed_event, keyed_event::nt_create_keyed_event, [0, 1, 2, 3]);
    ("NtOpenKeyedEvent", portable_nt_open_keyed_event, keyed_event::nt_open_keyed_event, [0, 1, 2]);
    ("NtWaitForKeyedEvent", portable_nt_wait_for_keyed_event, keyed_event::nt_wait_for_keyed_event, [0, 1, 2, 3]);
    ("NtReleaseKeyedEvent", portable_nt_release_keyed_event, keyed_event::nt_release_keyed_event, [0, 1, 2, 3]);
    ("NtCreateEventPair", portable_nt_create_event_pair, event_pair::nt_create_event_pair, [0, 1, 2]);
    ("NtOpenEventPair", portable_nt_open_event_pair, event_pair::nt_open_event_pair, [0, 1, 2]);
    ("NtSetLowEventPair", portable_nt_set_low_event_pair, event_pair::nt_set_low_event_pair, [0]);
    ("NtSetHighEventPair", portable_nt_set_high_event_pair, event_pair::nt_set_high_event_pair, [0]);
    ("NtWaitLowEventPair", portable_nt_wait_low_event_pair, event_pair::nt_wait_low_event_pair, [0]);
    ("NtWaitHighEventPair", portable_nt_wait_high_event_pair, event_pair::nt_wait_high_event_pair, [0]);
    ("NtSetLowWaitHighEventPair", portable_nt_set_low_wait_high_event_pair, event_pair::nt_set_low_wait_high_event_pair, [0]);
    ("NtSetHighWaitLowEventPair", portable_nt_set_high_wait_low_event_pair, event_pair::nt_set_high_wait_low_event_pair, [0]);
    ("NtWaitForMultipleObjects32", portable_nt_wait_for_multiple_objects32, sync::nt_wait_for_multiple_objects32, [0, 1, 2, 3, 4]);
    ("NtGetCurrentProcessorNumber", portable_nt_get_current_processor_number, system_services::nt_get_current_processor_number, []);
    ("NtFlushWriteBuffer", portable_nt_flush_write_buffer, system_services::nt_flush_write_buffer, []);
    ("NtAddAtom", portable_nt_add_atom, atom::nt_add_atom, [0, 1, 2]);
    ("NtFindAtom", portable_nt_find_atom, atom::nt_find_atom, [0, 1, 2]);
    ("NtDeleteAtom", portable_nt_delete_atom, atom::nt_delete_atom, [0]);
    ("NtQueryInformationAtom", portable_nt_query_information_atom, atom::nt_query_information_atom, [0, 1, 2, 3, 4]);
    ("NtAllocateLocallyUniqueId", portable_nt_allocate_luid, luid::nt_allocate_locally_unique_id, [0]);
    ("NtQuerySystemTime", portable_nt_query_system_time, clock::nt_query_system_time, [0]);
    ("NtQueryPerformanceCounter", portable_nt_query_performance_counter, clock::nt_query_performance_counter, [0, 1]);
    ("NtAccessCheck", portable_nt_access_check, security::nt_access_check, [0, 1, 2, 3, 4, 5, 6, 7]);
    ("NtAdjustPrivilegesToken", portable_nt_adjust_privileges_token, security::nt_adjust_privileges_token, [0, 1, 2, 3, 4, 5]);
    ("NtAdjustGroupsToken", portable_nt_adjust_groups_token, security::nt_adjust_groups_token, [0, 1, 2, 3, 4, 5]);
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
    ("NtOpenSection", portable_nt_open_section, section::nt_open_section, [0, 1, 2]);
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
    ("NtSetTimerEx", portable_nt_set_timer_ex, timer::nt_set_timer_ex, [0, 1, 2, 3]);
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

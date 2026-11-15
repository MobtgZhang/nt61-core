use super::*;
use ntdll::{file, object, timer};

fn named<T>(text: &str, attributes: u32, operation: impl FnOnce(&mut ObjectAttributes) -> T) -> T {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    let mut name = UnicodeString {
        length: (units.len() * 2) as u16,
        maximum_length: (units.len() * 2) as u16,
        buffer: units.as_mut_ptr(),
    };
    let mut oa = ObjectAttributes::with_name(&mut name);
    oa.attributes = attributes;
    operation(&mut oa)
}

#[test]
fn native_names_open_existing_state_and_reject_type_collisions() {
    let _guard = state_guard();
    let mut original = core::ptr::null_mut();
    let mut alias = core::ptr::null_mut();
    named("\\BaseNamedObjects\\CountedEvent", 2, |oa| unsafe {
        assert_eq!(
            nt_create_event(&mut original, 0x1f0003, oa, 1, 1),
            STATUS_SUCCESS
        );
        assert_eq!(object::query_handle_flags(original), Ok(1));
        assert_eq!(
            nt_create_event(&mut alias, 0x1f0003, oa, 0, 0),
            STATUS_OBJECT_NAME_COLLISION
        );
        assert!(alias.is_null());
        assert_eq!(
            nt_open_semaphore(&mut alias, 1, oa),
            STATUS_OBJECT_TYPE_MISMATCH
        );
        oa.attributes = 0x80;
        assert_eq!(
            nt_create_event(&mut alias, 0x1f0003, oa, 0, 0),
            STATUS_OBJECT_NAME_EXISTS
        );
        assert_ne!(original, alias);
        assert_eq!(object::query_handle_flags(alias), Ok(0));
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(original, 0, Some(0)),
            STATUS_TIMEOUT
        );
        // Reopening preserves the original auto-reset type and current state.
        assert_eq!(
            nt_set_event(original, core::ptr::null_mut()),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_TIMEOUT
        );
        assert_eq!(nt_close_sync(original), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(
            nt_open_event(&mut alias, 1, oa),
            STATUS_OBJECT_NAME_NOT_FOUND
        );
    });
}

#[test]
fn all_dispatcher_object_kinds_reopen_shared_state() {
    let _guard = state_guard();
    for kind in 0..3 {
        named(
            &format!("\\BaseNamedObjects\\Shared{kind}"),
            0,
            |oa| unsafe {
                let mut handle = core::ptr::null_mut();
                let mut alias = core::ptr::null_mut();
                match kind {
                    0 => {
                        assert_eq!(
                            nt_create_semaphore(&mut handle, 0x1f0003, oa, 1, 3),
                            STATUS_SUCCESS
                        );
                        assert_eq!(nt_open_semaphore(&mut alias, 0x100000, oa), STATUS_SUCCESS);
                    }
                    1 => {
                        assert_eq!(
                            nt_create_mutant(&mut handle, 0x1f0001, oa, 0),
                            STATUS_SUCCESS
                        );
                        assert_eq!(nt_open_mutant(&mut alias, 0x100000, oa), STATUS_SUCCESS);
                    }
                    _ => {
                        assert_eq!(
                            timer::nt_create_timer(&mut handle, 0x1f0003, oa, 1),
                            STATUS_SUCCESS
                        );
                        assert_eq!(
                            timer::nt_open_timer(&mut alias, 0x100000, oa),
                            STATUS_SUCCESS
                        );
                        assert_eq!(
                            timer::set_timer_kernel(
                                handle,
                                0,
                                core::ptr::null_mut(),
                                core::ptr::null_mut(),
                                0,
                                0
                            ),
                            STATUS_SUCCESS
                        );
                    }
                }
                assert_eq!(
                    nt_wait_for_single_object_kernel(alias, 0, Some(0)),
                    STATUS_SUCCESS
                );
                if kind == 1 {
                    assert_eq!(
                        nt_release_mutant(handle, core::ptr::null_mut()),
                        STATUS_SUCCESS
                    );
                } else {
                    assert_eq!(
                        nt_wait_for_single_object_kernel(handle, 0, Some(0)),
                        STATUS_TIMEOUT
                    );
                }
                assert_eq!(nt_close_sync(handle), STATUS_SUCCESS);
                assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
            },
        );
    }
}

#[test]
fn native_access_masks_gate_queries_mutations_waits_and_combined_operations() {
    let _guard = state_guard();
    named("\\BaseNamedObjects\\AccessEvent", 0, |oa| unsafe {
        let mut full = core::ptr::null_mut();
        assert_eq!(
            nt_create_event(&mut full, 0x10000000, oa, 1, 1),
            STATUS_SUCCESS
        ); // GENERIC_ALL
        let mut query = core::ptr::null_mut();
        assert_eq!(nt_open_event(&mut query, 0x80000000, oa), STATUS_SUCCESS); // GENERIC_READ
        let mut information = [0u32; 2];
        assert_eq!(
            nt_query_event(
                query,
                0,
                information.as_mut_ptr().cast(),
                8,
                core::ptr::null_mut()
            ),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_set_event(query, core::ptr::null_mut()),
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            nt_pulse_event(query, core::ptr::null_mut()),
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(query, 0, Some(0)),
            STATUS_ACCESS_DENIED
        );
        let mut wait = core::ptr::null_mut();
        assert_eq!(nt_open_event(&mut wait, 0x20000000, oa), STATUS_SUCCESS); // GENERIC_EXECUTE
        assert_eq!(
            nt_signal_and_wait_for_single_object(query, wait, 0, core::ptr::null_mut()),
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(wait, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(full, 0, Some(0)),
            STATUS_TIMEOUT
        );
        let mut none = core::ptr::null_mut();
        assert_eq!(nt_open_event(&mut none, 0, oa), STATUS_SUCCESS);
        assert_eq!(
            nt_query_event(
                none,
                0,
                information.as_mut_ptr().cast(),
                8,
                core::ptr::null_mut()
            ),
            STATUS_ACCESS_DENIED
        );
        for handle in [full, query, wait, none] {
            assert_eq!(nt_close_sync(handle), STATUS_SUCCESS);
        }
    });
    for kind in 0..2 {
        named(
            &format!("\\BaseNamedObjects\\Access{kind}"),
            0,
            |oa| unsafe {
                let mut handle = core::ptr::null_mut();
                if kind == 0 {
                    assert_eq!(
                        nt_create_semaphore(&mut handle, 1, oa, 0, 1),
                        STATUS_SUCCESS
                    );
                    assert_eq!(
                        nt_release_semaphore(handle, 1, core::ptr::null_mut()),
                        STATUS_ACCESS_DENIED
                    );
                } else {
                    assert_eq!(
                        timer::nt_create_timer(&mut handle, 1, oa, 1),
                        STATUS_SUCCESS
                    );
                    assert_eq!(
                        timer::set_timer_kernel(
                            handle,
                            0,
                            core::ptr::null_mut(),
                            core::ptr::null_mut(),
                            0,
                            0
                        ),
                        STATUS_ACCESS_DENIED
                    );
                    assert_eq!(
                        timer::nt_cancel_timer(handle, core::ptr::null_mut()),
                        STATUS_ACCESS_DENIED
                    );
                }
                assert_eq!(nt_close_sync(handle), STATUS_SUCCESS);
            },
        );
    }
}

#[test]
fn native_attributes_capture_and_rejection_preserve_handle_table() {
    let _guard = state_guard();
    let before = file::handle_count();
    let mut output = 0x123456usize as HANDLE;
    assert_eq!(
        unsafe { nt_create_event(&mut output, 3, 1usize as _, 1, 0) },
        STATUS_ACCESS_VIOLATION
    );
    for text in ["relative", "\\Unknown\\x", "\\BaseNamedObjects\\nested\\x"] {
        named(text, 0, |oa| {
            assert!(unsafe { nt_create_event(&mut output, 3, oa, 1, 0) } < 0);
        });
    }
    named("\\BaseNamedObjects\\BadAttributes", 0, |oa| unsafe {
        oa.length -= 1;
        assert_eq!(
            nt_create_event(&mut output, 3, oa, 1, 0),
            STATUS_INVALID_PARAMETER
        );
        oa.length += 1;
        (*oa.object_name).length |= 1;
        assert_eq!(
            nt_create_event(&mut output, 3, oa, 1, 0),
            STATUS_OBJECT_NAME_INVALID
        );
        (*oa.object_name).length &= !1;
        oa.security_descriptor = 0x10000usize as _;
        assert_eq!(
            nt_create_event(&mut output, 3, oa, 1, 0),
            STATUS_NOT_SUPPORTED
        );
        oa.security_descriptor = core::ptr::null_mut();
        oa.attributes = 0x200;
        assert_eq!(
            nt_create_event(&mut output, 3, oa, 1, 0),
            STATUS_ACCESS_DENIED
        );
        oa.attributes = 0;
        assert_eq!(
            nt_create_event(&mut output, 0x01000000, oa, 1, 0),
            STATUS_PRIVILEGE_NOT_HELD
        );
        assert_eq!(
            nt_create_event(&mut output, 4, oa, 1, 0),
            STATUS_ACCESS_DENIED
        );
    });
    assert_eq!(output as usize, 0x123456);
    assert_eq!(file::handle_count(), before);
}

#[test]
fn case_sensitive_and_ascii_insensitive_native_lookup() {
    let _guard = state_guard();
    let mut event = core::ptr::null_mut();
    named("\\BaseNamedObjects\\Case", 0, |oa| unsafe {
        assert_eq!(
            nt_create_event(&mut event, 0x1f0003, oa, 0, 0),
            STATUS_SUCCESS
        );
    });
    named("\\basenamedobjects\\case", 0x40, |oa| unsafe {
        let mut alias = core::ptr::null_mut();
        assert_eq!(nt_open_event(&mut alias, 1, oa), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
    });
    named("\\BaseNamedObjects\\case", 0, |oa| unsafe {
        let mut alias = core::ptr::null_mut();
        assert_eq!(
            nt_open_event(&mut alias, 1, oa),
            STATUS_OBJECT_NAME_NOT_FOUND
        );
    });
    assert_eq!(unsafe { nt_close_sync(event) }, STATUS_SUCCESS);
}

#[test]
fn final_handle_close_unlinks_name_even_while_a_wait_pins_old_object() {
    let _guard = state_guard();
    named("\\BaseNamedObjects\\RetireWhileWaiting", 0, |oa| unsafe {
        let mut event = core::ptr::null_mut();
        assert_eq!(
            nt_create_event(&mut event, 0x1f0003, oa, 0, 0),
            STATUS_SUCCESS
        );
        let queued = pending(&[event], false, 702, None);
        assert_eq!(nt_close_sync(event), STATUS_SUCCESS);
        let mut replacement = core::ptr::null_mut();
        assert_eq!(
            nt_create_event(&mut replacement, 0x1f0003, oa, 0, 1),
            STATUS_SUCCESS
        );
        assert_eq!(queued.poll(), None);
        assert_eq!(nt_close_sync(replacement), STATUS_SUCCESS);
        drop(queued);
    });
}

#[test]
fn concurrent_openif_creates_one_shared_dispatcher_object() {
    let _guard = state_guard();
    let before = file::handle_count();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                named("\\BaseNamedObjects\\ConcurrentName", 0x80, |oa| unsafe {
                    let mut event = core::ptr::null_mut();
                    let status = nt_create_event(&mut event, 0x1f0003, oa, 1, 1);
                    (status, event as usize)
                })
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|(status, _)| *status == STATUS_SUCCESS)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|(status, _)| *status == STATUS_OBJECT_NAME_EXISTS)
            .count(),
        7
    );
    assert_eq!(
        results
            .iter()
            .filter(
                |(_, handle)| nt_wait_for_single_object_kernel(*handle as HANDLE, 0, Some(0))
                    == STATUS_SUCCESS
            )
            .count(),
        1
    );
    for (_, handle) in results {
        assert_eq!(unsafe { nt_close_sync(handle as HANDLE) }, STATUS_SUCCESS);
    }
    assert_eq!(file::handle_count(), before);
}

#[test]
fn duplicate_native_and_kernel32_share_state_but_preserve_per_handle_rights_and_flags() {
    let _guard = state_guard();
    let current = -1isize as HANDLE;
    let original = create_event_kernel(1, true).unwrap();
    let mut alias = core::ptr::null_mut();
    unsafe {
        assert_eq!(
            object::nt_duplicate_object(current, original, current, &mut alias, 0x100000, 2, 0),
            STATUS_SUCCESS
        );
        assert_eq!(object::query_handle_flags(alias), Ok(1));
        assert_eq!(
            nt_set_event(alias, core::ptr::null_mut()),
            STATUS_ACCESS_DENIED
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(original, 0, Some(0)),
            STATUS_TIMEOUT
        );
        let mut denied = core::ptr::null_mut();
        assert_eq!(
            object::nt_duplicate_object(current, alias, current, &mut denied, 3, 0, 0),
            STATUS_ACCESS_DENIED
        );
        assert!(denied.is_null());
        kernel32::error::set_last_error(345);
        assert_eq!(
            kernel32::handle::duplicate_handle(current, original, current, &mut denied, 0, 256, 2),
            1
        );
        assert_eq!(kernel32::error::get_last_error(), 345);
        assert_eq!(object::query_handle_flags(denied), Ok(1));
        assert_eq!(nt_close_sync(original), STATUS_SUCCESS);
        assert_eq!(nt_set_event(denied, core::ptr::null_mut()), STATUS_SUCCESS);
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(denied), STATUS_SUCCESS);
    }
}

#[test]
fn duplicate_close_source_and_invalid_output_do_not_leak_handles() {
    let _guard = state_guard();
    let current = -1isize as HANDLE;
    let before = file::handle_count();
    let original = create_event_kernel(0, true).unwrap();
    unsafe {
        assert_eq!(
            object::nt_duplicate_object(current, original, current, 1usize as _, 0, 0, 2),
            STATUS_ACCESS_VIOLATION
        );
        assert_eq!(file::handle_count(), before + 1);
        let mut alias = core::ptr::null_mut();
        assert_eq!(
            object::nt_duplicate_object(current, original, current, &mut alias, 0, 0, 3),
            STATUS_SUCCESS
        );
        assert!(file::lookup_handle(original).is_none());
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
    }
    assert_eq!(file::handle_count(), before);
}

#[test]
fn win32_named_creation_reports_existing_and_separates_global_and_local_names() {
    let _guard = state_guard();
    use kernel32::{error, sync as win32};
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    let default = wide("Win32Shared");
    let local = wide("Local\\Win32Shared");
    let global = wide("Global\\Win32Shared");
    unsafe {
        error::set_last_error(123);
        let event = win32::create_event_w(core::ptr::null(), 0, 1, default.as_ptr());
        assert!(!event.is_null());
        assert_eq!(error::get_last_error(), 0);
        let alias = win32::create_event_w(core::ptr::null(), 1, 0, local.as_ptr());
        assert!(!alias.is_null());
        assert_eq!(error::get_last_error(), 183);
        let separate = win32::create_event_w(core::ptr::null(), 0, 0, global.as_ptr());
        assert_eq!(error::get_last_error(), 0);
        assert_eq!(win32::wait_for_single_object(alias, 0), 0);
        assert_eq!(win32::wait_for_single_object(event, 0), 0x102);
        assert_eq!(win32::wait_for_single_object(separate, 0), 0x102);
        let wrong = win32::create_mutex_w(core::ptr::null(), 0, local.as_ptr());
        assert!(wrong.is_null());
        assert_eq!(error::get_last_error(), 6);
        for handle in [event, alias, separate] {
            assert_eq!(nt_close_sync(handle), STATUS_SUCCESS);
        }
    }
}

#[test]
fn duplicate_close_source_also_closes_on_access_failure() {
    let _guard = state_guard();
    let current = -1isize as HANDLE;
    named("\\BaseNamedObjects\\CloseOnFailure", 0, |oa| unsafe {
        let mut original = core::ptr::null_mut();
        assert_eq!(nt_create_event(&mut original, 1, oa, 1, 0), STATUS_SUCCESS);
        let mut alias = core::ptr::null_mut();
        assert_eq!(
            object::nt_duplicate_object(current, original, current, &mut alias, 2, 0, 1),
            STATUS_ACCESS_DENIED
        );
        assert!(alias.is_null());
        assert_eq!(nt_close_sync(original), STATUS_INVALID_HANDLE);
        assert_eq!(
            nt_open_event(&mut alias, 1, oa),
            STATUS_OBJECT_NAME_NOT_FOUND
        );
    });
}

#[test]
fn win32_named_semaphores_mutants_timers_and_inheritance_use_shared_native_state() {
    let _guard = state_guard();
    #[repr(C)]
    struct Security {
        length: u32,
        descriptor: PVOID,
        inherit: i32,
    }
    let security = Security {
        length: core::mem::size_of::<Security>() as u32,
        descriptor: core::ptr::null_mut(),
        inherit: 256,
    };
    let security = (&security as *const Security).cast();
    unsafe {
        let sem_name: Vec<u16> = "Win32Semaphore".encode_utf16().chain(Some(0)).collect();
        let sem = kernel32::sync::create_semaphore_w(security, 1, 2, sem_name.as_ptr());
        assert_eq!(object::query_handle_flags(sem), Ok(1));
        let alias = kernel32::sync::create_semaphore_w(core::ptr::null(), 2, 3, sem_name.as_ptr());
        assert_eq!(kernel32::error::get_last_error(), 183);
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(sem, 0, Some(0)),
            STATUS_TIMEOUT
        );
        for h in [sem, alias] {
            assert_eq!(nt_close_sync(h), STATUS_SUCCESS);
        }
        let name: Vec<u16> = "Win32Mutant".encode_utf16().chain(Some(0)).collect();
        let mutant = kernel32::sync::create_mutex_w(security, 0, name.as_ptr());
        let alias = kernel32::sync::create_mutex_w(core::ptr::null(), 1, name.as_ptr());
        assert_eq!(kernel32::error::get_last_error(), 183);
        assert_eq!(
            nt_release_mutant(alias, core::ptr::null_mut()),
            STATUS_MUTEX_NOT_OWNED
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(mutant, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_release_mutant(alias, core::ptr::null_mut()),
            STATUS_SUCCESS
        );
        for h in [mutant, alias] {
            assert_eq!(nt_close_sync(h), STATUS_SUCCESS);
        }
        let name: Vec<u16> = "Win32Timer".encode_utf16().chain(Some(0)).collect();
        let handle = kernel32::sync_ext::create_waitable_timer_w(security, 0, name.as_ptr());
        let alias =
            kernel32::sync_ext::create_waitable_timer_w(core::ptr::null(), 1, name.as_ptr());
        assert_eq!(kernel32::error::get_last_error(), 183);
        assert_eq!(
            timer::set_timer_kernel(
                handle,
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0
            ),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(alias, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(
            nt_wait_for_single_object_kernel(handle, 0, Some(0)),
            STATUS_TIMEOUT
        );
        for h in [handle, alias] {
            assert_eq!(nt_close_sync(h), STATUS_SUCCESS);
        }
    }
}

#[test]
fn output_write_fault_rolls_back_new_name_and_existing_alias_without_leaking() {
    let _guard = state_guard();
    let before = file::handle_count();
    named("\\BaseNamedObjects\\FaultRollback", 0, |oa| unsafe {
        let mut handle = core::ptr::null_mut();
        mm::user_copy::FAIL_WRITE.with(|value| value.set((&mut handle as *mut HANDLE) as u64));
        assert_eq!(
            nt_create_event(&mut handle, 0x1f0003, oa, 1, 1),
            STATUS_ACCESS_VIOLATION
        );
        assert!(handle.is_null());
        assert_eq!(file::handle_count(), before);
        mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
        assert_eq!(
            nt_open_event(&mut handle, 1, oa),
            STATUS_OBJECT_NAME_NOT_FOUND
        );
        assert_eq!(
            nt_create_event(&mut handle, 0x1f0003, oa, 1, 1),
            STATUS_SUCCESS
        );
        let mut alias = core::ptr::null_mut();
        mm::user_copy::FAIL_WRITE.with(|value| value.set((&mut alias as *mut HANDLE) as u64));
        assert_eq!(nt_open_event(&mut alias, 1, oa), STATUS_ACCESS_VIOLATION);
        assert_eq!(file::handle_count(), before + 1);
        mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
        assert_eq!(
            nt_wait_for_single_object_kernel(handle, 0, Some(0)),
            STATUS_SUCCESS
        );
        assert_eq!(nt_close_sync(handle), STATUS_SUCCESS);
    });
    assert_eq!(file::handle_count(), before);
}

#[test]
fn duplicate_output_write_fault_releases_alias_and_leaves_original_usable() {
    let _guard = state_guard();
    let before = file::handle_count();
    let original = create_event_kernel(1, true).unwrap();
    let mut alias = core::ptr::null_mut();
    mm::user_copy::FAIL_WRITE.with(|value| value.set((&mut alias as *mut HANDLE) as u64));
    let current = -1isize as HANDLE;
    assert_eq!(
        unsafe { object::nt_duplicate_object(current, original, current, &mut alias, 0, 0, 2) },
        STATUS_ACCESS_VIOLATION
    );
    assert_eq!(file::handle_count(), before + 1);
    mm::user_copy::FAIL_WRITE.with(|value| value.set(0));
    assert_eq!(
        nt_wait_for_single_object_kernel(original, 0, Some(0)),
        STATUS_SUCCESS
    );
    assert_eq!(unsafe { nt_close_sync(original) }, STATUS_SUCCESS);
    assert_eq!(file::handle_count(), before);
}

#[test]
fn win32_open_preserves_access_inheritance_state_and_success_last_error() {
    let _guard = state_guard();
    use kernel32::{error, sync as win32, sync_ext};
    let name: Vec<u16> = "Local\\OpenContracts"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let event = win32::create_event_w(core::ptr::null(), 0, 1, name.as_ptr());
        assert!(!event.is_null());
        error::set_last_error(987);
        let alias = win32::open_event_w(0x100000, 256, name.as_ptr());
        assert!(!alias.is_null());
        assert_eq!(error::get_last_error(), 987);
        assert_eq!(object::query_handle_flags(alias), Ok(1));
        assert_eq!(win32::set_event(alias), 0);
        assert_eq!(error::get_last_error(), 5);
        assert_eq!(win32::wait_for_single_object(alias, 0), 0);
        assert_eq!(win32::wait_for_single_object(event, 0), 0x102);
        assert!(win32::open_mutex_w(1, 0, name.as_ptr()).is_null());
        assert_eq!(error::get_last_error(), 6);
        assert!(win32::open_semaphore_w(1, 0, name.as_ptr()).is_null());
        assert!(sync_ext::open_waitable_timer_w(1, 0, name.as_ptr()).is_null());
        assert_eq!(nt_close_sync(event), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert!(win32::open_event_w(1, 0, name.as_ptr()).is_null());
        assert_eq!(error::get_last_error(), 2);

        let semaphore = win32::create_semaphore_w(core::ptr::null(), 1, 2, name.as_ptr());
        let alias = win32::open_semaphore_w(0x100000, 0, name.as_ptr());
        assert!(!alias.is_null());
        assert_eq!(win32::wait_for_single_object(alias, 0), 0);
        assert_eq!(win32::release_semaphore(alias, 1, core::ptr::null_mut()), 0);
        assert_eq!(error::get_last_error(), 5);
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(semaphore), STATUS_SUCCESS);

        let mutex = win32::create_mutex_w(core::ptr::null(), 0, name.as_ptr());
        let alias = win32::open_mutex_w(0x100001, 0, name.as_ptr());
        assert!(!alias.is_null());
        assert_eq!(win32::wait_for_single_object(alias, 0), 0);
        assert_eq!(win32::release_mutex(alias), 1);
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(mutex), STATUS_SUCCESS);

        let timer = sync_ext::create_waitable_timer_w(core::ptr::null(), 0, name.as_ptr());
        let alias = sync_ext::open_waitable_timer_w(0x100000, 0, name.as_ptr());
        assert!(!alias.is_null());
        let due = 0i64;
        assert_eq!(
            sync_ext::set_waitable_timer(timer, &due, 0, core::ptr::null(), core::ptr::null(), 0),
            1
        );
        assert_eq!(win32::wait_for_single_object(alias, 0), 0);
        assert_eq!(sync_ext::cancel_waitable_timer(alias), 0);
        assert_eq!(error::get_last_error(), 5);
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(timer), STATUS_SUCCESS);
    }
}

#[test]
fn native_duplicate_uses_protect_close_bit_and_same_attributes() {
    let _guard = state_guard();
    let current = -1isize as HANDLE;
    let source = create_event_kernel(0, false).unwrap();
    let mut alias = core::ptr::null_mut();
    unsafe {
        assert_eq!(
            object::nt_duplicate_object(current, source, current, &mut alias, 0, 3, 2),
            STATUS_SUCCESS
        );
        assert_eq!(object::query_handle_flags(alias), Ok(3));
        assert_eq!(nt_close_sync(alias), STATUS_HANDLE_NOT_CLOSABLE);
        let mut same = core::ptr::null_mut();
        assert_eq!(
            object::nt_duplicate_object(current, alias, current, &mut same, 0, 0, 6),
            STATUS_SUCCESS
        );
        assert_eq!(object::query_handle_flags(same), Ok(3));
        object::set_handle_flags(alias, 2, 0).unwrap();
        object::set_handle_flags(same, 2, 0).unwrap();
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(same), STATUS_SUCCESS);
        assert_eq!(
            object::nt_duplicate_object(current, source, current, &mut alias, 0, 0x400, 2),
            STATUS_INVALID_PARAMETER
        );
        kernel32::error::set_last_error(0);
        assert_eq!(
            kernel32::handle::duplicate_handle(current, source, current, &mut alias, 0, 0, 4),
            0
        );
        assert_eq!(kernel32::error::get_last_error(), 87);
        assert_eq!(nt_close_sync(source), STATUS_SUCCESS);
    }
}

#[test]
fn close_only_duplicate_unlinks_name_and_honors_protection_without_allocating() {
    let _guard = state_guard();
    let current = -1isize as HANDLE;
    let before = file::handle_count();
    named("\\BaseNamedObjects\\CloseOnly", 0, |oa| unsafe {
        let mut source = core::ptr::null_mut();
        assert_eq!(
            nt_create_event(&mut source, 0x1f0003, oa, 0, 1),
            STATUS_SUCCESS
        );
        assert_eq!(
            object::nt_duplicate_object(
                current,
                source,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0,
                0
            ),
            STATUS_INVALID_PARAMETER
        );
        object::set_handle_flags(source, 2, 2).unwrap();
        assert_eq!(
            object::nt_duplicate_object(
                current,
                source,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0,
                1
            ),
            STATUS_HANDLE_NOT_CLOSABLE
        );
        assert_eq!(file::handle_count(), before + 1);
        object::set_handle_flags(source, 2, 0).unwrap();
        assert_eq!(
            object::nt_duplicate_object(
                current,
                source,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0,
                1
            ),
            STATUS_SUCCESS
        );
        assert_eq!(file::handle_count(), before);
        assert_eq!(
            nt_open_event(&mut source, 1, oa),
            STATUS_OBJECT_NAME_NOT_FOUND
        );
    });
}

#[test]
fn object_name_query_returns_counted_utf16_in_user_buffer_and_checks_lengths() {
    let _guard = state_guard();
    let before = file::handle_count();
    let text = "\\BaseNamedObjects\\QueryName";
    named(text, 0, |oa| unsafe {
        let mut source = core::ptr::null_mut();
        assert_eq!(
            nt_create_event(&mut source, 0x1f0003, oa, 0, 0),
            STATUS_SUCCESS
        );
        let mut required = 0;
        assert_eq!(
            object::nt_query_object(source, 1, core::ptr::null_mut(), 0, &mut required),
            STATUS_INFO_LENGTH_MISMATCH
        );
        assert_eq!(required as usize, 16 + text.encode_utf16().count() * 2 + 2);
        let mut buffer = vec![0xa5u8; required as usize + 8];
        assert_eq!(
            object::nt_query_object(
                source,
                1,
                buffer.as_mut_ptr().cast(),
                required - 1,
                &mut required
            ),
            STATUS_INFO_LENGTH_MISMATCH
        );
        assert!(buffer.iter().all(|value| *value == 0xa5));
        assert_eq!(
            object::nt_query_object(
                source,
                1,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required
            ),
            STATUS_SUCCESS
        );
        let string = buffer.as_ptr().cast::<UnicodeString>().read_unaligned();
        assert_eq!(string.length as usize, text.encode_utf16().count() * 2);
        assert_eq!(string.maximum_length, string.length + 2);
        assert_eq!(string.buffer as usize, buffer.as_ptr() as usize + 16);
        let units = (0..string.length as usize / 2)
            .map(|index| string.buffer.add(index).read_unaligned())
            .collect::<Vec<_>>();
        assert_eq!(String::from_utf16(&units).unwrap(), text);
        assert_eq!(string.buffer.add(units.len()).read_unaligned(), 0);
        assert!(buffer[required as usize..]
            .iter()
            .all(|value| *value == 0xa5));
        assert!(buffer[4..8].iter().all(|value| *value == 0));
        assert_eq!(
            object::nt_query_object(source, 1, 1usize as _, required, &mut required),
            STATUS_ACCESS_VIOLATION
        );
        let alias = file::duplicate_sync_handle(source).unwrap();
        assert_eq!(nt_close_sync(source), STATUS_SUCCESS);
        assert_eq!(
            object::nt_query_object(
                alias,
                1,
                buffer.as_mut_ptr().cast(),
                required,
                core::ptr::null_mut()
            ),
            STATUS_SUCCESS
        );
        assert_eq!(nt_close_sync(alias), STATUS_SUCCESS);
    });
    let unnamed = create_event_kernel(0, false).unwrap();
    let mut bytes = [0xa5u8; 16];
    let mut required = 0;
    unsafe {
        assert_eq!(
            object::nt_query_object(unnamed, 1, bytes.as_mut_ptr().cast(), 16, &mut required),
            STATUS_SUCCESS
        );
        assert_eq!(required, 16);
        assert_eq!(bytes, [0u8; 16]);
        assert_eq!(nt_close_sync(unnamed), STATUS_SUCCESS);
        assert_eq!(
            object::nt_query_object(unnamed, 1, bytes.as_mut_ptr().cast(), 16, &mut required),
            STATUS_INVALID_HANDLE
        );
    }
    assert_eq!(file::handle_count(), before);
}

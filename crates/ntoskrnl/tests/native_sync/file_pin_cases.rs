use super::*;
use core::sync::atomic::{AtomicUsize, Ordering};

static RETIRED: AtomicUsize = AtomicUsize::new(0);
static CLEANED: AtomicUsize = AtomicUsize::new(0);
static CLEANUP_PIN: std::sync::Mutex<Option<ntdll::file::FileHandleReference>> =
    std::sync::Mutex::new(None);

unsafe fn cleanup(entry: ntdll::file::HandleEntry) {
    // The final handle is gone, but cleanup owns an independent reference.
    let _ = ntdll::file::handle_count();
    assert_eq!(unsafe { *(entry.target as *const usize) }, 0x1234);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 0);
    CLEANED.fetch_add(1, Ordering::Relaxed);
}

#[test]
fn granted_file_access_and_process_ownership_are_enforced_at_reference_and_close() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    CLEANED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    let read_control = 0x0002_0000;
    let write_dac = 0x0004_0000;
    ntdll::file::FILE_PROCESS.with(|process| process.set(Some(30)));
    let handle = unsafe {
        ntdll::file::alloc_authorized_file_handle(payload, read_control, Some(30), cleanup, retire)
    };
    let reference = unsafe { ntdll::file::reference_file_handle_with_access(handle, read_control, retire) }.unwrap();
    assert!(matches!(unsafe { ntdll::file::reference_file_handle_with_access(handle, write_dac, retire) }, Err(STATUS_ACCESS_DENIED)));
    ntdll::file::FILE_PROCESS.with(|process| process.set(Some(31)));
    assert!(ntdll::file::lookup_handle(handle).is_none());
    assert!(matches!(unsafe { ntdll::file::reference_file_handle_with_access(handle, 0, retire) }, Err(STATUS_INVALID_HANDLE)));
    assert!(matches!(ntdll::file::close_handle(handle), Err(STATUS_INVALID_HANDLE)));
    ntdll::file::FILE_PROCESS.with(|process| process.set(Some(30)));
    ntdll::file::close_handle(handle).unwrap();
    assert_eq!(CLEANED.load(Ordering::Relaxed), 1);
    drop(reference);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
    ntdll::file::FILE_PROCESS.with(|process| process.set(None));
}

#[test]
fn last_file_handle_cleans_up_before_outstanding_reference_closes() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    CLEANED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    // SAFETY: these callbacks implement cleanup and destruction of this Box.
    let handle = unsafe { ntdll::file::alloc_file_handle(payload, cleanup, retire) };
    let pin = unsafe { ntdll::file::reference_file_handle(handle, retire) }.unwrap();
    assert!(!ntdll::file::close_handle(handle).unwrap().1);
    assert_eq!(CLEANED.load(Ordering::Relaxed), 1);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 0);
    let replacement = ntdll::file::alloc_handle(ntdll::file::HandleKind::Event, 0);
    assert_eq!(replacement, handle);
    drop(pin);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
    assert!(ntdll::file::free_handle(replacement));
}

unsafe fn cleanup_dropping_final_io_pin(entry: ntdll::file::HandleEntry) {
    drop(CLEANUP_PIN.lock().unwrap().take());
    // Without the close path's temporary reference the drop above destroys
    // the payload before the cleanup callback has finished using it.
    unsafe { cleanup(entry); }
}

#[test]
fn cleanup_can_drop_last_io_reference_without_closing_its_own_storage() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    CLEANED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    let handle = unsafe {
        ntdll::file::alloc_file_handle(payload, cleanup_dropping_final_io_pin, retire)
    };
    *CLEANUP_PIN.lock().unwrap() =
        Some(unsafe { ntdll::file::reference_file_handle(handle, retire) }.unwrap());
    assert!(ntdll::file::close_handle(handle).is_ok());
    assert_eq!(CLEANED.load(Ordering::Relaxed), 1);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
    assert!(matches!(ntdll::file::close_handle(handle), Err(STATUS_INVALID_HANDLE)));
}

#[test]
fn protected_file_defers_cleanup_and_unpinned_close_runs_both_callbacks() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    CLEANED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    let handle = unsafe { ntdll::file::alloc_file_handle(payload, cleanup, retire) };
    ntdll::file::update_handle_flags(handle, 2, 2).unwrap();
    assert!(matches!(ntdll::file::close_handle(handle), Err(STATUS_HANDLE_NOT_CLOSABLE)));
    assert_eq!(CLEANED.load(Ordering::Relaxed), 0);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 0);
    ntdll::file::update_handle_flags(handle, 2, 0).unwrap();
    // Callbacks already own teardown; callers must not destroy it again.
    assert!(!ntdll::file::close_handle(handle).unwrap().1);
    assert_eq!(CLEANED.load(Ordering::Relaxed), 1);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
}

#[test]
fn internal_file_rollback_also_runs_cleanup_and_close() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    CLEANED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    let handle = unsafe { ntdll::file::alloc_file_handle(payload, cleanup, retire) };
    assert!(ntdll::file::free_handle(handle));
    assert_eq!(CLEANED.load(Ordering::Relaxed), 1);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
}

#[test]
fn racing_file_close_and_reference_release_run_each_callback_once() {
    let _guard = state_guard();
    for _ in 0..100 {
        RETIRED.store(0, Ordering::Relaxed);
        CLEANED.store(0, Ordering::Relaxed);
        let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
        let handle = unsafe { ntdll::file::alloc_file_handle(payload, cleanup, retire) } as usize;
        let pin = unsafe { ntdll::file::reference_file_handle(handle as HANDLE, retire) }.unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let first_barrier = barrier.clone();
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            ntdll::file::close_handle(handle as HANDLE)
        });
        let second_barrier = barrier.clone();
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            ntdll::file::close_handle(handle as HANDLE)
        });
        barrier.wait();
        drop(pin);
        let outcomes = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(outcomes.iter().filter(|result| matches!(result, Err(STATUS_INVALID_HANDLE))).count(), 1);
        assert_eq!(CLEANED.load(Ordering::Relaxed), 1);
        assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn file_pin_checks_required_access_before_acquiring_payload() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    let handle = ntdll::file::alloc_access_handle(
        ntdll::file::HandleKind::File, payload, 2, false,
    ).unwrap();
    assert!(matches!(
        unsafe { ntdll::file::reference_file_handle_with_access(handle, 1, retire) },
        Err(STATUS_ACCESS_DENIED)
    ));
    let pin = unsafe { ntdll::file::reference_file_handle_with_access(handle, 2, retire) }.unwrap();
    assert!(!ntdll::file::close_handle(handle).unwrap().1);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 0);
    drop(pin);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
}

unsafe fn retire(entry: ntdll::file::HandleEntry) {
    // Reentering the table proves cleanup does not run under its lock.
    let _ = ntdll::file::handle_count();
    // SAFETY: each test transfers one Box to its published file payload.
    assert_eq!(
        unsafe { *Box::from_raw(entry.target as *mut usize) },
        0x1234
    );
    RETIRED.fetch_add(1, Ordering::Relaxed);
}

#[test]
fn file_pins_survive_close_and_handle_slot_reuse() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    let handle =
        ntdll::file::alloc_access_handle(ntdll::file::HandleKind::File, payload, 0, false).unwrap();
    // SAFETY: retire is the final destructor for this owned test payload.
    let first = unsafe { ntdll::file::reference_file_handle(handle, retire) }.unwrap();
    let second = unsafe { ntdll::file::reference_file_handle(handle, retire) }.unwrap();
    assert_eq!(ntdll::file::close_handle(handle).unwrap().1, false);
    assert!(ntdll::file::lookup_handle(handle).is_none());
    let replacement = ntdll::file::alloc_handle(ntdll::file::HandleKind::Event, 0);
    assert_eq!(replacement, handle);
    assert_eq!(first.entry().target, payload);
    assert_eq!(unsafe { *(second.entry().target as *const usize) }, 0x1234);
    drop(first);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 0);
    drop(second);
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
    assert!(matches!(
        ntdll::file::lookup_handle(replacement).unwrap().kind,
        ntdll::file::HandleKind::Event
    ));
    assert!(ntdll::file::free_handle(replacement));
}

#[test]
fn dropping_pin_with_live_file_handle_does_not_retire_it() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
    let handle = ntdll::file::alloc_handle(ntdll::file::HandleKind::File, payload);
    drop(unsafe { ntdll::file::reference_file_handle(handle, retire) }.unwrap());
    assert_eq!(RETIRED.load(Ordering::Relaxed), 0);
    let (entry, final_reference) = ntdll::file::close_handle(handle).unwrap();
    assert!(final_reference);
    unsafe {
        retire(entry);
    }
    assert_eq!(RETIRED.load(Ordering::Relaxed), 1);
}

#[test]
fn file_pin_validates_kind_and_rejects_closed_handles() {
    let _guard = state_guard();
    let handle = ntdll::file::alloc_handle(ntdll::file::HandleKind::Event, 0);
    assert!(matches!(
        unsafe { ntdll::file::reference_file_handle(handle, retire) },
        Err(STATUS_OBJECT_TYPE_MISMATCH)
    ));
    assert!(ntdll::file::free_handle(handle));
    assert!(matches!(
        unsafe { ntdll::file::reference_file_handle(handle, retire) },
        Err(STATUS_INVALID_HANDLE)
    ));
}

#[test]
fn concurrent_pin_and_final_close_retire_file_once() {
    let _guard = state_guard();
    RETIRED.store(0, Ordering::Relaxed);
    for iteration in 0..100 {
        let payload = Box::into_raw(Box::new(0x1234usize)) as u64;
        let handle = ntdll::file::alloc_handle(ntdll::file::HandleKind::File, payload) as usize;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let reader_barrier = barrier.clone();
        let reader = std::thread::spawn(move || {
            reader_barrier.wait();
            match unsafe { ntdll::file::reference_file_handle(handle as HANDLE, retire) } {
                Ok(pin) => {
                    std::thread::yield_now();
                    assert_eq!(unsafe { *(pin.entry().target as *const usize) }, 0x1234);
                    drop(pin);
                }
                Err(status) => assert_eq!(status, STATUS_INVALID_HANDLE),
            }
        });
        barrier.wait();
        let (entry, retired) = ntdll::file::close_handle(handle as HANDLE).unwrap();
        if retired {
            unsafe {
                retire(entry);
            }
        }
        reader.join().unwrap();
        assert_eq!(RETIRED.load(Ordering::Relaxed), iteration + 1);
    }
}

use super::*;
use core::sync::atomic::{AtomicUsize, Ordering};

static RETIRED: AtomicUsize = AtomicUsize::new(0);

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

//! Actual production cleanup/close request construction and completion path.
use super::*;

static FILE: AtomicUsize = AtomicUsize::new(0);
static REQUEST: AtomicUsize = AtomicUsize::new(0);
static SEEN: Mutex<Vec<u8>> = Mutex::new(Vec::new());

unsafe fn finish(request: *mut Irp) {
    // CLEANUP/ CLOSE must return completion status, not dispatch status.
    unsafe {
        (*request).io_status.status = STATUS_ACCESS_DENIED as u32;
        io_complete_request(request, 0);
    }
}

fn complete_pending() {
    let request = REQUEST.swap(0, Ordering::AcqRel) as *mut Irp;
    assert!(!request.is_null());
    unsafe { finish(request); }
}

unsafe extern "C" fn handler(device: *mut DeviceObject, request: *mut Irp) -> i32 {
    unsafe {
        let stack = &*(*request).current_stack;
        assert_eq!(stack.file_object as usize, FILE.load(Ordering::Relaxed));
        assert_eq!(stack.device_object, device);
        SEEN.lock().unwrap().push(stack.major_function);
        if stack.major_function == io::major::IRP_MJ_CLEANUP {
            REQUEST.store(request as usize, Ordering::Release);
            *power_cases::YIELD_HOOK.lock().unwrap() = Some(complete_pending);
            STATUS_PENDING
        } else {
            finish(request);
            STATUS_SUCCESS
        }
    }
}

#[test]
fn file_lifecycle_requests_retain_same_file_through_pending_cleanup_and_close() {
    let _guard = TEST_LOCK.lock();
    let baseline = mm::pool::live();
    SEEN.lock().unwrap().clear();
    let mut name = [];
    let mut owner = driver(&mut name);
    owner.major_functions[18] = Some(handler);
    owner.major_functions[2] = Some(handler);
    let device = device(&mut *owner, 0);
    let mut file = Box::new(io::FileObject);
    FILE.store(&mut *file as *mut _ as usize, Ordering::Relaxed);
    let mut requests = file_lifecycle::Requests::reserve().unwrap();
    unsafe {
        for major in [io::major::IRP_MJ_CLEANUP, io::major::IRP_MJ_CLOSE] {
            assert_eq!(requests.take(major).unwrap().submit(device, &mut *file), STATUS_ACCESS_DENIED);
            assert_eq!(REQUEST.load(Ordering::Acquire), 0);
        }
        drop(Box::from_raw(device));
    }
    assert_eq!(mm::pool::live(), baseline);
    assert_eq!(*SEEN.lock().unwrap(), [18, 2]);
}

#[test]
fn teardown_reservation_failure_releases_partial_pair_before_open() {
    let _guard = TEST_LOCK.lock();
    let baseline = mm::pool::live();
    SEEN.lock().unwrap().clear();
    for remaining in [0, 1] {
        mm::pool::FAIL_AFTER.store(remaining, Ordering::Relaxed);
        let result = file_lifecycle::Requests::reserve();
        mm::pool::FAIL_AFTER.store(-1, Ordering::Relaxed);
        assert!(matches!(result, Err(STATUS_NO_MEMORY)));
        assert_eq!(mm::pool::live(), baseline);
    }
    assert!(SEEN.lock().unwrap().is_empty());
}

#[test]
fn opened_file_teardown_still_dispatches_when_pool_is_exhausted() {
    let _guard = TEST_LOCK.lock();
    let baseline = mm::pool::live();
    SEEN.lock().unwrap().clear();
    let mut name = [];
    let mut owner = driver(&mut name);
    owner.major_functions[18] = Some(handler);
    owner.major_functions[2] = Some(handler);
    let device = device(&mut *owner, 0);
    let mut file = Box::new(io::FileObject);
    FILE.store(&mut *file as *mut _ as usize, Ordering::Relaxed);
    let mut requests = file_lifecycle::Requests::reserve().unwrap();
    mm::pool::FAIL_AFTER.store(0, Ordering::Relaxed);
    let statuses = [io::major::IRP_MJ_CLEANUP, io::major::IRP_MJ_CLOSE].map(|major| {
        unsafe { requests.take(major).unwrap().submit(device, &mut *file) }
    });
    mm::pool::FAIL_AFTER.store(-1, Ordering::Relaxed);
    unsafe { drop(Box::from_raw(device)); }
    assert_eq!(statuses, [STATUS_ACCESS_DENIED; 2]);
    assert_eq!(*SEEN.lock().unwrap(), [18, 2]);
    assert_eq!(mm::pool::live(), baseline);
}

#[test]
fn failed_create_and_invalid_teardown_release_unused_requests() {
    let _guard = TEST_LOCK.lock();
    let baseline = mm::pool::live();
    let requests = file_lifecycle::Requests::reserve().unwrap();
    assert_eq!(mm::pool::live(), baseline + 2);
    drop(requests); // failed CREATE never dispatched either teardown request
    assert_eq!(mm::pool::live(), baseline);

    let mut requests = file_lifecycle::Requests::reserve().unwrap();
    assert!(requests.take(io::major::IRP_MJ_READ).is_err());
    let cleanup = requests.take(io::major::IRP_MJ_CLEANUP).unwrap();
    assert!(requests.take(io::major::IRP_MJ_CLEANUP).is_err());
    let status = unsafe { cleanup.submit(null_mut(), null_mut()) };
    assert_eq!(status, STATUS_INVALID_DEVICE_REQUEST);
    assert_eq!(mm::pool::live(), baseline + 1);
    drop(requests);
    assert_eq!(mm::pool::live(), baseline);
}

//! Run the real dispatcher, completion walker and synchronous issuer.
use super::*;
use std::sync::atomic::AtomicI32;

static MODE: AtomicI32 = AtomicI32::new(0);
static REQUEST: AtomicUsize = AtomicUsize::new(0);
static WORKER: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

unsafe fn finish(irp: *mut Irp) {
    (*irp).io_status.status = STATUS_ACCESS_DENIED as u32;
    (*irp).io_status.information = 0x12345678;
    io_complete_request(irp, 0);
    // The synchronous issuer owns the stopped request until dispatch returns
    // (or the pending callback publishes ownership), rather than the driver.
}
fn pending_completion() {
    let irp = REQUEST.swap(0, Ordering::AcqRel) as *mut Irp;
    assert!(crate::mm::pool::owns(irp.cast()));
    unsafe {
        finish(irp);
    }
}
unsafe extern "C" fn handler(_: *mut DeviceObject, irp: *mut Irp) -> i32 {
    if MODE.load(Ordering::Relaxed) == 0 {
        finish(irp);
        assert!(crate::mm::pool::owns(irp.cast()));
        // Deliberately differs from the final IoStatus; use completion data.
        STATUS_SUCCESS
    } else if MODE.load(Ordering::Relaxed) == 1 {
        REQUEST.store(irp as usize, Ordering::Release);
        *crate::power_cases::YIELD_HOOK.lock().unwrap() = Some(pending_completion);
        STATUS_PENDING
    } else {
        let request = irp as usize;
        *WORKER.lock().unwrap() = Some(std::thread::spawn(move || unsafe {
            finish(request as *mut Irp);
        }));
        STATUS_PENDING
    }
}

#[test]
fn synchronous_issuer_waits_for_pending_failure_and_uses_final_iosb() {
    let _serial = TEST_LOCK.lock().unwrap();
    let baseline = crate::mm::pool::live();
    let mut name = [];
    let mut driver = driver(&mut name);
    driver.major_functions[3] = Some(handler);
    let device = device(&mut *driver, 0);
    for mode in [0, 1, 2] {
        MODE.store(mode, Ordering::Relaxed);
        let irp = allocate_irp(1);
        unsafe {
            (*(*irp).current_stack).major_function = 3;
            let result = crate::synchronous_request::submit_and_wait(device, irp);
            assert_eq!(result, (STATUS_ACCESS_DENIED, 0x12345678));
        }
        if let Some(worker) = WORKER.lock().unwrap().take() {
            worker.join().unwrap();
        }
        assert_eq!(REQUEST.load(Ordering::Acquire), 0);
        assert_eq!(crate::mm::pool::live(), baseline);
    }
    unsafe {
        drop(Box::from_raw(device));
    }
}

#[test]
fn dispatcher_errors_also_complete_and_release_request_storage() {
    let _serial = TEST_LOCK.lock().unwrap();
    let baseline = crate::mm::pool::live();
    let irp = allocate_irp(1);
    let result = unsafe { crate::synchronous_request::submit_and_wait(null_mut(), irp) };
    assert_eq!(result, (STATUS_NO_SUCH_DEVICE, 0));
    assert_eq!(crate::mm::pool::live(), baseline);
}

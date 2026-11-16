//! Actual production ownership/publication/teardown with host object storage.
use super::*;
use crate::file_device_binding::FileDeviceBinding;

static UNLOAD_GATE: Mutex<Option<std::sync::Arc<std::sync::Barrier>>> = Mutex::new(None);

unsafe extern "C" fn gated_unload(driver: *mut DriverObject) {
    let barrier = UNLOAD_GATE.lock().unwrap().as_ref().unwrap().clone();
    barrier.wait();
    barrier.wait();
    unload(driver);
}

#[test]
fn binding_prevents_unload_callback_and_device_delete_until_final_drop() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [0x46, 0x42, 0x31];
    let mut owner = driver(&mut name);
    let pointer = &mut *owner as *mut _;
    let target = device(pointer, 0);
    unsafe {
        assert!(device_lifecycle::publish_device(target));
        assert!(register_start_type_driver(pointer, ServiceStartType::DemandStart));
    }
    let before = CALLED.load(Ordering::Relaxed);
    let binding = FileDeviceBinding::find(b"FB1").unwrap();
    let clone = binding.try_clone().unwrap();
    assert_eq!(binding.device_ptr(), target);
    assert_eq!(binding.driver().as_ptr(), pointer);
    unsafe {
        assert_eq!((*target).ref_count.load(Ordering::Relaxed), 3);
        assert!(!unload_driver(pointer));
        assert!(!delete_device(target));
    }
    assert_eq!(CALLED.load(Ordering::Relaxed), before);
    drop(binding);
    unsafe { assert!(!unload_driver(pointer)); }
    drop(clone);
    unsafe { assert!(unload_driver(pointer)); }
    assert_eq!(CALLED.load(Ordering::Relaxed), before + 1);
    assert!(owner.device_object.is_null());
    assert!(matches!(FileDeviceBinding::find(b"FB1"), Err(STATUS_OBJECT_NAME_NOT_FOUND)));
}

#[test]
fn missing_device_and_overflow_roll_back_driver_ownership() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [0x46, 0x42, 0x32];
    let mut owner = driver(&mut name);
    let pointer = &mut *owner as *mut _;
    unsafe { assert!(register_start_type_driver(pointer, ServiceStartType::DemandStart)); }
    assert!(matches!(FileDeviceBinding::find(b"FB2"), Err(STATUS_NO_SUCH_DEVICE)));
    let target = device(pointer, 0);
    unsafe {
        assert!(device_lifecycle::publish_device(target));
        (*target).ref_count.store(u32::MAX, Ordering::Relaxed);
    }
    assert!(matches!(FileDeviceBinding::find(b"FB2"), Err(STATUS_INSUFFICIENT_RESOURCES)));
    unsafe {
        assert_eq!((*target).ref_count.load(Ordering::Relaxed), u32::MAX);
        (*target).ref_count.store(1, Ordering::Relaxed);
        // Both failure paths released the driver guard; callback can now run.
        assert!(unload_driver(pointer));
    }
}

#[test]
fn disabled_or_uninitialized_driver_cannot_be_bound() {
    let _lock = TEST_LOCK.lock().unwrap();
    for (suffix, start_type) in [(0x33, ServiceStartType::Disabled), (0x34, ServiceStartType::DemandStart)] {
        let mut name = [0x46, 0x42, suffix];
        let mut owner = driver(&mut name);
        if suffix == 0x34 { owner.driver_init = Some(failure); }
        let pointer = &mut *owner as *mut _;
        unsafe { assert!(register_start_type_driver(pointer, start_type)); }
        let ascii = [b'F', b'B', suffix as u8];
        assert!(matches!(DriverReference::find(&ascii), Err(STATUS_OBJECT_NAME_NOT_FOUND)));
        unsafe { assert!(unload_driver(pointer)); }
    }
}

#[test]
fn binding_clone_retains_original_device_when_list_head_changes() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [0x46, 0x42, 0x35];
    let mut owner = driver(&mut name);
    let pointer = &mut *owner as *mut _;
    let first = device(pointer, 0);
    unsafe {
        assert!(device_lifecycle::publish_device(first));
        assert!(register_start_type_driver(pointer, ServiceStartType::DemandStart));
    }
    let binding = FileDeviceBinding::find(b"FB5").unwrap();
    let second = device(pointer, 0);
    unsafe { assert!(device_lifecycle::publish_device(second)); }
    let clone = binding.try_clone().unwrap();
    let newer = FileDeviceBinding::find(b"FB5").unwrap();
    assert_eq!(clone.device_ptr(), first);
    assert_eq!(newer.device_ptr(), second);
    drop((binding, clone, newer));
    unsafe {
        assert!(delete_device(second));
        assert!(unload_driver(pointer));
    }
}

#[test]
fn retained_lookup_blocks_concurrent_unload_before_callback() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [0x46, 0x42, 0x36];
    let mut owner = driver(&mut name);
    let pointer = &mut *owner as *mut _;
    let target = device(pointer, 0);
    unsafe {
        assert!(device_lifecycle::publish_device(target));
        assert!(register_start_type_driver(pointer, ServiceStartType::DemandStart));
    }
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (drop_tx, drop_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let binding = FileDeviceBinding::find(b"FB6").unwrap();
        held_tx.send(()).unwrap();
        drop_rx.recv().unwrap();
        drop(binding);
    });
    held_rx.recv().unwrap();
    let before = CALLED.load(Ordering::Relaxed);
    unsafe { assert!(!unload_driver(pointer)); }
    assert_eq!(CALLED.load(Ordering::Relaxed), before);
    drop_tx.send(()).unwrap();
    worker.join().unwrap();
    unsafe { assert!(unload_driver(pointer)); }
}

#[test]
fn retained_device_lookup_races_actual_graph_locked_deletion() {
    let _lock = TEST_LOCK.lock().unwrap();
    for _ in 0..100 {
        let mut name = [0x46, 0x42, 0x37];
        let mut owner = driver(&mut name);
        let pointer = &mut *owner as *mut _;
        let target = device(pointer, 0);
        unsafe {
            assert!(device_lifecycle::publish_device(target));
            assert!(register_start_type_driver(pointer, ServiceStartType::DemandStart));
        }
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            match FileDeviceBinding::find(b"FB7") {
                Ok(binding) => { assert!(!binding.device_ptr().is_null()); drop(binding); }
                Err(status) => assert_eq!(status, STATUS_NO_SUCH_DEVICE),
            }
        });
        barrier.wait();
        let deleted = unsafe { delete_device(target) };
        worker.join().unwrap();
        unsafe {
            if !deleted { assert!(delete_device(target)); }
            assert!(owner.device_object.is_null());
            assert!(unload_driver(pointer));
        }
    }
}

#[test]
fn lookup_cannot_enter_driver_after_unload_reservation() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [0x46, 0x42, 0x39];
    let mut owner = driver(&mut name);
    owner.driver_unload = Some(gated_unload);
    let pointer = &mut *owner as *mut DriverObject;
    let target = device(pointer, 0);
    unsafe {
        assert!(device_lifecycle::publish_device(target));
        assert!(register_start_type_driver(pointer, ServiceStartType::DemandStart));
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    *UNLOAD_GATE.lock().unwrap() = Some(barrier.clone());
    // Keep caller-owned object storage alive until the unload thread joins.
    let address = pointer as usize;
    let worker = std::thread::spawn(move || unsafe { unload_driver(address as *mut _) });
    barrier.wait();
    // The callback has entered but has not deleted its device. Lookup must
    // reject the Unloading record even though both allocations still exist.
    assert!(matches!(FileDeviceBinding::find(b"FB9"), Err(STATUS_OBJECT_NAME_NOT_FOUND)));
    barrier.wait();
    assert!(worker.join().unwrap());
    *UNLOAD_GATE.lock().unwrap() = None;
    assert!(owner.device_object.is_null());
}

#[test]
fn graph_publication_rejects_duplicates_and_device_limit_atomically() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [0x46, 0x42, 0x38];
    let mut owner = driver(&mut name);
    let pointer = &mut *owner as *mut _;
    let first = device(pointer, 0);
    unsafe {
        assert!(device_lifecycle::publish_device_with(first, || {
            assert!(owner.device_object.is_null());
            (*first).namespace_handle = 1;
        }));
        assert_eq!((*owner.device_object).namespace_handle, 1);
        assert!(!device_lifecycle::publish_device_with(first, || panic!("duplicate was published")));
        for _ in 1..MAX_DEVICES {
            let next = device(pointer, 0);
            assert!(device_lifecycle::publish_device(next));
        }
        let rejected = device(pointer, 0);
        let original_head = owner.device_object;
        assert!(!device_lifecycle::publish_device_with(rejected, || panic!("full list was published")));
        assert_eq!(owner.device_object, original_head);
        assert!((*rejected).next_device.is_null());
        drop(Box::from_raw(rejected));
        while !owner.device_object.is_null() { assert!(delete_device(owner.device_object)); }
    }
}

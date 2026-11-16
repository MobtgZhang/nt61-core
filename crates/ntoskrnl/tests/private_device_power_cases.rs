//! Production power request, references and completion unwind with host storage.
use super::*;
use crate::{device_reference::DeviceReference, target_device::*};
use std::sync::atomic::{AtomicI32, AtomicUsize};
pub static YIELD_HOOK: Mutex<Option<fn()>> = Mutex::new(None);
static MODE: AtomicI32 = AtomicI32::new(0);
static TARGET: AtomicUsize = AtomicUsize::new(0);
static FILE: AtomicUsize = AtomicUsize::new(0);
static PENDING_IRP: AtomicUsize = AtomicUsize::new(0);
static REQUESTS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn query(device: *mut DeviceObject, request: *mut Irp) -> i32 {
    let slot = &*(*request).current_stack;
    assert_eq!(slot.major_function, 0x1b);
    assert_eq!(slot.minor_function, 7);
    assert_eq!(slot.parameters.as_u64, 4);
    assert_eq!(slot.file_object as usize, FILE.load(Ordering::Relaxed));
    // The request must not hold the graph lock across driver dispatch.
    drop(DeviceReference::acquire(device).unwrap());
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    if MODE.load(Ordering::Relaxed) == 6 {
        PENDING_IRP.store(request as usize, Ordering::Relaxed);
        *YIELD_HOOK.lock().unwrap() = Some(complete_pending);
        return STATUS_PENDING;
    }
    if MODE.load(Ordering::Relaxed) == 7 {
        return STATUS_ACCESS_DENIED;
    }
    finish(request)
}
unsafe fn finish(request: *mut Irp) -> i32 {
    let mode = MODE.load(Ordering::Relaxed);
    let mut status = STATUS_SUCCESS;
    let relation = match mode {
        1 => allocate_relations(&[]).unwrap(),
        2 => {
            let target =
                DeviceReference::acquire(TARGET.load(Ordering::Relaxed) as *mut _).unwrap();
            allocate_relations(&[&target, &target]).unwrap()
        }
        3 => null_mut(),
        4 => {
            let relations = allocate_relations(&[]).unwrap();
            (*relations).count = 1;
            relations // a null target entry, without a transferred reference
        }
        5 => {
            status = STATUS_ACCESS_DENIED;
            null_mut()
        }
        _ => {
            let target =
                DeviceReference::acquire(TARGET.load(Ordering::Relaxed) as *mut _).unwrap();
            match allocate_relations(&[&target]) {
                Ok(result) => result,
                Err(failure) => {
                    status = failure;
                    null_mut()
                }
            }
        }
    };
    (*request).io_status.status = status as u32;
    (*request).io_status.information = relation as usize;
    io_complete_request(request, 0);
    status
}
fn complete_pending() {
    let raw = PENDING_IRP.swap(0, Ordering::Relaxed) as *mut Irp;
    assert!(!raw.is_null());
    unsafe {
        finish(raw);
    }
}
struct Fixture {
    driver: Box<DriverObject>,
    source: *mut DeviceObject,
    target: *mut DeviceObject,
    file: Box<FileObject>,
}
impl Fixture {
    fn new() -> Self {
        let mut name = [];
        let mut driver = driver(&mut name);
        driver.major_functions[27] = Some(query);
        let source = device(&mut *driver, 0);
        let target = device(&mut *driver, 0);
        unsafe {
            (*source).next_device = target;
        }
        driver.device_object = source;
        let mut file = Box::new(FileObject);
        TARGET.store(target as usize, Ordering::Relaxed);
        FILE.store(&mut *file as *mut _ as usize, Ordering::Relaxed);
        MODE.store(0, Ordering::Relaxed);
        REQUESTS.store(0, Ordering::Relaxed);
        Self {
            driver,
            source,
            target,
            file,
        }
    }
    unsafe fn run(&mut self) -> Result<DeviceReference, i32> {
        let reference = DeviceReference::acquire(self.source).unwrap();
        query_target_device(&reference, &mut *self.file)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        crate::mm::pool::FAIL_AFTER.store(-1, Ordering::Relaxed);
        unsafe {
            assert_eq!((*self.source).ref_count.load(Ordering::Relaxed), 1);
            assert_eq!((*self.target).ref_count.load(Ordering::Relaxed), 1);
            assert!(delete_device(self.source));
            assert!(delete_device(self.target));
        }
        assert!(self.driver.device_object.is_null());
    }
}
#[test]
fn power_device_reference_defers_deletion_until_last_guard() {
    let _lock = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    unsafe {
        let reference = DeviceReference::acquire(fixture.target).unwrap();
        let clone = reference.try_clone().unwrap();
        assert!(!delete_device(fixture.target));
        drop(reference);
        assert_eq!((*fixture.target).ref_count.load(Ordering::Relaxed), 2);
        drop(clone);
        assert_eq!((*fixture.target).ref_count.load(Ordering::Relaxed), 1);
    }
}
#[test]
fn power_query_sync_returns_transferred_reference_and_native_states() {
    let _lock = TEST_LOCK.lock().unwrap();
    let before = crate::mm::pool::live();
    let mut fixture = Fixture::new();
    let target = unsafe { fixture.run().unwrap() };
    assert_eq!(target.as_ptr(), fixture.target);
    assert_eq!(REQUESTS.load(Ordering::Relaxed), 1);
    use device_power_state::DevicePowerState::*;
    for (state, native) in [(D0, 1), (D1, 2), (D2, 3), (D3, 4)] {
        unsafe {
            (*target.as_ptr()).power.set(state);
            assert_eq!((*target.as_ptr()).power.get().to_native(), native);
        }
    }
    assert_eq!(crate::mm::pool::live(), before);
    drop(target);
}
#[test]
fn power_pending_query_waits_for_real_completion_without_graph_lock() {
    let _lock = TEST_LOCK.lock().unwrap();
    let before = crate::mm::pool::live();
    let mut fixture = Fixture::new();
    MODE.store(6, Ordering::Relaxed);
    let target = unsafe { fixture.run().unwrap() };
    assert_eq!(target.as_ptr(), fixture.target);
    assert_eq!(PENDING_IRP.load(Ordering::Relaxed), 0);
    assert_eq!(crate::mm::pool::live(), before);
    drop(target);
}
#[test]
fn power_query_rejects_missing_ambiguous_or_null_targets_and_releases_all_refs() {
    let _lock = TEST_LOCK.lock().unwrap();
    let before = crate::mm::pool::live();
    let mut fixture = Fixture::new();
    for mode in 1..=4 {
        MODE.store(mode, Ordering::Relaxed);
        assert_eq!(unsafe { fixture.run() }.err(), Some(STATUS_NO_SUCH_DEVICE));
        assert_eq!(crate::mm::pool::live(), before);
        assert_eq!(
            unsafe { (*fixture.target).ref_count.load(Ordering::Relaxed) },
            1
        );
    }
}
#[test]
fn power_query_propagates_driver_and_uncompleted_sync_failures() {
    let _lock = TEST_LOCK.lock().unwrap();
    let before = crate::mm::pool::live();
    let mut fixture = Fixture::new();
    for mode in [5, 7] {
        MODE.store(mode, Ordering::Relaxed);
        assert_eq!(unsafe { fixture.run() }.err(), Some(STATUS_ACCESS_DENIED));
        assert_eq!(crate::mm::pool::live(), before);
    }
}
#[test]
fn power_query_allocation_failures_release_irp_and_device_refs() {
    let _lock = TEST_LOCK.lock().unwrap();
    let before = crate::mm::pool::live();
    let mut fixture = Fixture::new();
    for after in [0, 1] {
        crate::mm::pool::FAIL_AFTER.store(after, Ordering::Relaxed);
        assert_eq!(
            unsafe { fixture.run() }.err(),
            Some(STATUS_INSUFFICIENT_RESOURCES)
        );
        assert_eq!(crate::mm::pool::live(), before);
        crate::mm::pool::FAIL_AFTER.store(-1, Ordering::Relaxed);
    }
    assert_eq!(REQUESTS.load(Ordering::Relaxed), 1);
}
#[test]
fn power_cached_state_is_synchronized_between_threads() {
    use device_power_state::{CachedDevicePowerState, DevicePowerState};
    let cache = std::sync::Arc::new(CachedDevicePowerState::new(DevicePowerState::D0));
    let writer = cache.clone();
    let thread = std::thread::spawn(move || {
        for _ in 0..10_000 {
            writer.set(DevicePowerState::D1);
            writer.set(DevicePowerState::D3);
        }
    });
    for _ in 0..10_000 {
        assert!((1..=4).contains(&cache.get().to_native()));
    }
    thread.join().unwrap();
    assert_eq!(cache.get(), DevicePowerState::D3);
}
#[test]
fn power_referenced_stack_keeps_detached_nodes_alive_and_rejects_foreign_filters() {
    let _lock = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    unsafe {
        let mut stack = device_stack::DeviceStack::new(fixture.target).unwrap();
        assert!(!stack.detach_filter_device(fixture.target));
        assert!(stack.attach_filter_device(fixture.source));
        assert_eq!(stack.depth(), 2);
        let reference = DeviceReference::acquire(fixture.target).unwrap();
        let held = reference.reference_stack().unwrap();
        assert_eq!(held.top(), fixture.source);
        assert_eq!(held.depth(), 2);
        assert!(stack.detach_filter_device(fixture.source));
        assert!((*fixture.source).attached_device.is_null());
        assert!((*fixture.target).attached_to.is_none());
        assert!(!delete_device(fixture.source));
        drop(held);
        assert_eq!((*fixture.source).ref_count.load(Ordering::Relaxed), 1);
        drop(reference);
    }
}
#[test]
fn power_stack_rejects_a_filter_already_attached_above_another_device() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [];
    let mut driver = driver(&mut name);
    unsafe {
        let base = device(&mut *driver, 0);
        let filter = device(&mut *driver, 0);
        let other = device(&mut *driver, 0);
        (*base).next_device = filter;
        (*filter).next_device = other;
        driver.device_object = base;
        dispatch::attach_device(other, filter);
        let mut stack = device_stack::DeviceStack::new(base).unwrap();
        assert!(!stack.attach_filter_device(filter));
        assert!((*base).attached_to.is_none());
        assert_eq!((*filter).attached_to, Some(other));
        {
            let _graph = device_lifecycle::DEVICE_GRAPH_LOCK.lock();
            (*other).attached_device = null_mut();
            (*filter).attached_to = None;
        }
        assert!(delete_device(base));
        assert!(delete_device(filter));
        assert!(delete_device(other));
    }
}

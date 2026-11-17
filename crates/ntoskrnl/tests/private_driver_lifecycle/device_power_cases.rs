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
static TOPOLOGY_CHANGE: AtomicUsize = AtomicUsize::new(0);
static REPLACEMENT_CALLS: AtomicUsize = AtomicUsize::new(0);

struct Filter {
    driver: Box<DriverObject>,
    device: *mut DeviceObject,
    _name: Box<[u16]>,
}
impl Filter {
    fn new(suffix: u16, handler: DriverDispatch) -> Self {
        let mut name = vec![0x50, 0x46, suffix].into_boxed_slice();
        let mut driver = driver(&mut name);
        driver.major_functions[27] = handler;
        let device = device(&mut *driver, 0);
        unsafe {
            assert!(device_lifecycle::publish_device(device));
            assert!(register_start_type_driver(
                &mut *driver,
                ServiceStartType::DemandStart
            ));
        }
        Self {
            driver,
            device,
            _name: name,
        }
    }
}
impl Drop for Filter {
    fn drop(&mut self) {
        unsafe {
            assert!(unload_driver(&mut *self.driver));
        }
    }
}
struct TopologyChange {
    graph: device_stack::DeviceStack,
    filter: *mut DeviceObject,
    replacement: *mut DeviceObject,
    base: *mut DeviceObject,
}
unsafe extern "C" fn pending_filter(_: *mut DeviceObject, request: *mut Irp) -> i32 {
    PENDING_IRP.store(request as usize, Ordering::Release);
    *YIELD_HOOK.lock().unwrap() = Some(replace_filter_and_forward);
    STATUS_PENDING
}
unsafe extern "C" fn replacement_filter(_: *mut DeviceObject, request: *mut Irp) -> i32 {
    REPLACEMENT_CALLS.fetch_add(1, Ordering::Relaxed);
    (*request).io_status.status = STATUS_ACCESS_DENIED as u32;
    (*request).io_status.information = 0;
    io_complete_request(request, 0);
    STATUS_ACCESS_DENIED
}
fn replace_filter_and_forward() {
    let context = TOPOLOGY_CHANGE.load(Ordering::Acquire) as *mut TopologyChange;
    assert!(!context.is_null());
    unsafe {
        let change = &mut *context;
        assert!(change.graph.detach_filter_device(change.filter));
        assert!(change.graph.attach_filter_device(change.replacement));
        assert!(!unload_driver((*change.filter).driver_object));
        assert!(!unload_driver((*change.base).driver_object));
        assert!(!delete_device(change.filter));
        let request = PENDING_IRP.swap(0, Ordering::AcqRel) as *mut Irp;
        assert!(!request.is_null());
        assert_eq!(
            dispatch::forward_irp_to_lower(change.filter, request),
            STATUS_SUCCESS
        );
        // The issuer's MORE_PROCESSING_REQUIRED keeps the original route alive
        // until query_target_device resumes final completion.
        assert!(!unload_driver((*change.filter).driver_object));
    }
}

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
    _name: Box<[u16]>,
}
impl Fixture {
    fn new() -> Self {
        let mut name: Box<[u16]> = vec![0x50, 0x57, 0x52].into_boxed_slice();
        let mut driver = driver(&mut name);
        driver.major_functions[27] = Some(query);
        let source = device(&mut *driver, 0);
        let target = device(&mut *driver, 0);
        unsafe {
            (*source).next_device = target;
        }
        driver.device_object = source;
        unsafe {
            assert!(register_start_type_driver(
                &mut *driver,
                ServiceStartType::DemandStart
            ));
        }
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
            _name: name,
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
            assert!(unload_driver(&mut *self.driver));
        }
        assert!(self.driver.device_object.is_null());
    }
}
#[test]
fn power_device_reference_defers_deletion_until_last_guard() {
    let _lock = TEST_LOCK.lock();
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
    let _lock = TEST_LOCK.lock();
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
    let _lock = TEST_LOCK.lock();
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
fn power_pending_query_uses_original_route_after_filter_replacement() {
    let _lock = TEST_LOCK.lock();
    let before = crate::mm::pool::live();
    let mut fixture = Fixture::new();
    let filter = Filter::new(0x31, Some(pending_filter));
    let replacement = Filter::new(0x32, Some(replacement_filter));
    let mut change = TopologyChange {
        graph: device_stack::DeviceStack::new(fixture.source).unwrap(),
        filter: filter.device,
        replacement: replacement.device,
        base: fixture.source,
    };
    unsafe {
        assert!(change.graph.attach_filter_device(filter.device));
    }
    TOPOLOGY_CHANGE.store(&mut change as *mut _ as usize, Ordering::Release);
    REPLACEMENT_CALLS.store(0, Ordering::Relaxed);
    let target = unsafe { fixture.run().unwrap() };
    TOPOLOGY_CHANGE.store(0, Ordering::Release);
    assert_eq!(target.as_ptr(), fixture.target);
    assert_eq!(REQUESTS.load(Ordering::Relaxed), 1);
    assert_eq!(REPLACEMENT_CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(PENDING_IRP.load(Ordering::Relaxed), 0);
    unsafe {
        assert_eq!((*filter.device).ref_count.load(Ordering::Relaxed), 1);
        assert!(change.graph.detach_filter_device(replacement.device));
    }
    drop(target);
    drop(filter);
    drop(replacement);
    drop(fixture);
    assert_eq!(crate::mm::pool::live(), before);
}
#[test]
fn power_query_rejects_missing_ambiguous_or_null_targets_and_releases_all_refs() {
    let _lock = TEST_LOCK.lock();
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
    let _lock = TEST_LOCK.lock();
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
    let _lock = TEST_LOCK.lock();
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
    let _lock = TEST_LOCK.lock();
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
    let _lock = TEST_LOCK.lock();
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

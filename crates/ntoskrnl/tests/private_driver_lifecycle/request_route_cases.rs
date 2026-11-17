//! Exercise production IRP routing, registry guards and completion on actual
//! pending/forward/free paths, with host device and pool storage.
use super::*;
use crate::{device_reference::DeviceReference, request_route::RequestRoute};
use std::sync::atomic::AtomicI32;

static MODE: AtomicI32 = AtomicI32::new(0);
static PENDING: AtomicUsize = AtomicUsize::new(0);
static BASE_CALLS: AtomicUsize = AtomicUsize::new(0);
static FILTER_CALLS: AtomicUsize = AtomicUsize::new(0);
static UNEXPECTED_CALLS: AtomicUsize = AtomicUsize::new(0);
static COMPLETIONS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn base(device: *mut DeviceObject, irp: *mut Irp) -> i32 {
    BASE_CALLS.fetch_add(1, Ordering::Relaxed);
    assert!(!unload_driver((*device).driver_object));
    (*irp).io_status.status = STATUS_SUCCESS as u32;
    (*irp).io_status.information = 0x1234;
    io_complete_request(irp, 0);
    // With mode 1 the IRP and its route have already been freed. The active
    // dispatch frame must still pin the callback's image through this return.
    assert!(!unload_driver((*device).driver_object));
    assert!((*device).ref_count.load(Ordering::Relaxed) > 1);
    assert!(!delete_device(device));
    STATUS_SUCCESS
}

unsafe extern "C" fn filter(device: *mut DeviceObject, irp: *mut Irp) -> i32 {
    FILTER_CALLS.fetch_add(1, Ordering::Relaxed);
    if MODE.load(Ordering::Relaxed) == 0 {
        PENDING.store(irp as usize, Ordering::Release);
        STATUS_PENDING
    } else {
        let status = dispatch::forward_irp_to_lower(device, irp);
        assert!(!unload_driver((*device).driver_object));
        assert!(!delete_device(device));
        status
    }
}

unsafe extern "C" fn unexpected(_: *mut DeviceObject, _: *mut Irp) -> i32 {
    UNEXPECTED_CALLS.fetch_add(1, Ordering::Relaxed);
    panic!("an existing request reached a newly attached filter");
}

unsafe extern "C" fn stop(_: *mut DeviceObject, irp: *mut Irp, _: *mut ()) -> i32 {
    assert_eq!((*irp).io_status.information, 0x1234);
    COMPLETIONS.fetch_add(1, Ordering::Relaxed);
    STATUS_MORE_PROCESSING_REQUIRED
}

struct Owner {
    driver: Box<DriverObject>,
    device: *mut DeviceObject,
    _name: Box<[u16]>,
}
impl Owner {
    fn new(suffix: u16, handler: DriverDispatch) -> Self {
        let mut name = vec![0x52, 0x54, suffix].into_boxed_slice();
        let mut owner = driver(&mut name);
        owner.major_functions[3] = handler;
        let dev = device(&mut *owner, 0);
        unsafe {
            assert!(device_lifecycle::publish_device(dev));
            assert!(register_start_type_driver(
                &mut *owner,
                ServiceStartType::DemandStart
            ));
        }
        Self {
            driver: owner,
            device: dev,
            _name: name,
        }
    }
    fn route(&self) -> Result<RequestRoute, i32> {
        let retained = unsafe { DeviceReference::acquire(self.device)? };
        RequestRoute::capture(&retained)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            assert!(unload_driver(&mut *self.driver));
        }
    }
}

fn packet(route: RequestRoute, capture: bool) -> *mut Irp {
    let irp = allocate_irp(route.depth() + 1);
    assert!(!irp.is_null());
    unsafe {
        (*irp).route = Some(route);
        if capture {
            let issuer = (*irp).current_stack;
            (*issuer).completion_routine = Some(stop);
            (*issuer).control = irp_stack::SL_INVOKE_ON_SUCCESS;
        }
        irp_stack::skip_current_stack_location(irp);
        (*(*irp).current_stack).major_function = 3;
    }
    irp
}

fn reset() {
    MODE.store(0, Ordering::Relaxed);
    PENDING.store(0, Ordering::Relaxed);
    BASE_CALLS.store(0, Ordering::Relaxed);
    FILTER_CALLS.store(0, Ordering::Relaxed);
    UNEXPECTED_CALLS.store(0, Ordering::Relaxed);
    COMPLETIONS.store(0, Ordering::Relaxed);
}

#[test]
fn pending_route_survives_detach_and_replacement_and_blocks_each_driver_unload() {
    let _serial = TEST_LOCK.lock();
    reset();
    let before = mm::pool::live();
    let mut base_owner = Owner::new(0x31, Some(base));
    let mut filter_owner = Owner::new(0x32, Some(filter));
    let replacement = Owner::new(0x33, Some(unexpected));
    let mut graph = device_stack::DeviceStack::new(base_owner.device).unwrap();
    unsafe {
        assert!(graph.attach_filter_device(filter_owner.device));
    }
    let route = base_owner.route().unwrap();
    assert_eq!(route.top(), filter_owner.device);
    assert_eq!(route.depth(), 2);
    let irp = packet(route, true);
    unsafe {
        assert_eq!(
            dispatch::dispatch_irp(filter_owner.device, irp),
            STATUS_PENDING
        );
        assert!(graph.detach_filter_device(filter_owner.device));
        assert!(graph.attach_filter_device(replacement.device));
        assert!(!unload_driver(&mut *base_owner.driver));
        assert!(!unload_driver(&mut *filter_owner.driver));
        assert!(!delete_device(filter_owner.device));
    }
    // Forward on another host thread after the old links have been removed.
    // The still-owned IRP carries its original route, rather than a raw-link
    // read or a guard confined to the dispatcher's now-returned stack frame.
    let old_filter = filter_owner.device as usize;
    let worker = std::thread::spawn(move || unsafe {
        let request = PENDING.swap(0, Ordering::AcqRel) as *mut Irp;
        dispatch::forward_irp_to_lower(old_filter as *mut DeviceObject, request)
    });
    assert_eq!(worker.join().unwrap(), STATUS_SUCCESS);
    assert_eq!(FILTER_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(BASE_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(UNEXPECTED_CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(COMPLETIONS.load(Ordering::Relaxed), 1);
    unsafe {
        // MORE_PROCESSING_REQUIRED retains ownership through resumed completion.
        assert!(!unload_driver(&mut *filter_owner.driver));
        io_complete_request(irp, 0);
        assert_eq!((*filter_owner.device).ref_count.load(Ordering::Relaxed), 1);
        assert!(graph.detach_filter_device(replacement.device));
    }
    assert_eq!(mm::pool::live(), before);
}

#[test]
fn synchronous_route_free_keeps_both_active_callback_images_pinned_until_return() {
    let _serial = TEST_LOCK.lock();
    reset();
    MODE.store(1, Ordering::Relaxed);
    let before = mm::pool::live();
    let base_owner = Owner::new(0x34, Some(base));
    let filter_owner = Owner::new(0x35, Some(filter));
    let mut graph = device_stack::DeviceStack::new(base_owner.device).unwrap();
    unsafe {
        assert!(graph.attach_filter_device(filter_owner.device));
    }
    let irp = packet(base_owner.route().unwrap(), false);
    unsafe {
        assert_eq!(
            dispatch::dispatch_irp(filter_owner.device, irp),
            STATUS_SUCCESS
        );
        assert!(graph.detach_filter_device(filter_owner.device));
        assert_eq!((*base_owner.device).ref_count.load(Ordering::Relaxed), 1);
        assert_eq!((*filter_owner.device).ref_count.load(Ordering::Relaxed), 1);
    }
    assert_eq!(mm::pool::live(), before);
    assert_eq!(BASE_CALLS.load(Ordering::Relaxed), 1);
}

#[test]
fn synchronous_single_device_free_retains_active_callback_device() {
    let _serial = TEST_LOCK.lock();
    reset();
    let before = mm::pool::live();
    let owner = Owner::new(0x39, Some(base));
    let irp = packet(owner.route().unwrap(), false);
    unsafe {
        assert_eq!(dispatch::dispatch_irp(owner.device, irp), STATUS_SUCCESS);
        assert_eq!((*owner.device).ref_count.load(Ordering::Relaxed), 1);
    }
    assert_eq!(mm::pool::live(), before);
}

#[test]
fn route_capture_failure_releases_partial_device_and_driver_ownership() {
    let _serial = TEST_LOCK.lock();
    let mut name = [];
    let mut unregistered = driver(&mut name);
    let lower = device(&mut *unregistered, 0);
    unregistered.device_object = lower;
    let filter_owner = Owner::new(0x36, Some(filter));
    let mut graph = device_stack::DeviceStack::new(lower).unwrap();
    unsafe {
        assert!(graph.attach_filter_device(filter_owner.device));
    }
    let retained = unsafe { DeviceReference::acquire(lower).unwrap() };
    assert!(matches!(
        RequestRoute::capture(&retained),
        Err(STATUS_NO_SUCH_DEVICE)
    ));
    unsafe {
        assert_eq!((*lower).ref_count.load(Ordering::Relaxed), 2);
        assert_eq!((*filter_owner.device).ref_count.load(Ordering::Relaxed), 1);
        assert!(graph.detach_filter_device(filter_owner.device));
    }
    // Drop asserts that unload succeeds, then releases the driver's storage.
    drop(filter_owner);
    drop(retained);
    unsafe {
        assert!(delete_device(lower));
    }
}

#[test]
fn dynamic_pool_free_releases_route_before_reuse_and_shrink() {
    let _serial = TEST_LOCK.lock();
    let before = mm::pool::live();
    let owner = Owner::new(0x40, Some(base));
    let mut pool = irp_pool_complete::DynamicIrpPool::new(1, 2);
    let request = pool.allocate(2).unwrap();
    unsafe {
        (*request).route = Some(owner.route().unwrap());
        assert!((*owner.device).ref_count.load(Ordering::Relaxed) > 1);
        pool.free(request);
        assert_eq!((*owner.device).ref_count.load(Ordering::Relaxed), 1);
        let reused = pool.allocate(2).unwrap();
        assert_eq!(request, reused);
        assert!((*reused).route.is_none());
        assert_eq!((*reused).stack_count, 2);
        pool.free(reused);
    }
    assert!(pool.shrink(0) > 0);
    assert_eq!(mm::pool::live(), before);
}

#[test]
fn undispatched_packet_drop_releases_route_and_foreign_dispatch_cannot_escape_it() {
    let _serial = TEST_LOCK.lock();
    reset();
    let before = mm::pool::live();
    let mut owner = Owner::new(0x37, Some(base));
    let foreign = Owner::new(0x38, Some(unexpected));
    let irp = packet(owner.route().unwrap(), false);
    free_irp(irp);
    unsafe {
        assert_eq!((*owner.device).ref_count.load(Ordering::Relaxed), 1);
    }
    let irp = packet(owner.route().unwrap(), false);
    assert_eq!(
        unsafe { dispatch::dispatch_irp(foreign.device, irp) },
        STATUS_NO_SUCH_DEVICE
    );
    assert_eq!(UNEXPECTED_CALLS.load(Ordering::Relaxed), 0);
    let irp = packet(owner.route().unwrap(), false);
    assert_eq!(
        unsafe { dispatch::forward_irp_to_lower(owner.device, irp) },
        STATUS_INVALID_DEVICE_REQUEST
    );
    assert_eq!(mm::pool::live(), before);
    // All three paths released the registry guard, allowing a fresh reference
    // and proving no failed dispatch permanently pinned the driver.
    let reference = DriverReference::acquire(&mut *owner.driver).unwrap();
    drop(reference);
}

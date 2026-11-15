//! Host harness: compile the real lifecycle/dispatch/IRP modules with private
//! object and pool stubs; no privileged kernel code or native WDM claim.
//! rustc --edition=2021 --test crates/ntoskrnl/tests/private_driver_lifecycle.rs
#![allow(dead_code, unused_imports)]
extern crate alloc;
mod types {
    pub type NTSTATUS = i32;
}
use std::sync::Mutex;
static TEST_LOCK: Mutex<()> = Mutex::new(());
mod ke {
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
        unsafe impl<T> Sync for Spinlock<T> {}
        impl<T> Spinlock<T> {
            pub const fn new(value: T) -> Self {
                Self(std::sync::Mutex::new(value))
            }
            pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
                self.0.lock().unwrap()
            }
        }
    }
}
#[path = "../../libs/ntdll/src/ntdll/status.rs"]
pub mod status;
mod libs {
    pub mod ntdll {
        pub use crate::status;
    }
}
mod mm {
    pub mod pool {
        pub unsafe fn free(ptr: *mut u8) -> *mut u8 {
            drop(Box::from_raw(ptr as *mut crate::io::DeviceObject));
            ptr
        }
    }
}
mod ob {
    pub(crate) unsafe fn remove_external_io_object(handle: u64, _: *mut ()) -> bool {
        handle == 1
    }
}
#[path = "../src/io/apc.rs"]
pub mod apc;
#[path = "../src/io/device_lifecycle.rs"]
pub mod device_lifecycle;
#[path = "../src/io/dispatch.rs"]
pub mod dispatch;
#[path = "../src/io/driver_loader.rs"]
pub mod driver_loader;
#[path = "../src/io/irp_stack.rs"]
pub mod irp_stack;
mod io {
    use core::ptr::null_mut;
    use core::sync::atomic::AtomicU32;
    pub const MAX_DEVICES: usize = 64;
    pub mod major {
        pub const IRP_MJ_READ: u8 = 3;
        pub const IRP_MJ_WRITE: u8 = 4;
        pub const IRP_MJ_DEVICE_CONTROL: u8 = 14;
    }
    pub mod cancel {
        pub const IRP_SYNCHRONOUS_API: u32 = 4;
    }
    pub type DriverDispatch = Option<unsafe extern "C" fn(*mut DeviceObject, *mut Irp) -> i32>;
    pub struct UnicodeString {
        pub length: u16,
        pub maximum_length: u16,
        pub buffer: *mut u16,
    }
    pub struct DriverObject {
        pub driver_name: UnicodeString,
        pub driver_start: u64,
        pub driver_size: u64,
        pub device_object: *mut DeviceObject,
        pub driver_init: Option<unsafe extern "C" fn(*mut DriverObject, *mut u16) -> u32>,
        pub driver_unload: Option<unsafe extern "C" fn(*mut DriverObject)>,
        pub major_functions: [DriverDispatch; 28],
    }
    #[derive(Default)]
    pub struct Queue;
    impl Queue {
        pub fn is_empty(&self) -> bool {
            true
        }
    }
    pub struct DeviceObject {
        pub device_queue: Queue,
        pub driver_queues: Queue,
        pub driver_object: *mut DriverObject,
        pub next_device: *mut DeviceObject,
        pub ref_count: AtomicU32,
        pub current_irp: *mut Irp,
        pub attached_device: *mut DeviceObject,
        pub attached_to: Option<*mut DeviceObject>,
        pub vpb: *mut (),
        pub device_extension: *mut (),
        pub namespace_handle: u64,
    }
    pub enum DevicePnPState {
        Deleted,
    }
    impl DeviceObject {
        pub fn set_pnp_state(&mut self, _: DevicePnPState) {}
    }
    pub struct FileObject;
    #[derive(Clone, Copy)]
    pub struct ReadWriteParameters {
        pub byte_offset: u64,
        pub byte_count: u32,
        pub key: u32,
    }
    #[derive(Clone, Copy)]
    pub struct DeviceIoControlParameters {
        pub output_buffer_length: u32,
        pub input_buffer_length: u32,
        pub io_control_code: u32,
        pub type3_input_buffer: *mut (),
    }
    #[derive(Clone, Copy)]
    pub union IoParameters {
        pub as_u64: u64,
        pub read_write: ReadWriteParameters,
        pub device_io_control: DeviceIoControlParameters,
    }
    #[derive(Clone, Copy)]
    pub struct IoStackLocation {
        pub major_function: u8,
        pub minor_function: u8,
        pub flags: u8,
        pub control: u8,
        pub device_object: *mut DeviceObject,
        pub file_object: *mut FileObject,
        pub parameters: IoParameters,
        pub completion_routine:
            Option<unsafe extern "C" fn(*mut DeviceObject, *mut Irp, *mut ()) -> i32>,
        pub completion_context: *mut (),
    }
    pub struct IoStatus {
        pub status: u32,
        pub information: usize,
    }
    pub struct Irp {
        pub io_status: IoStatus,
        pub current_stack: *mut IoStackLocation,
        pub current_location: u8,
        pub stack_count: u8,
        pub flags: u32,
        pub pending_returned: u8,
        pub user_buffer: *mut (),
        pub completed: u32,
    }
    pub unsafe fn io_complete_request(irp: *mut Irp, _: i32) {
        (*irp).completed += 1;
    }
    pub fn allocate_irp(_: u8) -> *mut Irp {
        null_mut()
    }
    pub unsafe fn unregister_driver(_: *mut DriverObject) -> bool {
        true
    }
    pub fn free_irp(_: *mut Irp) {}
    pub use crate::device_lifecycle;
    pub use crate::dispatch;
    pub use crate::driver_loader;
    pub use crate::irp_stack;
    pub use device_lifecycle::delete_device;
}
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use io::driver_loader::*;
use io::*;
use libs::ntdll::status::*;
static CALLED: AtomicUsize = AtomicUsize::new(0);
fn driver(name: &mut [u16]) -> Box<DriverObject> {
    Box::new(DriverObject {
        driver_name: UnicodeString {
            length: (name.len() * 2) as u16,
            maximum_length: (name.len() * 2) as u16,
            buffer: name.as_mut_ptr(),
        },
        driver_start: 0,
        driver_size: 0,
        device_object: null_mut(),
        driver_init: None,
        driver_unload: Some(unload),
        major_functions: [None; 28],
    })
}
fn device(driver: *mut DriverObject, namespace_handle: u64) -> *mut DeviceObject {
    Box::into_raw(Box::new(DeviceObject {
        device_queue: Queue,
        driver_queues: Queue,
        driver_object: driver,
        next_device: null_mut(),
        ref_count: AtomicU32::new(1),
        current_irp: null_mut(),
        attached_device: null_mut(),
        attached_to: None,
        vpb: null_mut(),
        device_extension: null_mut(),
        namespace_handle,
    }))
}
unsafe extern "C" fn unload(driver: *mut DriverObject) {
    // Reentry would deadlock if callback ran under the registry lock.
    let _ = loaded_driver_count();
    assert!(!reference_driver(driver));
    if !(*driver).device_object.is_null() {
        assert!(delete_device((*driver).device_object));
    }
    CALLED.fetch_add(1, Ordering::Relaxed);
}
unsafe extern "C" fn success(driver: *mut DriverObject, path: *mut u16) -> u32 {
    assert!(!reference_driver(driver));
    assert_eq!(loaded_driver_count(), 0);
    assert_eq!(*path, 0x1234);
    (*driver).device_object = device(driver, 1);
    1 // Informational success must satisfy NT_SUCCESS.
}
unsafe extern "C" fn failure(driver: *mut DriverObject, _: *mut u16) -> u32 {
    let first = device(driver, 0);
    let second = device(driver, 1);
    (*second).next_device = first;
    (*driver).device_object = second;
    STATUS_INVALID_DEVICE_REQUEST as u32
}
#[test]
fn initialize_unload_and_reference_lifecycle() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [65u16, 66];
    let mut d = driver(&mut name);
    d.driver_init = Some(success);
    unsafe {
        assert!(register_start_type_driver(
            &mut *d,
            ServiceStartType::DemandStart
        ));
        assert!(!register_start_type_driver(
            &mut *d,
            ServiceStartType::DemandStart
        ));
        assert_eq!(initialize_registered_driver(&mut *d, &[0x1234, 0]), 1);
        assert_eq!(find_driver(b"AB"), &mut *d as *mut _);
        assert!(reference_driver(&mut *d));
        assert!(!unload_driver(&mut *d));
        assert!(dereference_driver(&mut *d));
        assert!(!dereference_driver(&mut *d));
        assert!(unload_driver(&mut *d));
        assert!(d.device_object.is_null());
        assert!(find_driver(b"AB").is_null());
    }
}
#[test]
fn failed_entry_rolls_back_new_devices_and_never_calls_unload() {
    let _lock = TEST_LOCK.lock().unwrap();
    let before = CALLED.load(Ordering::Relaxed);
    let mut name = [67u16];
    let mut d = driver(&mut name);
    d.driver_init = Some(failure);
    unsafe {
        assert!(register_start_type_driver(
            &mut *d,
            ServiceStartType::BootStart
        ));
        assert_eq!(load_boot_start_drivers(), 0);
        assert!(d.device_object.is_null());
        assert!(find_driver(b"C").is_null());
        assert!(unload_driver(&mut *d));
    }
    assert_eq!(CALLED.load(Ordering::Relaxed), before);
}
#[test]
fn lookup_second_record_and_enumeration_can_reenter() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut a = [68u16];
    let mut b = [69u16];
    let mut x = driver(&mut a);
    let mut y = driver(&mut b);
    unsafe {
        assert!(register_start_type_driver(
            &mut *x,
            ServiceStartType::AutoStart
        ));
        assert!(register_start_type_driver(
            &mut *y,
            ServiceStartType::AutoStart
        ));
        assert!(reference_driver(&mut *y));
        assert!(dereference_driver(&mut *y));
        enumerate_drivers(|_| {
            assert_eq!(loaded_driver_count(), 2);
            true
        });
        assert!(unload_driver(&mut *x));
        assert!(unload_driver(&mut *y));
    }
}
#[test]
fn malformed_counted_names_and_disk_load_are_rejected() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut a = [70u16];
    let mut d = driver(&mut a);
    unsafe {
        d.driver_name.length = 1;
        assert!(!register_start_type_driver(
            &mut *d,
            ServiceStartType::BootStart
        ));
        d.driver_name.length = 2;
        d.driver_name.buffer = null_mut();
        assert!(!register_start_type_driver(
            &mut *d,
            ServiceStartType::BootStart
        ));
    }
    assert!(load_driver(b"fake.sys", b"path", ServiceStartType::BootStart).is_null());
}
#[test]
fn deleting_busy_attached_or_namespace_referenced_device_is_refused() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [71u16];
    let mut d = driver(&mut name);
    unsafe {
        let dev = device(&mut *d, 2);
        d.device_object = dev;
        assert!(!delete_device(dev));
        (*dev).namespace_handle = 0;
        (*dev).ref_count.store(2, Ordering::Relaxed);
        assert!(!delete_device(dev));
        (*dev).ref_count.store(1, Ordering::Relaxed);
        (*dev).attached_to = Some(dev);
        assert!(!delete_device(dev));
        (*dev).attached_to = None;
        assert!(delete_device(dev));
        assert!(d.device_object.is_null());
    }
}
fn stack() -> IoStackLocation {
    IoStackLocation {
        major_function: 0,
        minor_function: 0,
        flags: 0,
        control: 0,
        device_object: null_mut(),
        file_object: null_mut(),
        parameters: IoParameters { as_u64: 0 },
        completion_routine: None,
        completion_context: null_mut(),
    }
}
fn irp(stack: *mut IoStackLocation, count: u8) -> Irp {
    Irp {
        io_status: IoStatus {
            status: 0,
            information: 99,
        },
        current_stack: stack,
        current_location: count,
        stack_count: count,
        flags: 0,
        pending_returned: 0,
        user_buffer: null_mut(),
        completed: 0,
    }
}
#[test]
fn missing_dispatch_completes_once_without_stale_byte_count() {
    let mut name = [72u16];
    let mut d = driver(&mut name);
    let dev = device(&mut *d, 0);
    let mut s = stack();
    let mut r = irp(&mut s, 1);
    unsafe {
        assert!(dispatch::DispatchTable::new()
            .major_functions
            .iter()
            .all(Option::is_none));
        assert_eq!(
            dispatch::dispatch_irp(dev, &mut r),
            STATUS_INVALID_DEVICE_REQUEST
        );
        assert_eq!(r.completed, 1);
        assert_eq!(r.io_status.information, 0);
        drop(Box::from_raw(dev));
    }
}
unsafe extern "C" fn completion(_: *mut DeviceObject, _: *mut Irp, _: *mut ()) -> i32 {
    0
}
#[test]
fn dispatch_attachment_pins_lower_and_rejects_replacement_and_cycles() {
    let mut name = [74u16];
    let mut d = driver(&mut name);
    let upper = device(&mut *d, 0);
    let lower = device(&mut *d, 0);
    let other = device(&mut *d, 0);
    unsafe {
        (*upper).next_device = lower;
        (*lower).next_device = other;
        d.device_object = upper;

        dispatch::attach_device(upper, upper);
        assert!((*upper).attached_device.is_null());
        assert!((*upper).attached_to.is_none());

        dispatch::attach_device(upper, lower);
        assert_eq!((*upper).attached_device, lower);
        assert_eq!((*lower).attached_to, Some(upper));
        assert!(!delete_device(lower));
        assert!(!delete_device(upper));

        dispatch::attach_device(upper, other); // would strand lower's backlink
        dispatch::attach_device(other, lower); // would strand upper's forward link
        dispatch::attach_device(lower, upper); // would close a two-device cycle
        assert_eq!((*upper).attached_device, lower);
        assert_eq!((*lower).attached_to, Some(upper));
        assert!((*lower).attached_device.is_null());
        assert!((*upper).attached_to.is_none());
        assert!((*other).attached_device.is_null());
        assert!((*other).attached_to.is_none());

        // Caller-serialized detach, then normal production deletion succeeds.
        (*upper).attached_device = null_mut();
        (*lower).attached_to = None;
        assert!(delete_device(lower));
        assert_eq!((*upper).next_device, other);
        assert!(delete_device(upper));
        assert!(delete_device(other));
        assert!(d.device_object.is_null());
    }
}
#[test]
fn forwarding_checks_stack_and_does_not_duplicate_completion() {
    let mut slots = [stack(), stack()];
    slots[1].completion_routine = Some(completion);
    slots[1].control = 0x70;
    let mut r = irp(&mut slots[1], 2);
    unsafe {
        irp_stack::copy_current_irp_stack_location_to_next(&mut r);
        assert!(slots[0].completion_routine.is_none());
        assert_eq!(slots[0].control, 0);
        irp_stack::skip_current_stack_location(&mut r);
        assert!(irp_stack::get_next_stack_location(&mut r).is_null());
        let mut upper = Box::new(DeviceObject {
            device_queue: Queue,
            driver_queues: Queue,
            driver_object: null_mut(),
            next_device: null_mut(),
            ref_count: AtomicU32::new(1),
            current_irp: null_mut(),
            attached_device: 1usize as *mut _,
            attached_to: None,
            vpb: null_mut(),
            device_extension: null_mut(),
            namespace_handle: 0,
        });
        assert_eq!(
            dispatch::forward_irp_to_lower(&mut *upper, &mut r),
            STATUS_INVALID_DEVICE_REQUEST
        );
        assert_eq!(r.completed, 1);
    }
}
#[test]
fn completion_registration_replaces_invocation_flags() {
    let mut s = stack();
    s.control = 0x71;
    let mut r = irp(&mut s, 1);
    unsafe {
        irp_stack::set_completion_routine(&mut r, Some(completion), null_mut(), false, true, false);
    }
    assert_eq!(s.control, 0x21);
}

#[test]
fn completion_apc_owns_status_after_source_is_freed_and_queue_moves() {
    let _lock = TEST_LOCK.lock().unwrap();
    use apc::{ApcMode, IoStatusBlock};
    unsafe extern "C" fn delivered(context: *mut (), status: *mut IoStatusBlock, _: u32) {
        // Reentering the queue is safe because delivery releases its lock.
        assert_eq!(apc::get_apc_count(900).1, 0);
        let observed = &mut *(context as *mut (u32, usize));
        *observed = ((*status).status, (*status).information);
    }
    let mut observed = (0u32, 0usize);
    let source = Box::new(IoStatusBlock {
        status: 0x80000005,
        information: 712,
    });
    assert!(apc::insert_owned_io_apc(
        900,
        ApcMode::KernelMode,
        Some(delivered),
        &mut observed as *mut _ as *mut (),
        *source
    ));
    drop(source);
    // Force global queues Vec growth. No pointer may target an earlier slot.
    for thread in 1000..1100 {
        assert!(apc::insert_owned_io_apc(
            thread,
            ApcMode::UserMode,
            None,
            null_mut(),
            IoStatusBlock::new()
        ));
    }
    assert_eq!(apc::deliver_kernel_apcs(900), 1);
    assert_eq!(observed, (0x80000005, 712));
    assert_eq!(apc::deliver_kernel_apcs(900), 0);
    for thread in 1000..1100 {
        apc::clear_thread_apcs(thread);
    }
    apc::clear_thread_apcs(900);
}

#[test]
fn missing_unload_and_disabled_initializer_are_not_run() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [73u16];
    let mut d = driver(&mut name);
    d.driver_unload = None;
    unsafe {
        assert!(register_start_type_driver(
            &mut *d,
            ServiceStartType::DemandStart
        ));
        assert!(!unload_driver(&mut *d));
        d.driver_unload = Some(unload);
        assert!(unload_driver(&mut *d));
    }
    let mut name = [74u16];
    let mut d = driver(&mut name);
    d.driver_init = Some(failure);
    unsafe {
        assert!(register_start_type_driver(
            &mut *d,
            ServiceStartType::Disabled
        ));
        assert_eq!(
            initialize_registered_driver(&mut *d, &[]),
            STATUS_INVALID_DEVICE_REQUEST
        );
        assert!(d.device_object.is_null());
        assert!(unload_driver(&mut *d));
    }
}

unsafe extern "C" fn busy_failure(driver: *mut DriverObject, _: *mut u16) -> u32 {
    let idle = device(driver, 0);
    let busy = device(driver, 0);
    (*busy).next_device = idle;
    (*busy).ref_count.store(2, Ordering::Relaxed);
    (*driver).device_object = busy;
    STATUS_INVALID_DEVICE_REQUEST as u32
}
#[test]
fn failed_entry_keeps_busy_device_alive_and_rolls_back_other_devices() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut name = [75u16];
    let mut d = driver(&mut name);
    d.driver_init = Some(busy_failure);
    unsafe {
        assert!(register_start_type_driver(
            &mut *d,
            ServiceStartType::DemandStart
        ));
        assert_eq!(
            initialize_registered_driver(&mut *d, &[]),
            STATUS_INVALID_DEVICE_REQUEST
        );
        let busy = d.device_object;
        assert!(!busy.is_null());
        assert!((*busy).next_device.is_null());
        assert!(!unload_driver(&mut *d));
        (*busy).ref_count.store(1, Ordering::Relaxed);
        assert!(delete_device(busy));
        assert!(unload_driver(&mut *d));
    }
}

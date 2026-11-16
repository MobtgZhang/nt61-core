//! Host regression compiling the actual ETHREAD and thread termination code.
//! Scheduler, pool, security and unrelated subsystem boundaries are fixtures.
#![allow(dead_code, unused_imports, non_camel_case_types)]
#[macro_export]
macro_rules! kprintln {
    ($($arg:tt)*) => {};
}
extern crate alloc;
#[path = "../src/se/acl.rs"]
pub mod acl;
#[path = "../src/ps/effective_token.rs"]
pub mod effective_token;
#[path = "../src/ps/execution_request.rs"]
pub mod execution_request;
#[path = "../src/ps/impersonation.rs"]
pub mod impersonation;
#[path = "../src/se/token.rs"]
pub mod production_token;
#[path = "../src/se/sid.rs"]
pub mod sid;
#[path = "../src/se/subject_context.rs"]
pub mod subject_context;
#[path = "../src/ps/thread.rs"]
pub mod thread_code;
#[path = "../src/ps/thread_list.rs"]
pub mod thread_list;
#[path = "../src/ps/stack_reaper.rs"]
pub mod stack_reaper;
mod ps {
    pub use crate::{stack_reaper, thread_list};
    pub use crate::{execution_request, impersonation, thread_code as thread};
    pub mod process {
        use crate::thread_code::Ethread;
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct ListEntry {
            pub flink: *mut Self,
            pub blink: *mut Self,
        }
        impl ListEntry {
            pub const fn new() -> Self {
                Self {
                    flink: core::ptr::null_mut(),
                    blink: core::ptr::null_mut(),
                }
            }
            pub fn init(&mut self) {
                self.flink = self;
                self.blink = self;
            }
            pub unsafe fn insert_tail(&mut self, entry: *mut Self) {
                (*entry).flink = self;
                (*entry).blink = self.blink;
                (*self.blink).flink = entry;
                self.blink = entry;
            }
        }
        pub struct Eprocess {
            pub unique_process_id: u64,
            pub kprocess_thread_list_head: ListEntry,
            pub token: TokenFastRef,
        }
        pub struct TokenFastRef(pub u64);
        impl TokenFastRef {
            pub fn get_object(&self) -> u64 {
                self.0
            }
        }
        pub const PID_SYSTEM: u64 = 4;
        pub const STATUS_ACCESS_DENIED: u32 = 0xc0000022;
        pub const STATUS_INVALID_PARAMETER: u32 = 0xc000000d;
        pub fn get_by_pid(_: u64) -> Option<&'static mut Eprocess> {
            None
        }
        pub use crate::effective_token::get_current_thread_token;
        pub fn find_thread_in_process(_: *mut Eprocess, _: u64) -> Option<*mut Ethread> {
            None
        }
        pub struct ProcessList {
            pub process_count: usize,
            pub processes: [*mut Eprocess; 0],
        }
        static LIST: crate::ke::sync::Spinlock<ProcessList> =
            crate::ke::sync::Spinlock::new(ProcessList {
                process_count: 0,
                processes: [],
            });
        pub fn get_process_list_ptr() -> &'static crate::ke::sync::Spinlock<ProcessList> {
            &LIST
        }
    }
}
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
        #[repr(C)]
        #[derive(Clone)]
        pub struct DispatcherHeader {
            pub type_: u8,
            pub signal_state: u8,
            pub size: u16,
            pub inserted: u8,
            pub spare: [u8; 3],
            pub absolute: u8,
            pub co_started: u8,
            pub co_terminated: u8,
            pub inactive: u8,
            pub reserved: [u64; 2],
        }
        impl DispatcherHeader {
            pub const fn new(type_: u8) -> Self {
                Self {
                    type_,
                    signal_state: 0,
                    size: 0,
                    inserted: 0,
                    spare: [0; 3],
                    absolute: 0,
                    co_started: 0,
                    co_terminated: 0,
                    inactive: 0,
                    reserved: [0; 2],
                }
            }
        }
    }
    pub mod scheduler {
        pub const QUANTUM_DEFAULT: u32 = 6;
        pub enum Quantum { Standard }
        pub fn init_thread_quantum(thread: *mut crate::thread_code::Ethread, _: Quantum) {
            unsafe { (*thread).kthread.quantum_initial = 6; (*thread).kthread.quantum_remaining = 6; }
        }
        pub fn get_current_cpu() -> usize { 0 }
        pub unsafe fn add_ready(_: *mut crate::thread_code::Ethread, _: u8) {}
        pub unsafe fn remove_ready(_: *mut crate::thread_code::Ethread) -> bool {
            true
        }
        pub unsafe fn set_thread_affinity(_: *mut crate::thread_code::Ethread, _: u64) -> bool {
            true
        }
        pub fn schedule() -> bool {
            false
        }
        pub fn idle_loop() -> ! {
            panic!("unexpected idle loop")
        }
    }
    pub mod time {
        pub fn get_system_time() -> u64 {
            42
        }
    }
    pub mod bugcheck {
        pub enum BugCheckCode {
            ProcessInitializationFailed,
        }
        pub fn bugcheck_with(_: BugCheckCode, _: u64, _: u64, _: u64, _: u64) -> ! {
            panic!("unexpected bugcheck")
        }
    }
    pub mod apc {
        pub type ApcRoutine = unsafe extern "C" fn(*mut u8, *mut u8, *mut u8);
        pub enum ApcKind {
            User,
            Kernel,
        }
        pub fn queue_thread_apc(
            _: *mut crate::thread_code::Ethread,
            _: ApcKind,
            _: ApcRoutine,
            _: *mut u8,
            _: *mut u8,
            _: *mut u8,
            _: *mut u8,
        ) -> bool {
            false
        }
        pub fn deliver_thread_apc(_: *mut crate::thread_code::Ethread, _: ApcKind) -> bool {
            false
        }
        pub fn has_pending_apc(_: *mut crate::thread_code::Ethread, _: ApcKind) -> bool {
            false
        }
    }
}
mod libs {
    pub mod ntdll {
        pub mod alert_state {
            pub struct UserAlertState;
            impl UserAlertState {
                pub const fn new() -> Self {
                    Self
                }
            }
        }
        pub mod file {
            pub fn terminate_user_alert(_: &super::alert_state::UserAlertState) {}
            pub fn abandon_mutants(_: u64) {}
        }
    }
    pub mod wow64 {
        pub mod types {
            pub struct Teb32;
            pub struct Context32;
        }
    }
    pub mod win32k {
        pub mod input_services {
            pub fn release_thread(_: u64) {}
        }
        pub mod message {
            pub fn release_thread(_: u64) {}
        }
        pub mod raw_input {
            pub fn release_thread(_: u64) {}
        }
        pub mod window {
            pub mod session_services {
                pub fn release_thread(_: u64) {}
            }
        }
    }
}
mod arch {
    pub fn disable_interrupts() {}
    pub fn seed_thread_stack(_: u64, _: u64, _: u64) -> u64 {
        0
    }
    pub mod common {
        pub mod percpu {
            std::thread_local! { pub static CURRENT: core::cell::Cell<*mut crate::thread_code::Ethread> = const { core::cell::Cell::new(core::ptr::null_mut()) }; }
            pub fn get_current_thread() -> *mut crate::thread_code::Ethread {
                CURRENT.with(|current| current.get())
            }
        }
    }
}
mod hal {
    pub mod serial {
        pub fn write_string_always(_: &str) {}
    }
}
mod se {
    pub use crate::{acl, production_token as token, sid};
    pub mod seaccess {
        pub enum ObTypeIndex {
            Thread,
        }
        pub enum AccessCheckResult {
            Allowed,
            Denied,
        }
        pub struct SecurityDescriptor;
        impl SecurityDescriptor {
            pub fn new_null_dacl() -> *const Self {
                core::ptr::null()
            }
        }
        pub fn build_process_security_descriptor(
            _: *mut crate::ps::process::Eprocess,
        ) -> *const SecurityDescriptor {
            core::ptr::null()
        }
        pub fn se_access_check(
            _: ObTypeIndex,
            _: *const SecurityDescriptor,
            _: u32,
            _: *const super::token::Token,
        ) -> (AccessCheckResult, u32) {
            (AccessCheckResult::Denied, 0)
        }
    }
}
mod mm {
    pub mod pool {
        use std::{
            alloc::{alloc_zeroed, dealloc, Layout},
            collections::HashMap,
            sync::{
                atomic::{AtomicIsize, Ordering},
                Mutex, OnceLock,
            },
        };
        pub enum PoolType {
            NonPaged,
        }
        static ALLOCATIONS: OnceLock<Mutex<HashMap<usize, Layout>>> = OnceLock::new();
        pub static FAIL_AFTER: AtomicIsize = AtomicIsize::new(-1);
        fn allocations() -> &'static Mutex<HashMap<usize, Layout>> {
            ALLOCATIONS.get_or_init(Default::default)
        }
        pub fn live() -> usize {
            allocations().lock().unwrap().len()
        }
        pub fn owns(pointer: *const u8) -> bool {
            allocations()
                .lock()
                .unwrap()
                .contains_key(&(pointer as usize))
        }
        pub fn allocate(_: PoolType, size: usize) -> *mut u8 {
            allocate_aligned(PoolType::NonPaged, size, 16)
        }
        pub fn allocate_aligned(_: PoolType, size: usize, align: usize) -> *mut u8 {
            let remaining = FAIL_AFTER.load(Ordering::Relaxed);
            if remaining == 0 {
                return core::ptr::null_mut();
            }
            if remaining > 0 {
                FAIL_AFTER.fetch_sub(1, Ordering::Relaxed);
            }
            let layout = Layout::from_size_align(size, align).unwrap();
            let pointer = unsafe { alloc_zeroed(layout) };
            if !pointer.is_null() {
                allocations()
                    .lock()
                    .unwrap()
                    .insert(pointer as usize, layout);
            }
            pointer
        }
        pub fn free_aligned(pointer: *mut u8) { unsafe { free(pointer); } }
        pub unsafe fn free(pointer: *mut u8) {
            let layout = allocations()
                .lock()
                .unwrap()
                .remove(&(pointer as usize))
                .expect("invalid/double pool free");
            dealloc(pointer, layout);
        }
    }
}

use execution_request::*;
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
const ALL: u32 = ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED | ES_AWAYMODE_REQUIRED;

#[test]
fn exiting_worker_retains_its_stack_until_another_thread_dispatches() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut thread = thread_code::Ethread::new();
    let stack = mm::pool::allocate_aligned(mm::pool::PoolType::NonPaged, 0x20000, 16);
    thread.kthread.stack_limit = stack as u64;
    thread_code::THREAD_COUNT.store(1, std::sync::atomic::Ordering::SeqCst);
    arch::common::percpu::CURRENT.with(|current| current.set(&mut thread));
    // SAFETY: this test owns the active ETHREAD and its allocated stack.
    unsafe { thread_code::ps_terminate_system_thread(&mut thread, 0) };
    assert!(mm::pool::owns(stack), "termination freed the executing stack");
    stack_reaper::reap(0, stack as u64);
    assert!(mm::pool::owns(stack), "first dispatch still uses the outgoing stack");
    arch::common::percpu::CURRENT.with(|current| current.set(core::ptr::null_mut()));
    stack_reaper::reap(0, 0);
    assert!(!mm::pool::owns(stack), "next dispatch did not reclaim the retired stack");
    stack_reaper::reap(0, 0); // Repeated dispatch must not double-free.
}

#[test]
fn real_thread_termination_releases_live_request_before_signaling() {
    let _guard = TEST_LOCK.lock().unwrap();
    for terminate in [
        thread_code::terminate,
        thread_code::ps_terminate_system_thread,
    ] {
        let mut thread = thread_code::Ethread::new();
        assert_eq!(
            thread.execution_request.prepare_continuous(1),
            Ok(ES_CONTINUOUS)
        );
        assert_eq!(thread.execution_request.apply_continuous(ALL), Ok(()));
        assert_eq!(
            execution_request_counts(),
            ExecutionRequestCounts {
                display: 1,
                system: 1,
                away: 1
            }
        );
        thread_code::THREAD_COUNT.store(1, std::sync::atomic::Ordering::SeqCst);
        // SAFETY: this locally owned ETHREAD has no stack or linked entries.
        unsafe { terminate(&mut thread, 0) };
        assert_eq!(
            execution_request_counts(),
            ExecutionRequestCounts::default(),
            "termination leaked a live power request"
        );
        assert_eq!(thread.kthread.header.signal_state, 1);
        assert_eq!(
            thread.execution_request.apply_continuous(ALL),
            Err(ExecutionRequestError::Terminating)
        );
    }
}

#[test]
fn preparation_retains_previous_state_without_applying_a_request() {
    let _guard = TEST_LOCK.lock().unwrap();
    let owner = ThreadExecutionRequest::new();
    assert_eq!(owner.continuous_state(), ES_CONTINUOUS);
    assert_eq!(
        owner.apply_continuous(ALL),
        Err(ExecutionRequestError::NotPrepared)
    );
    assert_eq!(owner.prepare_continuous(7), Ok(ES_CONTINUOUS));
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts::default()
    );
    // Abandoning the apply step (for example because previous-state output
    // failed) retains empty ownership; a later request can reuse it.
    assert_eq!(owner.prepare_continuous(7), Ok(ES_CONTINUOUS));
    assert_eq!(
        owner.apply_continuous(ES_CONTINUOUS | ES_SYSTEM_REQUIRED),
        Ok(())
    );
    assert_eq!(
        owner.prepare_continuous(7),
        Ok(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)
    );
    owner.terminate();
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts::default()
    );
}

#[test]
fn replacement_and_repetition_change_only_owned_contributions() {
    let _guard = TEST_LOCK.lock().unwrap();
    let first = thread_code::Ethread::new();
    let second = thread_code::Ethread::new();
    first.execution_request.prepare_continuous(1).unwrap();
    second.execution_request.prepare_continuous(1).unwrap();
    first.execution_request.apply_continuous(ALL).unwrap();
    first.execution_request.apply_continuous(ALL).unwrap();
    second.execution_request.apply_continuous(ALL).unwrap();
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts {
            display: 2,
            system: 2,
            away: 2
        }
    );
    first
        .execution_request
        .apply_continuous(ES_CONTINUOUS | ES_DISPLAY_REQUIRED)
        .unwrap();
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts {
            display: 2,
            system: 1,
            away: 1
        }
    );
    second
        .execution_request
        .apply_continuous(ES_CONTINUOUS)
        .unwrap();
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts {
            display: 1,
            system: 0,
            away: 0
        }
    );
    first
        .execution_request
        .apply_continuous(ES_CONTINUOUS)
        .unwrap();
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts::default()
    );
}

#[test]
fn noninteractive_session_disables_only_the_display_contribution() {
    let _guard = TEST_LOCK.lock().unwrap();
    for session_id in [0, u32::MAX] {
        let owner = ThreadExecutionRequest::new();
        owner.prepare_continuous(session_id).unwrap();
        owner.apply_continuous(ALL).unwrap();
        assert_eq!(
            owner.continuous_state(),
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_AWAYMODE_REQUIRED
        );
        assert_eq!(
            execution_request_counts(),
            ExecutionRequestCounts {
                display: 0,
                system: 1,
                away: 1
            }
        );
        // Creation context is retained after the request has been attached.
        owner.prepare_continuous(1).unwrap();
        owner.apply_continuous(ALL).unwrap();
        assert_eq!(execution_request_counts().display, 0);
        owner.terminate();
    }
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts::default()
    );
}

#[test]
fn invalid_and_transient_flags_cannot_mutate_continuous_accounting() {
    let _guard = TEST_LOCK.lock().unwrap();
    let owner = ThreadExecutionRequest::new();
    owner.prepare_continuous(1).unwrap();
    owner.apply_continuous(ALL).unwrap();
    for flags in [
        0,
        ES_SYSTEM_REQUIRED,
        ES_AWAYMODE_REQUIRED,
        ES_CONTINUOUS | 4,
        ES_CONTINUOUS | 0x80,
    ] {
        assert_eq!(
            owner.apply_continuous(flags),
            Err(ExecutionRequestError::InvalidFlags)
        );
        assert_eq!(owner.continuous_state(), ALL);
        assert_eq!(
            execution_request_counts(),
            ExecutionRequestCounts {
                display: 1,
                system: 1,
                away: 1
            }
        );
    }
    owner.terminate();
}

#[test]
fn destruction_and_repeated_release_never_remove_another_threads_request() {
    let _guard = TEST_LOCK.lock().unwrap();
    let retained = thread_code::Ethread::new();
    retained.execution_request.prepare_continuous(1).unwrap();
    retained.execution_request.apply_continuous(ALL).unwrap();
    {
        let destroyed = thread_code::Ethread::new();
        destroyed.execution_request.prepare_continuous(1).unwrap();
        destroyed.execution_request.apply_continuous(ALL).unwrap();
    }
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts {
            display: 1,
            system: 1,
            away: 1
        }
    );
    retained.execution_request.terminate();
    retained.execution_request.terminate();
    assert_eq!(
        retained.execution_request.prepare_continuous(1),
        Err(ExecutionRequestError::Terminating)
    );
    assert_eq!(retained.execution_request.continuous_state(), ES_CONTINUOUS);
    drop(retained);
    assert_eq!(
        execution_request_counts(),
        ExecutionRequestCounts::default()
    );
}

#[test]
fn racing_application_and_termination_cannot_resurrect_requests() {
    let _guard = TEST_LOCK.lock().unwrap();
    for _ in 0..64 {
        let owner = std::sync::Arc::new(ThreadExecutionRequest::new());
        owner.prepare_continuous(1).unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let setter = {
            let owner = owner.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..64 {
                    if owner.apply_continuous(ALL) == Err(ExecutionRequestError::Terminating) {
                        break;
                    }
                    if owner.apply_continuous(ES_CONTINUOUS)
                        == Err(ExecutionRequestError::Terminating)
                    {
                        break;
                    }
                }
            })
        };
        let terminator = {
            let owner = owner.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                owner.terminate();
            })
        };
        start.wait();
        setter.join().unwrap();
        terminator.join().unwrap();
        assert_eq!(
            execution_request_counts(),
            ExecutionRequestCounts::default()
        );
        assert_eq!(
            owner.apply_continuous(ALL),
            Err(ExecutionRequestError::Terminating)
        );
    }
}

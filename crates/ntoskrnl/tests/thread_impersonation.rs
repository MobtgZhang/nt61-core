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
        pub unsafe fn find_thread_in_process(_: *mut Eprocess, _: u64) -> Option<*mut Ethread> {
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
        pub unsafe fn set_priority(thread: *mut crate::thread_code::Ethread, priority: i8) {
            (*thread).kthread.priority = priority;
        }
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
        pub unsafe fn queue_thread_apc(
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
        pub unsafe fn deliver_thread_apc(_: *mut crate::thread_code::Ethread, _: ApcKind) -> bool {
            false
        }
        pub unsafe fn has_pending_apc(_: *mut crate::thread_code::Ethread, _: ApcKind) -> bool {
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

#[path = "../../libs/ntdll/src/ntdll/privilege.rs"]
mod privilege;
use impersonation::*;
use production_token::{ImpersonationLevel, Token};
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn snapshot() -> ImpersonationReference {
    ImpersonationReference {
        token: ImpersonationTokenReference::copy_from(&Token::new_impersonation(
            sid::sid::well_known(sid::WellKnownSid::AnonymousLogon),
            ImpersonationLevel::SecurityImpersonation,
        ))
        .unwrap(),
        level: ImpersonationLevel::SecurityImpersonation,
        copy_on_open: true,
        effective_only: false,
    }
}

#[test]
fn real_thread_termination_releases_owned_token_before_signaling() {
    let _guard = TEST_LOCK.lock().unwrap();
    for terminate in [
        thread_code::terminate,
        thread_code::ps_terminate_system_thread,
    ] {
        let mut thread = thread_code::Ethread::new();
        thread.impersonation.replace(snapshot()).unwrap();
        assert_eq!(mm::pool::live(), 2);
        thread_code::THREAD_COUNT.store(1, std::sync::atomic::Ordering::SeqCst);
        unsafe { terminate(&mut thread, 0) };
        assert_eq!(
            mm::pool::live(),
            0,
            "thread termination leaked impersonation token"
        );
        assert_eq!(thread.kthread.header.signal_state, 1);
        assert_eq!(
            thread.impersonation.replace(snapshot()),
            Err(ImpersonationError::Terminating)
        );
        assert_eq!(mm::pool::live(), 0);
    }
}

#[test]
fn pin_retains_token_and_dacl_across_replace_revert_and_termination() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut source =
        Token::new_impersonation(sid::SID_USERS, ImpersonationLevel::SecurityImpersonation);
    let mut dacl = acl::Acl::new();
    source.default_dacl = &mut dacl;
    let token = ImpersonationTokenReference::copy_from(&source).unwrap();
    assert_ne!(token.token().default_dacl, source.default_dacl);
    let pointer = token.token() as *const Token;
    let dacl_pointer = token.token().default_dacl;
    let owner = ThreadImpersonation::new();
    owner
        .replace(ImpersonationReference {
            token,
            level: ImpersonationLevel::SecurityIdentification,
            copy_on_open: true,
            effective_only: true,
        })
        .unwrap();
    let pin = owner.reference().unwrap();
    assert!(pin.copy_on_open && pin.effective_only);
    assert_eq!(pin.level, ImpersonationLevel::SecurityIdentification);
    owner.replace(snapshot()).unwrap();
    assert!(mm::pool::owns(pointer.cast()));
    assert!(mm::pool::owns(dacl_pointer.cast()));
    owner.revert();
    assert!(owner.reference().is_none());
    owner.terminate();
    assert!(pin.token.token().user.equals(&sid::SID_USERS));
    assert_eq!(mm::pool::live(), 3);
    drop(pin);
    assert_eq!(mm::pool::live(), 0);
}

#[test]
fn real_ethread_drop_releases_owned_snapshot() {
    let _guard = TEST_LOCK.lock().unwrap();
    let thread = thread_code::Ethread::new();
    thread.impersonation.replace(snapshot()).unwrap();
    let pin = thread.impersonation.reference().unwrap();
    drop(thread);
    assert_eq!(mm::pool::live(), 2);
    drop(pin);
    assert_eq!(mm::pool::live(), 0);
}

#[test]
fn allocation_failure_rolls_back_each_owned_resource_and_keeps_previous_token() {
    let _guard = TEST_LOCK.lock().unwrap();
    let owner = ThreadImpersonation::new();
    owner.replace(snapshot()).unwrap();
    let mut source =
        Token::new_impersonation(sid::SID_USERS, ImpersonationLevel::SecurityImpersonation);
    let mut dacl = acl::Acl::new();
    source.default_dacl = &mut dacl;
    for fail_after in 0..3 {
        mm::pool::FAIL_AFTER.store(fail_after, std::sync::atomic::Ordering::Relaxed);
        assert!(matches!(
            ImpersonationTokenReference::copy_from(&source),
            Err(ImpersonationError::NoMemory)
        ));
        mm::pool::FAIL_AFTER.store(-1, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(mm::pool::live(), 2);
        assert!(owner
            .reference()
            .unwrap()
            .token
            .token()
            .user
            .equals(&sid::sid::well_known(sid::WellKnownSid::AnonymousLogon)));
    }
    owner.revert();
    assert_eq!(mm::pool::live(), 0);
    assert!(matches!(
        ImpersonationTokenReference::copy_from(&Token::new_primary(sid::SID_USERS)),
        Err(ImpersonationError::WrongTokenType)
    ));
}

#[test]
fn concurrent_reference_replace_revert_and_termination_never_resurrects() {
    let _guard = TEST_LOCK.lock().unwrap();
    for _ in 0..64 {
        let owner = std::sync::Arc::new(ThreadImpersonation::new());
        owner.replace(snapshot()).unwrap();
        let readers: Vec<_> = (0..4)
            .map(|_| {
                let owner = owner.clone();
                std::thread::spawn(move || {
                    for _ in 0..32 {
                        if let Some(pin) = owner.reference() {
                            assert_eq!(
                                pin.token.token().token_type,
                                production_token::TokenTypeEnum::TokenImpersonation
                            );
                        }
                        let _ = owner.replace(snapshot());
                        owner.revert();
                    }
                })
            })
            .collect();
        owner.terminate();
        for reader in readers {
            reader.join().unwrap();
        }
        assert!(owner.reference().is_none());
        assert_eq!(mm::pool::live(), 0);
    }
}

#[test]
fn production_effective_token_and_subject_capture_pin_replaced_identity() {
    let _guard = TEST_LOCK.lock().unwrap();
    let primary = production_token::create_system_token();
    let mut process = ps::process::Eprocess {
        unique_process_id: 4,
        kprocess_thread_list_head: ps::process::ListEntry::new(),
        token: ps::process::TokenFastRef(primary as u64),
    };
    let mut thread = thread_code::Ethread::new();
    thread.threads_process = &mut process;
    arch::common::percpu::CURRENT.with(|current| current.set(&mut thread));
    assert!(privilege::system_logon());
    thread.impersonation.replace(snapshot()).unwrap();
    assert!(!privilege::system_logon());
    assert!(!privilege::enabled(12));
    let effective = effective_token::get_current_thread_token();
    let mut context = subject_context::SecuritySubjectContext::capture();
    assert_eq!(
        context.impersonation_level,
        ImpersonationLevel::SecurityImpersonation
    );
    assert_eq!(
        context.client_token as *const Token,
        effective.as_ref().unwrap() as *const Token
    );
    thread
        .impersonation
        .replace(ImpersonationReference {
            token: snapshot().token,
            level: ImpersonationLevel::SecurityIdentification,
            copy_on_open: false,
            effective_only: false,
        })
        .unwrap();
    assert_eq!(
        context.impersonation_level,
        ImpersonationLevel::SecurityImpersonation
    );
    thread.impersonation.revert();
    assert!(privilege::system_logon());
    thread.impersonation.terminate();
    assert!(effective
        .as_ref()
        .unwrap()
        .user
        .equals(&sid::sid::well_known(sid::WellKnownSid::AnonymousLogon)));
    context.release();
    assert!(context.client_token.is_null());
    assert_eq!(mm::pool::live(), 3); // process token plus pinned snapshot/control
    drop(effective);
    arch::common::percpu::CURRENT.with(|current| current.set(core::ptr::null_mut()));
    unsafe {
        mm::pool::free(primary.cast());
    }
    assert_eq!(mm::pool::live(), 0);
}

#[test]
fn created_threads_keep_process_membership_when_ready_nodes_move() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut process = ps::process::Eprocess {
        unique_process_id: 77,
        kprocess_thread_list_head: ps::process::ListEntry::new(),
        token: ps::process::TokenFastRef(0),
    };
    process.kprocess_thread_list_head.init();
    let first = unsafe { thread_code::create_thread(&mut process, 4096) }.unwrap();
    let second = unsafe { thread_code::create_thread(&mut process, 4096) }.unwrap();
    let process_head = core::ptr::addr_of_mut!(process.kprocess_thread_list_head);
    let first_member = core::ptr::addr_of_mut!(first.thread_list_entry);
    let second_member = core::ptr::addr_of_mut!(second.thread_list_entry);
    // The real constructor must publish the ETHREAD process node, leaving the
    // KTHREAD scheduling node available for an independently ordered queue.
    assert_eq!(process.kprocess_thread_list_head.flink, first_member);
    assert_eq!(process.kprocess_thread_list_head.blink, second_member);
    let mut ready = ps::process::ListEntry::new();
    ready.init();
    unsafe {
        ready.insert_tail(core::ptr::addr_of_mut!(second.kthread.global_thread_list_entry));
        ready.insert_tail(core::ptr::addr_of_mut!(first.kthread.global_thread_list_entry));
        assert_eq!((*first_member).flink, second_member);
        assert_eq!((*second_member).flink, process_head);
        assert_eq!((*first_member).blink, process_head);
        assert_eq!((*second_member).blink, first_member);
        assert_eq!(thread_list::count(&mut process), 2);
        assert_eq!(thread_list::find(&mut process, first.client_id.unique_thread), Some(first as *mut _));
        assert_eq!(thread_list::find(&mut process, second.client_id.unique_thread), Some(second as *mut _));
        assert!(!thread_list::attach(&mut process, first), "duplicate attach must not splice a linked node");
        // Remove the ready nodes before calling termination. The scheduler
        // boundary in this host harness does not manipulate a real PRCB.
        ready.init();
        first.kthread.global_thread_list_entry.init();
        second.kthread.global_thread_list_entry.init();
        thread_code::terminate(first, 0);
        assert_eq!(process.kprocess_thread_list_head.flink, second_member);
        thread_code::ps_terminate_system_thread(second, 0);
        assert_eq!(process.kprocess_thread_list_head.flink, process_head);
        assert_eq!(process.kprocess_thread_list_head.blink, process_head);
        mm::pool::free((first as *mut thread_code::Ethread).cast());
        mm::pool::free((second as *mut thread_code::Ethread).cast());
    }
    assert_eq!(mm::pool::live(), 0);
}

#[test]
fn membership_rejects_wrong_owner_and_detach_is_idempotent() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut process = ps::process::Eprocess {
        unique_process_id: 70,
        kprocess_thread_list_head: ps::process::ListEntry::new(),
        token: ps::process::TokenFastRef(0),
    };
    let mut thread = thread_code::Ethread::new();
    thread.client_id.unique_thread = 900;
    thread.threads_process = &mut process;
    unsafe {
        assert!(!thread_list::attach(&mut process, &mut thread));
        thread.kthread.process = &mut process;
        assert!(thread_list::attach(&mut process, &mut thread));
        assert!(!thread_list::attach(&mut process, &mut thread));
        assert_eq!(thread_list::count(&mut process), 1);
        assert_eq!(thread_list::find(&mut process, 900), Some(&mut thread as *mut _));
        thread_list::detach(&mut thread);
        thread_list::detach(&mut thread);
        assert_eq!(thread_list::count(&mut process), 0);
        assert_eq!(thread_list::find(&mut process, 900), None);
        assert!(thread_list::attach(&mut process, &mut thread));
        thread_list::detach(&mut thread);
    }
}

#[test]
fn concurrent_membership_publication_lookup_and_removal_preserves_links() {
    let _guard = TEST_LOCK.lock().unwrap();
    let mut process = Box::new(ps::process::Eprocess {
        unique_process_id: 71,
        kprocess_thread_list_head: ps::process::ListEntry::new(),
        token: ps::process::TokenFastRef(0),
    });
    let process_address = (&mut *process as *mut ps::process::Eprocess) as usize;
    let mut threads: Vec<_> = (0..8).map(|id| {
        let mut thread = Box::new(thread_code::Ethread::new());
        thread.client_id.unique_thread = id + 1000;
        thread.threads_process = process_address as *mut _;
        thread.kthread.process = process_address as *mut _;
        thread
    }).collect();
    let addresses: Vec<_> = threads.iter_mut().map(|thread| (&mut **thread as *mut thread_code::Ethread) as usize).collect();
    // All owner allocations remain stable until every worker has joined.
    // Only production helpers access links while workers are running.
    std::thread::scope(|scope| {
        for (index, &address) in addresses.iter().enumerate() {
            scope.spawn(move || {
                for _ in 0..128 {
                    unsafe {
                        let process = process_address as *mut ps::process::Eprocess;
                        let thread = address as *mut thread_code::Ethread;
                        assert!(thread_list::attach(process, thread));
                        assert_eq!(thread_list::find(process, index as u64 + 1000), Some(thread));
                        let count = thread_list::count(process);
                        assert!((1..=8).contains(&count));
                        thread_list::detach(thread);
                    }
                }
            });
        }
    });
    unsafe { assert_eq!(thread_list::count(&mut *process), 0); }
    let head = core::ptr::addr_of_mut!(process.kprocess_thread_list_head);
    assert_eq!(process.kprocess_thread_list_head.flink, head);
    assert_eq!(process.kprocess_thread_list_head.blink, head);
}

#[test]
fn asynchronous_worker_keeps_submitting_identity_and_restores_its_token() {
    let _guard = TEST_LOCK.lock().unwrap();
    let primary = production_token::create_system_token();
    let mut process = ps::process::Eprocess {
        unique_process_id: 4,
        kprocess_thread_list_head: ps::process::ListEntry::new(),
        token: ps::process::TokenFastRef(primary as u64),
    };
    let mut caller = thread_code::Ethread::new();
    caller.threads_process = &mut process;
    arch::common::percpu::CURRENT.with(|current| current.set(&mut caller));
    // A primary token is copied, rather than borrowed by the queued request.
    let primary_request = effective_token::get_current_thread_token()
        .snapshot_for_worker().unwrap();
    assert_ne!(primary_request.token.token() as *const Token, primary);
    assert_eq!(primary_request.token.token().token_type,
        production_token::TokenTypeEnum::TokenImpersonation);
    caller.impersonation.replace(snapshot()).unwrap();
    let request = effective_token::get_current_thread_token()
        .snapshot_for_worker().unwrap();
    caller.impersonation.revert();
    let mut worker = thread_code::Ethread::new();
    worker.threads_process = &mut process;
    arch::common::percpu::CURRENT.with(|current| current.set(&mut worker));
    assert!(privilege::system_logon());
    impersonation::with_current_thread_token(request, || {
        let effective = effective_token::get_current_thread_token();
        assert!(effective.as_ref().unwrap().user.equals(
            &sid::sid::well_known(sid::WellKnownSid::AnonymousLogon)));
        assert!(!privilege::system_logon());
    }).unwrap();
    assert!(privilege::system_logon());
    assert!(worker.impersonation.reference().is_none());
    // Existing worker impersonation also survives a failed/unwinding command.
    worker.impersonation.replace(snapshot()).unwrap();
    let original = worker.impersonation.reference().unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = impersonation::with_current_thread_token(primary_request, || {
            assert!(privilege::system_logon());
            panic!("command failed");
        });
    }));
    assert!(failure.is_err());
    assert_eq!(worker.impersonation.reference().unwrap().token.token() as *const Token,
        original.token.token() as *const Token);
    drop(original);
    worker.impersonation.revert();
    arch::common::percpu::CURRENT.with(|current| current.set(core::ptr::null_mut()));
    unsafe { mm::pool::free(primary.cast()); }
    assert_eq!(mm::pool::live(), 0);
}

#[test]
fn worker_token_scope_without_current_thread_never_runs_action() {
    let _guard = TEST_LOCK.lock().unwrap();
    arch::common::percpu::CURRENT.with(|current| current.set(core::ptr::null_mut()));
    let result = impersonation::with_current_thread_token(snapshot(), || {
        panic!("action must not run without a live current thread");
    });
    assert_eq!(result, Err(ImpersonationError::NoCurrentThread));
    assert_eq!(mm::pool::live(), 0);
}

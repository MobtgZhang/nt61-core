//! Exercise the actual two-phase sender and native validation with simulated
//! CPUs. Delayed remote maintenance must keep the calling service blocked.
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

static PAUSE: AtomicBool = AtomicBool::new(true);
static ENTERED: AtomicBool = AtomicBool::new(false);
static FETCHES: AtomicUsize = AtomicUsize::new(0);
mod arch {
    thread_local! {
        pub static CPU: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        pub static INTERRUPTS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    }
    pub fn interrupts_enabled() -> bool { INTERRUPTS.with(|v| v.get()) }
    pub fn disable_interrupts() { INTERRUPTS.with(|v| v.set(false)); }
    pub fn enable_interrupts() { INTERRUPTS.with(|v| v.set(true)); }
    pub fn send_tlb_shootdown_ipi() {} // Timer polling below supplies delivery.
    pub mod common {
        pub mod percpu {
            pub fn get_current_cpu_id() -> u32 { super::super::CPU.with(|v| v.get()) }
        }
        pub mod smp { pub fn cpu_count() -> u32 { 4 } }
        pub mod instruction_cache {
            pub fn publish_code_stores() {
                assert!(!super::super::interrupts_enabled());
                if super::percpu::get_current_cpu_id() == 1 {
                    crate::ENTERED.store(true, std::sync::atomic::Ordering::Release);
                    while crate::PAUSE.load(std::sync::atomic::Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                }
            }
            pub fn synchronize_code_fetch() {
                assert!(!super::super::interrupts_enabled());
                // A delayed data clean on any CPU must prevent *all* fetches.
                assert!(!crate::PAUSE.load(std::sync::atomic::Ordering::Acquire));
                crate::FETCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}
#[path = "../src/ke/instruction_shootdown.rs"]
pub mod instruction_shootdown;
pub mod ke {
    pub use crate::instruction_shootdown;
    pub mod scheduler { pub fn get_current_cpu_id() -> u32 { 0 } }
    pub mod tlb_shootdown { pub fn flush_process_write_buffers() {} }
}
pub mod mm { pub mod constants { pub const USER_LIMIT: u64 = 0x7fff_ffff_ffff; } }
#[path = "../../libs/ntdll/src/ntdll/system_services.rs"]
pub mod system_services;
pub mod ntdll {
    pub use crate::system_services;
    pub mod types { pub type HANDLE = *mut core::ffi::c_void; pub type PVOID = *mut core::ffi::c_void; }
    pub mod status {
        pub const STATUS_SUCCESS: i32 = 0;
        pub const STATUS_INVALID_HANDLE: i32 = 0xc0000008u32 as i32;
        pub const STATUS_ACCESS_DENIED: i32 = 0xc0000022u32 as i32;
        pub const STATUS_ACCESS_VIOLATION: i32 = 0xc0000005u32 as i32;
        pub const STATUS_OBJECT_TYPE_MISMATCH: i32 = 0xc0000024u32 as i32;
    }
    pub mod file {
        use super::{status::*, types::HANDLE};
        #[derive(PartialEq)] pub enum HandleKind { Process, Event }
        pub struct Handle { pub kind: HandleKind }
        pub fn with_sync_access<T>(handle: HANDLE, access: u32,
            operation: impl FnOnce(&mut [Handle]) -> Result<T, i32>) -> Result<T, i32> {
            assert_eq!(access, 0x20);
            match handle as usize {
                1 => operation(&mut [Handle { kind: HandleKind::Process }]),
                2 => Err(STATUS_ACCESS_DENIED),
                3 => operation(&mut [Handle { kind: HandleKind::Event }]),
                _ => Err(STATUS_INVALID_HANDLE),
            }
        }
    }
}
// Production module uses sibling NT library modules.
pub use ntdll::{file, status, types};

#[test]
fn remote_acknowledgements_validation_and_concurrent_senders() {
    use ntdll::{status::*, system_services::nt_flush_instruction_cache as flush};
    let null = core::ptr::null_mut();
    assert_eq!(flush(null, null, 0), STATUS_INVALID_HANDLE);
    assert_eq!(flush(2usize as _, null, 0), STATUS_ACCESS_DENIED);
    assert_eq!(flush(3usize as _, null, 0), STATUS_OBJECT_TYPE_MISMATCH);
    assert_eq!(flush(usize::MAX as _, usize::MAX as _, 1), STATUS_ACCESS_VIOLATION);
    assert_eq!(flush(null, 1usize as _, 0), STATUS_SUCCESS);
    assert_eq!(FETCHES.load(Ordering::Relaxed), 0);

    let stop = Arc::new(AtomicBool::new(false));
    let workers: Vec<_> = (1..4).map(|cpu| {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            arch::CPU.with(|v| v.set(cpu));
            while !stop.load(Ordering::Acquire) {
                instruction_shootdown::service_pending();
                std::thread::yield_now();
            }
            assert!(arch::interrupts_enabled());
        })
    }).collect();
    let (send, receive) = mpsc::channel();
    let sender = std::thread::spawn(move || {
        assert_eq!(flush(1usize as _, 1usize as _, 100), STATUS_SUCCESS);
        assert!(arch::interrupts_enabled());
        send.send(()).unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !ENTERED.load(Ordering::Acquire) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(FETCHES.load(Ordering::Relaxed), 0);
    assert!(receive.recv_timeout(Duration::from_millis(30)).is_err());
    PAUSE.store(false, Ordering::Release);
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    sender.join().unwrap();
    assert_eq!(FETCHES.load(Ordering::Relaxed), 4);
    stop.store(true, Ordering::Release);
    for worker in workers { worker.join().unwrap(); }

    // Every CPU now sends at once with interrupts disabled. Progress requires
    // senders to service each other's requests instead of waiting on IPIs.
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let finished = Arc::new(AtomicUsize::new(0));
    let mut senders = Vec::new();
    for cpu in 0..4 {
        let barrier = Arc::clone(&barrier);
        let finished = Arc::clone(&finished);
        senders.push(std::thread::spawn(move || {
            arch::CPU.with(|v| v.set(cpu));
            arch::disable_interrupts();
            barrier.wait();
            for _ in 0..20 { instruction_shootdown::flush_all(); }
            finished.fetch_add(1, Ordering::AcqRel);
            while finished.load(Ordering::Acquire) < 4 {
                instruction_shootdown::service_pending();
                std::thread::yield_now();
            }
            assert!(!arch::interrupts_enabled());
        }));
    }
    for sender in senders { sender.join().unwrap(); }
}

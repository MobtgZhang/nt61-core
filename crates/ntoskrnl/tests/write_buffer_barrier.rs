//! Exercise the production generation protocol with independently scheduled
//! CPUs. Architecture hooks supply IPIs and local flushes, not acknowledgements.
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

#[path = "../src/ke/tlb_shootdown.rs"]
pub mod shootdown;
#[path = "../src/ke/instruction_shootdown.rs"]
pub mod instruction_shootdown;

static TEST_LOCK: Mutex<()> = Mutex::new(());
static CPUS: AtomicU32 = AtomicU32::new(1);
static REQUESTS: AtomicU64 = AtomicU64::new(0);
static FLUSHES: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static INVALIDATED: AtomicU64 = AtomicU64::new(0);
static STOP: AtomicBool = AtomicBool::new(false);

mod arch {
    pub fn interrupts_enabled() -> bool { false }
    pub fn disable_interrupts() {}
    pub fn enable_interrupts() {}
    pub mod common {
        pub mod instruction_cache {
            pub fn publish_code_stores() {
                core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            }
            pub fn synchronize_code_fetch() {
                core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            }
        }
        pub mod percpu {
            thread_local! { pub static CPU: core::cell::Cell<u32> = const { core::cell::Cell::new(0) }; }
            pub fn get_current_cpu_id() -> u32 {
                CPU.with(|cpu| cpu.get())
            }
        }
        pub mod smp {
            pub fn cpu_count() -> u32 {
                crate::CPUS.load(core::sync::atomic::Ordering::Acquire)
            }
        }
    }
    pub fn invalidate_tlb(address: u64) {
        crate::INVALIDATED.store(address, core::sync::atomic::Ordering::Release);
    }
    pub fn flush_tlb() {
        crate::FLUSHES[common::percpu::get_current_cpu_id() as usize]
            .fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
    pub fn send_tlb_shootdown_ipi() {
        crate::REQUESTS.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

fn worker(cpu: u32) {
    arch::common::percpu::CPU.with(|id| id.set(cpu));
    while !STOP.load(Ordering::Acquire) {
        if cpu % 2 == 0 {
            shootdown::lazy_flush_if_pending();
        } else {
            shootdown::handle_shootdown_interrupt();
        }
        std::thread::yield_now();
    }
}

#[test]
fn single_cpu_mapping_invalidation_flushes_the_requested_address_without_ipis() {
    let _guard = TEST_LOCK.lock().unwrap();
    CPUS.store(1, Ordering::Release);
    arch::common::percpu::CPU.with(|id| id.set(0));
    let requests = REQUESTS.load(Ordering::Acquire);
    shootdown::invalidate_va_all_cpus(0x1234_5000);
    assert_eq!(INVALIDATED.load(Ordering::Acquire), 0x1234_5000);
    assert_eq!(REQUESTS.load(Ordering::Acquire), requests);
}

#[test]
fn a_delayed_cpu_must_acknowledge_before_the_native_flush_returns() {
    let _guard = TEST_LOCK.lock().unwrap();
    CPUS.store(2, Ordering::Release);
    STOP.store(false, Ordering::Release);
    let initial = REQUESTS.load(Ordering::Acquire);
    let (sender, receiver) = mpsc::channel();
    let caller = std::thread::spawn(move || {
        shootdown::flush_process_write_buffers();
        sender.send(()).unwrap();
    });
    while REQUESTS.load(Ordering::Acquire) == initial {
        std::thread::yield_now();
    }
    assert!(receiver.recv_timeout(Duration::from_millis(20)).is_err());
    let remote = std::thread::spawn(|| worker(1));
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    STOP.store(true, Ordering::Release);
    caller.join().unwrap();
    remote.join().unwrap();
    assert!(FLUSHES[1].load(Ordering::Acquire) != 0);
    assert_eq!(shootdown::stats().1, 0);
    CPUS.store(1, Ordering::Release);
}

#[test]
fn simultaneous_senders_service_each_other_without_an_irq_disabled_lock() {
    let _guard = TEST_LOCK.lock().unwrap();
    CPUS.store(4, Ordering::Release);
    STOP.store(false, Ordering::Release);
    let remotes = [2, 3].map(|cpu| std::thread::spawn(move || worker(cpu)));
    let target = shootdown::stats().0 + 400;
    let callers = [0, 1].map(|cpu| {
        std::thread::spawn(move || {
            arch::common::percpu::CPU.with(|id| id.set(cpu));
            for _ in 0..200 {
                shootdown::flush_process_write_buffers();
            }
            // Keep servicing the other sender until both are done.
            worker(cpu);
        })
    });
    // The protocol records each completed request. Avoid stopping one caller
    // while the other still needs its final acknowledgement.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while shootdown::stats().0 < target {
        assert!(std::time::Instant::now() < deadline, "concurrent flush deadlock");
        std::thread::yield_now();
    }
    STOP.store(true, Ordering::Release);
    for caller in callers {
        caller.join().unwrap();
    }
    for remote in remotes {
        remote.join().unwrap();
    }
    CPUS.store(1, Ordering::Release);
}

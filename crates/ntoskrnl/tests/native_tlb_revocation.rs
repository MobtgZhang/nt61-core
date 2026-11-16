//! Actual shootdown sender/acknowledgement loop with delayed simulated CPUs.
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

static CPUS: AtomicU32 = AtomicU32::new(1);
static FLUSHES: AtomicUsize = AtomicUsize::new(0);
mod arch {
    thread_local! {
        pub static CPU: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static INTERRUPTS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    }
    pub fn interrupts_enabled() -> bool {
        INTERRUPTS.with(|v| v.get())
    }
    pub fn disable_interrupts() {
        INTERRUPTS.with(|v| v.set(false));
    }
    pub fn enable_interrupts() {
        INTERRUPTS.with(|v| v.set(true));
    }
    pub fn invalidate_tlb(_: u64) {}
    pub fn flush_tlb() {
        assert!(!interrupts_enabled());
        crate::FLUSHES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
    pub fn send_tlb_shootdown_ipi() {} // Timer polling supplies delivery.
    pub mod common {
        pub mod percpu {
            pub fn get_current_cpu_id() -> u32 {
                super::super::CPU.with(|v| v.get())
            }
        }
        pub mod smp {
            pub fn cpu_count() -> u32 {
                crate::CPUS.load(std::sync::atomic::Ordering::Acquire)
            }
        }
    }
}
mod instruction_shootdown {
    pub fn service_pending() {}
}
#[path = "../src/ke/tlb_shootdown.rs"]
mod tlb_shootdown;

#[test]
fn revoke_waits_for_remote_cpus_and_concurrent_senders_make_progress() {
    // UP still needs a full flush to evict retained non-current ASIDs.
    tlb_shootdown::invalidate_va_all_cpus_complete(4096);
    assert_eq!(FLUSHES.load(Ordering::Acquire), 1);
    assert!(arch::interrupts_enabled());

    CPUS.store(4, Ordering::Release);
    let (send, receive) = mpsc::channel();
    let sender = std::thread::spawn(move || {
        tlb_shootdown::invalidate_va_all_cpus_complete(4096);
        send.send(()).unwrap();
    });
    assert!(
        receive.recv_timeout(Duration::from_millis(30)).is_err(),
        "protection revocation returned with three stale remote CPUs"
    );
    let stop = Arc::new(AtomicBool::new(false));
    let workers: Vec<_> = (1..4)
        .map(|cpu| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                arch::CPU.with(|v| v.set(cpu));
                while !stop.load(Ordering::Acquire) {
                    tlb_shootdown::lazy_flush_if_pending();
                    std::thread::yield_now();
                }
            })
        })
        .collect();
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    sender.join().unwrap();
    stop.store(true, Ordering::Release);
    for worker in workers {
        worker.join().unwrap();
    }

    // Senders entered with IRQs disabled must still acknowledge each other.
    let barrier = Arc::new(Barrier::new(4));
    let finished = Arc::new(AtomicUsize::new(0));
    let (send, receive) = mpsc::channel();
    let workers: Vec<_> = (0..4)
        .map(|cpu| {
            let barrier = Arc::clone(&barrier);
            let finished = Arc::clone(&finished);
            let send = send.clone();
            std::thread::spawn(move || {
                arch::CPU.with(|v| v.set(cpu));
                arch::disable_interrupts();
                barrier.wait();
                for page in 1..=20 {
                    tlb_shootdown::invalidate_va_all_cpus_complete(page * 4096);
                }
                finished.fetch_add(1, Ordering::AcqRel);
                let deadline = Instant::now() + Duration::from_secs(5);
                while finished.load(Ordering::Acquire) < 4 {
                    assert!(Instant::now() < deadline);
                    tlb_shootdown::lazy_flush_if_pending();
                    std::thread::yield_now();
                }
                assert!(!arch::interrupts_enabled());
                send.send(()).unwrap();
            })
        })
        .collect();
    for _ in 0..4 {
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    for worker in workers {
        worker.join().unwrap();
    }
}

//! Rendezvous for whole-machine instruction-cache synchronization. All online
//! CPUs stay in the kernel with interrupts disabled until data cleaning and
//! instruction synchronization finish. In particular no user CPU can migrate
//! dirty code lines between caches while another CPU sweeps its cache sets.
//! CPU membership is fixed after SMP bootstrap; CPU hotplug is not supported.
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MAX_CPUS: usize = 64;
static BUSY: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static ENTERED: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static CLEAN_DONE: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static FETCH_DONE: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

fn wait_for_all(done: &[AtomicU64; MAX_CPUS], generation: u64) {
    let count = crate::arch::common::smp::cpu_count() as usize;
    assert!((1..=MAX_CPUS).contains(&count));
    while done.iter().filter(|value| value.load(Ordering::Acquire) >= generation).count() < count {
        core::hint::spin_loop();
    }
}

/// IPI and timer fallback entry. A competing sender also enters this rendezvous
/// while waiting for ownership, even if its syscall entry disabled interrupts.
/// No dispatcher, allocator, process, or object lock may be held by the sender.
pub fn service_pending() {
    let restore = crate::arch::interrupts_enabled();
    crate::arch::disable_interrupts();
    let cpu = crate::arch::common::percpu::get_current_cpu_id() as usize;
    assert!(cpu < MAX_CPUS);
    let generation = GENERATION.load(Ordering::Acquire);
    if FETCH_DONE[cpu].load(Ordering::Acquire) < generation {
        ENTERED[cpu].store(generation, Ordering::Release);
        wait_for_all(&ENTERED, generation);
        crate::arch::common::instruction_cache::publish_code_stores();
        CLEAN_DONE[cpu].store(generation, Ordering::Release);
        wait_for_all(&CLEAN_DONE, generation);
        crate::arch::common::instruction_cache::synchronize_code_fetch();
        FETCH_DONE[cpu].store(generation, Ordering::Release);
        wait_for_all(&FETCH_DONE, generation);
    }
    if restore { crate::arch::enable_interrupts(); }
}

/// Flush all code caches, including aliases and other process address spaces.
/// The ownership gate serializes publications, but waiting senders participate
/// in the active rendezvous, so they never withhold an acknowledgement.
/// Interrupt handlers must only call service_pending(), never this function.
pub fn flush_all() {
    // Keep the gate owner runnable: it must not be preempted by another
    // syscall sender on the same CPU between acquisition and publication.
    let restore = crate::arch::interrupts_enabled();
    crate::arch::disable_interrupts();
    while BUSY.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        service_pending();
        core::hint::spin_loop();
    }
    GENERATION.try_update(Ordering::AcqRel, Ordering::Acquire, |value| value.checked_add(1))
        .expect("instruction cache generation exhausted");
    crate::arch::send_tlb_shootdown_ipi();
    service_pending();
    BUSY.store(false, Ordering::Release);
    if restore { crate::arch::enable_interrupts(); }
}

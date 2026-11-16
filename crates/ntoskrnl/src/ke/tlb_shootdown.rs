//! Cross-core TLB shootdown.
//!
//! A page-table change (COW copy, protection downgrade, unmap) is only
//! complete once every CPU that may hold a stale translation has
//! dropped it. A local `invlpg` / `tlbi VAE1` leaves the other cores'
//! TLBs untouched, so on SMP a thread on another core can keep writing
//! through an old writable (or reading a removed) mapping after the
//! PTE was rewritten.
//!
//! Each sender publishes a generation after its stores. Targets sample that
//! generation *before* a full local flush and memory fence, then acknowledge
//! only the sampled generation. This also permits concurrent senders: a later
//! acknowledgement subsumes every earlier request without a sender lock that
//! could prevent another CPU from servicing IPIs.
//!
//! Page-table callers retain the historical bounded wait and lazy fallback.
//! Permission revocation and the native process write-buffer service use an
//! unbounded acknowledgement wait; both require remote completion.

use core::sync::atomic::{AtomicU64, Ordering};

/// Maximum CPUs tracked for shootdown acknowledgement. Matches
/// `arch::common::percpu::MAX_PER_CPU`.
const MAX_SHOOTDOWN_CPUS: usize = 64;

/// Current request, published after the sender's preceding stores.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Highest generation each CPU has fully flushed. Indexed by the
/// per-CPU id (`percpu::get_current_cpu_id()`), which is stable for
/// the life of a CPU.
static DONE: [AtomicU64; MAX_SHOOTDOWN_CPUS] = [const { AtomicU64::new(0) }; MAX_SHOOTDOWN_CPUS];

/// How long the sender spins waiting for a remote acknowledgement
/// before falling back to the lazy tick flush. Generous, but bounded
/// so a wedge target cannot hang the shooter forever.
const ACK_SPIN_LIMIT: u64 = 20_000_000;

/// Statistics for diagnostics.
static SHOOTDOWN_COUNT: AtomicU64 = AtomicU64::new(0);
static TIMEOUT_COUNT: AtomicU64 = AtomicU64::new(0);

fn this_cpu_index() -> usize {
    let idx = crate::arch::common::percpu::get_current_cpu_id() as usize;
    if idx >= MAX_SHOOTDOWN_CPUS {
        0
    } else {
        idx
    }
}

/// Full local TLB flush plus per-CPU progress publication. This is
/// the body both the IPI handler and the lazy tick fallback run.
fn local_flush_and_publish() {
    // Keep the sampled CPU and the acknowledged CPU identical across a timer
    // preemption. Interrupt handlers already enter with interrupts disabled.
    let restore_interrupts = crate::arch::interrupts_enabled();
    crate::arch::disable_interrupts();
    // Loading after the flush could acknowledge a request published while
    // this CPU was flushing, without executing a fence after its publication.
    let gen = GENERATION.load(Ordering::Acquire);
    core::sync::atomic::fence(Ordering::SeqCst);
    crate::arch::flush_tlb();
    core::sync::atomic::fence(Ordering::SeqCst);
    let idx = this_cpu_index();
    DONE[idx].fetch_max(gen, Ordering::AcqRel);
    if restore_interrupts {
        crate::arch::enable_interrupts();
    }
}

/// Called from the architecture IPI/SGI handler. Flushes this CPU's
/// TLB and acknowledges the shootdown.
pub fn handle_shootdown_interrupt() {
    super::instruction_shootdown::service_pending();
    local_flush_and_publish();
}

/// Called from the timer tick on every CPU. If this CPU has not yet
/// acknowledged the current generation (e.g. it was inside a
/// critical section when the IPI arrived), catch up now.
pub fn lazy_flush_if_pending() {
    super::instruction_shootdown::service_pending();
    let idx = this_cpu_index();
    if DONE[idx].load(Ordering::Acquire) < GENERATION.load(Ordering::Acquire) {
        local_flush_and_publish();
    }
}

/// Invalidate `va` on every CPU: local entry invalidation plus a
/// cross-core shootdown with acknowledgement.
///
/// Use this at every point where a PTE is rewritten in a way other
/// CPUs could observe stale state: COW PTE replacement, protection
/// downgrades, and unmaps. Plain allocation-time map-ins do not need
/// it (a stale "not present" entry just re-faults).
pub fn invalidate_va_all_cpus(va: u64) {
    // Local entry first: the calling CPU is about to keep running
    // with the new PTE.
    crate::arch::invalidate_tlb(va);

    request_all_cpus(false);
}

/// Revoke a user permission before returning, with every online CPU's
/// acknowledgement. Call without locks needed by remote interrupt handlers.
pub fn invalidate_va_all_cpus_complete(va: u64) {
    crate::arch::invalidate_tlb(va);
    // Also discard non-current ASIDs on architectures retaining them. This
    // must happen even on UP, where request_all_cpus has no remote work.
    local_flush_and_publish();
    request_all_cpus(true);
}

/// Execute a full memory fence on every online processor before returning.
/// Called at passive level with no dispatcher/object locks held. While waiting
/// a concurrent sender also services incoming generations locally, even when
/// entered with hardware interrupts disabled by the syscall entry path.
pub fn flush_process_write_buffers() {
    core::sync::atomic::fence(Ordering::SeqCst);
    request_all_cpus(true);
    core::sync::atomic::fence(Ordering::SeqCst);
}

fn request_all_cpus(require_completion: bool) {
    let cpu_count = crate::arch::common::smp::cpu_count() as usize;
    if cpu_count <= 1 {
        return;
    }

    let gen = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    // Publish our target before the IPI goes out so a target that
    // services the interrupt immediately can observe the generation.
    core::sync::atomic::fence(Ordering::SeqCst);
    crate::arch::send_tlb_shootdown_ipi();

    // The local CPU has already invalidated the single VA; publish that
    // fact before waiting. A full flush is harmless and also catches
    // any adjacent mapping established while this request was in flight.
    local_flush_and_publish();

    let mut spin = 0u64;
    loop {
        let mut acked = 0;
        for idx in 0..MAX_SHOOTDOWN_CPUS {
            if DONE[idx].load(Ordering::Acquire) >= gen {
                acked += 1;
            }
        }
        // `DONE` starts at zero for every CPU, and the local CPU has
        // already published via the IPI path or will publish through
        // `lazy_flush_if_pending`; count the CPUs that reached `gen`.
        if acked >= cpu_count {
            SHOOTDOWN_COUNT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // No sender waits while withholding its own acknowledgement.
        lazy_flush_if_pending();
        spin += 1;
        if !require_completion && spin >= ACK_SPIN_LIMIT {
            TIMEOUT_COUNT.fetch_add(1, Ordering::Relaxed);
            // One or more CPUs are still behind. Do not spin forever:
            // their next timer tick runs `lazy_flush_if_pending` and
            // completes the flush. Bounded staleness beats a hang.
            return;
        }
        core::hint::spin_loop();
    }
}

/// Diagnostics: (successful shootdowns, senders that timed out).
pub fn stats() -> (u64, u64) {
    (
        SHOOTDOWN_COUNT.load(Ordering::Relaxed),
        TIMEOUT_COUNT.load(Ordering::Relaxed),
    )
}

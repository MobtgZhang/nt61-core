//! PSSS worker threads
//
//! NT 6.1 launches three system worker threads early in the
//! boot:
//
//!   - **BalanceSetManager** — wakes once per second to run
//!     the working-set trimmer and to refill the zeroed
//!     page list.
//!   - **MiZeroPageThread** — idles, popping free pages and
//!     pre-zeroing them. The demand-zero fault handler
//!     consumes from the zeroed list.
//!   - **MiModifiedPageWriter** — pops modified pages and
//!     writes them to the page file.
//
//!     NT 6.1 runs these as real system threads at PASSIVE_LEVEL.
//!     We create actual system threads via `ps_create_system_thread`
//!     during `psss::init()`; the timer tick only walks the
//!     dispatcher wait list (a cheap, ISR-safe operation).

use crate::ke::dispatch;

/// Timer-tick callback: walk the dispatcher wait list to time out
/// expired waits.  This is the only PSSS work that is safe to run
/// from the clock ISR.  Zero-page and modified-page-writer work runs
/// in dedicated system threads at PASSIVE_LEVEL.
pub fn psss_tick() {
    dispatch::tick_wait_list();
}

/// Entry point for the BalanceSetManager system thread.  Wakes once
/// per second to trim the working set and refill the zeroed page list.
extern "C" fn balance_set_manager_entry(_context: u64) {
    loop {
        // Busy-wait via scheduler yield; a proper implementation would
        // use a dispatcher timer, but the bootstrap has no timer wait.
        crate::ke::scheduler::yield_();
        crate::ke::scheduler::yield_();
        crate::ke::scheduler::yield_();
        // Trim the working set of the current process.
        let current_pml4 = crate::mm::vas::current_root();
        if current_pml4 != 0 {
            crate::mm::working_set::age_working_set(current_pml4);
            crate::mm::working_set::trim_working_set(current_pml4, 32);
        }
    }
}

/// Entry point for the MiZeroPageThread system thread.  Pops free
/// pages and pre-zeroes them.
extern "C" fn zero_page_thread_entry(_context: u64) {
    loop {
        crate::ke::scheduler::yield_();
        crate::mm::zeropage::tick();
    }
}

/// Create the three NT PSSS system threads.  Called from
/// `ke::init()` (or kernel_main) during bring-up.
pub fn init() {
    // NT 6.1 creates these threads at PASSIVE_LEVEL during
    // Phase 1 initialisation.  We create them here so the timer
    // ISR no longer runs memory-manager work inline.
    let _ = crate::ps::thread::ps_create_system_thread(
        balance_set_manager_entry as *const () as u64,
        0,
    );
    let _ = crate::ps::thread::ps_create_system_thread(
        zero_page_thread_entry as *const () as u64,
        0,
    );
    // MiModifiedPageWriter is started by `mm::writer::start_modified_page_writer()`.
}

//! Thread-owned continuous power requests.
//!
//! This is request accounting, not a native service or an idle policy. The
//! system/display policy consumers must be connected before exposing the native
//! execution-state service. Preparing ownership and applying its flags are
//! separate so an output fault can leave an empty request attached to a thread.

use crate::ke::sync::Spinlock;

pub(crate) const ES_SYSTEM_REQUIRED: u32 = 1;
pub(crate) const ES_DISPLAY_REQUIRED: u32 = 2;
pub(crate) const ES_AWAYMODE_REQUIRED: u32 = 0x40;
pub(crate) const ES_CONTINUOUS: u32 = 0x8000_0000;
const REQUEST_FLAGS: u32 = ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED | ES_AWAYMODE_REQUIRED;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionRequestCounts {
    pub display: u64,
    pub system: u64,
    pub away: u64,
}

static REQUEST_COUNTS: Spinlock<ExecutionRequestCounts> = Spinlock::new(ExecutionRequestCounts {
    display: 0,
    system: 0,
    away: 0,
});

/// A coherent snapshot of the contributions owned by live threads.
pub(crate) fn execution_request_counts() -> ExecutionRequestCounts {
    *REQUEST_COUNTS.lock()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionRequestError {
    InvalidFlags,
    NotPrepared,
    Terminating,
}

struct RequestState {
    flags: u32,
    allowed: u32,
    attached: bool,
    terminating: bool,
}

/// An exclusive owner embedded in ETHREAD's project extension. It cannot be
/// copied. All operations acquire the owner lock before the aggregate lock;
/// termination closes the owner permanently and releases each contribution once.
pub(crate) struct ThreadExecutionRequest {
    state: Spinlock<RequestState>,
}

impl ThreadExecutionRequest {
    pub(crate) const fn new() -> Self {
        Self {
            state: Spinlock::new(RequestState {
                flags: 0,
                allowed: 0,
                attached: false,
                terminating: false,
            }),
        }
    }

    /// Attach empty ownership if needed, returning the previous continuous
    /// state. Win7 disables display requests for session zero and no session.
    /// This does not apply any requested flags or reset an idle timer.
    pub(crate) fn prepare_continuous(&self, session_id: u32) -> Result<u32, ExecutionRequestError> {
        let mut state = self.state.lock();
        if state.terminating {
            return Err(ExecutionRequestError::Terminating);
        }
        if !state.attached {
            state.allowed = ES_SYSTEM_REQUIRED | ES_AWAYMODE_REQUIRED;
            if session_id != 0 && session_id != u32::MAX {
                state.allowed |= ES_DISPLAY_REQUIRED;
            }
            state.attached = true;
        }
        Ok(ES_CONTINUOUS | state.flags)
    }

    pub(crate) fn continuous_state(&self) -> u32 {
        ES_CONTINUOUS | self.state.lock().flags
    }

    /// Apply a previously prepared continuous request after the caller has
    /// safely published its previous-state output. This is deliberately not a
    /// transient request implementation. Unsupported session display requests
    /// contribute nothing, matching the original ignored PoSetPowerRequest error.
    pub(crate) fn apply_continuous(&self, flags: u32) -> Result<(), ExecutionRequestError> {
        if flags & ES_CONTINUOUS == 0 || flags & !(ES_CONTINUOUS | REQUEST_FLAGS) != 0 {
            return Err(ExecutionRequestError::InvalidFlags);
        }
        let mut state = self.state.lock();
        if state.terminating {
            return Err(ExecutionRequestError::Terminating);
        }
        if !state.attached {
            return Err(ExecutionRequestError::NotPrepared);
        }
        let next = flags & state.allowed;
        change_counts(state.flags, next);
        state.flags = next;
        Ok(())
    }

    /// Called before the owning thread becomes signaled or its pool storage is
    /// freed. Idempotence also makes ordinary Rust destruction safe afterwards.
    pub(crate) fn is_terminating(&self) -> bool { self.state.lock().terminating }

    pub(crate) fn terminate(&self) {
        let mut state = self.state.lock();
        change_counts(state.flags, 0);
        state.flags = 0;
        state.attached = false;
        state.terminating = true;
    }
}

impl Drop for ThreadExecutionRequest {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn change_counts(previous: u32, next: u32) {
    if previous == next {
        return;
    }
    let mut counts = REQUEST_COUNTS.lock();
    change_count(&mut counts.display, ES_DISPLAY_REQUIRED, previous, next);
    change_count(&mut counts.system, ES_SYSTEM_REQUIRED, previous, next);
    change_count(&mut counts.away, ES_AWAYMODE_REQUIRED, previous, next);
}

fn change_count(count: &mut u64, flag: u32, previous: u32, next: u32) {
    if previous & flag == next & flag {
        return;
    }
    if next & flag != 0 {
        *count += 1;
    } else {
        *count -= 1;
    }
}

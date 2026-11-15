//! Per-thread alert storage. Mutations serialize on the native dispatcher lock.
use core::sync::atomic::{AtomicU8, Ordering};

/// Stored in the ETHREAD extension, so unrelated TIDs never share alert state.
/// The dispatcher lock serializes this state with wait completion/termination.
pub struct UserAlertState(AtomicU8);

impl UserAlertState {
    pub const fn new() -> Self {
        Self(AtomicU8::new(0))
    }

    pub(crate) fn pending(&self) -> bool {
        self.0.load(Ordering::Relaxed) & 1 != 0
    }

    pub(crate) fn take(&self) -> bool {
        self.0.fetch_and(!1, Ordering::Relaxed) & 1 != 0
    }

    pub(crate) fn set(&self) {
        self.0.fetch_or(1, Ordering::Relaxed);
    }

    pub(crate) fn terminated(&self) -> bool {
        self.0.load(Ordering::Relaxed) & 2 != 0
    }

    pub(crate) fn terminate(&self) {
        self.0.store(2, Ordering::Relaxed);
    }
}

impl Default for UserAlertState {
    fn default() -> Self {
        Self::new()
    }
}

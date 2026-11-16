//! Serialize private synchronous file transfers without raising IRQL across
//! a pending driver completion. Storage lifetime is retained by the issuer.
use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct FileRequestLock(AtomicBool);

impl FileRequestLock {
    pub(crate) const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub(crate) fn lock(&self) -> FileRequestGuard<'_> {
        while self
            .0
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            crate::ke::scheduler::yield_();
        }
        FileRequestGuard(self)
    }
}

pub(crate) struct FileRequestGuard<'a>(&'a FileRequestLock);

impl Drop for FileRequestGuard<'_> {
    fn drop(&mut self) {
        self.0 .0.store(false, Ordering::Release);
    }
}

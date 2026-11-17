use super::*;
use crate::file_request_lock::FileRequestLock;
use std::sync::Arc;

#[test]
fn file_requests_serialize_position_and_release_on_early_return() {
    let _serial = TEST_LOCK.lock();
    let lock = Arc::new(FileRequestLock::new());
    let inside = Arc::new(AtomicUsize::new(0));
    let position = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let lock = lock.clone();
            let inside = inside.clone();
            let position = position.clone();
            scope.spawn(move || {
                for _ in 0..200 {
                    let _request = lock.lock();
                    assert_eq!(inside.fetch_add(1, Ordering::Relaxed), 0);
                    let previous = position.load(Ordering::Relaxed);
                    std::thread::yield_now();
                    position.store(previous + 1, Ordering::Relaxed);
                    assert_eq!(inside.fetch_sub(1, Ordering::Relaxed), 1);
                }
            });
        }
    });
    assert_eq!(position.load(Ordering::Relaxed), 800);
    fn early_return(lock: &FileRequestLock) -> Result<(), ()> {
        let _request = lock.lock();
        Err(())
    }
    assert_eq!(early_return(&lock), Err(()));
    let _again = lock.lock();
}

//! Wait for the system-volume owner without converting contention into I/O failure.
use core::sync::atomic::{AtomicBool, Ordering};

pub fn acquire(busy: &AtomicBool, mut yield_once: impl FnMut()) {
    while busy.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        yield_once();
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn competing_reader_waits_until_writer_releases_volume() {
        let busy=AtomicBool::new(true);
        let mut yielded=0;
        acquire(&busy, || {
            yielded+=1;
            // Model a preempted writeback completing during the yield.
            if yielded==3 {busy.store(false,Ordering::Release);}
        });
        assert!(yielded>=3);
        assert!(busy.load(Ordering::Relaxed));
    }
    #[test]
    fn readers_and_writers_never_overlap() {
        let busy=AtomicBool::new(false);
        let active=core::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..4 {scope.spawn(|| {
                for _ in 0..500 {
                    acquire(&busy,std::thread::yield_now);
                    assert_eq!(active.fetch_add(1,Ordering::SeqCst),0);
                    std::thread::yield_now();
                    assert_eq!(active.fetch_sub(1,Ordering::SeqCst),1);
                    busy.store(false,Ordering::Release);
                }
            });}
        });
    }
}

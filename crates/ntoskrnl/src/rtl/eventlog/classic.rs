//! Lossless classic Application events captured from isolated Win32 processes.
//! These records are retained in kernel memory; disk EVTX persistence and
//! ReadEventLog transport are not implemented by this backend.
use crate::arch::common::btl::nt61::ApplicationEvent;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct Record {
    pub record_number: u64,
    pub timestamp: u64,
    pub event: ApplicationEvent,
}
struct Log {
    records: Vec<Record>,
    next: u64,
    bytes: usize,
}
static APPLICATION: crate::ke::sync::Spinlock<Log> = crate::ke::sync::Spinlock::new(Log {
    records: Vec::new(), next: 1, bytes: 0,
});

/// Commit exactly one event or return ERROR_LOG_FILE_FULL without partial data.
pub fn report(event: ApplicationEvent, timestamp: u64) -> Result<(), u32> {
    let bytes = event.source.len() * 2 + event.sid.len() + event.data.len()
        + event.strings.iter().map(|s| (s.len() + 1) * 2).sum::<usize>() + 64;
    let mut log = APPLICATION.lock();
    if log.records.len() >= 256 || bytes > 4 * 1024 * 1024 - log.bytes { return Err(1502); }
    let record_number = log.next;
    let next = record_number.checked_add(1).ok_or(1502u32)?;
    log.records.try_reserve(1).map_err(|_| 8u32)?;
    log.records.push(Record { record_number, timestamp, event });
    log.next = next;
    log.bytes += bytes;
    Ok(())
}

/// Snapshot for the log service. A caller owns its copies after the lock drops.
pub fn snapshot() -> Vec<Record> { APPLICATION.lock().records.clone() }

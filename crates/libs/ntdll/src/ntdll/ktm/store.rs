//! Volatile KTM objects. Native handle entries own an open reference; parent
//! links and in-flight calls own separate references. Lock order is always the
//! native handle table followed by STORE, never the reverse.

use super::*;
use crate::ke::sync::Spinlock;

pub(super) const TRANSACTION_NOT_ACTIVE: NTSTATUS = 0xc0190003u32 as i32;
pub(super) const ALREADY_ABORTED: NTSTATUS = 0xc0190015u32 as i32;
pub(super) const ALREADY_COMMITTED: NTSTATUS = 0xc0190016u32 as i32;
pub(super) const TM_VOLATILE: NTSTATUS = 0xc019003bu32 as i32;
pub(super) const TRANSACTION_NOT_FOUND: NTSTATUS = 0xc019004eu32 as i32;
pub(super) const RM_NOT_FOUND: NTSTATUS = 0xc019004fu32 as i32;
pub(super) const TM_NOT_FOUND: NTSTATUS = 0xc0190051u32 as i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Guid(pub [u8; 16]);

impl Guid {
    pub const ZERO: Self = Self([0; 16]);
}

pub(super) struct Transaction {
    pub manager: u64,
    pub description: Vec<u8>,
    pub timeout: i64,
    pub deadline: Option<(bool, u64)>,
    pub outcome: u32,
    pub timed_out: bool,
    pub phase: u32,
}

impl Transaction {
    pub fn refresh(&mut self) {
        if self.outcome != 1 || self.phase == 3 {
            return;
        }
        if let Some((absolute, deadline)) = self.deadline {
            let now = crate::ke::time::get_time_snapshot();
            if (if absolute {
                now.system_time
            } else {
                now.interrupt_time
            }) >= deadline
            {
                self.outcome = 3;
                self.timed_out = true;
            }
        }
    }


}

pub(super) enum Payload {
    Manager { clock: i64 },
    Transaction(Transaction),
    ResourceManager { manager: u64, description: Vec<u8> },
    Enlistment(super::enlistment::Enlistment),
}

impl Payload {
    pub fn parent(&self) -> u64 {
        match self {
            Self::Manager { .. } => 0,
            Self::Transaction(transaction) => transaction.manager,
            Self::ResourceManager { manager, .. } => *manager,
            Self::Enlistment(enlistment) => enlistment.resource_manager,
        }
    }

    pub fn kind(&self) -> HandleKind {
        match self {
            Self::Manager { .. } => HandleKind::TransactionManager,
            Self::Transaction(_) => HandleKind::Transaction,
            Self::ResourceManager { .. } => HandleKind::ResourceManager,
            Self::Enlistment(_) => HandleKind::Enlistment,
        }
    }
}

pub(super) struct Record {
    pub id: u64,
    pub guid: Guid,
    pub references: usize,
    pub opens: usize,
    pub payload: Payload,
}

pub(super) struct Store {
    pub records: Vec<Record>,
    next_id: u64,
}

pub(super) static STORE: Spinlock<Store> = Spinlock::new(Store {
    records: Vec::new(),
    next_id: 1,
});

impl Store {
    pub fn get(&self, id: u64) -> Result<&Record, NTSTATUS> {
        self.records
            .iter()
            .find(|record| record.id == id)
            .ok_or(STATUS_INVALID_HANDLE)
    }

    pub fn get_mut(&mut self, id: u64) -> Result<&mut Record, NTSTATUS> {
        self.records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(STATUS_INVALID_HANDLE)
    }

    pub fn insert(&mut self, guid: Option<Guid>, payload: Payload) -> Result<u64, NTSTATUS> {
        let id = self.next_id;
        let next_id = id.checked_add(1).filter(|id| *id < (1 << 60)).ok_or(STATUS_NO_MEMORY)?;
        let guid = match guid {
            Some(guid) => guid,
            None => {
                let mut bytes = [0; 16];
                if !crate::crypto::csprng::fill(&mut bytes) {
                    return Err(STATUS_UNSUCCESSFUL);
                }
                // A monotonic component also guarantees uniqueness when the
                // entropy provider is deterministic in the host test fixture.
                bytes[..8].copy_from_slice(&id.to_le_bytes());
                bytes[7] = (bytes[7] & 0x0f) | 0x40;
                bytes[8] = (bytes[8] & 0x3f) | 0x80;
                Guid(bytes)
            }
        };
        if self.records.iter().any(|record| {
            record.guid == guid
                && record.payload.kind() == payload.kind()
                && record.payload.parent() == payload.parent()
        }) {
            return Err(0xc000022bu32 as i32); // STATUS_OBJECTID_EXISTS
        }
        self.records.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
        let parent = payload.parent();
        let tx = match &payload {
            Payload::Enlistment(value) => Some(value.transaction),
            _ => None,
        };
        // Validate both counters before changing either parent reference.
        for identity in [tx, (parent != 0).then_some(parent)].into_iter().flatten() {
            self.get(identity)?.references.checked_add(1).ok_or(STATUS_NO_MEMORY)?;
        }
        for identity in [tx, (parent != 0).then_some(parent)].into_iter().flatten() {
            self.get_mut(identity)?.references += 1;
        }
        self.records.push(Record {
            id,
            guid,
            references: 1,
            opens: 1,
            payload,
        });
        self.next_id = next_id;
        Ok(id)
    }

    pub fn open(
        &mut self,
        kind: HandleKind,
        guid: Guid,
        parent: Option<u64>,
    ) -> Result<u64, NTSTATUS> {
        let record = self
            .records
            .iter_mut()
            .find(|record| {
                record.payload.kind() == kind
                    && record.guid == guid
                    && parent.is_none_or(|parent| record.payload.parent() == parent)
            })
            .ok_or(match kind {
                HandleKind::Transaction => TRANSACTION_NOT_FOUND,
                HandleKind::ResourceManager => RM_NOT_FOUND,
                HandleKind::Enlistment => 0xc0190050u32 as i32,
                _ => TM_NOT_FOUND,
            })?;
        let references = record.references.checked_add(1).ok_or(STATUS_NO_MEMORY)?;
        let opens = record.opens.checked_add(1).ok_or(STATUS_NO_MEMORY)?;
        record.references = references;
        record.opens = opens;
        Ok(record.id)
    }

    fn release(&mut self, mut id: u64, open: bool) {
        if open {
            if let Ok(record) = self.get_mut(id) {
                debug_assert!(record.opens != 0);
                record.opens -= 1;
                if record.opens == 0 {
                    if let Payload::Transaction(transaction) = &mut record.payload {
                        // Closing the final transaction handle never commits work.
                        if transaction.outcome == 1 && transaction.phase != 3 {
                            transaction.outcome = 3;
                        }
                    }
                }
            }
        }
        while let Some(index) = self.records.iter().position(|record| record.id == id) {
            let record = &mut self.records[index];
            debug_assert!(record.references != 0);
            record.references -= 1;
            if record.references != 0 {
                break;
            }
            let payload = self.records.swap_remove(index).payload;
            if let Payload::Enlistment(enlistment) = &payload {
                if let Ok(record) = self.get_mut(enlistment.transaction) {
                    if let Payload::Transaction(transaction) = &mut record.payload {
                        if !enlistment.finished() && transaction.outcome == 1 && transaction.phase != 3 {
                            transaction.outcome = 3;
                        }
                    }
                }
                self.release(enlistment.transaction, false);
            }
            id = payload.parent();
            if id == 0 {
                break;
            }
        }
    }
}

/// A captured object identity stays alive even if its original handle closes
/// and the handle slot is reused before a new child handle is published.
pub(super) struct Pin(pub u64);

impl Drop for Pin {
    fn drop(&mut self) {
        STORE.lock().release(self.0, false);
    }
}

pub(super) fn pin(handle: HANDLE, kind: HandleKind, access: u32) -> Result<Pin, NTSTATUS> {
    file::with_sync_access(handle, access, |entries| {
        if entries[0].kind != kind {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        let mut store = STORE.lock();
        let record = store.get_mut(entries[0].target)?;
        record.references = record.references.checked_add(1).ok_or(STATUS_NO_MEMORY)?;
        Ok(Pin(record.id))
    })
}

pub(crate) fn release_open(id: u64) {
    STORE.lock().release(id, true);
}

pub(super) fn deadline(timeout: i64) -> Option<(bool, u64)> {
    let now = crate::ke::time::get_time_snapshot();
    if timeout < 0 {
        Some((
            false,
            now.interrupt_time.saturating_add(timeout.unsigned_abs()),
        ))
    } else if timeout > 0 {
        Some((true, timeout as u64))
    } else {
        None
    }
}

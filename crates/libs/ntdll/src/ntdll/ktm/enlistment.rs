//! Local volatile resource enlistments and ordered two-phase notifications.
//! Durable recovery and distributed transaction promotion require a log backend.
use super::*;
use core::sync::atomic::{AtomicU64, Ordering};

const ACTIVE: u32 = 0;
const PREPREPARING: u32 = 1;
const PREPREPARED: u32 = 2;
const PREPARING: u32 = 3;
const PREPARED: u32 = 4;
const COMMITTING: u32 = 5;
const COMMITTED: u32 = 6;
const ROLLING_BACK: u32 = 7;
const ROLLED_BACK: u32 = 8;
const READ_ONLY: u32 = 9;
const SINGLE_PHASE: u32 = 10;
const INVALID_REQUEST: NTSTATUS = 0xc0190013u32 as i32;
static NEXT_NOTIFICATION: AtomicU64 = AtomicU64::new(1);

pub(super) struct Notification {
    sequence: u64,
    key: u64,
    mask: u32,
    clock: i64,
}

pub(super) struct Enlistment {
    pub resource_manager: u64,
    pub transaction: u64,
    mask: u32,
    key: u64,
    state: u32,
    superior: bool,
    recovery: Vec<u8>,
    crm: [u8; 48],
    notifications: Vec<Notification>,
}

impl Enlistment {
    pub fn finished(&self) -> bool { matches!(self.state, COMMITTED | ROLLED_BACK | READ_ONLY) }
}

fn enlistment(store: &mut store::Store, id: u64) -> Result<&mut Enlistment, NTSTATUS> {
    match &mut store.get_mut(id)?.payload {
        Payload::Enlistment(value) => Ok(value),
        _ => Err(STATUS_OBJECT_TYPE_MISMATCH),
    }
}

fn transaction(store: &mut store::Store, id: u64) -> Result<&mut Transaction, NTSTATUS> {
    match &mut store.get_mut(id)?.payload {
        Payload::Transaction(value) => Ok(value),
        _ => Err(STATUS_OBJECT_TYPE_MISMATCH),
    }
}

/// Reserve every notification before changing any participant's state.
fn broadcast(store: &mut store::Store, tx: u64, mask: u32, pending: u32, skipped: u32) -> Result<(), NTSTATUS> {
    let manager = transaction(store, tx)?.manager;
    let clock = if manager == 0 { 0 } else {
        match store.get(manager)?.payload { Payload::Manager { clock } => clock, _ => 0 }
    };
    for record in &mut store.records {
        if let Payload::Enlistment(value) = &mut record.payload {
            if value.transaction == tx && !value.superior && !value.finished() && value.mask & mask != 0 {
                value.notifications.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
            }
        }
    }
    for record in &mut store.records {
        if let Payload::Enlistment(value) = &mut record.payload {
            if value.transaction != tx || value.superior || value.finished() { continue; }
            if mask == 8 && value.state == ROLLING_BACK { continue; }
            if mask == 8 { value.notifications.clear(); }
            if value.mask & mask != 0 {
                value.notifications.push(Notification { sequence: NEXT_NOTIFICATION.fetch_add(1, Ordering::Relaxed), key: value.key, mask, clock });
                value.state = pending;
            } else { value.state = skipped; }
        }
    }
    Ok(())
}

fn all_ready(store: &store::Store, tx: u64, ready: u32) -> bool {
    store.records.iter().all(|record| match &record.payload {
        Payload::Enlistment(value) if value.transaction == tx && !value.superior => value.finished() || value.state == ready,
        _ => true,
    })
}

fn notify_superior(store: &mut store::Store, tx: u64, mask: u32) -> Result<bool, NTSTATUS> {
    let superior = store.records.iter().find_map(|record| match &record.payload {
        Payload::Enlistment(value) if value.transaction == tx && value.superior => Some(record.id),
        _ => None,
    });
    let Some(id) = superior else { return Ok(false); };
    let manager = transaction(store, tx)?.manager;
    let clock = match store.get(manager)?.payload {
        Payload::Manager { clock } => clock,
        _ => return Err(STATUS_OBJECT_TYPE_MISMATCH),
    };
    let value = enlistment(store, id)?;
    if value.mask & mask != 0 {
        value.notifications.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
        value.notifications.push(Notification {
            sequence: NEXT_NOTIFICATION.fetch_add(1, Ordering::Relaxed), key: value.key, mask, clock,
        });
    }
    Ok(true)
}

fn commit_decision(store: &mut store::Store, tx: u64) -> Result<(), NTSTATUS> {
    broadcast(store, tx, 4, COMMITTING, COMMITTED)?;
    let value = transaction(store, tx)?;
    value.outcome = 2;
    value.phase = 3;
    value.deadline = None;
    Ok(())
}

pub(super) fn progress(store: &mut store::Store, tx: u64) -> Result<(), NTSTATUS> {
    transaction(store, tx)?.refresh();
    if transaction(store, tx)?.outcome == 3 {
        broadcast(store, tx, 8, ROLLING_BACK, ROLLED_BACK)?;
        if transaction(store, tx)?.phase != 4 && all_ready(store, tx, ROLLED_BACK) {
            notify_superior(store, tx, 0x80)?;
            transaction(store, tx)?.phase = 4;
        }
        return Ok(());
    }
    loop {
        let phase = transaction(store, tx)?.phase;
        match phase {
            1 if all_ready(store, tx, PREPREPARED) => {
                if notify_superior(store, tx, 0x10)? {
                    transaction(store, tx)?.phase = 6;
                    return Ok(());
                }
                broadcast(store, tx, 2, PREPARING, PREPARED)?;
                transaction(store, tx)?.phase = 2;
            }
            2 if all_ready(store, tx, PREPARED) => {
                if notify_superior(store, tx, 0x20)? {
                    transaction(store, tx)?.phase = 7;
                    return Ok(());
                }
                commit_decision(store, tx)?;
            }
            3 if all_ready(store, tx, COMMITTED) => {
                notify_superior(store, tx, 0x40)?;
                transaction(store, tx)?.phase = 4;
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}

fn start(store: &mut store::Store, tx: u64, commit: bool) -> Result<(), NTSTATUS> {
    transaction(store, tx)?.refresh();
    let value = transaction(store, tx)?;
    if value.outcome == 2 { return Err(store::ALREADY_COMMITTED); }
    if value.outcome == 3 { return Err(if value.timed_out { 0xc0000210u32 as i32 } else { store::ALREADY_ABORTED }); }
    if !commit {
        broadcast(store, tx, 8, ROLLING_BACK, ROLLED_BACK)?;
        transaction(store, tx)?.outcome = 3;
        return Ok(());
    }
    if value.phase != 0 { return Err(INVALID_REQUEST); }
    let mut participant = None;
    let mut participant_count = 0;
    for record in &store.records {
        if matches!(&record.payload, Payload::Enlistment(value)
            if value.transaction == tx && !value.superior && !value.finished()) {
            participant = Some(record.id);
            participant_count += 1;
        }
    }
    if let Some(superior) = store.records.iter().find_map(|record| match &record.payload {
        Payload::Enlistment(value) if value.transaction == tx && value.superior => Some(record.id),
        _ => None,
    }) {
        let value = enlistment(store, superior)?;
        if value.mask & 0x0400_0000 == 0 { return Err(INVALID_REQUEST); }
        value.notifications.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
        value.notifications.push(Notification { sequence: NEXT_NOTIFICATION.fetch_add(1, Ordering::Relaxed), key: value.key, mask: 0x0400_0000, clock: 0 });
        transaction(store, tx)?.phase = 5;
    } else if participant_count == 1 && enlistment(store, participant.ok_or(INVALID_REQUEST)?)?.mask & 0x200 != 0 {
        broadcast(store, tx, 0x200, SINGLE_PHASE, COMMITTED)?;
        transaction(store, tx)?.phase = 11;
    } else {
        broadcast(store, tx, 1, PREPREPARING, PREPREPARED)?;
        transaction(store, tx)?.phase = 1;
    }
    progress(store, tx)
}

pub(super) fn finish_transaction(handle: HANDLE, commit: bool, wait: bool) -> NTSTATUS {
    (|| {
        let object = pin(handle, HandleKind::Transaction, if commit { 8 } else { 16 })?;
        start(&mut STORE.lock(), object.0, commit)?;
        if !wait { return Ok(STATUS_SUCCESS); }
        loop {
            let done = {
                let mut store = STORE.lock();
                progress(&mut store, object.0)?;
                let outcome = transaction(&mut store, object.0)?.outcome;
                if commit && outcome == 3 { return Err(store::ALREADY_ABORTED); }
                transaction(&mut store, object.0)?.phase == 4
            };
            if done { return Ok(STATUS_SUCCESS); }
            crate::ke::scheduler::yield_();
        }
    })().unwrap_or_else(|status| status)
}

/// # Safety
/// Captures attributes and probes the output before publishing an enlistment.
pub unsafe extern "C" fn nt_create_enlistment(output: *mut HANDLE, access: u32,
    resource_manager: HANDLE, tx: HANDLE, attributes: *mut ObjectAttributes,
    options: u32, mask: u32, key: PVOID) -> NTSTATUS {
    (|| {
        if options & !1 != 0 || mask == 0 || mask & !0x3fff_ffff != 0 { return Err(STATUS_INVALID_PARAMETER); }
        if !valid_notification_mask(mask, options == 1) { return Err(STATUS_INVALID_PARAMETER); }
        // Recovery/distributed notifications need additional lifetime/state
        // machinery. Keep that backend limit separate from protocol validity.
        if mask & !0x0400_02ff != 0 { return Err(STATUS_NOT_SUPPORTED); }
        let resource = pin(resource_manager, HandleKind::ResourceManager, 8)?;
        let tx = pin(tx, HandleKind::Transaction, 4)?;
        publish(output, access, attributes, HandleKind::Enlistment, || {
            let mut store = STORE.lock();
            let manager = match store.get(resource.0)?.payload {
                Payload::ResourceManager { manager, .. } => manager,
                _ => return Err(STATUS_OBJECT_TYPE_MISMATCH),
            };
            let transaction = transaction(&mut store, tx.0)?;
            transaction.refresh();
            if transaction.manager != manager { return Err(0xc0190012u32 as i32); }
            if transaction.outcome != 1 || transaction.phase != 0 { return Err(store::TRANSACTION_NOT_ACTIVE); }
            if store.records.iter().any(|record| matches!(&record.payload,
                Payload::Enlistment(value) if value.transaction == tx.0 &&
                (value.resource_manager == resource.0 || (options == 1 && value.superior)))) {
                return Err(0xc0190033u32 as i32);
            }
            store.insert(None, Payload::Enlistment(Enlistment {
                resource_manager: resource.0, transaction: tx.0, mask, key: key as u64,
                state: ACTIVE, superior: options == 1, recovery: Vec::new(), crm: [0; 48], notifications: Vec::new(),
            }))
        })
    })().unwrap_or_else(|status| status)
}

/// Win7's protocol validator at original RVA 0x30e2a8, including its cold
/// PREPREPARE alternative at 0x3aa8eb. The caller validates public option and
/// reserved mask bits separately. These clauses apply before local-backend
/// capability filtering, so invalid masks cannot become supported enlistments.
pub(crate) fn valid_notification_mask(mask: u32, superior: bool) -> bool {
    if mask & 8 == 0 {
        return false;
    }
    if superior {
        return mask & 0x207 == 0;
    }
    if mask & 0x0600_00f0 != 0 {
        return false;
    }
    if mask & 2 != 0 && mask & 4 == 0 {
        return false;
    }
    if mask & 0x200 != 0 && mask & 2 == 0 {
        return false;
    }
    mask & (1 | 4) != 0
}

/// # Safety
/// Captures the GUID and attributes; a failed copyout releases the open reference.
pub unsafe extern "C" fn nt_open_enlistment(output: *mut HANDLE, access: u32,
    resource_manager: HANDLE, identity: *const [u8; 16], attributes: *mut ObjectAttributes) -> NTSTATUS {
    (|| {
        let resource = pin(resource_manager, HandleKind::ResourceManager, 1)?;
        let identity = guid(identity)?;
        publish_guid(output, access, attributes, HandleKind::Enlistment,
            || STORE.lock().open(HandleKind::Enlistment, identity, Some(resource.0)))
    })().unwrap_or_else(|status| status)
}

/// # Safety
/// The information and returned-size buffers are caller memory.
pub unsafe extern "C" fn nt_query_information_enlistment(handle: HANDLE, class: u32,
    output: PVOID, length: u32, returned: *mut u32) -> NTSTATUS {
    (|| {
        let object = pin(handle, HandleKind::Enlistment, 1)?;
        let mut store = STORE.lock();
        let identity = store.get(object.0)?.guid;
        let value = enlistment(&mut store, object.0)?;
        let tx = value.transaction;
        let resource = value.resource_manager;
        let bytes = match class {
            0 => {
                let mut bytes = zeroed(48)?;
                bytes[..16].copy_from_slice(&identity.0);
                bytes[16..32].copy_from_slice(&store.get(tx)?.guid.0);
                bytes[32..48].copy_from_slice(&store.get(resource)?.guid.0);
                bytes
            }
            1 => {
                let mut bytes = zeroed(value.recovery.len())?;
                bytes.copy_from_slice(&value.recovery);
                bytes
            }
            2 => {
                let mut bytes = zeroed(48)?;
                bytes.copy_from_slice(&value.crm);
                bytes
            },
            _ => return Err(STATUS_INVALID_INFO_CLASS),
        };
        drop(store);
        if !returned.is_null() { write_user_object(returned as u64, &(bytes.len() as u32))?; }
        if (length as usize) < bytes.len() { return Err(STATUS_INFO_LENGTH_MISMATCH); }
        if !bytes.is_empty() { copy_to_user(output as u64, &bytes)?; }
        Ok(STATUS_SUCCESS)
    })().unwrap_or_else(|status| status)
}

/// # Safety
/// Captures variable recovery data before modifying the resource object.
pub unsafe extern "C" fn nt_set_information_enlistment(handle: HANDLE, class: u32, input: PVOID, length: u32) -> NTSTATUS {
    (|| {
        if class != 1 && class != 2 { return Err(STATUS_INVALID_INFO_CLASS); }
        if class == 2 && length != 48 { return Err(STATUS_INFO_LENGTH_MISMATCH); }
        if length > 65536 { return Err(STATUS_INVALID_PARAMETER); }
        let mut bytes = zeroed(length as usize)?;
        if length != 0 { copy_from_user(&mut bytes, input as u64)?; }
        let object = pin(handle, HandleKind::Enlistment, 2)?;
        let mut store = STORE.lock();
        let value = enlistment(&mut store, object.0)?;
        if value.finished() { return Err(store::TRANSACTION_NOT_ACTIVE); }
        if class == 1 { value.recovery = bytes; } else { value.crm.copy_from_slice(&bytes); }
        Ok(STATUS_SUCCESS)
    })().unwrap_or_else(|status| status)
}

fn complete(handle: HANDLE, clock: *const i64, request: u32) -> NTSTATUS {
    (|| {
        let clock = if clock.is_null() { None } else { Some(read_user_object::<i64>(clock as u64)?) };
        let object = pin(handle, HandleKind::Enlistment, 8)?;
        let mut store = STORE.lock();
        let tx = enlistment(&mut store, object.0)?.transaction;
        progress(&mut store, tx)?;
        let value = enlistment(&mut store, object.0)?;
        if value.superior { return Err(INVALID_REQUEST); }
        let next = match (request, value.state) {
            (1, PREPREPARING) => PREPREPARED,
            (2, PREPARING) => PREPARED,
            (4, COMMITTING | SINGLE_PHASE) => COMMITTED,
            (8, ROLLING_BACK) => ROLLED_BACK,
            (16, PREPARING | PREPREPARING) => READ_ONLY,
            (32, SINGLE_PHASE) => ACTIVE,
            _ => return Err(INVALID_REQUEST),
        };
        let single_phase_commit = request == 4 && value.state == SINGLE_PHASE;
        let tx = value.transaction;
        let manager = transaction(&mut store, tx)?.manager;
        if let Some(provided) = clock {
            if manager != 0 {
                if let Payload::Manager { clock } = &mut store.get_mut(manager)?.payload {
                    if provided < *clock { return Err(INVALID_REQUEST); }
                    *clock = provided;
                }
            }
        }
        enlistment(&mut store, object.0)?.state = next;
        if single_phase_commit {
            let value = transaction(&mut store, tx)?;
            value.outcome = 2;
            value.phase = 4;
            value.deadline = None;
        }
        if request == 32 {
            broadcast(&mut store, tx, 2, PREPARING, PREPARED)?;
            transaction(&mut store, tx)?.phase = 2;
        }
        progress(&mut store, tx)?;
        Ok(STATUS_SUCCESS)
    })().unwrap_or_else(|status| status)
}

macro_rules! completion {
    ($name:ident, $request:expr) => {
        /// # Safety
        /// The optional virtual clock is captured from caller memory.
        pub unsafe extern "C" fn $name(handle: HANDLE, clock: *const i64) -> NTSTATUS {
            complete(handle, clock, $request)
        }
    };
}
completion!(nt_pre_prepare_complete, 1);
completion!(nt_prepare_complete, 2);
completion!(nt_commit_complete, 4);
completion!(nt_rollback_complete, 8);
completion!(nt_read_only_enlistment, 16);
completion!(nt_single_phase_reject, 32);

fn superior_request(handle: HANDLE, clock: *const i64, request: u32) -> NTSTATUS {
    (|| {
        let clock = if clock.is_null() { None } else { Some(read_user_object::<i64>(clock as u64)?) };
        let object = pin(handle, HandleKind::Enlistment, 16)?;
        let mut store = STORE.lock();
        let value = enlistment(&mut store, object.0)?;
        if !value.superior { return Err(INVALID_REQUEST); }
        let tx = value.transaction;
        transaction(&mut store, tx)?.refresh();
        if transaction(&mut store, tx)?.outcome != 1 {
            progress(&mut store, tx)?;
            return Err(store::TRANSACTION_NOT_ACTIVE);
        }
        let phase = transaction(&mut store, tx)?.phase;
        let valid = match request {
            1 => matches!(phase, 0 | 5),
            2 => matches!(phase, 0 | 5 | 6),
            4 => phase == 7,
            8 => true,
            _ => false,
        };
        if !valid { return Err(INVALID_REQUEST); }
        let manager = transaction(&mut store, tx)?.manager;
        if let Some(provided) = clock {
            if manager != 0 {
                if let Payload::Manager { clock } = &mut store.get_mut(manager)?.payload {
                    if provided < *clock { return Err(INVALID_REQUEST); }
                    *clock = provided;
                }
            }
        }
        match request {
            1 if phase == 0 || phase == 5 => {
                broadcast(&mut store, tx, 1, PREPREPARING, PREPREPARED)?;
                transaction(&mut store, tx)?.phase = 1;
            }
            2 if matches!(phase, 0 | 5 | 6) => {
                broadcast(&mut store, tx, 2, PREPARING, PREPARED)?;
                transaction(&mut store, tx)?.phase = 2;
            }
            4 if phase == 7 => { commit_decision(&mut store, tx)?; }
            8 => { start(&mut store, tx, false)?; }
            _ => return Err(INVALID_REQUEST),
        }
        progress(&mut store, tx)?;
        Ok(STATUS_SUCCESS)
    })().unwrap_or_else(|status| status)
}

macro_rules! superior {
    ($name:ident, $request:expr) => {
        /// # Safety
        /// The optional virtual clock is captured from caller memory.
        pub unsafe extern "C" fn $name(handle: HANDLE, clock: *const i64) -> NTSTATUS {
            superior_request(handle, clock, $request)
        }
    };
}
superior!(nt_pre_prepare_enlistment, 1);
superior!(nt_prepare_enlistment, 2);
superior!(nt_commit_enlistment, 4);
superior!(nt_rollback_enlistment, 8);

/// # Safety
/// Probes every output; the queue is consumed only after successful copyout.
pub unsafe extern "C" fn nt_get_notification_resource_manager(handle: HANDLE, output: PVOID,
    length: u32, timeout: *const i64, returned: *mut u32, asynchronous: u32, _context: u64) -> NTSTATUS {
    (|| {
        if asynchronous != 0 { return Err(STATUS_NOT_SUPPORTED); }
        if !returned.is_null() { write_user_object(returned as u64, &32u32)?; }
        if length < 32 { return Err(STATUS_BUFFER_TOO_SMALL); }
        probe_user_write(output as u64, 32)?;
        let deadline = if timeout.is_null() { None } else {
            let value = read_user_object::<i64>(timeout as u64)?;
            if value == 0 { Some((false, 0)) } else { store::deadline(value) }
        };
        let resource = pin(handle, HandleKind::ResourceManager, 16)?;
        loop {
            {
                let mut store = STORE.lock();
                // Progress timeouts before choosing the oldest notification.
                let mut transactions = Vec::new();
                for record in &store.records {
                    if let Payload::Enlistment(value) = &record.payload {
                        if value.resource_manager == resource.0 && !transactions.contains(&value.transaction) {
                            transactions.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
                            transactions.push(value.transaction);
                        }
                    }
                }
                for tx in transactions { progress(&mut store, tx)?; }
                let oldest = store.records.iter().filter_map(|record| match &record.payload {
                    Payload::Enlistment(value) if value.resource_manager == resource.0 => value.notifications.first().map(|n| (n.sequence, record.id)),
                    _ => None,
                }).min_by_key(|entry| entry.0);
                if let Some((_, id)) = oldest {
                    let value = enlistment(&mut store, id)?;
                    let notification = &value.notifications[0];
                    let mut bytes = [0u8; 32];
                    bytes[..8].copy_from_slice(&notification.key.to_le_bytes());
                    bytes[8..12].copy_from_slice(&notification.mask.to_le_bytes());
                    bytes[16..24].copy_from_slice(&notification.clock.to_le_bytes());
                    copy_to_user(output as u64, &bytes)?;
                    value.notifications.remove(0);
                    return Ok(STATUS_SUCCESS);
                }
            }
            if deadline.is_some_and(|(absolute, deadline)| {
                let now = crate::ke::time::get_time_snapshot();
                (if absolute { now.system_time } else { now.interrupt_time }) >= deadline
            }) { return Ok(STATUS_TIMEOUT); }
            crate::ke::scheduler::yield_();
        }
    })().unwrap_or_else(|status| status)
}

#[cfg(test)]
pub(crate) use notification_key_tests::queued_notification_key_regression;

#[cfg(test)]
mod notification_key_tests {
    use super::*;
    use core::ptr::{null, null_mut};

    fn change_key(handle: HANDLE, key: u64) {
        // No registered service currently changes a key. Exercise the private
        // mutation boundary without exposing an incomplete recovery service.
        let object = pin(handle, HandleKind::Enlistment, 0).unwrap();
        enlistment(&mut STORE.lock(), object.0).unwrap().key = key;
    }

    fn receive(resource: HANDLE, key: u64, mask: u32) {
        let mut packet = [0xa5u8; 32];
        let mut returned = 0;
        assert_eq!(unsafe { nt_get_notification_resource_manager(resource,
            packet.as_mut_ptr().cast(), 32, &0, &mut returned, 0, 0) }, STATUS_SUCCESS);
        assert_eq!(returned, 32);
        assert_eq!(u64::from_le_bytes(packet[..8].try_into().unwrap()), key);
        assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), mask);
    }

    pub(crate) fn queued_notification_key_regression() {
        let mut manager = null_mut();
        let mut resource = null_mut();
        let mut transaction = null_mut();
        let mut participant = null_mut();
        assert_eq!(unsafe { nt_create_transaction_manager(&mut manager, 0xf003f,
            null_mut(), null(), 1, 0) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_create_resource_manager(&mut resource, 0x1f007f,
            manager, &[0xe3; 16], null_mut(), 1, null()) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_create_transaction(&mut transaction, 0x1f003f,
            null_mut(), null(), manager, 0, 0, 0, null(), null()) }, STATUS_SUCCESS);
        assert_eq!(unsafe { nt_create_enlistment(&mut participant, 0xf001f,
            resource, transaction, null_mut(), 0, 0xf, 10usize as PVOID) }, STATUS_SUCCESS);

        assert_eq!(nt_commit_transaction(transaction, 0), STATUS_SUCCESS);
        change_key(participant, 20);
        assert_eq!(unsafe { nt_pre_prepare_complete(participant, null()) }, STATUS_SUCCESS);
        change_key(participant, 30);
        // Both packets were queued before this dequeue. They must retain the
        // two distinct keys captured when their protocol phases were emitted.
        receive(resource, 10, 1);
        receive(resource, 20, 2);
        assert_eq!(unsafe { nt_prepare_complete(participant, null()) }, STATUS_SUCCESS);
        change_key(participant, 40);
        receive(resource, 30, 4);
        assert_eq!(unsafe { nt_commit_complete(participant, null()) }, STATUS_SUCCESS);
        for handle in [participant, transaction, resource, manager] {
            assert_eq!(unsafe { file::nt_close(handle) }, STATUS_SUCCESS);
        }
    }
}

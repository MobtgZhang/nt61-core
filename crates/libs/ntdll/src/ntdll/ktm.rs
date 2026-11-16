//! Kernel Transaction Manager services for local volatile transactions.
//!
//! ABI: Win7 KTM declarations in wdm.h, winnt.h and ktmtypes.h. Payloads use
//! captured bytes rather than user pointers; output padding is always zero.
//! Durable CLFS logging, distributed promotion and registry
//! isolation are not implemented by this module and must never report success.

use super::file::{self, DispatcherState, HandleKind};
use super::object::{capture_attributes, map_sync_access, SyncAttributes};
use super::status::*;
use super::types::{ObjectAttributes, UnicodeString, HANDLE, NTSTATUS, PVOID};
use crate::mm::user_copy::{
    copy_from_user, copy_to_user, probe_user_write, read_user_object, write_user_object,
};
use alloc::vec::Vec;

#[path = "ktm/store.rs"]
mod store;
#[path = "ktm/enlistment.rs"]
mod enlistment;
pub use enlistment::*;
pub(crate) use store::release_open;
use store::{deadline, pin, Guid, Payload, Pin, Transaction, STORE};

fn capture_string(pointer: *const UnicodeString) -> Result<Vec<u8>, NTSTATUS> {
    if pointer.is_null() {
        return Ok(Vec::new());
    }
    let string = read_user_object::<UnicodeString>(pointer as u64)?;
    if string.length & 1 != 0 || string.length > string.maximum_length {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut bytes = zeroed(usize::from(string.length))?;
    if !bytes.is_empty() {
        copy_from_user(&mut bytes, string.buffer as u64)?;
    }
    Ok(bytes)
}

fn zeroed(length: usize) -> Result<Vec<u8>, NTSTATUS> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| STATUS_NO_MEMORY)?;
    bytes.resize(length, 0);
    Ok(bytes)
}

fn guid(pointer: *const [u8; 16]) -> Result<Guid, NTSTATUS> {
    read_user_object::<[u8; 16]>(pointer as u64).map(Guid)
}

fn optional_manager(handle: HANDLE, access: u32) -> Result<Option<Pin>, NTSTATUS> {
    if handle.is_null() {
        Ok(None)
    } else {
        pin(handle, HandleKind::TransactionManager, access).map(Some)
    }
}

fn publish(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    kind: HandleKind,
    initialize: impl FnOnce() -> Result<u64, NTSTATUS>,
) -> Result<NTSTATUS, NTSTATUS> {
    let attributes = capture_attributes(attributes)?;
    publish_captured(output, access, attributes, kind, initialize)
}

fn publish_captured(
    output: *mut HANDLE,
    access: u32,
    attributes: SyncAttributes,
    kind: HandleKind,
    initialize: impl FnOnce() -> Result<u64, NTSTATUS>,
) -> Result<NTSTATUS, NTSTATUS> {
    probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
    let access = map_sync_access(kind, access)?;
    let (handle, status) = file::create_object_handle(
        kind,
        DispatcherState::None,
        access,
        attributes,
        true,
        initialize,
    )?;
    if let Err(error) = write_user_object(output as u64, &handle) {
        file::free_handle(handle);
        return Err(error);
    }
    Ok(status)
}

// GUID lookup opens an independent native object that owns its own store
// reference. It must never take the named-object OPENIF fast path.
fn publish_guid(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    kind: HandleKind,
    initialize: impl FnOnce() -> Result<u64, NTSTATUS>,
) -> Result<NTSTATUS, NTSTATUS> {
    let captured = capture_attributes(attributes)?;
    if captured.name.is_some() {
        return Err(STATUS_NOT_SUPPORTED);
    }
    publish_captured(output, access, captured, kind, initialize)
}

/// # Safety
/// Pointers designate caller memory and are captured before object publication.
pub unsafe extern "C" fn nt_create_transaction_manager(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    log_file: *const UnicodeString,
    options: u32,
    commit_strength: u32,
) -> NTSTATUS {
    (|| {
        if options & !0x3f != 0 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let log = capture_string(log_file)?;
        if options != 1 || !log.is_empty() || commit_strength != 0 {
            return Err(STATUS_NOT_SUPPORTED);
        }
        publish(
            output,
            access,
            attributes,
            HandleKind::TransactionManager,
            || STORE.lock().insert(None, Payload::Manager { clock: 0 }),
        )
    })()
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Pointers designate caller memory; identity and names are captured.
pub unsafe extern "C" fn nt_open_transaction_manager(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    log_file: *const UnicodeString,
    identity: *const [u8; 16],
    options: u32,
) -> NTSTATUS {
    (|| {
        if options != 0 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        if !capture_string(log_file)?.is_empty() {
            return Err(STATUS_NOT_SUPPORTED);
        }
        let identity = guid(identity)?;
        publish_guid(
            output,
            access,
            attributes,
            HandleKind::TransactionManager,
            || {
                STORE
                    .lock()
                    .open(HandleKind::TransactionManager, identity, None)
            },
        )
    })()
    .unwrap_or_else(|status| status)
}

/// Set TransactionManagerRecoveryInformation on an existing manager.
/// Win7 accepts only class 3 with no payload; volatile managers cannot recover
/// a durable log. Every manager published by this backend is volatile.
pub extern "C" fn nt_set_information_transaction_manager(
    handle: HANDLE,
    class: u32,
    _information: PVOID,
    length: u32,
) -> NTSTATUS {
    if class != 3 {
        return STATUS_INVALID_INFO_CLASS;
    }
    if length != 0 {
        return STATUS_INFO_LENGTH_MISMATCH;
    }
    (|| {
        let object = pin(handle, HandleKind::TransactionManager, 2)?;
        let status = match &STORE.lock().get(object.0)?.payload {
            Payload::Manager { .. } => Ok(store::TM_VOLATILE),
            _ => Err(STATUS_OBJECT_TYPE_MISMATCH),
        };
        status
    })()
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Pointers designate caller memory and are captured before the new handle.
pub unsafe extern "C" fn nt_create_transaction(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    uow: *const [u8; 16],
    manager: HANDLE,
    options: u32,
    isolation_level: u32,
    isolation_flags: u32,
    timeout: *const i64,
    description: *const UnicodeString,
) -> NTSTATUS {
    (|| {
        if options & !1 != 0 || isolation_level != 0 || isolation_flags != 0 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let identity = if uow.is_null() {
            None
        } else {
            Some(guid(uow)?)
        };
        let timeout = if timeout.is_null() {
            0
        } else {
            read_user_object::<i64>(timeout as u64)?
        };
        let description = capture_string(description)?;
        let manager = optional_manager(manager, 0x20)?;
        let transaction = Transaction {
            manager: manager.as_ref().map_or(0, |manager| manager.0),
            description,
            timeout,
            deadline: deadline(timeout),
            outcome: 1,
            timed_out: false,
            phase: 0,
        };
        publish(output, access, attributes, HandleKind::Transaction, || {
            STORE
                .lock()
                .insert(identity, Payload::Transaction(transaction))
        })
    })()
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Pointers designate caller memory and are captured by the native copy layer.
pub unsafe extern "C" fn nt_open_transaction(
    output: *mut HANDLE,
    access: u32,
    attributes: *mut ObjectAttributes,
    uow: *const [u8; 16],
    manager: HANDLE,
) -> NTSTATUS {
    (|| {
        let identity = guid(uow)?;
        let manager = optional_manager(manager, 0x20)?;
        let parent = Some(manager.as_ref().map_or(0, |manager| manager.0));
        publish_guid(output, access, attributes, HandleKind::Transaction, || {
            STORE.lock().open(HandleKind::Transaction, identity, parent)
        })
    })()
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Pointers designate caller memory and are captured before object publication.
pub unsafe extern "C" fn nt_create_resource_manager(
    output: *mut HANDLE,
    access: u32,
    manager: HANDLE,
    identity: *const [u8; 16],
    attributes: *mut ObjectAttributes,
    options: u32,
    description: *const UnicodeString,
) -> NTSTATUS {
    (|| {
        if options & !3 != 0 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        if options != 1 {
            return Err(STATUS_NOT_SUPPORTED);
        }
        let identity = guid(identity)?;
        let description = capture_string(description)?;
        let manager = pin(manager, HandleKind::TransactionManager, 0x10)?;
        publish(
            output,
            access,
            attributes,
            HandleKind::ResourceManager,
            || {
                STORE.lock().insert(
                    Some(identity),
                    Payload::ResourceManager {
                        manager: manager.0,
                        description,
                    },
                )
            },
        )
    })()
    .unwrap_or_else(|status| status)
}

/// # Safety
/// Pointers designate caller memory and are captured by the native copy layer.
pub unsafe extern "C" fn nt_open_resource_manager(
    output: *mut HANDLE,
    access: u32,
    manager: HANDLE,
    identity: *const [u8; 16],
    attributes: *mut ObjectAttributes,
) -> NTSTATUS {
    (|| {
        let identity = guid(identity)?;
        let manager = pin(manager, HandleKind::TransactionManager, 1)?;
        publish_guid(
            output,
            access,
            attributes,
            HandleKind::ResourceManager,
            || {
                STORE
                    .lock()
                    .open(HandleKind::ResourceManager, identity, Some(manager.0))
            },
        )
    })()
    .unwrap_or_else(|status| status)
}

fn query(
    handle: HANDLE,
    kind: HandleKind,
    class: u32,
    output: PVOID,
    length: u32,
    returned: *mut u32,
) -> NTSTATUS {
    (|| {
        if !returned.is_null() {
            probe_user_write(returned as u64, 4)?;
        }
        let object = pin(handle, kind, 1)?;
        let bytes = {
            let mut store = STORE.lock();
            let record = store.get_mut(object.0)?;
            let mut bytes;
            match &mut record.payload {
                Payload::Manager { clock } => match class {
                    0 => {
                        bytes = zeroed(24)?;
                        bytes[..16].copy_from_slice(&record.guid.0);
                        bytes[16..24].copy_from_slice(&clock.to_le_bytes());
                    }
                    1 | 2 | 4 => return Err(store::TM_VOLATILE),
                    5 => {
                        let oldest = store
                            .records
                            .iter()
                            .filter(|record| {
                                matches!(&record.payload, Payload::Transaction(transaction)
                                if transaction.manager == object.0 && transaction.outcome == 1)
                            })
                            .min_by_key(|record| record.id)
                            .map_or(Guid::ZERO, |record| record.guid);
                        bytes = zeroed(16)?;
                        bytes.copy_from_slice(&oldest.0);
                    }
                    _ => return Err(STATUS_INVALID_INFO_CLASS),
                },
                Payload::Transaction(transaction) => {
                    transaction.refresh();
                    match class {
                        0 => {
                            bytes = zeroed(24)?;
                            bytes[..16].copy_from_slice(&record.guid.0);
                            let state = 1u32;
                            bytes[16..20].copy_from_slice(&state.to_le_bytes());
                            bytes[20..24].copy_from_slice(&transaction.outcome.to_le_bytes());
                        }
                        1 => {
                            bytes = zeroed(24 + transaction.description.len())?;
                            bytes[8..16].copy_from_slice(&transaction.timeout.to_le_bytes());
                            bytes[16..20].copy_from_slice(&transaction.outcome.to_le_bytes());
                            bytes[20..24].copy_from_slice(
                                &(transaction.description.len() as u32).to_le_bytes(),
                            );
                            bytes[24..].copy_from_slice(&transaction.description);
                        }
                        2 => {
                            let mut pairs = Vec::new();
                            for record in &store.records {
                                if let Payload::Enlistment(enlistment) = &record.payload {
                                    if enlistment.transaction == object.0 {
                                        pairs.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
                                        pairs.push((record.guid, enlistment.resource_manager));
                                    }
                                }
                            }
                            bytes = zeroed(4 + pairs.len() * 32)?;
                            bytes[..4].copy_from_slice(&(pairs.len() as u32).to_le_bytes());
                            for (index, (identity, resource_manager)) in pairs.into_iter().enumerate() {
                                let offset = 4 + index * 32;
                                bytes[offset..offset + 16].copy_from_slice(&identity.0);
                                bytes[offset + 16..offset + 32].copy_from_slice(&store.get(resource_manager)?.guid.0);
                            }
                        }
                        3..=5 => return Err(STATUS_NOT_SUPPORTED),
                        _ => return Err(STATUS_INVALID_INFO_CLASS),
                    }
                }
                Payload::Enlistment(_) => return Err(STATUS_OBJECT_TYPE_MISMATCH),
                Payload::ResourceManager { description, .. } => match class {
                    0 => {
                        bytes = zeroed(20 + description.len())?;
                        bytes[..16].copy_from_slice(&record.guid.0);
                        bytes[16..20].copy_from_slice(&(description.len() as u32).to_le_bytes());
                        bytes[20..].copy_from_slice(description);
                    }
                    1 => return Err(STATUS_NOT_SUPPORTED),
                    _ => return Err(STATUS_INVALID_INFO_CLASS),
                },
            }
            bytes
        };
        let required = bytes.len() as u32;
        if !returned.is_null() {
            write_user_object(returned as u64, &required)?;
        }
        if length < required {
            return Err(STATUS_INFO_LENGTH_MISMATCH);
        }
        copy_to_user(output as u64, &bytes)?;
        Ok(STATUS_SUCCESS)
    })()
    .unwrap_or_else(|status| status)
}

macro_rules! query_service {
    ($name:ident, $kind:ident) => {
        /// # Safety
        /// Output pointers are probed and copied through the native copy layer.
        pub unsafe extern "C" fn $name(
            handle: HANDLE,
            class: u32,
            output: PVOID,
            length: u32,
            returned: *mut u32,
        ) -> NTSTATUS {
            query(handle, HandleKind::$kind, class, output, length, returned)
        }
    };
}
query_service!(nt_query_information_transaction_manager, TransactionManager);
query_service!(nt_query_information_transaction, Transaction);
query_service!(nt_query_information_resource_manager, ResourceManager);

/// # Safety
/// The variable-length input is captured in full before transaction mutation.
pub unsafe extern "C" fn nt_set_information_transaction(
    handle: HANDLE,
    class: u32,
    input: PVOID,
    length: u32,
) -> NTSTATUS {
    (|| {
        if class != 1 {
            return Err(if class <= 5 {
                STATUS_NOT_SUPPORTED
            } else {
                STATUS_INVALID_INFO_CLASS
            });
        }
        if length < 24 {
            return Err(STATUS_INFO_LENGTH_MISMATCH);
        }
        let header = read_user_object::<[u8; 24]>(input as u64)?;
        let isolation = u64::from_le_bytes(
            header[..8]
                .try_into()
                .map_err(|_| STATUS_INVALID_PARAMETER)?,
        );
        let timeout = i64::from_le_bytes(
            header[8..16]
                .try_into()
                .map_err(|_| STATUS_INVALID_PARAMETER)?,
        );
        let count = u32::from_le_bytes(
            header[20..24]
                .try_into()
                .map_err(|_| STATUS_INVALID_PARAMETER)?,
        );
        if isolation != 0 || count & 1 != 0 || count > length - 24 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let mut description = zeroed(count as usize)?;
        if !description.is_empty() {
            copy_from_user(
                &mut description,
                (input as u64)
                    .checked_add(24)
                    .ok_or(STATUS_ACCESS_VIOLATION)?,
            )?;
        }
        let object = pin(handle, HandleKind::Transaction, 2)?;
        let mut store = STORE.lock();
        let Payload::Transaction(transaction) = &mut store.get_mut(object.0)?.payload else {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        };
        transaction.refresh();
        if transaction.outcome != 1 {
            return Err(store::TRANSACTION_NOT_ACTIVE);
        }
        transaction.description = description;
        transaction.timeout = timeout;
        transaction.deadline = deadline(timeout);
        Ok(STATUS_SUCCESS)
    })()
    .unwrap_or_else(|status| status)
}

pub extern "C" fn nt_commit_transaction(handle: HANDLE, wait: u8) -> NTSTATUS {
    enlistment::finish_transaction(handle, true, wait != 0)
}

pub extern "C" fn nt_rollback_transaction(handle: HANDLE, wait: u8) -> NTSTATUS {
    enlistment::finish_transaction(handle, false, wait != 0)
}

/// Enumerate a stable page of live KTM GUIDs. LastQuery identifies the previous
/// record rather than an array index, so closing another object cannot skip one.
///
/// # Safety
/// Cursor and returned length designate caller-owned memory.
pub unsafe extern "C" fn nt_enumerate_transaction_object(
    root: HANDLE,
    query_type: u32,
    cursor: PVOID,
    length: u32,
    returned: *mut u32,
) -> NTSTATUS {
    (|| {
        let kind = match query_type {
            0 => HandleKind::Transaction,
            1 => HandleKind::TransactionManager,
            2 => HandleKind::ResourceManager,
            3 => HandleKind::Enlistment,
            _ => return Err(STATUS_INVALID_PARAMETER),
        };
        let root = if kind == HandleKind::Enlistment && !root.is_null() {
            Some(pin(root, HandleKind::ResourceManager, 1)?)
        } else {
            optional_manager(root, 1)?
        };
        if root.is_some() && kind == HandleKind::TransactionManager {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        if length < 20 {
            if !returned.is_null() {
                write_user_object(returned as u64, &36u32)?;
            }
            return Err(STATUS_BUFFER_TOO_SMALL);
        }
        let previous = guid(cursor.cast())?;
        let capacity = (length as usize - 20) / 16;
        // Reserving an unbounded caller-supplied length while holding STORE
        // would unnecessarily expose the kernel to memory pressure.
        let mut store = STORE.lock();
        let previous_id = if previous == Guid::ZERO {
            0
        } else {
            store
                .records
                .iter()
                .find(|record| {
                    record.guid == previous
                        && record.payload.kind() == kind
                        && root
                            .as_ref()
                            .is_none_or(|root| record.payload.parent() == root.0)
                })
                .map(|record| record.id)
                .ok_or(STATUS_NO_MORE_ENTRIES)?
        };
        let mut ids = Vec::new();
        for record in &mut store.records {
            if let Payload::Transaction(transaction) = &mut record.payload {
                transaction.refresh();
            }
            if record.id > previous_id
                && record.payload.kind() == kind
                && root
                    .as_ref()
                    .is_none_or(|root| record.payload.parent() == root.0)
            {
                ids.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
                ids.push((record.id, record.guid));
            }
        }
        drop(store);
        ids.sort_unstable_by_key(|entry| entry.0);
        if ids.is_empty() {
            return Err(STATUS_NO_MORE_ENTRIES);
        }
        if capacity == 0 {
            if !returned.is_null() {
                write_user_object(returned as u64, &36u32)?;
            }
            return Err(STATUS_BUFFER_TOO_SMALL);
        }
        let count = capacity.min(ids.len());
        let mut bytes = zeroed(20 + 16 * count)?;
        bytes[..16].copy_from_slice(&ids[count - 1].1 .0);
        bytes[16..20].copy_from_slice(&(count as u32).to_le_bytes());
        for (index, (_, id)) in ids[..count].iter().enumerate() {
            bytes[20 + 16 * index..36 + 16 * index].copy_from_slice(&id.0);
        }
        // Probe every output before writing either one.
        probe_user_write(cursor as u64, bytes.len())?;
        if !returned.is_null() {
            probe_user_write(returned as u64, 4)?;
        }
        copy_to_user(cursor as u64, &bytes)?;
        if !returned.is_null() {
            write_user_object(returned as u64, &(bytes.len() as u32))?;
        }
        Ok(STATUS_SUCCESS)
    })()
    .unwrap_or_else(|status| status)
}

/// Capture a ResourceManagerBasicInformation description without allowing the
/// caller to replace the stable resource-manager GUID.
/// # Safety
/// Input designates a caller-owned buffer of `length` bytes.
pub unsafe extern "C" fn nt_set_information_resource_manager(
    handle: HANDLE,
    class: u32,
    input: PVOID,
    length: u32,
) -> NTSTATUS {
    (|| {
        if class != 0 { return Err(STATUS_INVALID_INFO_CLASS); }
        if length < 20 { return Err(STATUS_INFO_LENGTH_MISMATCH); }
        let header = read_user_object::<[u8; 20]>(input as u64)?;
        let count = u32::from_le_bytes(header[16..20].try_into().map_err(|_| STATUS_INVALID_PARAMETER)?);
        if count & 1 != 0 || count > length - 20 { return Err(STATUS_INVALID_PARAMETER); }
        let mut captured = zeroed(count as usize)?;
        if count != 0 {
            copy_from_user(&mut captured, (input as u64).checked_add(20).ok_or(STATUS_ACCESS_VIOLATION)?)?;
        }
        let object = pin(handle, HandleKind::ResourceManager, 2)?;
        let mut store = STORE.lock();
        let Payload::ResourceManager { description, .. } = &mut store.get_mut(object.0)?.payload else {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        };
        *description = captured;
        Ok(STATUS_SUCCESS)
    })().unwrap_or_else(|status| status)
}

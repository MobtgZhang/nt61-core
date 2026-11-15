//! Native handles reference shared objects; synchronization transactions hold
//! the table lock across observation and consumption. Handle aliases therefore
//! see the same signal/ownership state, and final close retires object metadata.
use super::super::alert_state::UserAlertState;
use super::super::status::{STATUS_INVALID_HANDLE, STATUS_INVALID_PARAMETER};
use super::super::types::{HANDLE, NTSTATUS};
use crate::ke::sync::Spinlock;
use alloc::vec::Vec;
use core::ptr;

const MAX_HANDLES: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct MutantState {
    pub owner_thread: u64,
    pub owner_process: u64,
    pub depth: u32,
    pub abandoned: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimerState {
    pub timer_type: u32,
    pub deadline: u64,
    pub absolute: bool,
    pub period_ms: u32,
    pub signaled: bool,
    pub armed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatcherState {
    None,
    Mutant(MutantState),
    Timer(TimerState),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct HandleEntry {
    pub kind: HandleKind,
    pub target: u64,
    pub identity: u64,
    pub dispatcher: DispatcherState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandleKind {
    File,
    Process,
    Thread,
    Token,
    Section,
    Event,
    Mutant,
    Semaphore,
    Timer,
    /// Pipe slot index and direction bit (bit 32).
    Pipe,
    /// AFD endpoint handle identifier.
    Afd,
}

#[derive(Clone, Copy)]
struct HandleSlot {
    object: usize,
    flags: u8,
    access: u32,
}

#[derive(Clone, Copy)]
struct HandleObject {
    entry: HandleEntry,
    handles: usize,
    waits: usize,
}

static HANDLE_TABLE: Spinlock<HandleTable> = Spinlock::new(HandleTable::new());

struct HandleTable {
    slots: [Option<HandleSlot>; MAX_HANDLES],
    objects: [Option<HandleObject>; MAX_HANDLES],
    next_identity: u64,
    names: Vec<(usize, Vec<u16>)>,
    waits: [Option<WaitRequest>; 256],
    next_wait_identity: u64,
    pending_waits: [u64; 4],
    timers: [u64; MAX_HANDLES / 64],
}

impl HandleTable {
    const fn new() -> Self {
        Self {
            slots: [None; MAX_HANDLES],
            objects: [None; MAX_HANDLES],
            next_identity: 1,
            names: Vec::new(),
            waits: [None; 256],
            next_wait_identity: 1,
            pending_waits: [0; 4],
            timers: [0; MAX_HANDLES / 64],
        }
    }
}

pub(crate) fn alloc_handle(kind: HandleKind, target: u64) -> HANDLE {
    alloc_dispatcher_handle(kind, target, DispatcherState::None)
}

pub(crate) fn alloc_dispatcher_handle(
    kind: HandleKind,
    target: u64,
    dispatcher: DispatcherState,
) -> HANDLE {
    let mut table = HANDLE_TABLE.lock();
    let access = match kind {
        HandleKind::Event | HandleKind::Semaphore | HandleKind::Timer => 0x001f0003,
        HandleKind::Mutant => 0x001f0001,
        _ => u32::MAX,
    };
    table
        .allocate(kind, target, dispatcher, access, 0)
        .unwrap_or(ptr::null_mut())
}

impl HandleTable {
    fn allocate(
        &mut self,
        kind: HandleKind,
        target: u64,
        dispatcher: DispatcherState,
        access: u32,
        flags: u8,
    ) -> Result<HANDLE, NTSTATUS> {
        use super::super::status::STATUS_NO_MEMORY;
        let slot = self
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(STATUS_NO_MEMORY)?;
        let object = self
            .objects
            .iter()
            .position(Option::is_none)
            .ok_or(STATUS_NO_MEMORY)?;
        let identity = self.next_identity;
        self.next_identity = identity.checked_add(1).ok_or(STATUS_NO_MEMORY)?;
        self.objects[object] = Some(HandleObject {
            entry: HandleEntry {
                kind,
                target,
                identity,
                dispatcher,
            },
            handles: 1,
            waits: 0,
        });
        self.slots[slot] = Some(HandleSlot {
            object,
            flags,
            access,
        });
        if kind == HandleKind::Timer {
            self.timers[object / 64] |= 1 << (object % 64);
        }
        Ok(encode_handle(slot))
    }

    fn alias(&mut self, object: usize, access: u32, flags: u8) -> Result<HANDLE, NTSTATUS> {
        let slot = self
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(super::super::status::STATUS_NO_MEMORY)?;
        self.objects[object]
            .as_mut()
            .ok_or(STATUS_INVALID_HANDLE)?
            .handles += 1;
        self.slots[slot] = Some(HandleSlot {
            object,
            flags,
            access,
        });
        Ok(encode_handle(slot))
    }
}

/// Resolve a name and allocate the handle in one transaction. Names are weak
/// namespace links: final handle close removes them even if a wait pins state.
pub(crate) fn create_sync_handle(
    kind: HandleKind,
    target: u64,
    dispatcher: DispatcherState,
    access: u32,
    attributes: super::super::object::SyncAttributes,
    create: bool,
) -> Result<(HANDLE, NTSTATUS), NTSTATUS> {
    use super::super::status::*;
    let mut table = HANDLE_TABLE.lock();
    if let Some(name) = &attributes.name {
        if let Some((object, _)) = table.names.iter().find(|(_, existing)| {
            super::super::object::equal_sync_name(existing, name, attributes.case_insensitive)
        }) {
            let object = *object;
            if table.objects[object]
                .ok_or(STATUS_INVALID_HANDLE)?
                .entry
                .kind
                != kind
            {
                return Err(STATUS_OBJECT_TYPE_MISMATCH);
            }
            if create && !attributes.open_if {
                return Err(STATUS_OBJECT_NAME_COLLISION);
            }
            let handle = table.alias(object, access, attributes.flags)?;
            return Ok((
                handle,
                if create {
                    STATUS_OBJECT_NAME_EXISTS
                } else {
                    STATUS_SUCCESS
                },
            ));
        }
        if !create {
            return Err(STATUS_OBJECT_NAME_NOT_FOUND);
        }
        table.names.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
    } else if !create {
        return Err(STATUS_OBJECT_NAME_INVALID);
    }
    let handle = table.allocate(kind, target, dispatcher, access, attributes.flags)?;
    if let Some(name) = attributes.name {
        let object = table.slots[decode_handle(handle)]
            .ok_or(STATUS_INVALID_HANDLE)?
            .object;
        table.names.push((object, name));
    }
    Ok((handle, STATUS_SUCCESS))
}

/// Check access while holding the dispatcher lock, preventing handle reuse
/// between the check and state mutation.
pub(crate) fn with_sync_access<T>(
    handle: HANDLE,
    access: u32,
    operation: impl FnOnce(&mut [HandleEntry]) -> Result<T, NTSTATUS>,
) -> Result<T, NTSTATUS> {
    with_sync_handles_access(&[handle], access, operation)
}

/// Create an alias of a synchronization object, preserving its identity/state.
/// Other object kinds have additional resource ownership contracts and must
/// gain their own final-close integration before they can be aliased here.
pub(crate) fn duplicate_sync_handle(handle: HANDLE) -> Result<HANDLE, NTSTATUS> {
    duplicate_sync_handle_with_access(handle, 0, 0, 2)
}

/// Close-only duplication must validate the type under the same lock as close.
/// Other resource kinds require their resource-specific teardown path.
pub(crate) fn close_sync_handle(handle: HANDLE) -> Result<(), NTSTATUS> {
    let mut table = HANDLE_TABLE.lock();
    let slot = table
        .slots
        .get(decode_handle(handle))
        .and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    let entry = table.objects[slot.object]
        .ok_or(STATUS_INVALID_HANDLE)?
        .entry;
    if !matches!(
        entry.kind,
        HandleKind::Event | HandleKind::Semaphore | HandleKind::Mutant | HandleKind::Timer
    ) {
        return Err(super::super::status::STATUS_NOT_SUPPORTED);
    }
    if slot.flags & 2 != 0 {
        return Err(super::super::status::STATUS_HANDLE_NOT_CLOSABLE);
    }
    table.retire_sync_slot(decode_handle(handle), slot.object);
    Ok(())
}

impl HandleTable {
    fn retire_sync_slot(&mut self, slot: usize, object: usize) {
        self.slots[slot] = None;
        if let Some(value) = &mut self.objects[object] {
            value.handles -= 1;
        }
        if self.objects[object].is_some_and(|value| value.handles == 0) {
            self.names.retain(|(index, _)| *index != object);
            if self.objects[object].is_some_and(|value| value.waits == 0) {
                self.objects[object] = None;
                self.timers[object / 64] &= !(1 << (object % 64));
            }
        }
    }
}

/// Synchronization aliases have no independent resource teardown. Cross-process
/// ownership and arbitrary access escalation require the security subsystem.
pub(crate) fn duplicate_sync_handle_with_access(
    handle: HANDLE,
    desired_access: u32,
    flags: u8,
    options: u32,
) -> Result<HANDLE, NTSTATUS> {
    use super::super::status::*;
    if options & !7 != 0 || flags & !3 != 0 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut table = HANDLE_TABLE.lock();
    let source_index = decode_handle(handle);
    let source = table
        .slots
        .get(source_index)
        .and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    let entry = table.objects[source.object]
        .ok_or(STATUS_INVALID_HANDLE)?
        .entry;
    if !matches!(
        entry.kind,
        HandleKind::Event | HandleKind::Semaphore | HandleKind::Mutant | HandleKind::Timer
    ) {
        return Err(STATUS_NOT_SUPPORTED);
    }
    if options & 1 != 0 && source.flags & 2 != 0 {
        return Err(STATUS_HANDLE_NOT_CLOSABLE);
    }
    let result = (|| {
        let access = if options & 2 != 0 {
            source.access
        } else {
            super::super::object::map_sync_access(entry.kind, desired_access)?
        };
        if access & !source.access != 0 {
            return Err(STATUS_ACCESS_DENIED);
        }
        let attributes = if options & 4 != 0 {
            source.flags
        } else {
            flags
        };
        table.alias(source.object, access, attributes)
    })();
    // CLOSE_SOURCE applies even when access validation or target allocation
    // fails. Only dispatcher objects reach this path, so final close has no
    // external file/section resource teardown to run after unlocking.
    if options & 1 != 0 {
        table.retire_sync_slot(source_index, source.object);
    }
    result
}

pub(crate) fn free_handle(handle: HANDLE) -> bool {
    remove_handle(handle, false).is_ok()
}

/// Remove a caller-owned handle, honoring HANDLE_FLAG_PROTECT_FROM_CLOSE.
/// Resource destruction must follow this atomic removal, never precede it.
pub(crate) fn close_handle(handle: HANDLE) -> Result<(HandleEntry, bool), NTSTATUS> {
    remove_handle(handle, true)
}

pub(crate) fn handle_flags(handle: HANDLE) -> Result<u8, NTSTATUS> {
    let table = HANDLE_TABLE.lock();
    table
        .slots
        .get(decode_handle(handle))
        .and_then(|slot| *slot)
        .map(|slot| slot.flags)
        .ok_or(STATUS_INVALID_HANDLE)
}

/// Resolve THREAD_SUSPEND_RESUME before releasing the handle-table lock. No
/// operation later reuses the handle value, so close/reallocation cannot retarget
/// this alert to an unrelated thread.
pub(crate) fn thread_alert_target(handle: HANDLE) -> Result<u64, NTSTATUS> {
    use super::super::status::*;
    let table = HANDLE_TABLE.lock();
    let slot = table
        .slots
        .get(decode_handle(handle))
        .and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    let object = table.objects[slot.object].ok_or(STATUS_INVALID_HANDLE)?;
    if object.entry.kind != HandleKind::Thread {
        return Err(STATUS_OBJECT_TYPE_MISMATCH);
    }
    if slot.access & 2 == 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    Ok(object.entry.target)
}

/// Alert an already queued alertable wait immediately; otherwise retain one
/// pending alert. Completed waits are never overwritten, and duplicate alerts
/// coalesce. The state belongs to a live, allocated ETHREAD, not a TID bucket.
pub(crate) fn alert_user_thread(tid: u64, state: &UserAlertState) -> NTSTATUS {
    use super::super::status::*;
    let mut table = HANDLE_TABLE.lock();
    if state.terminated() {
        return STATUS_THREAD_IS_TERMINATING;
    }
    if state.pending() {
        return STATUS_SUCCESS;
    }
    for index in 0..table.waits.len() {
        if table.waits[index]
            .is_some_and(|wait| wait.tid == tid && wait.alertable && wait.result.is_none())
        {
            if let Some(wait) = &mut table.waits[index] {
                wait.result = Some(STATUS_ALERTED);
            }
            table.pending_waits[index / 64] &= !(1 << (index % 64));
            return STATUS_SUCCESS;
        }
    }
    state.set();
    STATUS_SUCCESS
}

pub(crate) fn take_user_alert(state: &UserAlertState) -> bool {
    let _table = HANDLE_TABLE.lock();
    state.take()
}

pub(crate) fn terminate_user_alert(state: &UserAlertState) {
    let _table = HANDLE_TABLE.lock();
    state.terminate();
}

/// Copy the name while the slot and its object identity are locked together.
/// A concurrent close/reuse cannot make a query report another object's name.
pub(crate) fn sync_object_name(handle: HANDLE) -> Result<Option<Vec<u16>>, NTSTATUS> {
    use super::super::status::{STATUS_NOT_IMPLEMENTED, STATUS_NO_MEMORY};
    let table = HANDLE_TABLE.lock();
    let slot = table
        .slots
        .get(decode_handle(handle))
        .and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    let entry = table.objects[slot.object]
        .ok_or(STATUS_INVALID_HANDLE)?
        .entry;
    if !matches!(
        entry.kind,
        HandleKind::Event | HandleKind::Semaphore | HandleKind::Mutant | HandleKind::Timer
    ) {
        return Err(STATUS_NOT_IMPLEMENTED);
    }
    table
        .names
        .iter()
        .find(|(index, _)| *index == slot.object)
        .map(|(_, name)| {
            let mut result = Vec::new();
            result
                .try_reserve_exact(name.len())
                .map_err(|_| STATUS_NO_MEMORY)?;
            result.extend_from_slice(name);
            Ok(result)
        })
        .transpose()
}

/// Both flag updates and close decisions serialize on the same table lock.
pub(crate) fn update_handle_flags(handle: HANDLE, mask: u8, flags: u8) -> Result<(), NTSTATUS> {
    let mut table = HANDLE_TABLE.lock();
    let slot = table
        .slots
        .get_mut(decode_handle(handle))
        .and_then(Option::as_mut)
        .ok_or(STATUS_INVALID_HANDLE)?;
    slot.flags = (slot.flags & !(mask & 3)) | (flags & mask & 3);
    Ok(())
}

fn remove_handle(handle: HANDLE, protect: bool) -> Result<(HandleEntry, bool), NTSTATUS> {
    let mut table = HANDLE_TABLE.lock();
    let index = decode_handle(handle);
    let slot = table
        .slots
        .get(index)
        .and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?;
    if protect && slot.flags & 2 != 0 {
        return Err(super::super::status::STATUS_HANDLE_NOT_CLOSABLE);
    }
    let object = table.objects[slot.object]
        .as_mut()
        .ok_or(STATUS_INVALID_HANDLE)?;
    let entry = object.entry;
    object.handles -= 1;
    table.slots[index] = None;
    let object = table.objects[slot.object]
        .as_ref()
        .ok_or(STATUS_INVALID_HANDLE)?;
    let last_handle = object.handles == 0;
    let retire = last_handle && object.waits == 0;
    if last_handle {
        table.names.retain(|(index, _)| *index != slot.object);
    }
    let retired = if retire {
        table.timers[slot.object / 64] &= !(1 << (slot.object % 64));
        table.objects[slot.object].take().map(|object| object.entry)
    } else {
        None
    };
    drop(table);
    if let Some(HandleEntry {
        kind: HandleKind::Section,
        target,
        ..
    }) = retired
    {
        super::super::section::release_section(target);
    }
    Ok((entry, retired.is_some()))
}

pub(crate) fn lookup_handle(handle: HANDLE) -> Option<HandleEntry> {
    let table = HANDLE_TABLE.lock();
    let slot = table
        .slots
        .get(decode_handle(handle))
        .and_then(|slot| *slot)?;
    table.objects[slot.object].map(|object| object.entry)
}

/// Run a synchronization transaction without allowing concurrent close/reuse.
/// Changes commit only on success. WaitAny aliases cannot restore consumed state.
/// The closure must not reenter the handle table or retain any borrowed entry.
pub(crate) fn with_sync_handles<T>(
    handles: &[HANDLE],
    operation: impl FnOnce(&mut [HandleEntry]) -> Result<T, NTSTATUS>,
) -> Result<T, NTSTATUS> {
    with_sync_handles_access(handles, 0, operation)
}

fn with_sync_handles_access<T>(
    handles: &[HANDLE],
    access: u32,
    operation: impl FnOnce(&mut [HandleEntry]) -> Result<T, NTSTATUS>,
) -> Result<T, NTSTATUS> {
    if handles.is_empty() || handles.len() > 64 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut table = HANDLE_TABLE.lock();
    let mut entries = [HandleEntry {
        kind: HandleKind::Event,
        target: 0,
        identity: 0,
        dispatcher: DispatcherState::None,
    }; 64];
    let mut objects = [0; 64];
    for (offset, &handle) in handles.iter().enumerate() {
        let slot = table
            .slots
            .get(decode_handle(handle))
            .and_then(|slot| *slot)
            .ok_or(STATUS_INVALID_HANDLE)?;
        if slot.access & access != access {
            return Err(super::super::status::STATUS_ACCESS_DENIED);
        }
        let object = slot.object;
        entries[offset] = table.objects[object].ok_or(STATUS_INVALID_HANDLE)?.entry;
        objects[offset] = object;
    }
    let result = operation(&mut entries[..handles.len()])?;
    let mut changed = [false; 64];
    for index in 0..handles.len() {
        changed[index] =
            table.objects[objects[index]].is_some_and(|object| object.entry != entries[index]);
    }
    for index in 0..handles.len() {
        if changed[index] {
            if let Some(object) = &mut table.objects[objects[index]] {
                object.entry = entries[index];
            }
        }
    }
    table.satisfy_waiters();
    Ok(result)
}

/// Release every mutant owned by an exiting thread. The next acquiring waiter
/// observes STATUS_ABANDONED, including when other handles still reference it.
pub(crate) fn abandon_mutants(thread: u64) {
    if thread == 0 {
        return;
    }
    let mut table = HANDLE_TABLE.lock();
    // A terminating thread must not reacquire an object while its ownership
    // is being abandoned. Its syscall stack may never run Drop again.
    for index in 0..table.waits.len() {
        if table.waits[index].is_some_and(|wait| wait.tid == thread) {
            table.remove_wait(index);
        }
    }
    for object in table.objects.iter_mut().flatten() {
        if let DispatcherState::Mutant(state) = &mut object.entry.dispatcher {
            if state.depth != 0 && state.owner_thread == thread {
                state.depth = 0;
                state.owner_thread = 0;
                state.owner_process = 0;
                state.abandoned = true;
            }
        }
    }
    table.satisfy_waiters();
}

/// Publish expiration without user memory access. Snapshot clocks before taking
/// the object table so the clock ISR never inverts the table -> clock order.
pub(crate) fn tick_timers() {
    let now = crate::ke::time::get_time_snapshot();
    let mut table = HANDLE_TABLE.lock();
    for word in 0..table.timers.len() {
        let mut bits = table.timers[word];
        while bits != 0 {
            let index = word * 64 + bits.trailing_zeros() as usize;
            bits &= bits - 1;
            if let Some(object) = &mut table.objects[index] {
                if let DispatcherState::Timer(state) = &mut object.entry.dispatcher {
                    super::super::timer::refresh_timer_at(
                        state,
                        now.system_time,
                        now.interrupt_time,
                    );
                }
            }
        }
    }
    table.satisfy_waiters_at(now.system_time, now.interrupt_time);
}

#[derive(Clone, Copy)]
struct WaitRequest {
    identity: u64,
    objects: [usize; 64],
    count: usize,
    all: bool,
    pid: u64,
    tid: u64,
    alertable: bool,
    deadline: Option<(bool, u64)>, // absolute wall time, or relative uptime
    result: Option<NTSTATUS>,
}

/// A live queued wait pins all target objects, so closing and reusing a handle
/// slot cannot redirect it to a different object. Drop also cancels a wait when
/// its caller unwinds in the host tests. Kernel termination cleanup is separate.
pub(crate) struct QueuedWait {
    slot: usize,
    identity: u64,
}

impl QueuedWait {
    pub(crate) fn poll(&self) -> Option<NTSTATUS> {
        let mut table = HANDLE_TABLE.lock();
        table.satisfy_waiters();
        match table.waits[self.slot] {
            Some(wait) if wait.identity == self.identity => wait.result,
            _ => Some(super::super::status::STATUS_THREAD_IS_TERMINATING),
        }
    }
}

impl Drop for QueuedWait {
    fn drop(&mut self) {
        let mut table = HANDLE_TABLE.lock();
        if table.waits[self.slot].is_some_and(|wait| wait.identity == self.identity) {
            table.remove_wait(self.slot);
        }
    }
}

pub(crate) enum WaitStart {
    Complete(NTSTATUS),
    Pending(QueuedWait),
}

pub(crate) fn begin_wait(
    handles: &[HANDLE],
    all: bool,
    pid: u64,
    tid: u64,
    timeout: Option<i64>,
    alert: Option<&UserAlertState>,
) -> Result<WaitStart, NTSTATUS> {
    let mut table = HANDLE_TABLE.lock();
    let wait = table.prepare_wait(handles, all, pid, tid, timeout)?;
    table.start_wait(wait, alert)
}

/// Validate both objects before signaling and registering the new wait under
/// one dispatcher lock. The signal can satisfy existing queued requests first.
pub(crate) fn signal_and_begin_wait(
    signal: HANDLE,
    handle: HANDLE,
    pid: u64,
    tid: u64,
    timeout: Option<i64>,
    alert: Option<&UserAlertState>,
) -> Result<WaitStart, NTSTATUS> {
    use super::super::status::*;
    let mut table = HANDLE_TABLE.lock();
    let wait = table.prepare_wait(&[handle], false, pid, tid, timeout)?;
    let object = table
        .slots
        .get(decode_handle(signal))
        .and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?
        .object;
    let mut entry = table.objects[object].ok_or(STATUS_INVALID_HANDLE)?.entry;
    let slot = table.slots[decode_handle(signal)].ok_or(STATUS_INVALID_HANDLE)?;
    if matches!(entry.kind, HandleKind::Event | HandleKind::Semaphore) && slot.access & 2 == 0 {
        return Err(STATUS_ACCESS_DENIED);
    }
    match entry.kind {
        HandleKind::Event => entry.target |= 1,
        HandleKind::Semaphore => {
            if entry.target as u32 == (entry.target >> 32) as u32 {
                return Err(STATUS_SEMAPHORE_LIMIT_EXCEEDED);
            }
            entry.target += 1;
        }
        HandleKind::Mutant => {
            let DispatcherState::Mutant(state) = &mut entry.dispatcher else {
                return Err(STATUS_INVALID_HANDLE);
            };
            if state.depth == 0 || state.owner_thread != tid || tid == 0 {
                return Err(STATUS_MUTEX_NOT_OWNED);
            }
            state.depth -= 1;
            if state.depth == 0 {
                state.owner_thread = 0;
                state.owner_process = 0;
            }
        }
        _ => return Err(STATUS_OBJECT_TYPE_MISMATCH),
    }
    // Reserve capacity before publishing a signal so resource exhaustion does
    // not partially execute this combined operation.
    if table.waits.iter().all(Option::is_some) {
        return Err(STATUS_NO_MEMORY);
    }
    if let Some(object) = &mut table.objects[object] {
        object.entry = entry;
    }
    table.satisfy_waiters();
    table.start_wait(wait, alert)
}

impl HandleTable {
    fn prepare_wait(
        &self,
        handles: &[HANDLE],
        all: bool,
        pid: u64,
        tid: u64,
        timeout: Option<i64>,
    ) -> Result<WaitRequest, NTSTATUS> {
        use super::super::status::*;
        if handles.is_empty() || handles.len() > 64 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        let now = crate::ke::time::get_time_snapshot();
        let deadline = timeout.map(|raw| {
            if raw < 0 {
                (false, now.interrupt_time.saturating_add(raw.unsigned_abs()))
            } else {
                (true, raw as u64)
            }
        });
        let mut wait = WaitRequest {
            identity: 0,
            objects: [0; 64],
            count: handles.len(),
            all,
            pid,
            tid,
            alertable: false,
            deadline,
            result: None,
        };
        for (index, &handle) in handles.iter().enumerate() {
            let slot = self
                .slots
                .get(decode_handle(handle))
                .and_then(|slot| *slot)
                .ok_or(STATUS_INVALID_HANDLE)?;
            if slot.access & 0x0010_0000 == 0 {
                return Err(STATUS_ACCESS_DENIED);
            }
            let object = slot.object;
            if all && wait.objects[..index].contains(&object) {
                return Err(STATUS_INVALID_PARAMETER);
            }
            wait.objects[index] = object;
            let entry = self.objects[object].ok_or(STATUS_INVALID_HANDLE)?.entry;
            if !matches!(
                entry.kind,
                HandleKind::Event | HandleKind::Semaphore | HandleKind::Mutant | HandleKind::Timer
            ) {
                return Err(
                    if matches!(entry.kind, HandleKind::Process | HandleKind::Thread) {
                        STATUS_NOT_SUPPORTED
                    } else {
                        STATUS_OBJECT_TYPE_MISMATCH
                    },
                );
            }
            if entry.kind == HandleKind::Mutant && tid == 0 {
                return Err(STATUS_INVALID_HANDLE);
            }
        }
        Ok(wait)
    }

    fn start_wait(
        &mut self,
        mut wait: WaitRequest,
        alert: Option<&UserAlertState>,
    ) -> Result<WaitStart, NTSTATUS> {
        use super::super::status::*;
        let now = crate::ke::time::get_time_snapshot();
        if let Some(result) = self.try_satisfy(&wait, now.system_time, now.interrupt_time, false) {
            return Ok(WaitStart::Complete(result));
        }
        wait.alertable = alert.is_some();
        if alert.is_some_and(UserAlertState::take) {
            return Ok(WaitStart::Complete(STATUS_ALERTED));
        }
        if wait.deadline.is_some_and(|(absolute, deadline)| {
            deadline
                <= if absolute {
                    now.system_time
                } else {
                    now.interrupt_time
                }
        }) {
            return Ok(WaitStart::Complete(STATUS_TIMEOUT));
        }
        let slot = self
            .waits
            .iter()
            .position(Option::is_none)
            .ok_or(STATUS_NO_MEMORY)?;
        wait.identity = self.next_wait_identity;
        self.next_wait_identity = self
            .next_wait_identity
            .checked_add(1)
            .ok_or(STATUS_NO_MEMORY)?;
        for &object in &wait.objects[..wait.count] {
            if let Some(object) = &mut self.objects[object] {
                object.waits += 1;
            }
        }
        wait.result = None;
        self.waits[slot] = Some(wait);
        self.pending_waits[slot / 64] |= 1 << (slot % 64);
        Ok(WaitStart::Pending(QueuedWait {
            slot,
            identity: wait.identity,
        }))
    }

    /// Observe every object first; a failing WaitAll consumes nothing.
    fn try_satisfy(
        &mut self,
        wait: &WaitRequest,
        wall: u64,
        uptime: u64,
        check_timeout: bool,
    ) -> Option<NTSTATUS> {
        use super::super::status::*;
        let mut selected = None;
        let mut all_ready = true;
        for (index, &object) in wait.objects[..wait.count].iter().enumerate() {
            let entry = &mut self.objects[object].as_mut()?.entry;
            let ready = match entry.kind {
                HandleKind::Event => entry.target & 1 != 0,
                HandleKind::Semaphore => entry.target as u32 != 0,
                HandleKind::Mutant => match entry.dispatcher {
                    DispatcherState::Mutant(state) => {
                        state.depth == 0 || state.owner_thread == wait.tid
                    }
                    _ => return Some(STATUS_INVALID_HANDLE),
                },
                HandleKind::Timer => match &mut entry.dispatcher {
                    DispatcherState::Timer(state) => {
                        super::super::timer::refresh_timer_at(state, wall, uptime);
                        state.signaled
                    }
                    _ => return Some(STATUS_INVALID_HANDLE),
                },
                _ => return Some(STATUS_OBJECT_TYPE_MISMATCH),
            };
            all_ready &= ready;
            if ready && selected.is_none() {
                selected = Some(index);
            }
        }
        if if wait.all {
            !all_ready
        } else {
            selected.is_none()
        } {
            return if check_timeout
                && wait.deadline.is_some_and(|(absolute, deadline)| {
                    deadline <= if absolute { wall } else { uptime }
                }) {
                Some(STATUS_TIMEOUT)
            } else {
                None
            };
        }
        // Check every recursive acquisition before modifying any other object.
        for (index, &object) in wait.objects[..wait.count].iter().enumerate() {
            if wait.all || Some(index) == selected {
                if let DispatcherState::Mutant(state) =
                    self.objects[object].as_ref()?.entry.dispatcher
                {
                    if state.depth == 0x8000_0001 {
                        return Some(STATUS_MUTANT_LIMIT_EXCEEDED);
                    }
                }
            }
        }
        let mut abandoned = None;
        for (index, &object) in wait.objects[..wait.count].iter().enumerate() {
            if wait.all || Some(index) == selected {
                let entry = &mut self.objects[object].as_mut()?.entry;
                match entry.kind {
                    HandleKind::Event if entry.target & 0x100 == 0 => entry.target &= !1,
                    HandleKind::Semaphore => entry.target -= 1,
                    HandleKind::Mutant => {
                        if let DispatcherState::Mutant(state) = &mut entry.dispatcher {
                            if state.abandoned {
                                abandoned.get_or_insert(index);
                            }
                            state.abandoned = false;
                            state.owner_thread = wait.tid;
                            state.owner_process = wait.pid;
                            state.depth += 1;
                        }
                    }
                    HandleKind::Timer => {
                        if let DispatcherState::Timer(state) = &mut entry.dispatcher {
                            if state.timer_type == 1 {
                                state.signaled = false;
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        Some(if let Some(index) = abandoned {
            STATUS_ABANDONED_WAIT_0 + if wait.all { 0 } else { index as i32 }
        } else if wait.all {
            STATUS_SUCCESS
        } else {
            selected.unwrap_or(0) as i32
        })
    }

    fn satisfy_waiters(&mut self) {
        let now = crate::ke::time::get_time_snapshot();
        self.satisfy_waiters_at(now.system_time, now.interrupt_time);
    }

    fn satisfy_waiters_at(&mut self, wall: u64, uptime: u64) {
        for word in 0..self.pending_waits.len() {
            let mut bits = self.pending_waits[word];
            while bits != 0 {
                let index = word * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                if let Some(wait) = self.waits[index] {
                    let result = self.try_satisfy(&wait, wall, uptime, true);
                    if result.is_some() {
                        self.pending_waits[word] &= !(1 << (index % 64));
                    }
                    if let Some(request) = &mut self.waits[index] {
                        request.result = result;
                    }
                }
            }
        }
    }

    fn remove_wait(&mut self, slot: usize) {
        self.pending_waits[slot / 64] &= !(1 << (slot % 64));
        if let Some(wait) = self.waits[slot].take() {
            for &index in &wait.objects[..wait.count] {
                if let Some(object) = &mut self.objects[index] {
                    object.waits -= 1;
                    if object.waits == 0 && object.handles == 0 {
                        // Only synchronization objects may be pinned by waits.
                        self.objects[index] = None;
                        self.timers[index / 64] &= !(1 << (index % 64));
                    }
                }
            }
        }
    }
}

/// Set and immediately clear an event under the same dispatcher lock. Only
/// requests already queued can observe the pulse, including atomic WaitAll.
pub(crate) fn pulse_event(handle: HANDLE, previous: *mut u32) -> NTSTATUS {
    use super::super::status::*;
    let mut table = HANDLE_TABLE.lock();
    let Some(slot) = table
        .slots
        .get(decode_handle(handle))
        .and_then(|slot| *slot)
    else {
        return STATUS_INVALID_HANDLE;
    };
    let Some(object) = &mut table.objects[slot.object] else {
        return STATUS_INVALID_HANDLE;
    };
    if object.entry.kind != HandleKind::Event {
        return STATUS_OBJECT_TYPE_MISMATCH;
    }
    if slot.access & 2 == 0 {
        return STATUS_ACCESS_DENIED;
    }
    if !previous.is_null() {
        if let Err(status) = crate::mm::user_copy::write_user_object(
            previous as u64,
            &((object.entry.target & 1) as u32),
        ) {
            return status;
        }
    }
    object.entry.target |= 1;
    table.satisfy_waiters();
    if let Some(object) = &mut table.objects[slot.object] {
        object.entry.target &= !1;
    }
    STATUS_SUCCESS
}

fn encode_handle(index: usize) -> HANDLE {
    (index + 1) as HANDLE
}

fn decode_handle(handle: HANDLE) -> usize {
    (handle as usize).checked_sub(1).unwrap_or(usize::MAX)
}

pub fn handle_count() -> usize {
    HANDLE_TABLE
        .lock()
        .slots
        .iter()
        .filter(|entry| entry.is_some())
        .count()
}

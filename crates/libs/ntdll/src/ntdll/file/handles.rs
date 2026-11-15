//! Native handles reference shared objects; synchronization transactions hold
//! the table lock across observation and consumption. Handle aliases therefore
//! see the same signal/ownership state, and final close retires object metadata.
use super::super::status::{STATUS_INVALID_HANDLE, STATUS_INVALID_PARAMETER};
use super::super::types::{HANDLE, NTSTATUS};
use crate::ke::sync::Spinlock;
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
    let Some(slot) = table.slots.iter().position(Option::is_none) else {
        return ptr::null_mut();
    };
    let Some(object) = table.objects.iter().position(Option::is_none) else {
        return ptr::null_mut();
    };
    let identity = table.next_identity;
    let Some(next) = identity.checked_add(1) else {
        return ptr::null_mut();
    };
    table.next_identity = next;
    table.objects[object] = Some(HandleObject {
        entry: HandleEntry {
            kind,
            target,
            identity,
            dispatcher,
        },
        handles: 1,
        waits: 0,
    });
    table.slots[slot] = Some(HandleSlot { object, flags: 0 });
    if kind == HandleKind::Timer {
        table.timers[object / 64] |= 1 << (object % 64);
    }
    encode_handle(slot)
}

/// Create an alias of a synchronization object, preserving its identity/state.
/// Other object kinds have additional resource ownership contracts and must
/// gain their own final-close integration before they can be aliased here.
pub(crate) fn duplicate_sync_handle(handle: HANDLE) -> Result<HANDLE, NTSTATUS> {
    let mut table = HANDLE_TABLE.lock();
    let object = table
        .slots
        .get(decode_handle(handle))
        .and_then(|slot| *slot)
        .ok_or(STATUS_INVALID_HANDLE)?
        .object;
    let entry = table.objects[object].ok_or(STATUS_INVALID_HANDLE)?.entry;
    if !matches!(
        entry.kind,
        HandleKind::Event | HandleKind::Semaphore | HandleKind::Mutant | HandleKind::Timer
    ) {
        return Err(super::super::status::STATUS_NOT_SUPPORTED);
    }
    let slot = table
        .slots
        .iter()
        .position(Option::is_none)
        .ok_or(super::super::status::STATUS_NO_MEMORY)?;
    if let Some(value) = &mut table.objects[object] {
        value.handles += 1;
    }
    table.slots[slot] = Some(HandleSlot { object, flags: 0 });
    Ok(encode_handle(slot))
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
    let retired = if object.handles == 0 && object.waits == 0 {
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
        let object = table
            .slots
            .get(decode_handle(handle))
            .and_then(|slot| *slot)
            .ok_or(STATUS_INVALID_HANDLE)?
            .object;
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
) -> Result<WaitStart, NTSTATUS> {
    let mut table = HANDLE_TABLE.lock();
    let wait = table.prepare_wait(handles, all, pid, tid, timeout)?;
    table.start_wait(wait)
}

/// Validate both objects before signaling and registering the new wait under
/// one dispatcher lock. The signal can satisfy existing queued requests first.
pub(crate) fn signal_and_begin_wait(
    signal: HANDLE,
    handle: HANDLE,
    pid: u64,
    tid: u64,
    timeout: Option<i64>,
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
    table.start_wait(wait)
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
            deadline,
            result: None,
        };
        for (index, &handle) in handles.iter().enumerate() {
            let object = self
                .slots
                .get(decode_handle(handle))
                .and_then(|slot| *slot)
                .ok_or(STATUS_INVALID_HANDLE)?
                .object;
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

    fn start_wait(&mut self, mut wait: WaitRequest) -> Result<WaitStart, NTSTATUS> {
        use super::super::status::*;
        let now = crate::ke::time::get_time_snapshot();
        if let Some(result) = self.try_satisfy(&wait, now.system_time, now.interrupt_time) {
            return Ok(WaitStart::Complete(result));
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
    fn try_satisfy(&mut self, wait: &WaitRequest, wall: u64, uptime: u64) -> Option<NTSTATUS> {
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
            return if wait.deadline.is_some_and(|(absolute, deadline)| {
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
                    let result = self.try_satisfy(&wait, wall, uptime);
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

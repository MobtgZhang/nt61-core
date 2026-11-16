//! Native completion ports. Packets are FIFO, shared by named opens and handle
//! aliases, and remain queued if copying a result to the caller fails.
//!
//! Lock order: native handle table -> port registry -> port queue. Blocking
//! removal keeps an Arc to the original port, so recycled handles cannot redirect
//! a waiter. Scheduler concurrency throttling and file/IRP association are pending.

use super::file::{self, DispatcherState, HandleKind};
use super::status::*;
use super::types::{IoStatusBlock, ObjectAttributes, HANDLE, NTSTATUS, PVOID};
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{probe_user_write, read_user_object, write_user_object};
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Packet {
    key: u64,
    context: u64,
    status: i32,
    padding: u32,
    information: u64,
}

// Every packet owns its link. A reserved node is allocated once and can be
// inserted without allocating memory in the target port.
struct Node {
    busy: AtomicBool,
    data: Spinlock<NodeData>,
}
struct NodeData {
    packet: Packet,
    next: Option<Arc<Node>>,
}
impl Node {
    fn new() -> Self {
        Self { busy: AtomicBool::new(false), data: Spinlock::new(NodeData {
            packet: Packet::default(), next: None,
        }) }
    }
}
struct Port {
    closed: bool,
    head: Option<Arc<Node>>,
    tail: Option<Arc<Node>>,
    depth: u32,
}
impl Port {
    fn push(&mut self, node: Arc<Node>, packet: Packet) -> Result<(), NTSTATUS> {
        if self.closed { return Err(STATUS_INVALID_HANDLE); }
        let depth = self.depth.checked_add(1).ok_or(STATUS_NO_MEMORY)?;
        if node.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(STATUS_INVALID_PARAMETER);
        }
        { let mut data = node.data.lock(); data.packet = packet; data.next = None; }
        if let Some(tail) = &self.tail { tail.data.lock().next = Some(Arc::clone(&node)); }
        else { self.head = Some(Arc::clone(&node)); }
        self.tail = Some(node);
        self.depth = depth;
        Ok(())
    }
    fn pop(&mut self) {
        if let Some(head) = self.head.take() {
            self.head = head.data.lock().next.take();
            head.busy.store(false, Ordering::Release);
            self.depth -= 1;
            if self.head.is_none() { self.tail = None; }
        }
    }
    fn clear(&mut self) { while self.head.is_some() { self.pop(); } }
}
impl Drop for Port {
    fn drop(&mut self) { self.clear(); }
}
static RESERVES: Spinlock<Vec<(u64, Arc<Node>)>> = Spinlock::new(Vec::new());
static NEXT_RESERVE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

pub(crate) fn close_reserve(id: u64) {
    let mut reserves = RESERVES.lock();
    if let Some(index) = reserves.iter().position(|(key, _)| *key == id) {
        reserves.swap_remove(index);
    }
}
fn allocate_reserve() -> Result<u64, NTSTATUS> {
    let mut reserves = RESERVES.lock();
    reserves.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
    let id = NEXT_RESERVE.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| STATUS_NO_MEMORY)?;
    reserves.push((id, Arc::new(Node::new())));
    Ok(id)
}

/// # Safety
/// Captures object attributes and publishes a typed, reusable completion reserve.
/// User APC reserves require the separate APC reserve backend.
pub unsafe extern "C" fn nt_allocate_reserve_object(output: *mut HANDLE,
    attributes: *mut ObjectAttributes, kind: u32) -> NTSTATUS {
    (|| {
        if kind > 1 { return Err(STATUS_INVALID_PARAMETER); }
        if kind == 0 { return Err(STATUS_NOT_SUPPORTED); }
        probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
        let attributes = super::object::capture_attributes(attributes)?;
        let (handle, status) = file::create_object_handle(HandleKind::IoReserve,
            DispatcherState::None, 0xf0003, attributes, true, allocate_reserve)?;
        if let Err(status) = write_user_object(output as u64, &handle) {
            file::free_handle(handle);
            return Err(status);
        }
        Ok(status)
    })().unwrap_or_else(|status| status)
}

pub extern "C" fn nt_set_io_completion_ex(handle: HANDLE, reserve: HANDLE, key: u64,
    context: u64, status: NTSTATUS, information: u64) -> NTSTATUS {
    (|| {
        let port = reference_port(handle, 2)?;
        let node = file::with_sync_access(reserve, 2, |entries| {
            if entries[0].kind != HandleKind::IoReserve { return Err(STATUS_OBJECT_TYPE_MISMATCH); }
            RESERVES.lock().iter().find(|(id, _)| *id == entries[0].target)
                .map(|(_, node)| Arc::clone(node)).ok_or(STATUS_INVALID_HANDLE)
        })?;
        port.lock().push(node, Packet { key, context, status, padding: 0, information })?;
        Ok(STATUS_SUCCESS)
    })().unwrap_or_else(|status| status)
}

static PORTS: Spinlock<Vec<(u64, Arc<Spinlock<Port>>)>> = Spinlock::new(Vec::new());
static NEXT_PORT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

fn allocate_port() -> Result<u64, NTSTATUS> {
    let mut ports = PORTS.lock();
    ports.try_reserve(1).map_err(|_| STATUS_NO_MEMORY)?;
    let id = NEXT_PORT
        .try_update(core::sync::atomic::Ordering::Relaxed, core::sync::atomic::Ordering::Relaxed,
            |id| id.checked_add(1))
        .map_err(|_| STATUS_NO_MEMORY)?;
    ports.push((id, Arc::new(Spinlock::new(Port { closed: false, head: None, tail: None, depth: 0 }))));
    Ok(id)
}

pub(crate) fn close_port(id: u64) {
    let mut ports = PORTS.lock();
    if let Some(index) = ports.iter().position(|(key, _)| *key == id) {
        let (_, port) = ports.swap_remove(index);
        let mut port = port.lock();
        port.closed = true;
        port.clear();
    }
}

fn reference_port(handle: HANDLE, access: u32) -> Result<Arc<Spinlock<Port>>, NTSTATUS> {
    file::with_sync_access(handle, access, |entries| {
        if entries[0].kind != HandleKind::IoCompletion {
            return Err(STATUS_OBJECT_TYPE_MISMATCH);
        }
        PORTS.lock().iter().find(|(id, _)| *id == entries[0].target)
            .map(|(_, port)| Arc::clone(port)).ok_or(STATUS_INVALID_HANDLE)
    })
}

fn create_or_open(output: *mut HANDLE, access: u32, attributes: *mut ObjectAttributes, create: bool) -> NTSTATUS {
    let result = (|| {
        probe_user_write(output as u64, core::mem::size_of::<HANDLE>())?;
        let attributes = super::object::capture_attributes(attributes)?;
        let access = super::object::map_sync_access(HandleKind::IoCompletion, access)?;
        let (handle, status) = file::create_object_handle(HandleKind::IoCompletion,
            DispatcherState::None, access, attributes, create, allocate_port)?;
        if let Err(status) = write_user_object(output as u64, &handle) {
            file::free_handle(handle);
            return Err(status);
        }
        Ok(status)
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// The output and optional OBJECT_ATTRIBUTES are captured through user_copy.
pub unsafe extern "C" fn nt_create_io_completion(output: *mut HANDLE, access: u32,
    attributes: *mut ObjectAttributes, _concurrency: u32) -> NTSTATUS {
    create_or_open(output, access, attributes, true)
}

/// # Safety
/// The output and required named OBJECT_ATTRIBUTES are user addresses.
pub unsafe extern "C" fn nt_open_io_completion(output: *mut HANDLE, access: u32,
    attributes: *mut ObjectAttributes) -> NTSTATUS {
    create_or_open(output, access, attributes, false)
}

pub extern "C" fn nt_set_io_completion(handle: HANDLE, key: u64, context: u64,
    status: NTSTATUS, information: u64) -> NTSTATUS {
    let result = (|| {
        let port = reference_port(handle, 2)?;
        let mut port = port.lock();
        port.push(Arc::new(Node::new()), Packet { key, context, status, padding: 0, information })?;
        Ok(STATUS_SUCCESS)
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// Both outputs are probed; the native class is IoCompletionBasicInformation.
pub unsafe extern "C" fn nt_query_io_completion(handle: HANDLE, class: u32,
    output: PVOID, length: u32, returned: *mut u32) -> NTSTATUS {
    let result = (|| {
        if class != 0 { return Err(STATUS_INVALID_INFO_CLASS); }
        if length != 4 { return Err(STATUS_INFO_LENGTH_MISMATCH); }
        probe_user_write(output as u64, 4)?;
        if !returned.is_null() { probe_user_write(returned as u64, 4)?; }
        let port = reference_port(handle, 1)?;
        let port = port.lock();
        let depth = port.depth;
        write_user_object(output as u64, &depth)?;
        if !returned.is_null() { write_user_object(returned as u64, &4u32)?; }
        Ok(STATUS_SUCCESS)
    })();
    result.unwrap_or_else(|status| status)
}

struct Timeout {
    deadline: Option<(u64, bool)>,
}

impl Timeout {
    fn capture(pointer: *mut i64) -> Result<Self, NTSTATUS> {
        let deadline = if pointer.is_null() { None } else {
            let value = read_user_object::<i64>(pointer as u64)?;
            Some(if value < 0 {
                let now = crate::ke::time::get_time_snapshot().interrupt_time;
                (now.saturating_add(value.unsigned_abs()), false)
            } else { (value as u64, true) })
        };
        Ok(Self { deadline })
    }

    fn expired(&self) -> bool {
        self.deadline.is_some_and(|(deadline, absolute)| {
            let now = crate::ke::time::get_time_snapshot();
            (if absolute { now.system_time } else { now.interrupt_time }) >= deadline
        })
    }
}

fn remove(port: Arc<Spinlock<Port>>, timeout: Timeout, alertable: bool,
    mut copy: impl FnMut(&Port) -> Result<usize, NTSTATUS>) -> NTSTATUS {
    loop {
        {
            let mut port = port.lock();
            if port.closed { return 0x80; } // STATUS_ABANDONED_WAIT_0
            if port.head.is_some() {
                let count = match copy(&port) {
                    Ok(count) => count,
                    Err(status) => return status,
                };
                for _ in 0..count { port.pop(); }
                return STATUS_SUCCESS;
            }
        }
        if alertable && super::alert::take_current_alert() { return STATUS_ALERTED; }
        if timeout.expired() { return STATUS_TIMEOUT; }
        crate::ke::scheduler::yield_();
    }
}

/// # Safety
/// All outputs and the optional timeout are validated before removing a packet.
pub unsafe extern "C" fn nt_remove_io_completion(handle: HANDLE, key: *mut u64,
    context: *mut u64, iosb: *mut IoStatusBlock, timeout: *mut i64) -> NTSTATUS {
    let result = (|| {
        probe_user_write(key as u64, 8)?;
        probe_user_write(context as u64, 8)?;
        probe_user_write(iosb as u64, 16)?;
        let timeout = Timeout::capture(timeout)?;
        let port = reference_port(handle, 2)?;
        Ok(remove(port, timeout, false, |port| {
            let packet = port.head.as_ref().ok_or(STATUS_INVALID_PARAMETER)?.data.lock().packet;
            write_user_object(key as u64, &packet.key)?;
            write_user_object(context as u64, &packet.context)?;
            // Explicit bytes initialize the ABI padding in IO_STATUS_BLOCK.
            let status = [packet.status as u32 as u64, packet.information];
            write_user_object(iosb as u64, &status)?;
            Ok(1)
        }))
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// The packet array, removed count and optional timeout are caller addresses.
pub unsafe extern "C" fn nt_remove_io_completion_ex(handle: HANDLE, output: PVOID,
    count: u32, removed: *mut u32, timeout: *mut i64, alertable: u8) -> NTSTATUS {
    let result = (|| {
        if count == 0 || count > 65_536 { return Err(STATUS_INVALID_PARAMETER); }
        probe_user_write(output as u64, count as usize * core::mem::size_of::<Packet>())?;
        probe_user_write(removed as u64, 4)?;
        let timeout = Timeout::capture(timeout)?;
        let port = reference_port(handle, 2)?;
        Ok(remove(port, timeout, alertable != 0, |port| {
            let copied = (port.depth as usize).min(count as usize);
            let mut current = port.head.clone();
            for index in 0..copied {
                let node = current.take().ok_or(STATUS_INVALID_PARAMETER)?;
                let data = node.data.lock();
                write_user_object(output as u64 + (index * 32) as u64, &data.packet)?;
                current = data.next.clone();
            }
            write_user_object(removed as u64, &(copied as u32))?;
            Ok(copied)
        }))
    })();
    result.unwrap_or_else(|status| status)
}

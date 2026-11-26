//! Process-owned, preemptible guest threads. CPU state, stack, TEB, TLS and
//! callback continuations are private to each thread; mappings are shared.
//! Waits poll kernel events without blocking the process's other guest threads.
use super::*;
use super::win32::sync;
use super::win32::{allocate, free, ThreadLocal};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use crate::arch::common::btl::exec::GuestState;
use crate::arch::common::btl::{abi::GuestAbi, ir::X86Reg, nt61::GuestServices, runtime::StopReason};

const RETURN_GATE: u64 = 0x7fbe_0000;
const CLOCK: u64 = 0x7fbd_0000;
const HANDLE_BASE: u64 = 0x1000_0000;
const MAX_THREADS: usize = 128;
const MAX_HANDLES: usize = 4096;
const THREAD_ALL_ACCESS: u32 = 0x001f_ffff;
const THREAD_QUERY: u32 = 0x0040 | 0x0800;
const THREAD_SET: u32 = 0x0020 | 0x0400;
const THREAD_SET_CONTEXT: u32 = 0x0010;
const SYNCHRONIZE: u32 = 0x0010_0000;
const WAIT_IO_COMPLETION: u64 = 0xc0;

struct Apc { procedure: u64, data: u64 }
struct ApcFrame { saved: GuestState, return_sp: u64 }

#[derive(Default)]
struct Apcs {
    pending: VecDeque<Apc>,
    frames: Vec<ApcFrame>,
    dispatch: bool,
    before_entry: bool,
}

#[derive(Clone, Copy)]
enum PriorityClass {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}

impl PriorityClass {
    const fn win32_value(self) -> u64 {
        match self {
            Self::Idle => 0x40,
            Self::BelowNormal => 0x4000,
            Self::Normal => 0x20,
            Self::AboveNormal => 0x8000,
            Self::High => 0x80,
        }
    }

    fn effective_priority(self, relative: i32) -> i32 {
        let base = match self {
            Self::Idle => 4,
            Self::BelowNormal => 6,
            Self::Normal => 8,
            Self::AboveNormal => 10,
            Self::High => 13,
        };
        match relative {
            -15 => 1,
            15 => 15,
            _ => base + relative,
        }
    }
}

struct Handle { value: u64, id: u32, access: u32, flags: u32 }

#[derive(Clone, Copy)]
pub struct Deadline { start: u64, frequency: u64, milliseconds: u32 }
#[derive(Clone, Copy)]
enum WaitObject { Event(u64), Thread(u32), Local(u64) }
#[derive(Clone)]
pub enum Target {
    Yield, Event(u64), Thread(u32), Local(u64), Multiple(Arc<WaitSet>), Sleep, Critical(u64), Srw(u64, bool), RunOnce(u64, u32),
}
pub struct WaitSet {
    objects: Vec<WaitObject>,
    all: bool,
}
#[derive(Clone)]
pub struct Wait { pub target: Target, deadline: Option<Deadline>, alertable: bool }
impl Wait {
    pub(super) fn lock(target: Target) -> Self { Self { target, deadline: None, alertable: false } }
}
struct Info {
    id: u32,
    priority: i32,
    priority_boost_disabled: bool,
    wait_boost: bool,
    suspended: u32,
    exit: Option<u32>,
    detaching: Option<u32>,
    wait: Option<Wait>,
    apcs: Apcs,
    allocations: Vec<u64>,
    tls_base: u64,
    return_sp: u64,
}
impl Info {
    fn effective_priority(&self, class: PriorityClass) -> i32 {
        (class.effective_priority(self.priority) + i32::from(self.wait_boost)).min(15)
    }

    fn complete_wait(&mut self, wait: &Wait, result: u64) {
        self.wait = None;
        self.apcs.dispatch = result == WAIT_IO_COMPLETION;
        self.wait_boost = !self.priority_boost_disabled && match wait.target {
            Target::Multiple(_) => result < 64 || (0x80..0xc0).contains(&result),
            Target::Local(_) => matches!(result, 0 | 0x80),
            Target::Event(_) | Target::Thread(_) | Target::Critical(_) | Target::Srw(_, _) => result == 0,
            _ => false,
        };
    }
}
struct Parked {
    info: Info,
    thread: GuestThread,
    teb: u64,
    startup: Option<tls::Startup>,
    local: ThreadLocal,
}
pub(super) struct State {
    priority_class: PriorityClass,
    primary_id: u32,
    primary_exit: Option<u32>,
    current: Info,
    parked: Vec<Parked>,
    next_id: u32,
    handles: Vec<Handle>,
    next_handle: u64,
    completed_retired: u64,
    rotate: bool,
    slice: u64,
}
impl State {
    pub(super) fn new(tls_base: u64) -> Self {
        Self { priority_class: PriorityClass::Normal, primary_id: 1, primary_exit: None, current: Info { id: 1, priority: 0,
            priority_boost_disabled: false, wait_boost: false, suspended: 0, exit: None,
            detaching: None, wait: None, apcs: Apcs::default(), allocations: Vec::new(), tls_base, return_sp: 0 },
            parked: Vec::new(), next_id: 2, handles: Vec::new(), next_handle: HANDLE_BASE + 4,
            completed_retired: 0, rotate: false, slice: 4096 }
    }

    pub(super) fn invalidate_code(&mut self) {
        for thread in &mut self.parked { thread.thread.invalidate_code(); }
    }

    pub(super) fn tls_threads(&self, current_teb: u64) -> impl Iterator<Item = (u64, u64)> + Clone + '_ {
        core::iter::once((&self.current, current_teb))
            .chain(self.parked.iter().map(|thread| (&thread.info, thread.teb)))
            .filter(|(info, _)| info.exit.is_none())
            .map(|(info, teb)| (teb, info.tls_base))
    }
}

pub(super) fn is_handle(handle: u64) -> bool { (HANDLE_BASE..HANDLE_BASE + 0x1000_0000).contains(&handle) }
fn handle_info(image: &GuestImage, handle: u64) -> Option<(u32, u32)> {
    if handle == if image.thread.context.mode == DecodeMode::X86 { 0xffff_fffe } else { u64::MAX - 1 } {
        return Some((image.threads.current.id, THREAD_ALL_ACCESS));
    }
    image.threads.handles.iter().find(|h| h.value == handle).map(|h| (h.id, h.access))
}
fn reserve_handle(image: &mut GuestImage) -> Result<(u64, u64), u32> {
    if image.threads.handles.len() >= MAX_HANDLES { return Err(8); }
    let value = image.threads.next_handle;
    let next = value.checked_add(4).filter(|v| is_handle(*v)).ok_or(8u32)?;
    image.threads.handles.try_reserve(1).map_err(|_| 8u32)?;
    Ok((value, next))
}
fn info(image: &GuestImage, id: u32) -> Option<&Info> {
    if image.threads.current.id == id { Some(&image.threads.current) }
    else { image.threads.parked.iter().find(|t| t.info.id == id).map(|t| &t.info) }
}
fn info_mut(image: &mut GuestImage, id: u32) -> Option<&mut Info> {
    if image.threads.current.id == id { Some(&mut image.threads.current) }
    else { image.threads.parked.iter_mut().find(|t| t.info.id == id).map(|t| &mut t.info) }
}

pub(super) fn create(image: &mut GuestImage, a: &[u64]) -> Result<u64, u32> {
    if image.pending_library.is_some() { return Err(120); }
    let flags = a[4] as u32;
    if flags & !0x10004 != 0 { return Err(87); }
    // Security/inheritance needs a process handle table and token validation.
    if a[0] != 0 { return Err(120); }
    if a[5] != 0 && !image.memory.probe_write(a[5], 4) { return Err(998); }
    image.reap_threads();
    if image.threads.parked.len() + 1 >= MAX_THREADS { return Err(8); }
    image.threads.parked.try_reserve(1).map_err(|_| 8u32)?;
    let (handle, next_handle) = reserve_handle(image)?;
    let id = image.threads.next_id;
    let next_id = id.checked_add(1).filter(|next| *next & 0xfff != 0).ok_or(8u32)?;
    let default = image.stack_reserve.max(0x10000);
    let requested = usize::try_from(a[1]).map_err(|_| 8u32)?;
    let size = if flags & 0x10000 != 0 && requested != 0 { requested.checked_add(0xffff).ok_or(8u32)? / 0x10000 * 0x10000 }
        else if requested >= default { requested.checked_add(0xfffff).ok_or(8u32)? / 0x100000 * 0x100000 }
        else { default };
    if size > 16 * 1024 * 1024 { return Err(8); }
    let mut allocations = Vec::new();
    let result = (|| {
        // Keep a no-access page below the stack. The bounded executor commits
        // the reserved usable range and faults on overflow; no host stack is used.
        let stack = allocate(image, 0, size as u64 + PAGE as u64, READ | WRITE)?;
        allocations.push(stack);
        let region = image.memory.regions.iter_mut().find(|r| r.base == stack).ok_or(8u32)?;
        region.permissions[0] = 0;
        region.protections[0] = 1;
        let x86 = image.thread.context.mode == DecodeMode::X86;
        let teb = allocate(image, 0, super::teb_size(image.thread.context.mode) as u64, READ | WRITE)?;
        allocations.push(teb);
        let width = if x86 { 4 } else { 8 };
        let top = stack + PAGE as u64 + size as u64;
        let sp = top - if x86 { 8 } else { 40 };
        let mut thread = GuestThread::new(image.thread.context.mode, a[2], sp);
        if x86 { thread.context.fs_base = teb; } else {
            thread.context.gs_base = teb;
            thread.context.set_reg(X86Reg::Rcx, a[3]);
        }
        // NT_TIB, CLIENT_ID and PEB pointer use the guest's pointer width.
        let fields: &[(u64,u64)] = if x86 {
            &[(0,0xffff_ffff),(4,top),(8,stack+PAGE as u64),(0x18,teb),(0x20,u64::from(image.process_id())),(0x24,u64::from(id)),(0x30,image.peb)]
        } else {
            &[(8,top),(0x10,stack+PAGE as u64),(0x30,teb),(0x40,u64::from(image.process_id())),(0x48,u64::from(id)),(0x60,image.peb)]
        };
        for &(off, val) in fields { if !image.memory.write_virt(teb+off, &val.to_le_bytes()[..width]) { return Err(998); } }
        if !image.memory.regions.iter().any(|r| r.base == RETURN_GATE) {
            image.memory.regions.push(Region::new(RETURN_GATE, PAGE, READ).map_err(|_| 8u32)?);
        }
        if !image.memory.regions.iter().any(|r| r.base == CLOCK) {
            image.memory.regions.push(Region::new(CLOCK, PAGE, READ | WRITE).map_err(|_| 8u32)?);
        }
        if !image.memory.write_virt(sp,&RETURN_GATE.to_le_bytes()[..width]) { return Err(998); }
        if x86 && !image.memory.write_virt(sp+4,&(a[3] as u32).to_le_bytes()) { return Err(998); }
        // Reserve an aligned vector/template area before cloning static TLS.
        let tls_size = image.initialization.data_size();
        let tls_base = allocate(image, 0, tls_size as u64, READ | WRITE)?;
        allocations.push(tls_base);
        image.memory.regions.retain(|r| r.base != tls_base);
        image.initialization.install_at(&mut image.memory, &thread, teb, tls_base, 2)
            .map_err(|_| 8u32)?;
        let disabled: Vec<_> = image.module_metadata.iter().filter(|m| m.thread_notifications_disabled).map(|m| m.base).collect();
        let startup = image.initialization.notifications(&disabled, false, &thread);
        Ok((thread, teb, tls_base, startup, sp + 8))
    })();
    let (thread, teb, tls_base, startup, return_sp) = match result {
        Ok(value) => value,
        Err(error) => { for p in allocations { let _ = free(image,p); } return Err(error); }
    };
    if a[5] != 0 { image.memory.write_virt(a[5], &id.to_le_bytes()); }
    image.threads.next_id = next_id;
    image.threads.next_handle = next_handle;
    image.threads.handles.push(Handle { value: handle, id, access: THREAD_ALL_ACCESS, flags: 0 });
    image.threads.parked.push(Parked { thread, teb, startup, local: ThreadLocal::default(),
        info: Info { id, priority: 0, priority_boost_disabled: false, wait_boost: false,
            suspended: u32::from(flags & 4 != 0), exit: None,
            detaching: None, wait: None, apcs: Apcs { before_entry: true, ..Apcs::default() },
            allocations, tls_base, return_sp } });
    Ok(handle)
}

pub(super) fn open(image: &mut GuestImage, access: u32, inherit: bool, id: u32) -> Result<u64, u32> {
    if info(image, id).is_none() { return Err(87); }
    if access & !THREAD_ALL_ACCESS != 0 { return Err(5); }
    let (value, next) = reserve_handle(image)?;
    image.threads.handles.push(Handle { value, id, access, flags: u32::from(inherit) });
    image.threads.next_handle = next;
    Ok(value)
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64,u32> {
    let (id, access) = handle_info(image,a[0]).ok_or(6u32)?;
    if (matches!(name, "GetExitCodeThread" | "GetThreadId" | "GetProcessIdOfThread" | "GetThreadPriority") && access & THREAD_QUERY == 0)
        || (name == "SetThreadPriority" && access & THREAD_SET == 0)
        || (matches!(name, "SuspendThread" | "ResumeThread") && access & 2 == 0) { return Err(5); }
    if name == "GetExitCodeThread" {
        let code = info(image,id).ok_or(6u32)?.exit.unwrap_or(259);
        if !image.memory.write_virt(a[1], &code.to_le_bytes()) { return Err(998); }
        return Ok(1);
    }
    if name == "GetThreadId" { return Ok(u64::from(id)); }
    if name == "GetProcessIdOfThread" { return Ok(u64::from(image.process_id())); }
    if name == "GetThreadPriority" {
        return Ok(u64::from(info(image, id).ok_or(6u32)?.priority as u32));
    }
    let t = info_mut(image,id).ok_or(6u32)?;
    if t.exit.is_some() { return Err(5); }
    if name == "SetThreadPriority" {
        let priority = a[1] as i32;
        t.priority = match priority {
            -16 | -15 => -15,
            -2..=2 => priority,
            15 | 16 => 15,
            _ => return Err(87),
        };
        return Ok(1);
    }
    let old = t.suspended;
    if name == "SuspendThread" { t.suspended = old.checked_add(1).filter(|v| *v <= 127).ok_or(156u32)?; }
    else { t.suspended = old.saturating_sub(1); }
    Ok(u64::from(old))
}

pub(super) fn priority_boost_disabled(image: &GuestImage, handle: u64) -> Result<bool, u32> {
    let (id, access) = handle_info(image, handle).ok_or(6u32)?;
    if access & THREAD_QUERY == 0 { return Err(5); }
    Ok(info(image, id).ok_or(6u32)?.priority_boost_disabled)
}

pub(super) fn set_priority_boost_disabled(image: &mut GuestImage, handle: u64, disabled: bool) -> Result<u64, u32> {
    let (id, access) = handle_info(image, handle).ok_or(6u32)?;
    if access & THREAD_SET == 0 { return Err(5); }
    let thread = info_mut(image, id).ok_or(6u32)?;
    thread.priority_boost_disabled = disabled;
    if disabled { thread.wait_boost = false; }
    Ok(1)
}
pub(super) fn close(image: &mut GuestImage, handle: u64) -> Result<(),u32> {
    let index = image.threads.handles.iter().position(|h| h.value == handle).ok_or(6u32)?;
    if image.threads.handles[index].flags & 2 != 0 { return Err(5); }
    image.threads.handles.remove(index);
    Ok(())
}

/// Same-process thread handles only. Each duplicate has independent access,
/// inheritance/protection flags and lifetime; it retains the thread object.
/// Cross-process duplication and handles owned by other backends stay explicit.
pub(super) fn duplicate(image: &mut GuestImage, a: &[u64]) -> Result<u64,u32> {
    let options = a[6] as u32;
    let result = (|| {
        if options & !3 != 0 { return Err(87); }
        let process = if image.thread.context.mode == DecodeMode::X86 { u32::MAX as u64 } else { u64::MAX };
        if a[0] != process || a[2] != process { return Err(120); }
        let (id, source_access) = handle_info(image,a[1]).ok_or(if is_handle(a[1]) || a[1] == 0 {6u32} else {120u32})?;
        if options & 1 != 0 && image.threads.handles.iter().any(|h| h.value == a[1] && h.flags & 2 != 0) { return Err(5); }
        let width = if image.thread.context.mode == DecodeMode::X86 {4} else {8};
        if a[3] != 0 && !image.memory.probe_write(a[3],width) { return Err(998); }
        let access = if options & 2 != 0 { source_access } else { a[4] as u32 };
        // Generic mapping and rights escalation need object-manager/token
        // access checks. Do not silently grant them from a restricted handle.
        if access & !THREAD_ALL_ACCESS != 0 { return Err(120); }
        if access & !source_access != 0 { return Err(5); }
        let (value,next) = reserve_handle(image)?;
        if a[3] != 0 && !image.memory.write_virt(a[3], &value.to_le_bytes()[..width]) { return Err(998); }
        image.threads.handles.push(Handle { value, id, access, flags: u32::from(a[5] as u32 != 0) });
        image.threads.next_handle = next;
        Ok(1)
    })();
    // DUPLICATE_CLOSE_SOURCE closes a real source even if duplication fails.
    if options & 1 != 0 && is_handle(a[1]) {
        close(image,a[1])?;
    }
    result
}

pub(super) fn set_handle_information(image: &mut GuestImage, a: &[u64]) -> Result<u64,u32> {
    let mask = a[1] as u32;
    if mask & !3 != 0 { return Err(87); }
    let h = image.threads.handles.iter_mut().find(|h| h.value == a[0]).ok_or(6u32)?;
    h.flags = (h.flags & !mask) | (a[2] as u32 & mask);
    Ok(1)
}
pub(super) fn handle_flags(image: &GuestImage, handle: u64) -> Result<u32, u32> {
    image.threads.handles.iter().find(|entry| entry.value == handle)
        .map(|entry| entry.flags).ok_or(6)
}
fn clock(image: &mut GuestImage, services: &mut dyn GuestServices) -> Result<(u64,u64),u32> {
    let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
    win32::sleep_counter(abi,services,&mut image.memory,CLOCK).map_err(|_| 120)
}
fn deadline(image: &mut GuestImage, milliseconds: u32, services: &mut dyn GuestServices) -> Result<Option<Deadline>,u32> {
    if milliseconds == u32::MAX { return Ok(None); }
    let (start,frequency) = clock(image,services)?;
    Ok(Some(Deadline {start,frequency,milliseconds}))
}
pub(super) fn sleep(image: &mut GuestImage, ms: u32, alertable: bool, services: &mut dyn GuestServices) -> Result<Wait,u32> {
    if ms == 0 || (alertable && image.has_pending_apc()) {
        return Ok(Wait { target: Target::Yield, deadline: None, alertable });
    }
    Ok(Wait { target: Target::Sleep, deadline: deadline(image,ms,services)?, alertable })
}
pub(super) fn wait(image: &mut GuestImage, handle: u64, ms: u32, alertable: bool, services: &mut dyn GuestServices) -> Result<Result<u64,Wait>,u32> {
    let target = if let Some((id,access)) = handle_info(image,handle) {
        if access & SYNCHRONIZE == 0 { return Err(5); }
        if info(image,id).ok_or(6u32)?.exit.is_some() { return Ok(Ok(0)); }
        Target::Thread(id)
    } else if sync::is_wait_handle(handle) {
        let thread = image.thread_id();
        if sync::wait_object_ready(image, handle, thread)? && !(alertable && image.has_pending_apc()) {
            if let Some(value) = sync::acquire_wait_object(image, handle, thread)? {
                return Ok(Ok(value));
            }
        }
        Target::Local(handle)
    } else {
        if is_handle(handle) { return Err(6); }
        match services.wait_event(handle,Some(0)).map_err(win32::status_error)? {
            0 => return Ok(Ok(0)),
            0x102 => {},
            _ => return Err(120),
        }
        Target::Event(handle)
    };
    if alertable && image.has_pending_apc() {
        return Ok(Err(Wait { target, deadline: None, alertable }));
    }
    if ms == 0 { return Ok(Ok(0x102)); }
    Ok(Err(Wait { target, deadline: deadline(image,ms,services)?, alertable }))
}

pub(super) fn wait_multiple(
    image: &mut GuestImage, handles: &[u64], all: bool, milliseconds: u32,
    alertable: bool, services: &mut dyn GuestServices,
) -> Result<Result<u64, Wait>, u32> {
    let mut objects = Vec::with_capacity(handles.len());
    let mut local_ids = [0; 64];
    for (index, &handle) in handles.iter().enumerate() {
        if handles[..index].contains(&handle) { return Err(87); }
        if let Some((id, access)) = handle_info(image, handle) {
            if access & SYNCHRONIZE == 0 { return Err(5); }
            info(image, id).ok_or(6u32)?;
            objects.push(WaitObject::Thread(id));
        } else if sync::is_wait_handle(handle) {
            let id = sync::wait_identity(image, handle)?;
            if all && local_ids[..index].contains(&id) { return Err(87); }
            local_ids[index] = id;
            sync::wait_object_ready(image, handle, image.thread_id())?;
            objects.push(WaitObject::Local(handle));
        } else {
            if is_handle(handle) { return Err(6); }
            services.validate_wait_event(handle).map_err(win32::status_error)?;
            objects.push(WaitObject::Event(handle));
        }
    }
    let set = WaitSet { objects, all };
    let apc_ready = alertable && image.has_pending_apc();
    if !apc_ready {
        if let Some(value) = set.poll(image, image.thread_id(), services)? { return Ok(Ok(value)); }
        if milliseconds == 0 { return Ok(Ok(0x102)); }
    }
    let deadline = if apc_ready { None } else { deadline(image, milliseconds, services)? };
    Ok(Err(Wait { target: Target::Multiple(Arc::new(set)), deadline, alertable }))
}

impl WaitSet {
    fn poll(&self, image: &mut GuestImage, thread: u32, services: &mut dyn GuestServices) -> Result<Option<u64>, u32> {
        let mut handles = [0; 64];
        let mut indices = [0; 64];
        let mut count = 0;
        let mut ready = None;
        for (index, object) in self.objects.iter().enumerate() {
            match *object {
                WaitObject::Event(handle) => {
                    handles[count] = handle;
                    indices[count] = index as u64;
                    count += 1;
                }
                WaitObject::Thread(id) => {
                    let exited = info(image, id).is_some_and(|thread| thread.exit.is_some());
                    if self.all && !exited { return Ok(None); }
                    if !self.all && exited {
                        ready = Some((index as u64, None));
                        break;
                    }
                }
                WaitObject::Local(handle) => {
                    let available = sync::wait_object_ready(image, handle, thread)?;
                    if self.all && !available { return Ok(None); }
                    if !self.all && available {
                        ready = Some((index as u64, Some(handle)));
                        break;
                    }
                }
            }
        }
        if count != 0 {
            let result = services.poll_events(&handles[..count], self.all).map_err(win32::status_error)?;
            if self.all {
                if result == 0x102 { return Ok(None); }
                if result != 0 { return Err(120); }
            } else if result != 0x102 {
                return indices[..count].get(result as usize).copied().map(Some).ok_or(120);
            }
        }
        if self.all {
            let mut abandoned = None;
            for object in &self.objects {
                if let WaitObject::Local(handle) = *object {
                    if sync::acquire_wait_object(image, handle, thread)? == Some(0x80) {
                        abandoned = Some(0x80);
                    }
                }
            }
            return Ok(Some(abandoned.unwrap_or(0)));
        }
        match ready {
            Some((index, Some(handle))) => Ok(sync::acquire_wait_object(image, handle, thread)?.map(|value| index + value)),
            Some((index, None)) => Ok(Some(index)),
            None => Ok(None),
        }
    }
}

pub(super) fn queue_apc(image: &mut GuestImage, procedure: u64, handle: u64, data: u64) -> Result<u64, u32> {
    let (id, access) = handle_info(image, handle).ok_or(6u32)?;
    if access & THREAD_SET_CONTEXT == 0 { return Err(5); }
    let thread = info_mut(image, id).ok_or(6u32)?;
    if thread.exit.is_some() || thread.detaching.is_some() { return Err(5); }
    let pending = &mut thread.apcs.pending;
    if pending.len() >= 4096 { return Err(1816); }
    pending.try_reserve(1).map_err(|_| 8u32)?;
    pending.push_back(Apc { procedure, data });
    Ok(1)
}

impl GuestImage {
    pub(super) fn has_pending_apc(&self) -> bool {
        !self.threads.current.apcs.pending.is_empty()
    }

    pub(super) fn begin_apc(&mut self) -> Result<(), StopReason> {
        let apcs = &mut self.threads.current.apcs;
        let first = apcs.before_entry && self.startup.is_none() && self.pending_library.is_none();
        apcs.before_entry &= !first;
        if !(core::mem::take(&mut apcs.dispatch) || first) { return Ok(()); }
        let Some(apc) = apcs.pending.pop_front() else { return Ok(()); };

        let saved = self.thread.context;
        let fault = StopReason::DataFault { pc: saved.rip };
        let old_sp = saved.reg(X86Reg::Rsp);
        let mut frame = [0; 40];
        let (sp, size) = if saved.mode == DecodeMode::X86 {
            frame[..4].copy_from_slice(&(tls::RETURN_GATE as u32).to_le_bytes());
            frame[4..8].copy_from_slice(&(apc.data as u32).to_le_bytes());
            (old_sp.checked_sub(8).ok_or(fault)?, 8)
        } else {
            frame[..8].copy_from_slice(&tls::RETURN_GATE.to_le_bytes());
            let sp = (old_sp.checked_sub(40).ok_or(fault)? & !15).checked_sub(8).ok_or(fault)?;
            (sp, 40)
        };
        if apcs.frames.len() >= 64 || apcs.frames.try_reserve(1).is_err()
            || !self.memory.write_virt(sp, &frame[..size])
        {
            return Err(fault);
        }
        apcs.frames.push(ApcFrame { saved, return_sp: sp + 8 });
        self.thread.context.set_reg(X86Reg::Rsp, sp);
        self.thread.context.set_reg(X86Reg::Rip, apc.procedure);
        if saved.mode == DecodeMode::X64 {
            self.thread.context.set_reg(X86Reg::Rcx, apc.data);
        }
        Ok(())
    }

    pub(super) fn complete_apc(&mut self, reason: StopReason) -> bool {
        let apcs = &mut self.threads.current.apcs;
        if reason != (StopReason::InstructionFault { pc: tls::RETURN_GATE })
            || !apcs.frames.last().is_some_and(|frame| frame.return_sp == self.thread.context.reg(X86Reg::Rsp))
        {
            return false;
        }
        self.thread.context = apcs.frames.pop().unwrap().saved;
        apcs.dispatch = true;
        true
    }

    pub(super) fn process_priority_class(&self, process: u64) -> Result<u64, u32> {
        let pseudo_handle = if self.thread.context.mode == DecodeMode::X86 {
            u64::from(u32::MAX)
        } else {
            u64::MAX
        };
        if process != pseudo_handle { return Err(6); }
        Ok(self.threads.priority_class.win32_value())
    }

    pub(super) fn set_process_priority_class(&mut self, process: u64, flags: u32) -> Result<u64, u32> {
        if matches!(flags, 0x10_0000 | 0x20_0000) {
            self.process_priority_class(process)?;
            return Err(if flags == 0x20_0000 { 403 } else { 120 });
        }
        let priority = [PriorityClass::Idle, PriorityClass::BelowNormal, PriorityClass::Normal,
            PriorityClass::AboveNormal, PriorityClass::High].into_iter()
            .find(|class| u64::from(flags) & class.win32_value() != 0)
            .or_else(|| (flags & 0x100 != 0).then_some(PriorityClass::High))
            .ok_or(87u32)?;
        self.process_priority_class(process)?;
        self.threads.priority_class = priority;
        Ok(1)
    }

    pub fn thread_id(&self) -> u32 { self.threads.current.id }

    pub fn primary_thread_exit_code(&self) -> Option<u32> { self.threads.primary_exit }

    pub(super) fn assign_thread_identity(&mut self, id: u32) -> Result<(), super::LoadError> {
        let (offset, width) = if self.thread.context.mode == DecodeMode::X86 { (0x24, 4) } else { (0x48, 8) };
        if !self.memory.write_virt(self.teb + offset, &u64::from(id).to_le_bytes()[..width]) {
            return Err(super::LoadError::InvalidImage);
        }
        self.threads.current.id = id;
        self.threads.primary_id = id;
        self.threads.next_id = id + 1;
        Ok(())
    }
    pub(in crate::arch::common::btl) fn has_live_thread(&self, id: u32) -> bool {
        info(self, id).is_some_and(|thread| thread.exit.is_none())
    }
    pub fn retired(&self) -> u64 {
        self.threads.parked.iter().fold(self.thread.retired.saturating_add(self.threads.completed_retired), |sum,t| sum.saturating_add(t.thread.retired))
    }
    pub(super) fn has_other_threads(&self) -> bool { self.threads.parked.iter().any(|t| t.info.exit.is_none()) }
    pub(super) fn active_notification_in(&self, modules: &[u64]) -> bool {
        core::iter::once(&self.startup).chain(self.threads.parked.iter().map(|thread| &thread.startup))
            .flatten().any(|startup| startup.active_in(modules))
    }
    pub(super) fn discard_module_notifications(&mut self, modules: &[u64]) {
        for startup in core::iter::once(&mut self.startup)
            .chain(self.threads.parked.iter_mut().map(|thread| &mut thread.startup)).flatten() {
            startup.discard_queued(modules);
        }
    }
    fn reap_threads(&mut self) {
        let mut retired = self.threads.completed_retired;
        let mut retained: Vec<u32> = self.threads.handles.iter().map(|h| h.id).collect();
        // A wait already owns a thread-object reference even if its handle is
        // closed while another guest thread is running.
        for wait in core::iter::once(self.threads.current.wait.as_ref()).chain(self.threads.parked.iter().map(|t| t.info.wait.as_ref())).flatten() {
            match &wait.target {
                Target::Thread(id) => retained.push(*id),
                Target::Multiple(set) => retained.extend(set.objects.iter().filter_map(|object| match object {
                    WaitObject::Thread(id) => Some(*id),
                    WaitObject::Event(_) | WaitObject::Local(_) => None,
                })),
                _ => {}
            }
        }
        self.threads.parked.retain(|t| {
            if t.info.exit.is_some() && !retained.contains(&t.info.id) {
                retired = retired.saturating_add(t.thread.retired);
                false
            } else { true }
        });
        self.threads.completed_retired = retired;
    }
    pub(super) fn yield_thread(&mut self) { self.threads.rotate = true; }
    pub(super) fn terminate_thread(&mut self, code: u32) {
        // NtTerminateThread bypasses DLL_THREAD_DETACH, unlike ExitThread.
        self.startup = None;
        self.threads.current.detaching = Some(code);
        self.threads.current.wait = None;
        self.threads.current.apcs = Apcs::default();
        self.finish_detach();
    }
    pub(super) fn clear_tls_slot(&mut self, slot: usize) {
        if let Some(v) = self.win32.local.tls.get_mut(slot) { *v = 0; }
        for t in &mut self.threads.parked { if let Some(v) = t.local.tls.get_mut(slot) { *v = 0; } }
    }
    pub(super) fn park(&mut self, wait: Wait) { self.threads.current.wait = Some(wait); self.threads.rotate = true; }
    pub(super) fn exit_thread(&mut self, code: u32) {
        let disabled: Vec<_> = self.module_metadata.iter().filter(|m|m.thread_notifications_disabled).map(|m|m.base).collect();
        self.startup = self.initialization.notifications(&disabled,true,&self.thread);
        self.threads.current.detaching = Some(code);
        self.threads.current.wait = None;
        self.threads.current.apcs = Apcs::default();
        self.thread.context.set_reg(X86Reg::Rax,u64::from(code));
    }
    pub(super) fn detaching(&self) -> bool { self.threads.current.detaching.is_some() }
    pub(super) fn finish_detach(&mut self) {
        if self.startup.is_none() {
            if let Some(code) = self.threads.current.detaching.take() {
                win32::cleanup_thread(self);
                self.threads.current.exit = Some(code);
                if self.threads.current.id == self.threads.primary_id {
                    self.threads.primary_exit = Some(code);
                }
                self.threads.rotate = true;
                self.initialization.remove_thread(&mut self.memory, self.teb);
                let allocations = core::mem::take(&mut self.threads.current.allocations);
                for p in allocations { let _ = free(self,p); }
            }
        }
    }
    pub(super) fn returned_thread(&self, reason: StopReason) -> bool {
        self.threads.current.id != self.threads.primary_id && reason == (StopReason::InstructionFault { pc: RETURN_GATE })
            && self.thread.context.reg(X86Reg::Rsp) == self.threads.current.return_sp
    }
    // Readiness is sampled without executing guest code or blocking a host
    // wait. Auto-reset events are consumed by exactly one ready waiter.
    fn ready(&mut self, thread_id: u32, wait: &Wait, services: &mut dyn GuestServices) -> Result<Option<(u64,Option<u32>)>,StopReason> {
        let width = if self.thread.context.mode == DecodeMode::X86 {4} else {8};
        let pc = self.thread.context.rip;
        let fault = || StopReason::DataFault { pc };
        let mut b = [0;8];
        let result = match wait.target {
            Target::Yield => Some((0,None)),
            Target::Thread(id) => info(self,id).and_then(|t|t.exit).map(|_|(0,None)),
            Target::Event(handle) => match services.wait_event(handle,Some(0)) {
                Ok(0) => Some((0,None)), Ok(0x102) => None,
                Err(status) => Some((u64::from(u32::MAX),Some(win32::status_error(status)))),
                _ => return Err(StopReason::UnsupportedApi {id:144}),
            },
            Target::Local(handle) => match sync::acquire_wait_object(self, handle, thread_id) {
                Ok(value) => value.map(|value| (value, None)),
                Err(error) => Some((u64::from(u32::MAX), Some(error))),
            },
            Target::Multiple(ref set) => match set.poll(self, thread_id, services) {
                Ok(value) => value.map(|value| (value, None)),
                Err(error) => Some((u64::from(u32::MAX), Some(error))),
            },
            Target::Critical(address) => {
                let at = address + width as u64 + 8;
                if self.memory.read_virt(at,&mut b[..width]) != width { return Err(fault()); }
                if u64::from_le_bytes(b) == 0 {
                    for (at, value, n) in [(address+width as u64, (-2i32) as u32 as u64,4),
                        (address+width as u64+4,1,4),(at,u64::from(thread_id),width)] {
                        if !self.memory.write_virt(at,&value.to_le_bytes()[..n]) { return Err(fault()); }
                    }
                    Some((0,None))
                } else { None }
            }
            Target::Srw(address,shared) => {
                if self.memory.read_virt(address,&mut b[..width]) != width { return Err(fault()); }
                let old = u64::from_le_bytes(b);
                if old == 0 || (shared && old & 15 == 1 && old >> 4 != 0) {
                    let next = if old == 0 {if shared {0x11u64} else {1}} else {old.checked_add(16).ok_or_else(fault)?};
                    if !self.memory.write_virt(address,&next.to_le_bytes()[..width]) { return Err(fault()); }
                    Some((0,None))
                } else { None }
            }
            Target::Sleep => None,
            Target::RunOnce(address, number) => {
                if self.memory.read_virt(address, &mut b[..width]) != width { return Err(fault()); }
                // Retry the actual API after success OR failure of the owner.
                // Failure resets zero, so another waiter must run its callback.
                if u64::from_le_bytes(b) & 3 != 1 { Some((u64::from(number), None)) } else { None }
            }
        };
        if result.is_some() { return Ok(result); }
        if let Some(d) = wait.deadline {
            let (now,frequency) = clock(self,services).map_err(|_| StopReason::UnsupportedApi {id:144})?;
            if frequency != d.frequency || now < d.start { return Err(StopReason::UnsupportedApi {id:144}); }
            if u128::from(now-d.start) * 1000 >= u128::from(d.milliseconds) * u128::from(frequency) {
                return Ok(Some((if matches!(wait.target,Target::Sleep) {0} else {0x102},None)));
            }
        }
        Ok(None)
    }
    pub(super) fn schedule(&mut self, services: &mut dyn GuestServices) -> Result<bool,StopReason> {
        // Poll parked waiters before selecting the next runnable context.
        for i in 0..self.threads.parked.len() {
            let t = &self.threads.parked[i];
            if t.info.suspended != 0 || t.info.exit.is_some() { continue; }
            if let Some(wait) = t.info.wait.clone() {
                let id = t.info.id;
                let completion = if wait.alertable && !t.info.apcs.pending.is_empty() {
                    Some((WAIT_IO_COMPLETION, None))
                } else { self.ready(id,&wait,services)? };
                if let Some((value,error)) = completion {
                    let t = &mut self.threads.parked[i];
                    t.info.complete_wait(&wait, value);
                    t.thread.context.set_reg(X86Reg::Rax,value);
                    if let Some(error) = error {
                        let off = if t.thread.context.mode == DecodeMode::X86 {0x34} else {0x68};
                        self.memory.write_virt(t.teb+off,&error.to_le_bytes());
                    }
                }
            }
        }
        if let Some(wait) = self.threads.current.wait.clone() {
            if self.threads.current.suspended == 0 {
                let completion = if wait.alertable && self.has_pending_apc() {
                    Some((WAIT_IO_COMPLETION, None))
                } else { self.ready(self.thread_id(),&wait,services)? };
                if let Some((value,error)) = completion {
                    self.threads.current.complete_wait(&wait, value);
                    self.thread.context.set_reg(X86Reg::Rax,value);
                    if let Some(error) = error {
                        let off = if self.thread.context.mode == DecodeMode::X86 {0x34} else {0x68};
                        self.memory.write_virt(self.teb+off,&error.to_le_bytes());
                    }
                }
            }
        }
        let runnable = |t: &Info| t.suspended == 0 && t.exit.is_none() && t.wait.is_none();
        if self.pending_library.is_some() {
            if self.threads.slice == 0 { self.threads.slice = 4096; }
            return Ok(runnable(&self.threads.current));
        }
        let class = self.threads.priority_class;
        let next = self.threads.parked.iter().enumerate()
            .filter(|(_, thread)| runnable(&thread.info))
            .min_by_key(|(_, thread)| -thread.info.effective_priority(class));
        if runnable(&self.threads.current) {
            let current_priority = self.threads.current.effective_priority(class);
            let keep_current = next.is_none_or(|(_, thread)| {
                let priority = thread.info.effective_priority(class);
                current_priority > priority || (current_priority == priority && !self.threads.rotate)
            });
            if keep_current {
                if self.threads.rotate { self.threads.slice = 4096; }
                self.threads.rotate = false;
                return Ok(true);
            }
        }
        self.threads.rotate = false;
        if let Some((i, _)) = next {
            let mut next = self.threads.parked.remove(i);
            core::mem::swap(&mut self.thread,&mut next.thread);
            core::mem::swap(&mut self.teb,&mut next.teb);
            core::mem::swap(&mut self.startup,&mut next.startup);
            core::mem::swap(&mut self.win32.local,&mut next.local);
            core::mem::swap(&mut self.threads.current,&mut next.info);
            self.threads.parked.push(next);
            self.threads.slice = 4096;
            return Ok(true);
        }
        self.threads.slice = 4096;
        Ok(runnable(&self.threads.current))
    }
    pub(super) fn all_threads_exited(&self) -> Option<u32> {
        let code = self.threads.current.exit?;
        if self.threads.parked.iter().all(|t|t.info.exit.is_some()) {Some(code)} else {None}
    }
    pub(super) fn thread_slice(&self) -> u64 { self.threads.slice }
    pub(super) fn account_thread_slice(&mut self, retired: u64) {
        self.threads.slice = self.threads.slice.saturating_sub(retired);
        if self.threads.slice == 0 {
            self.threads.current.wait_boost = false;
            self.threads.rotate = true;
        }
    }
}

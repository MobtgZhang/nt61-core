//! Process-owned, preemptible guest threads. CPU state, stack, TEB, TLS and
//! callback continuations are private to each thread; mappings are shared.
//! Waits poll kernel events without blocking the process's other guest threads.
use super::*;
use super::win32::{allocate, free, ThreadLocal};
use crate::arch::common::btl::{abi::GuestAbi, ir::X86Reg, nt61::GuestServices, runtime::StopReason};

const RETURN_GATE: u64 = 0x7fbe_0000;
const CLOCK: u64 = 0x7fbd_0000;
const HANDLE_BASE: u64 = 0x1000_0000;
const MAX_THREADS: usize = 128;
const MAX_HANDLES: usize = 4096;
const THREAD_ALL_ACCESS: u32 = 0x001f_ffff;
const THREAD_QUERY: u32 = 0x0040 | 0x0800;
const SYNCHRONIZE: u32 = 0x0010_0000;

struct Handle { value: u64, id: u32, access: u32, flags: u32 }

#[derive(Clone, Copy)]
pub struct Deadline { start: u64, frequency: u64, milliseconds: u32 }
#[derive(Clone, Copy)]
pub enum Target { Yield, Event(u64), Thread(u32), Sleep, Critical(u64), Srw(u64, bool), RunOnce(u64, u32) }
#[derive(Clone, Copy)]
pub struct Wait { pub target: Target, deadline: Option<Deadline> }
impl Wait {
    pub(super) fn lock(target: Target) -> Self { Self { target, deadline: None } }
}
struct Info {
    id: u32,
    suspended: u32,
    exit: Option<u32>,
    detaching: Option<u32>,
    wait: Option<Wait>,
    allocations: Vec<u64>,
    return_sp: u64,
}
struct Parked {
    info: Info,
    thread: GuestThread,
    teb: u64,
    startup: Option<tls::Startup>,
    local: ThreadLocal,
}
pub(super) struct State {
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
    pub(super) fn new() -> Self {
        Self { current: Info { id: 1, suspended: 0, exit: None,
            detaching: None, wait: None, allocations: Vec::new(), return_sp: 0 },
            parked: Vec::new(), next_id: 2, handles: Vec::new(), next_handle: HANDLE_BASE + 4,
            completed_retired: 0, rotate: false, slice: 4096 }
    }

    pub(super) fn invalidate_code(&mut self) {
        for thread in &mut self.parked { thread.thread.invalidate_code(); }
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
    let next_id = id.checked_add(1).filter(|id| *id < 0x0400_0000).ok_or(8u32)?;
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
        let teb = allocate(image, 0, PAGE as u64, READ | WRITE)?;
        allocations.push(teb);
        let x86 = image.thread.context.mode == DecodeMode::X86;
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
            &[(0,0xffff_ffff),(4,top),(8,stack+PAGE as u64),(0x18,teb),(0x20,1),(0x24,u64::from(id)),(0x30,image.peb)]
        } else {
            &[(8,top),(0x10,stack+PAGE as u64),(0x30,teb),(0x40,1),(0x48,u64::from(id)),(0x60,image.peb)]
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
        let tls_base = if tls_size == 0 { 0 } else {
            let base = allocate(image, 0, tls_size as u64, READ | WRITE)?;
            allocations.push(base);
            // install_at replaces this reserved placeholder with its TLS data.
            image.memory.regions.retain(|r| r.base != base);
            base
        };
        image.initialization.clone().install_at(&mut image.memory, &thread, teb, tls_base, 2)
            .map_err(|_| 8u32)?;
        let disabled: Vec<_> = image.module_metadata.iter().filter(|m| m.thread_notifications_disabled).map(|m| m.base).collect();
        let startup = image.initialization.notifications(&disabled, false, &thread);
        Ok((thread, teb, startup, sp + 8))
    })();
    let (thread, teb, startup, return_sp) = match result {
        Ok(value) => value,
        Err(error) => { for p in allocations { let _ = free(image,p); } return Err(error); }
    };
    if a[5] != 0 { image.memory.write_virt(a[5], &id.to_le_bytes()); }
    image.threads.next_id = next_id;
    image.threads.next_handle = next_handle;
    image.threads.handles.push(Handle { value: handle, id, access: THREAD_ALL_ACCESS, flags: 0 });
    image.threads.parked.push(Parked { thread, teb, startup, local: ThreadLocal::default(),
        info: Info { id, suspended: u32::from(flags & 4 != 0), exit: None,
            detaching: None, wait: None, allocations, return_sp } });
    Ok(handle)
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64,u32> {
    let (id, access) = handle_info(image,a[0]).ok_or(6u32)?;
    if (matches!(name, "GetExitCodeThread" | "GetThreadId") && access & THREAD_QUERY == 0)
        || (matches!(name, "SuspendThread" | "ResumeThread") && access & 2 == 0) { return Err(5); }
    if name == "GetExitCodeThread" {
        let code = info(image,id).ok_or(6u32)?.exit.unwrap_or(259);
        if !image.memory.write_virt(a[1], &code.to_le_bytes()) { return Err(998); }
        return Ok(1);
    }
    if name == "GetThreadId" { return Ok(u64::from(id)); }
    let t = info_mut(image,id).ok_or(6u32)?;
    if t.exit.is_some() { return Err(5); }
    let old = t.suspended;
    if name == "SuspendThread" { t.suspended = old.checked_add(1).filter(|v| *v <= 127).ok_or(156u32)?; }
    else { t.suspended = old.saturating_sub(1); }
    Ok(u64::from(old))
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
fn clock(image: &mut GuestImage, services: &mut dyn GuestServices) -> Result<(u64,u64),u32> {
    let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
    win32::sleep_counter(abi,services,&mut image.memory,CLOCK).map_err(|_| 120)
}
fn deadline(image: &mut GuestImage, milliseconds: u32, services: &mut dyn GuestServices) -> Result<Option<Deadline>,u32> {
    if milliseconds == u32::MAX { return Ok(None); }
    let (start,frequency) = clock(image,services)?;
    Ok(Some(Deadline {start,frequency,milliseconds}))
}
pub(super) fn sleep(image: &mut GuestImage, ms: u32, services: &mut dyn GuestServices) -> Result<Wait,u32> {
    if ms == 0 { return Ok(Wait { target: Target::Yield, deadline: None }); }
    Ok(Wait { target: Target::Sleep, deadline: deadline(image,ms,services)? })
}
pub(super) fn wait(image: &mut GuestImage, handle: u64, ms: u32, services: &mut dyn GuestServices) -> Result<Result<u64,Wait>,u32> {
    let target = if let Some((id,access)) = handle_info(image,handle) {
        if access & SYNCHRONIZE == 0 { return Err(5); }
        if info(image,id).ok_or(6u32)?.exit.is_some() { return Ok(Ok(0)); }
        Target::Thread(id)
    } else {
        if is_handle(handle) { return Err(6); }
        match services.wait_event(handle,Some(0)).map_err(win32::status_error)? {
            0 => return Ok(Ok(0)),
            0x102 => {},
            _ => return Err(120),
        }
        Target::Event(handle)
    };
    if ms == 0 { return Ok(Ok(0x102)); }
    Ok(Err(Wait { target, deadline: deadline(image,ms,services)? }))
}

impl GuestImage {
    pub fn thread_id(&self) -> u32 { self.threads.current.id }
    pub fn retired(&self) -> u64 {
        self.threads.parked.iter().fold(self.thread.retired.saturating_add(self.threads.completed_retired), |sum,t| sum.saturating_add(t.thread.retired))
    }
    pub(super) fn has_other_threads(&self) -> bool { self.threads.parked.iter().any(|t| t.info.exit.is_none()) }
    fn reap_threads(&mut self) {
        let mut retired = self.threads.completed_retired;
        let mut retained: Vec<u32> = self.threads.handles.iter().map(|h| h.id).collect();
        // A wait already owns a thread-object reference even if its handle is
        // closed while another guest thread is running.
        for wait in core::iter::once(self.threads.current.wait).chain(self.threads.parked.iter().map(|t| t.info.wait)).flatten() {
            if let Target::Thread(id) = wait.target { retained.push(id); }
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
        self.thread.context.set_reg(X86Reg::Rax,u64::from(code));
    }
    pub(super) fn detaching(&self) -> bool { self.threads.current.detaching.is_some() }
    pub(super) fn finish_detach(&mut self) {
        if self.startup.is_none() {
            if let Some(code) = self.threads.current.detaching.take() {
                win32::cleanup_thread(self);
                self.threads.current.exit = Some(code);
                self.threads.rotate = true;
                let allocations = core::mem::take(&mut self.threads.current.allocations);
                for p in allocations { let _ = free(self,p); }
            }
        }
    }
    pub(super) fn returned_thread(&self, reason: StopReason) -> bool {
        self.threads.current.id != 1 && reason == (StopReason::InstructionFault { pc: RETURN_GATE })
            && self.thread.context.reg(X86Reg::Rsp) == self.threads.current.return_sp
    }
    // Readiness is sampled without executing guest code or blocking a host
    // wait. Auto-reset events are consumed by exactly one ready waiter.
    fn ready(&mut self, thread_id: u32, wait: Wait, services: &mut dyn GuestServices) -> Result<Option<(u64,Option<u32>)>,StopReason> {
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
            if let Some(wait) = t.info.wait {
                let id = t.info.id;
                if let Some((value,error)) = self.ready(id,wait,services)? {
                    let t = &mut self.threads.parked[i];
                    t.info.wait = None;
                    t.thread.context.set_reg(X86Reg::Rax,value);
                    if let Some(error) = error {
                        let off = if t.thread.context.mode == DecodeMode::X86 {0x34} else {0x68};
                        self.memory.write_virt(t.teb+off,&error.to_le_bytes());
                    }
                }
            }
        }
        if let Some(wait) = self.threads.current.wait {
            if self.threads.current.suspended == 0 {
                if let Some((value,error)) = self.ready(self.thread_id(),wait,services)? {
                    self.threads.current.wait = None;
                    self.thread.context.set_reg(X86Reg::Rax,value);
                    if let Some(error) = error {
                        let off = if self.thread.context.mode == DecodeMode::X86 {0x34} else {0x68};
                        self.memory.write_virt(self.teb+off,&error.to_le_bytes());
                    }
                }
            }
        }
        let runnable = |t: &Info| t.suspended == 0 && t.exit.is_none() && t.wait.is_none();
        if !self.threads.rotate && runnable(&self.threads.current) { return Ok(true); }
        self.threads.rotate = false;
        if let Some(i) = self.threads.parked.iter().position(|t|runnable(&t.info)) {
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
        if self.threads.slice == 0 { self.threads.rotate = true; }
    }
}

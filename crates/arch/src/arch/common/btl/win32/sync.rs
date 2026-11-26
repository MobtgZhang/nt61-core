//! Unnamed backend events and critical sections in one isolated GuestImage.
//! Event signal/reset/wait state belongs to the kernel dispatcher, with
//! process-owned guest handles. Named events require a guest namespace.
//! Recursive ownership uses the active guest thread ID. Contended critical
//! sections and SRW locks yield to the guest scheduler; unsupported foreign
//! wait queues are never silently acquired or released.
use super::*;

const WAIT_HANDLE_BASE: u64 = 0x4900_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const SEMAPHORE_MODIFY_STATE: u32 = 2;
const SEMAPHORE_ALL_ACCESS: u32 = 0x001f_0003;

enum WaitObject {
    Mutex { owner: Option<u32>, depth: u32, abandoned: bool },
    Semaphore { count: i32, maximum: i32, name: Option<Vec<u16>> },
}

struct WaitHandle {
    value: u64,
    object: u64,
    access: u32,
}

#[derive(Default)]
pub(super) struct State {
    sections: Vec<(u64, u64)>, // guest CRITICAL_SECTION address, owned debug allocation
    objects: Vec<(u64, WaitObject)>,
    handles: Vec<WaitHandle>,
    wait_serial: u64,
}

pub(in crate::arch::common::btl) fn is_wait_handle(handle: u64) -> bool {
    (WAIT_HANDLE_BASE..WAIT_HANDLE_BASE + 0x0100_0000).contains(&handle)
}

fn insert_wait_handle(state: &mut State, object: Option<u64>, access: u32) -> Result<u64, ApiError> {
    let handle = WAIT_HANDLE_BASE + state.wait_serial;
    if !is_wait_handle(handle) || state.handles.len() >= 4096 {
        return Err(8u32.into());
    }
    state.handles.try_reserve(1).map_err(|_| 8u32)?;
    state.wait_serial += 4;
    state.handles.push(WaitHandle { value: handle, object: object.unwrap_or(handle), access });
    Ok(handle)
}

fn create_wait_object(state: &mut State, object: WaitObject, access: u32) -> Result<u64, ApiError> {
    state.objects.try_reserve(1).map_err(|_| 8u32)?;
    let handle = insert_wait_handle(state, None, access)?;
    state.objects.push((handle, object));
    Ok(handle)
}

fn wait_object_id(state: &State, handle: u64, access: u32) -> Result<u64, u32> {
    let entry = state.handles.iter().find(|entry| entry.value == handle).ok_or(6u32)?;
    if entry.access & access != access { return Err(5); }
    Ok(entry.object)
}

pub(in crate::arch::common::btl) fn wait_identity(image: &GuestImage, handle: u64) -> Result<u64, u32> {
    wait_object_id(&image.win32.sync, handle, SYNCHRONIZE)
}

fn wait_object_mut(state: &mut State, handle: u64, access: u32) -> Result<&mut WaitObject, u32> {
    let id = wait_object_id(state, handle, access)?;
    state.objects.iter_mut().find(|(candidate, _)| *candidate == id).map(|(_, object)| object).ok_or(6)
}

pub(in crate::arch::common::btl) fn wait_object_ready(image: &GuestImage, handle: u64, thread: u32) -> Result<bool, u32> {
    let id = wait_identity(image, handle)?;
    let (_, object) = image.win32.sync.objects.iter().find(|(candidate, _)| *candidate == id).ok_or(6u32)?;
    match *object {
        WaitObject::Mutex { owner, depth, .. } => {
            if owner == Some(thread) && depth == u32::MAX {
                return Err(87);
            }
            Ok(owner.is_none() || owner == Some(thread))
        }
        WaitObject::Semaphore { count, .. } => Ok(count > 0),
    }
}

pub(in crate::arch::common::btl) fn acquire_wait_object(image: &mut GuestImage, handle: u64, thread: u32) -> Result<Option<u64>, u32> {
    match wait_object_mut(&mut image.win32.sync, handle, SYNCHRONIZE)? {
        WaitObject::Mutex { owner, depth, abandoned } => {
            if owner.is_some_and(|owner| owner != thread) {
                return Ok(None);
            }
            *depth = depth.checked_add(1).ok_or(87u32)?;
            *owner = Some(thread);
            Ok(Some(if core::mem::take(abandoned) { 0x80 } else { 0 }))
        }
        WaitObject::Semaphore { count, .. } if *count > 0 => {
            *count -= 1;
            Ok(Some(0))
        }
        WaitObject::Semaphore { .. } => Ok(None),
    }
}

pub(super) fn close_wait_object(image: &mut GuestImage, handle: u64) -> Result<(), ApiError> {
    let state = &mut image.win32.sync;
    let index = state.handles.iter().position(|entry| entry.value == handle).ok_or(6u32)?;
    let id = state.handles.swap_remove(index).object;
    if !state.handles.iter().any(|entry| entry.object == id) {
        let index = state.objects.iter().position(|(candidate, _)| *candidate == id).ok_or(6u32)?;
        state.objects.swap_remove(index);
    }
    Ok(())
}

fn semaphore_name(memory: &mut dyn GuestMemory, address: u64, wide: bool) -> Result<Vec<u16>, ApiError> {
    if address == 0 { return Err(87u32.into()); }
    let width = if wide { 2 } else { 1 };
    let mut name = Vec::new();
    for index in 0..260 {
        let pointer = address.checked_add(index * width).ok_or(NOACCESS)?;
        let unit = word(memory, pointer, width as usize)? as u16;
        if unit == 0 {
            return if name.is_empty() { Err(123u32.into()) } else { Ok(name) };
        }
        if unit == u16::from(b'\\') { return Err(123u32.into()); }
        name.try_reserve(1).map_err(|_| 8u32)?;
        name.push(if wide { unit } else { nls::decode_ansi(unit as u8) });
    }
    Err(206u32.into())
}

fn find_semaphore(state: &State, name: &[u16]) -> Option<u64> {
    state.objects.iter().find_map(|(id, object)| match object {
        WaitObject::Semaphore { name: Some(candidate), .. } if candidate == name => Some(*id),
        _ => None,
    })
}

fn semaphore_access(requested: u32) -> Result<u32, ApiError> {
    let mut access = requested & 0x0fff_ffff;
    for (generic, specific) in [(0x8000_0000, 0x0002_0001), (0x4000_0000, 0x0002_0002),
        (0x2000_0000, 0x0012_0000), (0x1000_0000, SEMAPHORE_ALL_ACCESS)]
    {
        if requested & generic != 0 { access |= specific; }
    }
    if access & !SEMAPHORE_ALL_ACCESS != 0 { return Err(5u32.into()); }
    Ok(access)
}

pub(super) fn abandon_mutexes(image: &mut GuestImage) {
    let thread = image.thread_id();
    for (_, object) in &mut image.win32.sync.objects {
        if let WaitObject::Mutex { owner, depth, abandoned } = object {
            if *owner == Some(thread) {
                *owner = None;
                *depth = 0;
                *abandoned = true;
            }
        }
    }
}

pub(super) fn handles(name: &str) -> bool {
    matches!(name, "InitializeCriticalSection" | "InitializeCriticalSectionAndSpinCount"
        | "InitializeCriticalSectionEx" | "EnterCriticalSection" | "TryEnterCriticalSection"
        | "LeaveCriticalSection" | "DeleteCriticalSection"
        | "InitializeConditionVariable" | "WakeConditionVariable" | "WakeAllConditionVariable"
        | "AcquireSRWLockExclusive" | "AcquireSRWLockShared"
        | "ReleaseSRWLockExclusive" | "ReleaseSRWLockShared"
        | "TryAcquireSRWLockExclusive" | "TryAcquireSRWLockShared"
        | "RtlAcquireSRWLockExclusive" | "RtlAcquireSRWLockShared"
        | "RtlReleaseSRWLockExclusive" | "RtlReleaseSRWLockShared"
        | "CreateEventW" | "SetEvent" | "ResetEvent"
        | "CreateMutexA" | "CreateMutexW" | "ReleaseMutex"
        | "CreateSemaphoreA" | "CreateSemaphoreW" | "OpenSemaphoreA" | "OpenSemaphoreW" | "ReleaseSemaphore"
        | "WaitForSingleObject" | "WaitForSingleObjectEx"
        | "WaitForMultipleObjects" | "WaitForMultipleObjectsEx")
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    match name {
        "CreateMutexA" | "CreateMutexW" => {
            if a[0] != 0 || a[2] != 0 {
                return Err(120u32.into());
            }
            let owner = (a[1] as u32 != 0).then(|| image.thread_id());
            return create_wait_object(&mut image.win32.sync, WaitObject::Mutex {
                owner,
                depth: u32::from(owner.is_some()),
                abandoned: false,
            }, SYNCHRONIZE);
        }
        "ReleaseMutex" => {
            let thread = image.thread_id();
            let WaitObject::Mutex { owner, depth, .. } = wait_object_mut(&mut image.win32.sync, a[0], 0)? else {
                return Err(6u32.into());
            };
            if *owner != Some(thread) {
                return Err(288u32.into());
            }
            *depth -= 1;
            if *depth == 0 {
                *owner = None;
            }
            return Ok(1);
        }
        "CreateSemaphoreA" | "CreateSemaphoreW" => {
            let (count, maximum) = (a[1] as i32, a[2] as i32);
            if maximum <= 0 || !(0..=maximum).contains(&count) {
                return Err(87u32.into());
            }
            if a[0] != 0 {
                return Err(120u32.into());
            }
            let object_name = if a[3] == 0 { None } else {
                Some(semaphore_name(&mut image.memory, a[3], name.ends_with('W'))?)
            };
            let state = &mut image.win32.sync;
            let existing = object_name.as_deref().and_then(|name| find_semaphore(state, name));
            let handle = if let Some(id) = existing {
                insert_wait_handle(state, Some(id), SEMAPHORE_ALL_ACCESS)?
            } else {
                create_wait_object(state, WaitObject::Semaphore { count, maximum, name: object_name }, SEMAPHORE_ALL_ACCESS)?
            };
            error(image, if existing.is_some() { 183 } else { 0 });
            return Ok(handle);
        }
        "OpenSemaphoreA" | "OpenSemaphoreW" => {
            if a[1] as u32 != 0 { return Err(120u32.into()); }
            let name = semaphore_name(&mut image.memory, a[2], name.ends_with('W'))?;
            let state = &mut image.win32.sync;
            let id = find_semaphore(state, &name).ok_or(2u32)?;
            return insert_wait_handle(state, Some(id), semaphore_access(a[0] as u32)?);
        }
        "ReleaseSemaphore" => {
            let release = a[1] as i32;
            if release <= 0 {
                return Err(87u32.into());
            }
            let WaitObject::Semaphore { count, maximum, .. } = wait_object_mut(&mut image.win32.sync, a[0], SEMAPHORE_MODIFY_STATE)? else {
                return Err(6u32.into());
            };
            let next = count.checked_add(release).filter(|next| next <= maximum).ok_or(298u32)?;
            if a[2] != 0 && !image.memory.write_virt(a[2], &count.to_le_bytes()) {
                return Err(998u32.into());
            }
            *count = next;
            return Ok(1);
        }
        "CreateEventW" => {
            if a[0] != 0 { return Err(120u32.into()); }
            if a[3] != 0 {
                let _ = string(&mut image.memory, a[3], true)?;
                return Err(120u32.into());
            }
            return services.create_event(a[1] as u32 != 0, a[2] as u32 != 0).map_err(|status| status_error(status).into());
        }
        "SetEvent" | "ResetEvent" => {
            services.change_event(a[0], name == "SetEvent").map_err(status_error)?;
            return Ok(1);
        }
        "WaitForSingleObject" | "WaitForSingleObjectEx" => {
            let alertable = name.ends_with("Ex") && a[2] as u32 != 0;
            let milliseconds = a[1] as u32;
            if image.has_other_threads() || super::super::threads::is_handle(a[0]) || is_wait_handle(a[0])
                || services.is_child_handle(a[0]) || (alertable && image.has_pending_apc())
            {
                return match super::super::threads::wait(image, a[0], milliseconds, alertable, services)? {
                    Ok(value) => Ok(value), Err(wait) => Err(ApiError::Pending(wait)),
                };
            }
            let timeout = if milliseconds == u32::MAX { None } else { Some(-i64::from(milliseconds) * 10_000) };
            let status = services.wait_event(a[0], timeout).map_err(status_error)?;
            return if matches!(status, 0 | 0x102) { Ok(u64::from(status)) } else { Err(120u32.into()) };
        }
        "WaitForMultipleObjects" | "WaitForMultipleObjectsEx" => {
            let count = a[0] as u32 as usize;
            if !(1..=64).contains(&count) { return Err(INVALID_PARAMETER.into()); }
            let mut bytes = [0; 64 * 8];
            let bytes = &mut bytes[..count * width];
            if image.memory.read_virt(a[1], bytes) != bytes.len() { return Err(998u32.into()); }
            let mut handles = [0; 64];
            for (handle, bytes) in handles.iter_mut().zip(bytes.chunks_exact(width)) {
                let mut value = [0; 8];
                value[..width].copy_from_slice(bytes);
                *handle = u64::from_le_bytes(value);
            }
            let alertable = name.ends_with("Ex") && a[4] as u32 != 0;
            return match super::super::threads::wait_multiple(
                image, &handles[..count], a[2] as u32 != 0, a[3] as u32, alertable, services,
            )? {
                Ok(value) => Ok(value),
                Err(wait) => Err(ApiError::Pending(wait)),
            };
        }
        _ => {}
    }
    if name.contains("SRWLock") { return srw(image, name, a[0], width); }
    if matches!(name, "InitializeConditionVariable" | "WakeConditionVariable" | "WakeAllConditionVariable") {
        let address = a[0];
        if address == 0 || !image.memory.probe_write(address, width) {
            return Err(ApiError::MemoryFault);
        }
        if name == "InitializeConditionVariable" {
            put(&mut image.memory, address, 0, width).map_err(|_| ApiError::MemoryFault)?;
        } else if word(&mut image.memory, address, width).map_err(|_| ApiError::MemoryFault)? != 0 {
            // No guest can sleep on a condition variable until this executor
            // has a guest-thread scheduler. The Windows static initializer is
            // zero; waking its empty waiter set has no observable effect.
            // A nonempty/foreign queue must never be discarded as a success.
            return Err(ApiError::Unsupported);
        }
        return Ok(0);
    }
    let size = if x86 { 24 } else { 40 };
    let address = a[0];
    if address == 0 || !image.memory.probe_write(address, size) {
        return Err(ApiError::MemoryFault);
    }
    let mut data = read(&mut image.memory, address, size).map_err(|_| ApiError::MemoryFault)?;
    let index = image.win32.sync.sections.iter().position(|(p, _)| *p == address);
    if name.starts_with("Initialize") {
        if index.is_some() || image.win32.sync.sections.len() >= 4096 {
            return Err(INVALID_PARAMETER.into());
        }
        let flags = if name == "InitializeCriticalSectionEx" { a[2] as u32 } else { 0 };
        // Windows 7's public InitializeCriticalSectionEx flag. Unsupported
        // private RTL options must not silently change the lock contract.
        if flags & !0x0100_0000 != 0 { return Err(INVALID_PARAMETER.into()); }
        let no_debug = flags & 0x0100_0000 != 0;
        let debug = if no_debug { 0 } else {
            let debug_size = if x86 { 32 } else { 48 };
            let p = heap_allocate(image, debug_size)?;
            let mut bytes = vec![0; debug_size as usize];
            // RTL_CRITICAL_SECTION_DEBUG: pointer follows the two USHORTs
            // (padding on x64), then a self-linked ProcessLocksList.
            let offset = width;
            bytes[offset..offset + width].copy_from_slice(&address.to_le_bytes()[..width]);
            let list = p + (2 * width) as u64;
            for at in [2 * width, 3 * width] {
                bytes[at..at + width].copy_from_slice(&list.to_le_bytes()[..width]);
            }
            if !image.memory.write_virt(p, &bytes) {
                heap_free(image, p)?;
                return Err(ApiError::MemoryFault);
            }
            p
        };
        data.fill(0);
        let debug_field = if no_debug { u64::MAX } else { debug };
        data[..width].copy_from_slice(&debug_field.to_le_bytes()[..width]);
        data[width..width + 4].copy_from_slice(&(-1i32).to_le_bytes());
        // Honor the same processor topology as GetProcessAffinityMask when
        // the host supplies it. A host without topology runs one guest thread.
        // Contended blocking/spinning still requires a guest-thread scheduler.
        let multiprocessor = services.processor_mask().unwrap_or(1).count_ones() > 1;
        let spin = if multiprocessor && name != "InitializeCriticalSection" {
            a[1] as u32 & 0x7fff_ffff
        } else { 0 };
        let spin_at = width + 8 + 2 * width;
        data[spin_at..spin_at + width].copy_from_slice(&(spin as u64).to_le_bytes()[..width]);
        if !image.memory.write_virt(address, &data) {
            if debug != 0 { heap_free(image, debug)?; }
            return Err(ApiError::MemoryFault);
        }
        image.win32.sync.sections.push((address, debug));
        return Ok(u64::from(name != "InitializeCriticalSection"));
    }
    let index = index.ok_or(ApiError::MemoryFault)?;
    let recursion_at = width + 4;
    let owner_at = width + 8;
    let mut owner_bytes = [0u8; 8];
    owner_bytes[..width].copy_from_slice(&data[owner_at..owner_at + width]);
    let owner = u64::from_le_bytes(owner_bytes);
    let recursion = i32::from_le_bytes(data[recursion_at..recursion_at + 4].try_into().map_err(|_| ApiError::MemoryFault)?);
    let current_thread = u64::from(image.thread_id());
    let result;
    match name {
        "EnterCriticalSection" | "TryEnterCriticalSection" => {
            if owner != 0 && owner != current_thread {
                return if name == "TryEnterCriticalSection" { Ok(0) }
                    else if image.has_other_threads() { Err(ApiError::Pending(super::super::threads::Wait::lock(super::super::threads::Target::Critical(address)))) }
                    else { Err(ApiError::Unsupported) };
            }
            if recursion < 0 || (owner == 0 && recursion != 0) || (owner != 0 && recursion == 0) {
                return Err(ApiError::MemoryFault);
            }
            let next = recursion.checked_add(1).ok_or(ApiError::MemoryFault)?;
            // Win7 uncontended LockCount encodes the locked bit, not recursion.
            data[width..width + 4].copy_from_slice(&(-2i32).to_le_bytes());
            data[recursion_at..recursion_at + 4].copy_from_slice(&next.to_le_bytes());
            data[owner_at..owner_at + width].copy_from_slice(&current_thread.to_le_bytes()[..width]);
            result = u64::from(name == "TryEnterCriticalSection");
        }
        "LeaveCriticalSection" => {
            if owner != current_thread || recursion <= 0 { return Err(ApiError::MemoryFault); }
            let next = recursion - 1;
            data[recursion_at..recursion_at + 4].copy_from_slice(&next.to_le_bytes());
            if next == 0 {
                data[owner_at..owner_at + width].fill(0);
                data[width..width + 4].copy_from_slice(&(-1i32).to_le_bytes());
            }
            result = 0;
        }
        "DeleteCriticalSection" => {
            if owner != 0 || recursion != 0 { return Err(ApiError::MemoryFault); }
            data.fill(0);
            result = 0;
        }
        _ => return Err(ApiError::Unsupported),
    }
    if !image.memory.write_virt(address, &data) { return Err(ApiError::MemoryFault); }
    if name == "DeleteCriticalSection" {
        let (_, debug) = image.win32.sync.sections.remove(index);
        if debug != 0 { heap_free(image, debug)?; }
    }
    Ok(result)
}

fn srw(image: &mut GuestImage, name: &str, address: u64, width: usize) -> Result<u64, ApiError> {
    if address == 0 || !image.memory.probe_write(address, width) {
        return Err(ApiError::MemoryFault);
    }
    let value = word(&mut image.memory, address, width).map_err(|_| ApiError::MemoryFault)?;
    let name = name.strip_prefix("Rtl").unwrap_or(name);
    let shared = name.ends_with("Shared");
    let releasing = name.starts_with("Release");
    let trying = name.starts_with("Try");
    // Win7's uncontended word has bit 0 locked and the shared count above
    // bit 3. Waiting/waking bits require real scheduler-owned wait blocks.
    // One GuestImage currently executes one thread, so these transitions
    // cannot race with another guest thread. Do not consume foreign queues.
    let readers = if value & 15 == 1 { value >> 4 } else { 0 };
    let next = if releasing {
        if shared && readers != 0 {
            if readers == 1 { 0 } else { value - 16 }
        } else if !shared && value == 1 { 0 }
        else { return Err(ApiError::Unsupported); }
    } else if value == 0 {
        if shared { 0x11 } else { 1 }
    } else if shared && readers != 0 {
        let maximum = if width == 4 { u64::from(u32::MAX) } else { u64::MAX };
        value.checked_add(16).filter(|next| *next <= maximum).ok_or(ApiError::Unsupported)?
    } else {
        return if trying { Ok(0) }
            else if image.has_other_threads() { Err(ApiError::Pending(super::super::threads::Wait::lock(super::super::threads::Target::Srw(address,shared)))) }
            else { Err(ApiError::Unsupported) };
    };
    put(&mut image.memory, address, next, width).map_err(|_| ApiError::MemoryFault)?;
    Ok(u64::from(trying))
}

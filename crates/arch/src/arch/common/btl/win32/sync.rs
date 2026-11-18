//! Unnamed backend events and critical sections in one isolated GuestImage.
//! Event signal/reset/wait state belongs to the kernel dispatcher, with
//! process-owned guest handles. Named events and alertable waits need guest
//! namespaces/APC delivery before they can be exposed here.
//! Recursive ownership uses the active guest thread ID. Contended critical
//! sections and SRW locks yield to the guest scheduler; unsupported foreign
//! wait queues are never silently acquired or released.
use super::*;

#[derive(Default)]
pub(super) struct State {
    sections: Vec<(u64, u64)>, // guest CRITICAL_SECTION address, owned debug allocation
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
        | "WaitForSingleObject" | "WaitForSingleObjectEx")
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    match name {
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
            if name.ends_with("Ex") && a[2] as u32 != 0 { return Err(120u32.into()); }
            let milliseconds = a[1] as u32;
            if image.has_other_threads() || super::super::threads::is_handle(a[0]) {
                return match super::super::threads::wait(image, a[0], milliseconds, services)? {
                    Ok(value) => Ok(value), Err(wait) => Err(ApiError::Pending(wait)),
                };
            }
            let timeout = if milliseconds == u32::MAX { None } else { Some(-i64::from(milliseconds) * 10_000) };
            let status = services.wait_event(a[0], timeout).map_err(status_error)?;
            return if matches!(status, 0 | 0x102) { Ok(u64::from(status)) } else { Err(120u32.into()) };
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

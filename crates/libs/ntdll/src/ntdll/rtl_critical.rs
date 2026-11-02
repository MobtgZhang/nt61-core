//! ntdll — RTL critical section operations
//
//! Implements the `Rtl*CriticalSection` family used for user-mode
//! synchronization. Critical sections are lightweight in-process
//! locks that use atomic operations for the fast path and fall
//! back to kernel events for contention.
//
//! References: MSDN Library "Windows 7" — RTL_CRITICAL_SECTION.

use super::status::{STATUS_INVALID_PARAMETER, STATUS_SUCCESS};
use super::sync::{nt_create_event, nt_reset_event, nt_set_event};
use super::types::{ObjectAttributes, RtlCriticalSection, HANDLE, NTSTATUS};
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

const CRITICAL_SECTION_DEFAULT_SPIN_COUNT: u32 = 4000;

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_initialize_critical_section(
    critical_section: *mut RtlCriticalSection,
) -> NTSTATUS {
    if critical_section.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let cs = &mut *critical_section;
    let debug = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<super::types::RtlCriticalSectionDebug>(),
    );
    if debug.is_null() {
        return super::status::STATUS_NO_MEMORY;
    }
    let debug = debug as *mut super::types::RtlCriticalSectionDebug;
    ptr::write_bytes(debug, 0, 1);
    (*debug).r#type = 0;
    (*debug).critical_section = critical_section;

    cs.debug_info = debug;
    cs.lock_count = -1;
    cs.recursion_count = 0;
    cs.owning_thread = ptr::null_mut();
    cs.lock_semaphore = ptr::null_mut();
    cs.spin_count = CRITICAL_SECTION_DEFAULT_SPIN_COUNT as usize;
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_initialize_critical_section_and_spin_count(
    critical_section: *mut RtlCriticalSection,
    spin_count: u32,
) -> NTSTATUS {
    let status = rtl_initialize_critical_section(critical_section);
    if status == STATUS_SUCCESS {
        (*critical_section).spin_count = spin_count as usize;
    }
    status
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_enter_critical_section(
    critical_section: *mut RtlCriticalSection,
) -> NTSTATUS {
    if critical_section.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let cs = &mut *critical_section;
    let current_thread = get_current_thread_handle();
    if cs.owning_thread == current_thread {
        cs.recursion_count += 1;
        let lock_ptr = &cs.lock_count as *const i32 as *mut AtomicI32;
        (*lock_ptr).fetch_add(1, Ordering::Acquire);
        return STATUS_SUCCESS;
    }

    let lock_ptr = &cs.lock_count as *const i32 as *mut AtomicI32;
    let spin_count = cs.spin_count.max(CRITICAL_SECTION_DEFAULT_SPIN_COUNT as usize);
    let mut spins = 0usize;

    loop {
        if (*lock_ptr)
            .compare_exchange(-1, 0, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            cs.owning_thread = current_thread;
            cs.recursion_count = 1;
            break;
        }

        if (*lock_ptr).load(Ordering::Acquire) == -1 {
            continue;
        }

        if spins >= spin_count {
            if !cs.debug_info.is_null() {
                let debug = cs.debug_info;
                unsafe { (*debug).contention_count = (*debug).contention_count.wrapping_add(1) };
            }
            if cs.lock_semaphore.is_null() {
                let mut oa = ObjectAttributes::new();
                let mut event: HANDLE = ptr::null_mut();
                if nt_create_event(&mut event, 0, &mut oa, 1, 0) == super::status::STATUS_SUCCESS {
                    cs.lock_semaphore = event;
                }
            }
            if !cs.lock_semaphore.is_null() {
                nt_reset_event(cs.lock_semaphore, ptr::null_mut());
            }
            // NT critical sections block in the kernel when the spin
            // budget is exhausted. The current runtime has no blocking
            // user-mode scheduler primitive, so keep waiting while the
            // event remains unsignaled. Releasing the section signals
            // the event, and ownership is only taken under the atomic
            // count, preserving exclusion.
            loop {
                if (*lock_ptr)
                    .compare_exchange(-1, 0, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    cs.owning_thread = current_thread;
                    cs.recursion_count = 1;
                    return STATUS_SUCCESS;
                }
                core::hint::spin_loop();
            }
        }

        core::hint::spin_loop();
        spins = spins.saturating_add(1);
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_try_enter_critical_section(
    critical_section: *mut RtlCriticalSection,
) -> u32 {
    if critical_section.is_null() {
        return 0;
    }
    let cs = &mut *critical_section;
    let current_thread = get_current_thread_handle();

    if !cs.owning_thread.is_null() && cs.owning_thread == current_thread {
        let lock_ptr = &cs.lock_count as *const i32 as *mut AtomicI32;
        (*lock_ptr).fetch_add(1, Ordering::Acquire);
        cs.recursion_count += 1;
        return 1;
    }

    let lock_ptr = &cs.lock_count as *const i32 as *mut AtomicI32;
    if (*lock_ptr)
        .compare_exchange(-1, 0, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        cs.owning_thread = current_thread;
        cs.recursion_count = 1;
        1
    } else {
        0
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_leave_critical_section(
    critical_section: *mut RtlCriticalSection,
) -> NTSTATUS {
    if critical_section.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let cs = &mut *critical_section;
    let current_thread = get_current_thread_handle();
    if cs.owning_thread != current_thread || cs.recursion_count <= 0 {
        return super::status::STATUS_INVALID_PARAMETER;
    }

    cs.recursion_count -= 1;
    if cs.recursion_count > 0 {
        let lock_ptr = &cs.lock_count as *const i32 as *mut AtomicI32;
        (*lock_ptr).fetch_sub(1, Ordering::Release);
        return STATUS_SUCCESS;
    }

    let lock_ptr = &cs.lock_count as *const i32 as *mut AtomicI32;
    let old_count = (*lock_ptr).fetch_sub(1, Ordering::Release);

    if old_count > 0 && !cs.lock_semaphore.is_null() {
        nt_set_event(cs.lock_semaphore, ptr::null_mut());
    }

    if cs.recursion_count == 0 {
        cs.owning_thread = ptr::null_mut();
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_delete_critical_section(
    critical_section: *mut RtlCriticalSection,
) -> NTSTATUS {
    if critical_section.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    let cs = &mut *critical_section;

    if !cs.lock_semaphore.is_null() {
        super::file::nt_close(cs.lock_semaphore);
        cs.lock_semaphore = ptr::null_mut();
    }

    cs.lock_count = -1;
    cs.recursion_count = 0;
    cs.owning_thread = ptr::null_mut();
    if !cs.debug_info.is_null() {
        let debug = cs.debug_info;
        cs.debug_info = ptr::null_mut();
        crate::mm::pool::free(debug as *mut u8);
    }
    STATUS_SUCCESS
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_set_critical_section_spin_count(
    critical_section: *mut RtlCriticalSection,
    spin_count: u32,
) -> u32 {
    if critical_section.is_null() {
        return 0;
    }
    let cs = &mut *critical_section;
    let old = cs.spin_count as u32;
    cs.spin_count = spin_count as usize;
    old
}

fn get_current_thread_handle() -> HANDLE {
    let ethread = crate::ps::thread::get_current_ethread();
    if ethread.is_null() {
        ptr::null_mut()
    } else {
        ethread.cast::<core::ffi::c_void>()
    }
}

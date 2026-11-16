//! Boot-local LUID allocation, shared by all architectures.
use super::{
    status::{STATUS_NO_MEMORY, STATUS_SUCCESS},
    types::NTSTATUS,
};
use core::sync::atomic::{AtomicU64, Ordering};

// Skip well-known authentication and privilege IDs. Never reuse IDs in a boot.
static NEXT_LUID: AtomicU64 = AtomicU64::new(0x1000);
pub extern "C" fn nt_allocate_locally_unique_id(output: *mut u64) -> NTSTATUS {
    if output as usize % 4 != 0 {
        return 0x80000002u32 as i32;
    }
    if let Err(status) = crate::mm::user_copy::probe_user_write(output as u64, 8) {
        return status;
    }
    let value = match allocate() {
        Ok(value) => value,
        Err(status) => return status,
    };
    match crate::mm::user_copy::write_user_object(output as u64, &value) {
        Ok(()) => STATUS_SUCCESS,
        Err(status) => status,
    }
}

/// Reserve a boot-unique identifier for a kernel object.
pub(crate) fn allocate() -> Result<u64, NTSTATUS> {
    let mut value = NEXT_LUID.load(Ordering::Relaxed);
    loop {
        let Some(next) = value.checked_add(1) else {
            return Err(STATUS_NO_MEMORY);
        };
        match NEXT_LUID.compare_exchange_weak(value, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => value = current,
        }
    }
    Ok(value)
}

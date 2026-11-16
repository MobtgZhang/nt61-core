//! Architecture-independent processor queries and local memory ordering.

pub extern "C" fn nt_get_current_processor_number() -> u32 {
    crate::ke::scheduler::get_current_cpu_id()
}

/// Flush this processor's pending writes. Process-wide instruction visibility
/// is the separate NtFlushProcessWriteBuffers service and needs an IPI barrier.
pub extern "C" fn nt_flush_write_buffer() -> u32 {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    0
}

/// Every online CPU must execute its fence before the caller may reclaim
/// data protected by a process-wide write-buffer flush.
pub extern "C" fn nt_flush_process_write_buffers() -> u32 {
    crate::ke::tlb_shootdown::flush_process_write_buffers();
    0
}

/// Synchronize code publication on every online processor. A whole-cache sweep
/// covers ranges in another address space without touching its virtual memory.
/// Process handles still require PROCESS_VM_WRITE before any side effect.
pub extern "C" fn nt_flush_instruction_cache(
    process: super::types::HANDLE,
    base: super::types::PVOID,
    length: usize,
) -> i32 {
    use super::{file::{with_sync_access, HandleKind}, status::*};
    // NT treats a non-null empty range as a no-op, before handle validation.
    if !base.is_null() {
        if length == 0 { return STATUS_SUCCESS; }
        if base as u64 > crate::mm::constants::USER_LIMIT {
            return STATUS_ACCESS_VIOLATION;
        }
    }
    if process as usize != usize::MAX {
        let validation = with_sync_access(process, 0x20, |entries| {
            if entries[0].kind != HandleKind::Process {
                return Err(STATUS_OBJECT_TYPE_MISMATCH);
            }
            Ok(())
        });
        if let Err(status) = validation { return status; }
    }
    crate::ke::instruction_shootdown::flush_all();
    STATUS_SUCCESS
}

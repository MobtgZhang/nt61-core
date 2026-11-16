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

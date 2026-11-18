//! Memory Diagnostic Commands
//
//! Provides command-line interface for memory testing in Safe Mode.

/// Print memory status
pub fn cmd_memory_status() {
    use crate::mm;

    // // crate::kprintln!("[INFO]   [MEMORY]   0  ");
    // // crate::kprintln!("[INFO]   [MEMORY]   0  === Memory Status ===");
    // // crate::kprintln!("[INFO]   [MEMORY]   0  ");

    // Get memory info from PFN database
    let free_count = mm::pfn::get_free_pfns();
    let _ = &free_count;
    let total_count = mm::pfn::get_database_count();
    let _ = &total_count;

    // // crate::kprintln!("[INFO]   [MEMORY]   0  Physical Memory:");
    // // crate::kprintln!("[INFO]   [MEMORY]   0    Total Pages: {}", total_count);
    // // crate::kprintln!("[INFO]   [MEMORY]   0    Free Pages:  {}", free_count);
    // // crate::kprintln!("[INFO]   [MEMORY]   0    Used Pages:  {}", total_count.saturating_sub(free_count));
}

/// Run memory test
pub fn cmd_memory_test() {
    use crate::mm::memtest;

    // // crate::kprintln!("[INFO]   [DIAG]     0  ");
    // // crate::kprintln!("[INFO]   [DIAG]     0  Running memory test...");
    // // crate::kprintln!("[DEBUG]  [DIAG]     0    Patterns: {}", memtest::TestPatterns::PATTERNS.len());

    // Get test range from available memory
    let test_start = 0x100_000; // 1MB
    let test_end = 0x100_000 + (64 * 1024 * 1024); // 64MB test range

    // // crate::kprintln!("[DEBUG]  [DIAG]     0    Range: 0x{:016X} - 0x{:016X}",
    // //               test_start, test_end);

    // Run tests using quick_test
    let passed = memtest::quick_test(test_start, test_end);

    if passed {
        // // crate::kprintln!("[INFO]   [DIAG]     0  Memory test PASSED");
        // // crate::kprintln!("[INFO]   [DIAG]     0  {} KB tested, 0 errors", (test_end - test_start) / 1024);
    } else {
        // // crate::kprintln!("[ERROR]  [DIAG]     0  Memory test FAILED");
        // // crate::kprintln!("[ERROR]  [DIAG]     0  Errors detected in test range");
    }
}

/// Dump PFN database state
pub fn cmd_pfn_dump() {

    // // crate::kprintln!("[INFO]   [PFN]      0  ");
    // // crate::kprintln!("[INFO]   [PFN]      0  === PFN Database Dump ===");
    // // crate::kprintln!("[INFO]   [PFN]      0  ");
    // // crate::kprintln!("[INFO]   [PFN]      0    Total Pages: {}", pfn::get_database_count());
    // // crate::kprintln!("[INFO]   [PFN]      0    Free PFNs:  {}", pfn::get_free_pfns());
    // // crate::kprintln!("[INFO]   [PFN]      0    Database Base: 0x{:016X}", pfn::get_database_base());
}

/// Import only the validated runtime-page handoff, after mounting the system volume.
/// Keep a readable report and a one-time desktop notification payload.
pub fn publish_boot_result(info: &crate::boot_types::BootInfo) {
    use nt61_boot_types::memory_diagnostic as md;
    if info.memtest_signature != md::SIGNATURE || info.memtest_size != md::SIZE as u64 || info.memtest_base == 0 { return; }
    // SAFETY: winload reserves this exact payload in runtime memory before EBS;
    // BootInfo is the trusted loader contract and size/signature are checked above.
    let bytes = unsafe { core::slice::from_raw_parts(info.memtest_base as *const u8, md::SIZE) };
    let Some(record) = md::ResultRecord::decode(bytes) else { return; };
    if record.status != info.memtest_status { return; }
    // Windows 7 exposes completed diagnostics through the System channel.
    // Use the repository's event backend and the corresponding provider/IDs.
    if matches!(record.status, md::PASSED | md::FAILED) {
        use crate::rtl::eventlog::{self, EventLevel};
        eventlog::ensure_initialized();
        let (id, level) = if record.status == md::PASSED {
            (1201, EventLevel::Information)
        } else {
            (1202, EventLevel::Error)
        };
        eventlog::write_system_event(id, level, b"Microsoft-Windows-MemoryDiagnostics-Results", record.summary());
        crate::hal::serial::write_string_always(&alloc::format!("[MEMORY-RESULT] system event={}\r\n", id));
    }
    let report = alloc::format!("Windows Memory Diagnostic\r\n{}\r\nStatus: {}\r\nCompleted passes: {}\r\nTested bytes per pass: {}\r\nAvailable bytes: {}\r\nExcluded bytes: {}\r\nErrors: {}\r\nFirst failure: address={:#x} expected={:#x} actual={:#x}\r\n", record.summary(), record.status, record.passes, record.tested, record.available, record.excluded, record.errors, record.address, record.expected, record.actual);
    let write = |path, data: &[u8]| crate::fs::system_volume::write(path, data).or_else(|_| crate::fs::system_volume::create(path, data));
    let saved = write("C:\\Windows\\MemoryDiagnosticResults.txt", report.as_bytes()).is_ok();
    let pending = write("C:\\Windows\\MemoryDiagnosticResults.bin", bytes).is_ok();
    // Only acknowledge after both durable outputs exist. Winload retries an
    // unacknowledged result on the next boot rather than losing it on a crash.
    if saved && pending && crate::fs::system_volume::flush().is_ok() {
        let acknowledged = write("C:\\Windows\\MemoryDiagnosticResults.ack", bytes)
            .and_then(|()| crate::fs::system_volume::flush()).is_ok();
        crate::hal::serial::write_string_always(&alloc::format!("[MEMORY-RESULT] acknowledged={} run={}\r\n", acknowledged, record.run_id));
    }
    crate::hal::serial::write_string_always(&alloc::format!("[MEMORY-RESULT] published status={} report={} notification={}\r\n", record.status, saved, pending));
}

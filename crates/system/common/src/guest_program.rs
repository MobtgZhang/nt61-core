//! Mounted-volume x86/x64 console PE execution using the portable backend.
//! This is a bounded command invocation, not a substitute for a full Win32
//! process (GUI dispatch, SEH and guest thread creation
//! remain separate). Normal imports link only to checked guest DLL code.
use crate::arch::common::btl::{
    io::{IoHost, IoServices},
    pe::ImageRunResult,
    runtime::StopReason,
};
use alloc::{format, string::String, vec::Vec};

struct VolumeIo {
    output: fn(&[u8]),
    application_directory: String,
}
impl IoHost for VolumeIo {
    fn memory_status(&mut self) -> Result<crate::arch::common::btl::nt61::MemoryStatus, u32> {
        Ok(crate::arch::common::btl::nt61::MemoryStatus {
            physical_total: crate::mm::get_total_physical(),
            physical_available: crate::mm::get_free_physical(),
            paging_total: crate::mm::pagefile::get_total_pagefile_pages().saturating_mul(4096),
            paging_available: crate::mm::pagefile::get_total_free_pagefile_pages()
                .saturating_mul(4096),
        })
    }
    fn processor_mask(&mut self) -> Result<u64, u32> {
        let count = crate::arch::common::smp::cpu_count().min(64);
        if count == 0 {
            return Err(0xc0000001);
        }
        Ok(u64::MAX >> (64 - count))
    }
    fn clocks(&mut self) -> Result<(u64, u64), u32> {
        Ok((
            crate::ke::time::get_system_time(),
            crate::ke::time::get_interrupt_time(),
        ))
    }
    fn delay_execution(&mut self, alertable: bool, interval: i64) -> u32 {
        crate::libs::ntdll::sync::nt_delay_execution_kernel(u8::from(alertable), interval) as u32
    }
    fn read_library(
        &mut self,
        name: &str,
        mode: crate::arch::common::btl::ir::DecodeMode,
    ) -> Result<Vec<u8>, u32> {
        use crate::arch::common::btl::pe::ImportResolver;
        crate::userspace::loader::VolumeLibraries {
            application_directory: &self.application_directory,
        }
        .read_library(name, mode)
    }
    fn open_read(&mut self, path: &str) -> Result<(), u32> {
        // This read validates file existence through the mounted FS without
        // loading its complete contents or opening directories as files.
        crate::fs::system_volume::read_at(path, 0, 1).map(|_| ())
    }
    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, u32> {
        crate::fs::system_volume::read_at(path, offset, len)
    }
    fn console_write(&mut self, bytes: &[u8]) -> Result<(), u32> {
        (self.output)(bytes);
        Ok(())
    }
}

/// `None` means no executable was found. An existing but unsupported PE returns
/// an error and diagnostic, never a fake successful process or native jump.
pub fn try_execute(
    line: &str,
    directory: &str,
    search_path: &str,
    output: fn(&[u8]),
) -> Option<u32> {
    let line = line.trim();
    let name = if let Some(quoted) = line.strip_prefix('"') {
        let (name, tail) = quoted.split_once('"')?;
        if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
            return None;
        }
        name
    } else {
        line.split_whitespace().next()?
    };
    let leaf = name.rsplit(['\\', '/']).next()?;
    let name = if leaf.contains('.') {
        String::from(name)
    } else {
        format!("{name}.exe")
    };
    if !name.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    let mut candidates = Vec::new();
    if name.contains(['\\', '/']) || name.contains(':') {
        let absolute = if name.as_bytes().get(1) == Some(&b':') || name.starts_with(['\\', '/']) {
            name
        } else {
            format!("{directory}\\{name}")
        };
        candidates.push(absolute);
    } else {
        candidates.push(format!("{directory}\\{name}"));
        for entry in search_path.split(';').filter(|e| !e.is_empty()) {
            candidates.push(format!("{}\\{name}", entry.trim_matches('"')));
        }
    }
    for candidate in candidates {
        let Ok(candidate) = crate::fs::system_volume::normalize(&candidate) else {
            continue;
        };
        let header = match crate::fs::system_volume::read_at(&candidate, 0, 64) {
            Ok(header) => header,
            Err(_) => continue,
        };
        if header.len() < 64 || &header[..2] != b"MZ" {
            output(b"The file is not a valid Windows executable.\r\n");
            return Some(193);
        }
        let mut image =
            match crate::userspace::loader::load_translated_image(&candidate, line, directory) {
                Ok(image) => image,
                Err(error) => {
                    let status = error.ntstatus();
                    let diagnostic = format!(
                        "Cannot execute {candidate}: {error}; NTSTATUS 0x{status:08X}.\r\n"
                    );
                    crate::hal::serial::write_string_always("[PE] ");
                    crate::hal::serial::write_string_always(&diagnostic);
                    output(diagnostic.as_bytes());
                    return Some(status);
                }
            };
        let mut services = IoServices::new(VolumeIo {
            output,
            application_directory: candidate
                .rsplit_once('\\')
                .map_or(directory, |(parent, _)| parent)
                .into(),
        });
        // Bounded maintenance/desktop console execution. Every quantum lets
        // runnable kernel/desktop threads proceed; no global BTL lock is held.
        // The supplied Firefox SFX executes ~3.7M instructions just to unpack
        // itself. Keep a finite bound while retaining a scheduler yield per quantum.
        for _ in 0..4096 {
            match image.run(2048, &mut services) {
                ImageRunResult::Exited(code) => {
                    if !image.win32.failures.is_empty() {
                        let diagnostic = format!(
                            "[PE] Guest exited {code}; recent API failures: {:?}\r\n",
                            image.win32.failures
                        );
                        crate::hal::serial::write_string_always(&diagnostic);
                        output(diagnostic.as_bytes());
                    }
                    return Some(code);
                }
                ImageRunResult::Yielded => crate::ke::scheduler::yield_(),
                ImageRunResult::Fault(reason) => {
                    let status = match reason {
                        StopReason::InitializationFailed { .. } => 0xc0000142,
                        _ => 0xc000001d,
                    };
                    let api = image
                        .win32
                        .last_api
                        .and_then(crate::arch::common::btl::pe::win32::catalog::resolve)
                        .map(|api| format!("{}!{}", api.dll, api.name))
                        .unwrap_or_default();
                    let diagnostic = format!(
                        "Guest execution fault in {candidate}: {reason:?}; API {api}; NTSTATUS 0x{status:08X}.\r\n"
                    );
                    crate::hal::serial::write_string_always("[PE] ");
                    crate::hal::serial::write_string_always(&diagnostic);
                    output(diagnostic.as_bytes());
                    return Some(status);
                }
            }
        }
        output(b"Guest instruction budget exceeded; console invocation stopped.\r\n");
        return Some(1460);
    }
    None
}

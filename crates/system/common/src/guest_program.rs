//! Mounted-volume x86/x64 console PE execution using the portable backend.
//! This is a bounded command invocation, not a substitute for a full Win32
//! process (DLL initialization, GUI dispatch, SEH and guest thread creation
//! remain separate). Normal imports link only to checked guest DLL code.
use crate::arch::common::btl::{
    io::{IoHost, IoServices},
    pe::ImageRunResult,
};
use alloc::{format, string::String, vec::Vec};

struct VolumeIo {
    output: fn(&[u8]),
}
impl IoHost for VolumeIo {
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
        let mut services = IoServices::new(VolumeIo { output });
        // Bounded maintenance/desktop console execution. Every quantum lets
        // runnable kernel/desktop threads proceed; no global BTL lock is held.
        for _ in 0..512 {
            match image.run(2048, &mut services) {
                ImageRunResult::Exited(code) => return Some(code),
                ImageRunResult::Yielded => crate::ke::scheduler::yield_(),
                ImageRunResult::Fault(reason) => {
                    output(format!("Guest execution fault: {reason:?}.\r\n").as_bytes());
                    return Some(0xc000001d);
                }
            }
        }
        output(b"Guest instruction budget exceeded; console invocation stopped.\r\n");
        return Some(1460);
    }
    None
}

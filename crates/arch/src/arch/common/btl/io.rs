//! Checked NT 6.1 console and read-only file services for isolated PE guests.
//! All nested guest structures are copied by ABI; no address becomes a host pointer.
use super::{
    abi::GuestAbi,
    memory::GuestMemory,
    nt61::{self, BasicServices, GuestServices, Service, ServiceResult},
};
use alloc::{string::String, vec, vec::Vec};
const INVALID_PARAMETER: u32 = 0xc000_000d;
const INVALID_HANDLE: u32 = 0xc000_0008;
const ACCESS_DENIED: u32 = 0xc000_0022;
const EOF: u32 = 0xc000_0011;
const MAX_IO: usize = 65536;
pub const STD_INPUT: u64 = 0x10;
pub const STD_OUTPUT: u64 = 0x14;
pub const STD_ERROR: u64 = 0x18;

/// Host operations receive owned, validated values rather than guest pointers.
pub trait IoHost {
    fn open_read(&mut self, path: &str) -> Result<(), u32>;
    fn read_at(&mut self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>, u32>;
    fn console_write(&mut self, bytes: &[u8]) -> Result<(), u32>;
}
struct File {
    handle: u64,
    path: String,
    position: u64,
}
pub struct IoServices<H> {
    pub host: H,
    files: Vec<File>,
    next_handle: u64,
    fs_redirection: bool,
}
impl<H: IoHost> IoServices<H> {
    pub fn new(host: H) -> Self {
        Self {
            host,
            files: Vec::new(),
            next_handle: 0x100,
            fs_redirection: true,
        }
    }
    /// Per-guest-thread state: callers retain this value for disable/revert.
    /// This does not change another guest's filesystem view.
    pub fn set_fs_redirection(&mut self, enabled: bool) -> bool {
        core::mem::replace(&mut self.fs_redirection, enabled)
    }
    fn invoke(
        &mut self,
        abi: GuestAbi,
        service: Service,
        a: &[u64],
        m: &mut dyn GuestMemory,
    ) -> Result<u32, u32> {
        let width = if abi == GuestAbi::X86_32 { 4 } else { 8 };
        match service {
            Service::CreateFile => {
                // FILE_OPEN, synchronous read-only file, no EA or allocation hint.
                if a[7] != 1 || a[4] != 0 || a[9] != 0 || a[10] != 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                if a[1] & !(0x8000_0000 | 0x0012_0089) != 0 {
                    return Err(ACCESS_DENIED);
                }
                if a[8] & !0x60 != 0 || a[8] & 0x30 == 0 || a[6] & !7 != 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                writable(m, a[0], width)?;
                writable(m, a[3], width * 2)?;
                let oa = read(m, a[2], if width == 4 { 24 } else { 48 })?;
                if word(&oa, 0, 4) != oa.len() as u64 {
                    return Err(INVALID_PARAMETER);
                }
                let root = word(&oa, if width == 4 { 4 } else { 8 }, width);
                let name = word(&oa, if width == 4 { 8 } else { 16 }, width);
                let attributes = word(&oa, if width == 4 { 12 } else { 24 }, 4);
                let sd = word(&oa, if width == 4 { 16 } else { 32 }, width);
                let sqos = word(&oa, if width == 4 { 20 } else { 40 }, width);
                if root != 0 || sd != 0 || sqos != 0 || attributes & !0x40 != 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                let us = read(m, name, if width == 4 { 8 } else { 16 })?;
                let len = word(&us, 0, 2) as usize;
                let max = word(&us, 2, 2) as usize;
                if len == 0 || len > 32766 || len % 2 != 0 || len > max {
                    return Err(INVALID_PARAMETER);
                }
                let bytes = read(m, word(&us, if width == 4 { 4 } else { 8 }, width), len)?;
                let utf16: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                let path = String::from_utf16(&utf16).map_err(|_| INVALID_PARAMETER)?;
                if path.contains('\0') {
                    return Err(INVALID_PARAMETER);
                }
                let path = path.strip_prefix("\\??\\").unwrap_or(&path);
                // The host VFS resolves the volume; relative NT paths need RootDirectory semantics.
                if path.as_bytes().get(1..3) != Some(b":\\") {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                if self.files.len() >= 64 {
                    return Err(0xc000_011f);
                }
                let path = guest_file_path(abi, path, self.fs_redirection)?;
                self.host.open_read(&path)?;
                let handle = self.next_handle;
                let next = handle
                    .checked_add(4)
                    .filter(|n| *n <= u32::MAX as u64)
                    .ok_or(0xc000_011fu32)?;
                put(m, a[0], handle, width)?;
                iosb(m, a[3], width, 0, 1)?; // FILE_OPENED
                self.files.push(File {
                    handle,
                    path,
                    position: 0,
                });
                self.next_handle = next;
                Ok(0)
            }
            Service::Close => {
                let index = self
                    .files
                    .iter()
                    .position(|f| f.handle == a[0])
                    .ok_or(INVALID_HANDLE)?;
                self.files.remove(index);
                Ok(0)
            }
            Service::ReadFile | Service::WriteFile => {
                // APC/event completion and keyed IO require real dispatcher objects.
                if a[1..4].iter().any(|v| *v != 0) || a[8] != 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                let len = usize::try_from(a[6]).map_err(|_| INVALID_PARAMETER)?;
                if len > MAX_IO {
                    return Err(INVALID_PARAMETER);
                }
                writable(m, a[4], width * 2)?;
                if service == Service::WriteFile {
                    if a[0] != STD_OUTPUT && a[0] != STD_ERROR {
                        return Err(INVALID_HANDLE);
                    }
                    if a[7] != 0 {
                        return Err(INVALID_PARAMETER);
                    }
                    let bytes = read(m, a[5], len)?;
                    self.host.console_write(&bytes)?;
                    iosb(m, a[4], width, 0, len as u64)?;
                    return Ok(0);
                }
                let file = self
                    .files
                    .iter_mut()
                    .find(|f| f.handle == a[0])
                    .ok_or(INVALID_HANDLE)?;
                writable(m, a[5], len)?;
                let offset = if a[7] == 0 {
                    file.position
                } else {
                    let b = read(m, a[7], 8)?;
                    let raw = word(&b, 0, 8);
                    if raw == u64::MAX - 1 {
                        file.position
                    } else if raw > i64::MAX as u64 {
                        return Err(INVALID_PARAMETER);
                    } else {
                        raw
                    }
                };
                let bytes = self.host.read_at(&file.path, offset, len)?;
                if bytes.len() > len {
                    return Err(0xc000_0185);
                }
                let end = offset
                    .checked_add(bytes.len() as u64)
                    .ok_or(INVALID_PARAMETER)?;
                if !m.write_virt(a[5], &bytes) {
                    return Err(nt61::STATUS_ACCESS_VIOLATION);
                }
                let status = if len != 0 && bytes.is_empty() { EOF } else { 0 };
                iosb(m, a[4], width, status, bytes.len() as u64)?;
                file.position = end;
                Ok(status)
            }
            _ => Err(nt61::STATUS_NOT_IMPLEMENTED),
        }
    }
}
impl<H: IoHost> GuestServices for IoServices<H> {
    fn call(
        &mut self,
        abi: GuestAbi,
        service: Service,
        args: &[u64],
        memory: &mut dyn GuestMemory,
    ) -> ServiceResult {
        if abi == GuestAbi::Arm32 {
            return ServiceResult::Return(nt61::STATUS_INVALID_SYSTEM_SERVICE);
        }
        if args.len() != service.argument_count() {
            return ServiceResult::Return(INVALID_PARAMETER);
        }
        if matches!(
            service,
            Service::TerminateProcess | Service::TerminateThread | Service::YieldExecution
        ) {
            return BasicServices.call(abi, service, args, memory);
        }
        ServiceResult::Return(
            self.invoke(abi, service, args, memory)
                .unwrap_or_else(|s| s),
        )
    }
}
fn word(bytes: &[u8], offset: usize, width: usize) -> u64 {
    let mut b = [0; 8];
    b[..width].copy_from_slice(&bytes[offset..offset + width]);
    u64::from_le_bytes(b)
}
fn read(m: &mut dyn GuestMemory, va: u64, len: usize) -> Result<Vec<u8>, u32> {
    let mut b = vec![0; len];
    if m.read_virt(va, &mut b) != len {
        Err(nt61::STATUS_ACCESS_VIOLATION)
    } else {
        Ok(b)
    }
}
fn writable(m: &mut dyn GuestMemory, va: u64, len: usize) -> Result<(), u32> {
    if m.probe_write(va, len) {
        Ok(())
    } else {
        Err(nt61::STATUS_ACCESS_VIOLATION)
    }
}
fn put(m: &mut dyn GuestMemory, va: u64, value: u64, width: usize) -> Result<(), u32> {
    if m.write_virt(va, &value.to_le_bytes()[..width]) {
        Ok(())
    } else {
        Err(nt61::STATUS_ACCESS_VIOLATION)
    }
}
fn iosb(
    m: &mut dyn GuestMemory,
    va: u64,
    width: usize,
    status: u32,
    count: u64,
) -> Result<(), u32> {
    let mut b = [0; 16];
    b[..4].copy_from_slice(&status.to_le_bytes());
    b[width..width * 2].copy_from_slice(&count.to_le_bytes()[..width]);
    if m.write_virt(va, &b[..width * 2]) {
        Ok(())
    } else {
        Err(nt61::STATUS_ACCESS_VIOLATION)
    }
}

/// File view for this project's C:\Windows installation. Win7 WOW64 redirects
/// System32, except its documented shared subtrees (including DriverStore on
/// Windows 7). Sysnative is a virtual alias only for 32-bit callers.
/// See learn.microsoft.com/windows/win32/winprog64/file-system-redirector.
/// Relative names, other NT namespaces and UAC elevation are not handled here.
pub fn guest_file_path(abi: GuestAbi, path: &str, enabled: bool) -> Result<String, u32> {
    if abi == GuestAbi::Arm32 {
        return Err(nt61::STATUS_INVALID_SYSTEM_SERVICE);
    }
    let bytes = path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || &bytes[1..3] != b":\\"
        || path.contains('\0')
    {
        return Err(INVALID_PARAMETER);
    }
    // Resolve dot components before choosing a view; System32\.. must not
    // accidentally escape into a different view after VFS normalization.
    let mut parts: Vec<&str> = Vec::new();
    for part in path[3..].split(['\\', '/']) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(INVALID_PARAMETER);
                }
            }
            _ => parts.push(part),
        }
    }
    if abi == GuestAbi::X86_32
        && bytes[0].eq_ignore_ascii_case(&b'C')
        && parts
            .first()
            .is_some_and(|s| s.eq_ignore_ascii_case("Windows"))
    {
        if parts
            .get(1)
            .is_some_and(|s| s.eq_ignore_ascii_case("Sysnative"))
        {
            parts[1] = "System32";
        } else if enabled
            && parts
                .get(1)
                .is_some_and(|s| s.eq_ignore_ascii_case("System32"))
        {
            let shared = parts.get(2).is_some_and(|s| {
                ["catroot", "catroot2", "driverstore", "logfiles", "spool"]
                    .iter()
                    .any(|name| s.eq_ignore_ascii_case(name))
            }) || (parts
                .get(2)
                .is_some_and(|s| s.eq_ignore_ascii_case("drivers"))
                && parts.get(3).is_some_and(|s| s.eq_ignore_ascii_case("etc")));
            if !shared {
                parts[1] = "SysWOW64";
            }
        } else if enabled && parts.len() == 2 && parts[1].eq_ignore_ascii_case("regedit.exe") {
            parts.insert(1, "SysWOW64");
        }
    }
    let mut result = String::from(&path[..3]);
    result.push_str(&parts.join("\\"));
    Ok(result)
}

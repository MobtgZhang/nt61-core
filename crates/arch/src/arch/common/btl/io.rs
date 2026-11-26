//! Checked NT 6.1 console and synchronous file services for isolated PE guests.
//! All nested guest structures are copied by ABI; no address becomes a host pointer.
use super::{
    abi::GuestAbi,
    memory::GuestMemory,
    nt61::{self, BasicServices, GuestServices, Service, ServiceResult, SocketKind},
};
use alloc::{string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
#[path = "io/file_ranges.rs"]
mod file_ranges;
pub use file_ranges::{FileLockKey, FileRangeLocks};

static NEXT_FILE_OWNER: AtomicU64 = AtomicU64::new(1);
const INVALID_PARAMETER: u32 = 0xc000_000d;
const INVALID_HANDLE: u32 = 0xc000_0008;
const ACCESS_DENIED: u32 = 0xc000_0022;
const EOF: u32 = 0xc000_0011;
const MAX_IO: usize = 65536;
const FILE_WRITE_ATTRIBUTES: u64 = 0x100;
const GENERIC_WRITE: u64 = 0x4000_0000;
pub const STD_INPUT: u64 = 0x10;
pub const STD_OUTPUT: u64 = 0x14;
pub const STD_ERROR: u64 = 0x18;

/// Host operations receive owned, validated values rather than guest pointers.
pub trait IoHost {
    fn create_process(&mut self, _request: nt61::ChildRequest) -> Result<nt61::ChildProcess, u32> { Err(120) }
    fn resolve_ipv4(&mut self, _name: &str) -> Result<[u8; 4], u32> { Err(11003) }
    fn open_socket(&mut self, _kind: SocketKind) -> Result<u32, u32> { Err(10050) }
    fn close_socket(&mut self, _socket: u32) -> Result<(), u32> { Err(10038) }
    fn connect_tcp(&mut self, _socket: u32, _ip: [u8; 4], _port: u16) -> Result<(), u32> { Err(10045) }
    fn send_tcp(&mut self, _socket: u32, _bytes: &[u8]) -> Result<usize, u32> { Err(10045) }
    fn recv_tcp(&mut self, _socket: u32, _bytes: &mut [u8]) -> Result<usize, u32> { Err(10045) }
    /// Backend event identifiers are private to IoServices and never guest
    /// handles. The creator retains ownership until close_event succeeds.
    fn create_event(&mut self, _manual: bool, _signaled: bool) -> Result<u64, u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn change_event(&mut self, _event: u64, _signaled: bool) -> Result<(), u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn wait_event(&mut self, _event: u64, _timeout: Option<i64>) -> Result<u32, u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn poll_events(&mut self, _events: &[u64], _all: bool) -> Result<u32, u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn close_event(&mut self, _event: u64) -> Result<(), u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn create_memory_dc(&mut self, _source: u64) -> Option<u64> { None }
    fn delete_memory_dc(&mut self, _dc: u64) -> bool { false }
    fn device_caps(&mut self, _dc: u64, _index: i32) -> Option<u32> { None }
    fn draw_text_layout(&mut self, _dc: u64, _clip: Option<[i32; 4]>, _lines: &[(i32, i32, &[u16])]) -> bool { false }
    fn create_font_w(&mut self, _description: &[u8; 92]) -> Option<u64> { None }
    fn select_font(&mut self, _dc: u64, _font: u64) -> Option<u64> { None }
    fn delete_font(&mut self, _font: u64) -> bool { false }
    fn enumerate_fonts_w(&mut self, _dc: u64, _family: Option<&str>) -> Option<Vec<nt61::FontEnumeration>> { None }
    fn text_metrics_w(&mut self, _dc: u64) -> Option<[u8; 60]> { None }
    fn text_face_w(&mut self, _dc: u64) -> Option<Vec<u16>> { None }
    fn text_extent_w(&mut self, _dc: u64, _text: &[u16]) -> Option<[i32; 2]> { None }
    fn set_bk_mode(&mut self, _dc: u64, _mode: i32) -> Option<i32> { None }
    fn set_map_mode(&mut self, _dc: u64, _mode: i32) -> Option<i32> { None }
    fn set_dc_extent(&mut self, _dc: u64, _kind: nt61::DcExtent, _extent: [i32; 2]) -> Option<(bool, [i32; 2])> { None }
    fn logical_to_device(&mut self, _dc: u64, _points: &mut [u8]) -> bool { false }
    fn create_directory(&mut self, _path: &str) -> Result<(), u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn directory_entries(&mut self, _path: &str) -> Result<Vec<nt61::DirectoryEntry>, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn file_attributes(&mut self, _path: &str) -> Result<u32, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn file_basic_information(&mut self, _path: &str) -> Result<nt61::FileBasicInformation, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn set_file_attributes(&mut self, _path: &str, _attributes: u32) -> Result<(), u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn delete_file(&mut self, _path: &str) -> Result<(), u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn remove_directory(&mut self, _path: &str) -> Result<(), u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn move_file(&mut self, _source: &str, _destination: &str) -> Result<(), u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn set_file_times(&mut self, _path: &str, _times: [Option<u64>; 3]) -> Result<(), u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn application_log_available(&mut self) -> bool { false }
    fn report_application_event(&mut self, _event: nt61::ApplicationEvent) -> Result<(), u32> { Err(120) }

    fn native_processor_info(&mut self) -> Result<nt61::NativeProcessorInfo, u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn unbiased_interrupt_time(&mut self) -> Result<u64, u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn time_adjustment(&mut self) -> Result<nt61::TimeAdjustment, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn random_bytes(&mut self, _bytes: &mut [u8]) -> Result<(), u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn printer_available(&mut self) -> bool { false }
    fn print_document(&mut self, _data: &[u8]) -> Result<u32,u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn message_beep(&mut self, _kind: u32) -> bool { false }

    /// Copied private desktop protocol. Never accepts a guest process pointer.
    fn desktop_dialog(&mut self, _request: &[u8]) -> Result<alloc::vec::Vec<u8>, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }

    fn memory_status(&mut self) -> Result<nt61::MemoryStatus, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn processor_mask(&mut self) -> Result<u64, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn read_library(&mut self, _name: &str, _mode: super::ir::DecodeMode) -> Result<Vec<u8>, u32> {
        Err(0xc000_0135)
    }
    fn read_system_library(&mut self, _name: &str, _mode: super::ir::DecodeMode) -> Result<Vec<u8>, u32> {
        Err(0xc000_0135)
    }
    fn resolve_library(&mut self, name: &str, mode: super::ir::DecodeMode, system: bool)
        -> Result<super::pe::ResolvedLibrary, u32> {
        let bytes = if system { self.read_system_library(name, mode) } else { self.read_library(name, mode) }?;
        Ok(super::pe::ResolvedLibrary { bytes, path: None })
    }
    fn open_read(&mut self, path: &str) -> Result<(), u32>;
    fn open_file(&mut self, path: &str, disposition: u32, writable: bool, directory: bool) -> Result<(u64, bool), u32> {
        if disposition != 1 || writable || directory {
            return Err(nt61::STATUS_NOT_IMPLEMENTED);
        }
        self.open_read(path)?;
        Ok((1, false))
    }
    fn open_tracked_file(&mut self, path: &str, disposition: u32, writable: bool, directory: bool) -> Result<(u64, bool, Option<u64>), u32> {
        let (information, directory) = self.open_file(path, disposition, writable, directory)?;
        Ok((information, directory, None))
    }
    fn close_tracked_file(&mut self, _identity: u64) {}
    fn set_file_range_lock(&mut self, _key: FileLockKey, _offset: u64, _length: u64, _lock: bool) -> Result<(), u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn release_file_range_locks(&mut self, _key: FileLockKey) {}
    fn file_size(&mut self, _path: &str) -> Result<u64, u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn flush_file_buffers(&mut self) -> Result<(), u32> { Err(nt61::STATUS_NOT_IMPLEMENTED) }
    fn volume_allocation(&mut self, _directory: &str) -> Result<nt61::VolumeAllocation, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn read_at(&mut self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>, u32>;
    fn write_at(&mut self, _path: &str, _offset: u64, _bytes: &[u8]) -> Result<usize, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn read_open_file(&mut self, path: &str, _key: Option<FileLockKey>, offset: u64, length: usize) -> Result<Vec<u8>, u32> {
        self.read_at(path, offset, length)
    }
    fn write_open_file(&mut self, path: &str, _key: Option<FileLockKey>, offset: u64, bytes: &[u8]) -> Result<usize, u32> {
        self.write_at(path, offset, bytes)
    }
    fn read_mapped_file(&mut self, path: &str, identity: Option<u64>, offset: u64, length: usize) -> Result<Vec<u8>, u32> {
        if identity.is_some() { return Err(nt61::STATUS_NOT_IMPLEMENTED); }
        self.read_at(path, offset, length)
    }
    fn write_mapped_file(&mut self, _path: &str, _identity: Option<u64>, _offset: u64, _bytes: &[u8]) -> Result<usize, u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    fn console_write(&mut self, bytes: &[u8]) -> Result<(), u32>;
    /// NT wall time and monotonic uptime in 100-ns units respectively.
    fn clocks(&mut self) -> Result<(u64, u64), u32> {
        Err(nt61::STATUS_NOT_IMPLEMENTED)
    }
    /// Captured NT interval in 100-ns units; never a guest address.
    fn delay_execution(&mut self, _alertable: bool, _interval: i64) -> u32 {
        nt61::STATUS_NOT_IMPLEMENTED
    }
}
struct File {
    handle: u64,
    identity: Option<u64>,
    owner: u64,
    inheritable: bool,
    path: String,
    position: u64,
    readable: bool,
    writable: bool,
    write_attributes: bool,
    directory: bool,
    overlapped: bool,
}
impl File {
    fn lock_key(&self) -> Option<FileLockKey> {
        self.identity.map(|record| FileLockKey { record, owner: self.owner })
    }
}

fn close_open_file(host: &mut impl IoHost, file: File) {
    if let Some(key) = file.lock_key() {
        host.release_file_range_locks(key);
        host.close_tracked_file(key.record);
    }
}

struct MappedFile {
    mapping: u64,
    identity: Option<u64>,
    path: String,
    writable: bool,
}
struct GuestSocket {
    handle: u64,
    backend: u32,
    kind: SocketKind,
    connected: bool,
}

pub struct IoServices<H: IoHost> {
    pub host: H,
    files: Vec<File>,
    mapped_files: Vec<MappedFile>,
    events: Vec<(u64, u64)>, // guest handle, backend-owned event identifier
    children: Vec<(u64, u64, nt61::ChildProcess)>,
    sockets: Vec<GuestSocket>,
    memory_dcs: Vec<(u64, u64)>,
    fonts: Vec<(u64, u64)>,
    stock_font: Option<(u64, u64)>,
    next_handle: u64,
    fs_redirection: bool,
}
impl<H: IoHost> IoServices<H> {
    pub fn new(host: H) -> Self {
        Self {
            host,
            files: Vec::new(),
            mapped_files: Vec::new(),
            events: Vec::new(),
            children: Vec::new(),
            sockets: Vec::new(),
            memory_dcs: Vec::new(),
            fonts: Vec::new(),
            stock_font: None,
            next_handle: 0x100,
            fs_redirection: true,
        }
    }
    /// Per-guest-thread state: callers retain this value for disable/revert.
    /// This does not change another guest's filesystem view.
    pub fn set_fs_redirection(&mut self, enabled: bool) -> bool {
        core::mem::replace(&mut self.fs_redirection, enabled)
    }

    fn tcp_index(&self, handle: u64) -> Result<usize, u32> {
        let index = self.sockets.iter().position(|socket| socket.handle == handle).ok_or(10038u32)?;
        if self.sockets[index].kind != SocketKind::TcpV4 { return Err(10045); }
        Ok(index)
    }

    fn connected_tcp(&self, handle: u64) -> Result<u32, u32> {
        let socket = &self.sockets[self.tcp_index(handle)?];
        if !socket.connected { return Err(10057); }
        Ok(socket.backend)
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
            Service::DelayExecution => {
                let bytes = read(m, a[1], 8)?;
                let interval = word(&bytes, 0, 8) as i64;
                Ok(self.host.delay_execution(a[0] as u8 != 0, interval))
            }
            Service::QuerySystemTime => {
                writable(m, a[0], 8)?;
                let (wall, _) = self.host.clocks()?;
                put(m, a[0], wall, 8)?;
                Ok(0)
            }
            Service::QueryPerformanceCounter => {
                writable(m, a[0], 8)?;
                if a[1] != 0 {
                    writable(m, a[1], 8)?;
                }
                let (_, monotonic) = self.host.clocks()?;
                put(m, a[0], monotonic, 8)?;
                if a[1] != 0 {
                    put(m, a[1], 10_000_000, 8)?;
                }
                Ok(0)
            }
            Service::CreateFile => {
                if !(1..=5).contains(&a[7]) || a[4] != 0 || a[9] != 0 || a[10] != 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                if a[1] & !(0xc000_0000 | 0x0012_019f) != 0 {
                    return Err(ACCESS_DENIED);
                }
                if a[8] & !0x0000_4074 != 0 || a[8] & 0x30 == 0x30 || a[6] & !7 != 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                let overlapped = a[8] & 0x30 == 0;
                if overlapped && a[8] & 0x40 == 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                let readable = a[1] & 0x8000_0001 != 0;
                let can_write = a[1] & 0x4000_0002 != 0;
                if matches!(a[7], 4 | 5) && !can_write {
                    return Err(ACCESS_DENIED);
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
                if root != 0 || sd != 0 || sqos != 0 || attributes & !0x42 != 0 {
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
                let handle = self.next_handle;
                let next = handle
                    .checked_add(4)
                    .filter(|n| *n <= u32::MAX as u64)
                    .ok_or(0xc000_011fu32)?;
                self.files.try_reserve(1).map_err(|_| 0xc000_0017u32)?;
                let owner = NEXT_FILE_OWNER.try_update(Ordering::Relaxed, Ordering::Relaxed, |next| next.checked_add(1))
                    .map_err(|_| 0xc000_011fu32)?;
                let (information, directory, identity) = self.host.open_tracked_file(&path, a[7] as u32, can_write, a[8] & 0x4040 == 0x4000)?;
                if let Err(status) = put(m, a[0], handle, width).and_then(|_| iosb(m, a[3], width, 0, information)) {
                    if let Some(identity) = identity { self.host.close_tracked_file(identity); }
                    return Err(status);
                }
                self.files.push(File {
                    handle,
                    identity,
                    owner,
                    inheritable: attributes & 2 != 0,
                    path,
                    position: 0,
                    readable,
                    writable: can_write,
                    write_attributes: a[1] & (GENERIC_WRITE | FILE_WRITE_ATTRIBUTES) != 0,
                    directory,
                    overlapped,
                });
                self.next_handle = next;
                Ok(0)
            }
            Service::Close => {
                if let Some(index) = self.children.iter().position(|(process, thread, _)| {
                    a[0] != 0 && (*process == a[0] || *thread == a[0])
                }) {
                    let (process, thread, _) = &mut self.children[index];
                    if *process == a[0] { *process = 0; } else { *thread = 0; }
                    if *process == 0 && *thread == 0 { self.children.swap_remove(index); }
                    return Ok(0);
                }
                if let Some(index) = self.events.iter().position(|event| event.0 == a[0]) {
                    self.host.close_event(self.events[index].1)?;
                    self.events.remove(index);
                    return Ok(0);
                }
                let index = self
                    .files
                    .iter()
                    .position(|f| f.handle == a[0])
                    .ok_or(INVALID_HANDLE)?;
                close_open_file(&mut self.host, self.files.remove(index));
                Ok(0)
            }
            Service::WaitForSingleObject => {
                if a[1] as u8 != 0 { return Err(nt61::STATUS_NOT_IMPLEMENTED); }
                let timeout = if a[2] == 0 { None } else {
                    Some(word(&read(m, a[2], 8)?, 0, 8) as i64)
                };
                self.wait_event(a[0], timeout)
            }
            Service::ReadFile | Service::WriteFile => {
                // APC/event completion and keyed IO require real dispatcher objects.
                if a[1..4].iter().any(|v| *v != 0) || a[8] != 0 {
                    return Err(nt61::STATUS_NOT_IMPLEMENTED);
                }
                let len = usize::try_from(a[6]).map_err(|_| INVALID_PARAMETER)?;
                if service == Service::WriteFile && len > MAX_IO {
                    return Err(INVALID_PARAMETER);
                }
                writable(m, a[4], width * 2)?;
                if service == Service::WriteFile && matches!(a[0], STD_OUTPUT | STD_ERROR) {
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
                if file.directory || (service == Service::WriteFile && !file.writable)
                    || (service == Service::ReadFile && !file.readable) {
                    return Err(ACCESS_DENIED);
                }
                let offset = if a[7] == 0 {
                    if file.overlapped { return Err(INVALID_PARAMETER); }
                    file.position
                } else {
                    let b = read(m, a[7], 8)?;
                    let raw = word(&b, 0, 8);
                    if raw == u64::MAX - 1 {
                        if file.overlapped { return Err(INVALID_PARAMETER); }
                        file.position
                    } else if raw > i64::MAX as u64 {
                        return Err(INVALID_PARAMETER);
                    } else {
                        raw
                    }
                };
                if service == Service::WriteFile {
                    offset.checked_add(len as u64).filter(|end| *end <= i64::MAX as u64)
                        .ok_or(INVALID_PARAMETER)?;
                    let bytes = if len == 0 { Vec::new() } else { read(m, a[5], len)? };
                    let count = self.host.write_open_file(&file.path, file.lock_key(), offset, &bytes)?;
                    if count > len { return Err(0xc000_0185); }
                    iosb(m, a[4], width, 0, count as u64)?;
                    if !file.overlapped { file.position = offset + count as u64; }
                    return Ok(0);
                }
                writable(m, a[5], len)?;
                let mut transferred = 0;
                while transferred < len {
                    let count = (len - transferred).min(MAX_IO);
                    let chunk_offset = offset.checked_add(transferred as u64).ok_or(INVALID_PARAMETER)?;
                    let address = a[5].checked_add(transferred as u64).ok_or(INVALID_PARAMETER)?;
                    let bytes = match self.host.read_open_file(&file.path, file.lock_key(), chunk_offset, count) {
                        Ok(bytes) => bytes,
                        Err(status) => {
                            iosb(m, a[4], width, status, transferred as u64)?;
                            if !file.overlapped { file.position = chunk_offset; }
                            return Ok(status);
                        }
                    };
                    if bytes.len() > count {
                        return Err(0xc000_0185);
                    }
                    if !bytes.is_empty() && !m.write_virt(address, &bytes) {
                        return Err(nt61::STATUS_ACCESS_VIOLATION);
                    }
                    transferred += bytes.len();
                    if bytes.len() < count {
                        break;
                    }
                }
                let end = offset
                    .checked_add(transferred as u64)
                    .ok_or(INVALID_PARAMETER)?;
                let status = if len != 0 && transferred == 0 { EOF } else { 0 };
                iosb(m, a[4], width, status, transferred as u64)?;
                if !file.overlapped { file.position = end; }
                Ok(status)
            }
            _ => Err(nt61::STATUS_NOT_IMPLEMENTED),
        }
    }
}
impl<H: IoHost> Drop for IoServices<H> {
    fn drop(&mut self) {
        self.close_sockets();
        for file in self.files.drain(..) {
            close_open_file(&mut self.host, file);
        }
        for file in self.mapped_files.drain(..) {
            if let Some(identity) = file.identity { self.host.close_tracked_file(identity); }
        }
        for (_, event) in self.events.drain(..) { let _ = self.host.close_event(event); }
        for (_, dc) in self.memory_dcs.drain(..) { self.host.delete_memory_dc(dc); }
        for (_, font) in self.fonts.drain(..) { self.host.delete_font(font); }
    }
}
impl<H: IoHost> GuestServices for IoServices<H> {
    fn set_file_range_lock(&mut self, handle: u64, offset: u64, length: u64, lock: bool) -> Result<(), u32> {
        let file = self.files.iter().find(|file| file.handle == handle).ok_or(INVALID_HANDLE)?;
        if file.directory || !(file.readable || file.writable) { return Err(ACCESS_DENIED); }
        let key = file.lock_key().ok_or(nt61::STATUS_NOT_IMPLEMENTED)?;
        self.host.set_file_range_lock(key, offset, length, lock)
    }
    fn volume_allocation(&mut self, abi: GuestAbi, directory: &str) -> Result<nt61::VolumeAllocation, u32> {
        let directory = guest_file_path(abi, directory, self.fs_redirection)?;
        self.host.volume_allocation(&directory)
    }
    fn flush_file_buffers(&mut self, handle: u64) -> Result<(), u32> {
        let file = self.files.iter().find(|file| file.handle == handle).ok_or(INVALID_HANDLE)?;
        if file.directory || !file.writable { return Err(ACCESS_DENIED); }
        self.host.flush_file_buffers()
    }
    fn retain_file_mapping(&mut self, file: Option<u64>, writable: bool) -> Result<(u64, u64), u32> {
        let mapping = self.next_handle;
        let next = mapping.checked_add(4).filter(|next| *next <= u64::from(u32::MAX)).ok_or(0xc000_0017u32)?;
        let size = if let Some(handle) = file {
            let file = self.files.iter().find(|file| file.handle == handle).ok_or(INVALID_HANDLE)?;
            if file.directory || !file.readable || (writable && !file.writable) { return Err(ACCESS_DENIED); }
            if self.mapped_files.iter().any(|other| (writable || other.writable)
                && (file.path.eq_ignore_ascii_case(&other.path) || (file.identity.is_some() && file.identity == other.identity))) {
                return Err(0xc000_0043);
            }
            let size = self.host.file_size(&file.path)?;
            let (_, directory, identity) = self.host.open_tracked_file(&file.path, 1, writable, false)?;
            if directory || identity != file.identity {
                if let Some(id) = identity { self.host.close_tracked_file(id); }
                return Err(INVALID_HANDLE);
            }
            self.mapped_files.push(MappedFile { mapping, path: file.path.clone(), identity, writable });
            size
        } else { 0 };
        self.next_handle = next;
        Ok((mapping, size))
    }
    fn release_file_mapping(&mut self, mapping: u64) {
        if let Some(index) = self.mapped_files.iter().position(|file| file.mapping == mapping) {
            if let Some(identity) = self.mapped_files.swap_remove(index).identity {
                self.host.close_tracked_file(identity);
            }
        }
    }
    fn read_file_mapping(&mut self, mapping: u64, offset: u64, length: usize) -> Result<Vec<u8>, u32> {
        let file = self.mapped_files.iter().find(|file| file.mapping == mapping).ok_or(INVALID_HANDLE)?;
        self.host.read_mapped_file(&file.path, file.identity, offset, length)
    }
    fn write_file_mapping(&mut self, mapping: u64, offset: u64, bytes: &[u8]) -> Result<usize, u32> {
        let file = self.mapped_files.iter().find(|file| file.mapping == mapping).ok_or(INVALID_HANDLE)?;
        if !file.writable { return Err(ACCESS_DENIED); }
        self.host.write_mapped_file(&file.path, file.identity, offset, bytes)
    }
    fn create_process(&mut self, request: nt61::ChildRequest) -> Result<[u32; 4], u32> {
        if self.children.len() >= 64 { return Err(4); }
        let process = u32::try_from(self.next_handle).map_err(|_| 4u32)?;
        let next = process.checked_add(8).ok_or(4u32)?;
        self.children.try_reserve(1).map_err(|_| 8u32)?;
        let child = self.host.create_process(request)?;
        let thread = process + 4;
        let information = [process, thread, child.process_id, child.thread_id];
        self.children.push((u64::from(process), u64::from(thread), child));
        self.next_handle = u64::from(next);
        Ok(information)
    }

    fn child_info(&self, handle: u64, thread_handle: bool) -> Result<(u32, u32, u32), u32> {
        let (_, _, child) = self.children.iter().find(|(process, thread, _)| {
            handle != 0 && handle == if thread_handle { *thread } else { *process }
        }).ok_or(6u32)?;
        let id = if thread_handle { child.thread_id } else { child.process_id };
        Ok((id, child.process_id, child.exit_code(thread_handle).unwrap_or(259)))
    }

    fn is_child_handle(&self, handle: u64) -> bool {
        handle != 0 && self.children.iter().any(|(process, thread, _)| *process == handle || *thread == handle)
    }
    fn select_file_view(&mut self, redirection_enabled: bool) {
        self.set_fs_redirection(redirection_enabled);
    }

    fn resolve_ipv4(&mut self, name: &str) -> Result<[u8; 4], u32> {
        self.host.resolve_ipv4(name)
    }

    fn open_socket(&mut self, kind: SocketKind) -> Result<u64, u32> {
        if self.sockets.len() >= 256 { return Err(10024); }
        let handle = self.next_handle;
        let next = handle.checked_add(4).filter(|next| *next <= u64::from(u32::MAX)).ok_or(10024u32)?;
        self.sockets.try_reserve(1).map_err(|_| 10055u32)?;
        let backend = self.host.open_socket(kind)?;
        self.sockets.push(GuestSocket { handle, backend, kind, connected: false });
        self.next_handle = next;
        Ok(handle)
    }

    fn close_socket(&mut self, handle: u64) -> Result<(), u32> {
        let index = self.sockets.iter().position(|socket| socket.handle == handle).ok_or(10038u32)?;
        self.host.close_socket(self.sockets[index].backend)?;
        self.sockets.swap_remove(index);
        Ok(())
    }

    fn connect_tcp(&mut self, handle: u64, ip: [u8; 4], port: u16) -> Result<(), u32> {
        let index = self.tcp_index(handle)?;
        let socket = &mut self.sockets[index];
        if socket.connected { return Err(10056); }
        self.host.connect_tcp(socket.backend, ip, port)?;
        socket.connected = true;
        Ok(())
    }

    fn send_tcp(&mut self, handle: u64, bytes: &[u8]) -> Result<usize, u32> {
        let socket = self.connected_tcp(handle)?;
        if bytes.is_empty() { return Ok(0); }
        self.host.send_tcp(socket, bytes)
    }

    fn recv_tcp(&mut self, handle: u64, bytes: &mut [u8]) -> Result<usize, u32> {
        let socket = self.connected_tcp(handle)?;
        self.host.recv_tcp(socket, bytes)
    }

    fn close_sockets(&mut self) {
        for socket in self.sockets.drain(..) {
            let _ = self.host.close_socket(socket.backend);
        }
    }

    fn create_event(&mut self, manual: bool, signaled: bool) -> Result<u64, u32> {
        if self.events.len() >= 256 { return Err(0xc000011f); }
        let handle = self.next_handle;
        let next = handle.checked_add(4).filter(|value| *value <= u64::from(u32::MAX)).ok_or(0xc000011fu32)?;
        let event = self.host.create_event(manual, signaled)?;
        self.events.push((handle, event));
        self.next_handle = next;
        Ok(handle)
    }
    fn change_event(&mut self, handle: u64, signaled: bool) -> Result<(), u32> {
        let event = self.events.iter().find(|event| event.0 == handle).ok_or(INVALID_HANDLE)?.1;
        self.host.change_event(event, signaled)
    }
    fn wait_event(&mut self, handle: u64, timeout: Option<i64>) -> Result<u32, u32> {
        if self.is_child_handle(handle) {
            return if timeout == Some(0) { self.poll_events(&[handle], false) }
                else { Err(nt61::STATUS_NOT_IMPLEMENTED) };
        }
        let event = self.events.iter().find(|event| event.0 == handle).ok_or(INVALID_HANDLE)?.1;
        self.host.wait_event(event, timeout)
    }
    fn validate_wait_event(&self, handle: u64) -> Result<(), u32> {
        (self.is_child_handle(handle) || self.events.iter().any(|event| event.0 == handle))
            .then_some(()).ok_or(INVALID_HANDLE)
    }
    fn poll_events(&mut self, handles: &[u64], all: bool) -> Result<u32, u32> {
        let mut events = [0; 64];
        let mut indices = [0; 64];
        if handles.is_empty() || handles.len() > events.len() { return Err(INVALID_PARAMETER); }
        let mut count = 0;
        let mut ready = None;
        for (index, &handle) in handles.iter().enumerate() {
            if let Some((_, thread, child)) = self.children.iter().find(|(process, thread, _)| {
                handle != 0 && (*process == handle || *thread == handle)
            }) {
                match (all, child.exit_code(handle == *thread)) {
                    (true, None) => return Ok(0x102),
                    (false, Some(_)) => { ready = Some(index as u32); break; }
                    _ => {}
                }
            } else {
                events[count] = self.events.iter().find(|event| event.0 == handle).ok_or(INVALID_HANDLE)?.1;
                indices[count] = index as u32;
                count += 1;
            }
        }
        if count == 0 { return Ok(if all { 0 } else { ready.unwrap_or(0x102) }); }
        let result = self.host.poll_events(&events[..count], all)?;
        if all { return Ok(result); }
        if result == 0x102 { return Ok(ready.unwrap_or(result)); }
        indices.get(..count).and_then(|values| values.get(result as usize)).copied().ok_or(INVALID_PARAMETER)
    }

    fn create_memory_dc(&mut self, source: u64) -> Option<u64> {
        let source = if source == 0 { 0 } else {
            self.memory_dcs.iter().find(|(handle, _)| *handle == source)?.1
        };
        let handle = self.next_handle;
        let next = handle.checked_add(4).filter(|value| *value <= u64::from(u32::MAX))?;
        self.memory_dcs.try_reserve(1).ok()?;
        let dc = self.host.create_memory_dc(source)?;
        self.memory_dcs.push((handle, dc));
        self.next_handle = next;
        Some(handle)
    }

    fn delete_memory_dc(&mut self, handle: u64) -> bool {
        let Some(index) = self.memory_dcs.iter().position(|(guest, _)| *guest == handle) else {
            return false;
        };
        if !self.host.delete_memory_dc(self.memory_dcs[index].1) {
            return false;
        }
        self.memory_dcs.swap_remove(index);
        true
    }

    fn text_metrics_w(&mut self, handle: u64) -> Option<[u8; 60]> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == handle)?.1;
        self.host.text_metrics_w(dc)
    }

    fn text_face_w(&mut self, handle: u64) -> Option<Vec<u16>> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == handle)?.1;
        self.host.text_face_w(dc)
    }

    fn create_font_w(&mut self, description: &[u8; 92]) -> Option<u64> {
        let guest = self.next_handle;
        let next = guest.checked_add(4).filter(|next| *next <= u32::MAX.into())?;
        self.fonts.try_reserve(1).ok()?;
        let font = self.host.create_font_w(description)?;
        self.fonts.push((guest, font));
        self.next_handle = next;
        Some(guest)
    }

    fn select_font(&mut self, dc: u64, font: u64) -> Option<u64> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == dc)?.1;
        let font = self.fonts.iter().copied().chain(self.stock_font)
            .find(|(guest, _)| *guest == font)?.1;
        let reserved = if self.stock_font.is_none() {
            Some(self.next_handle.checked_add(4).filter(|next| *next <= u32::MAX.into())?)
        } else { None };
        let previous = self.host.select_font(dc, font)?;
        if let Some((guest, _)) = self.fonts.iter().copied().chain(self.stock_font)
            .find(|(_, native)| *native == previous) {
            return Some(guest);
        }
        let next = reserved?;
        let guest = self.next_handle;
        self.stock_font = Some((guest, previous));
        self.next_handle = next;
        Some(guest)
    }

    fn delete_font(&mut self, handle: u64) -> bool {
        if self.stock_font.is_some_and(|(guest, _)| guest == handle) { return true; }
        let Some(index) = self.fonts.iter().position(|(guest, _)| *guest == handle) else {
            return false;
        };
        if !self.host.delete_font(self.fonts[index].1) { return false; }
        self.fonts.swap_remove(index);
        true
    }

    fn enumerate_fonts_w(&mut self, dc: u64, family: Option<&str>) -> Option<Vec<nt61::FontEnumeration>> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == dc)?.1;
        self.host.enumerate_fonts_w(dc, family)
    }
    fn text_extent_w(&mut self, handle: u64, text: &[u16]) -> Option<[i32; 2]> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == handle)?.1;
        self.host.text_extent_w(dc, text)
    }

    fn device_caps(&mut self, handle: u64, index: i32) -> Option<u32> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == handle)?.1;
        self.host.device_caps(dc, index)
    }

    fn draw_text_layout(&mut self, handle: u64, clip: Option<[i32; 4]>, lines: &[(i32, i32, &[u16])]) -> bool {
        self.memory_dcs.iter().find(|(guest, _)| *guest == handle)
            .is_some_and(|&(_, dc)| self.host.draw_text_layout(dc, clip, lines))
    }

    fn set_bk_mode(&mut self, handle: u64, mode: i32) -> Option<i32> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == handle)?.1;
        self.host.set_bk_mode(dc, mode)
    }

    fn set_map_mode(&mut self, handle: u64, mode: i32) -> Option<i32> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == handle)?.1;
        self.host.set_map_mode(dc, mode)
    }

    fn set_dc_extent(&mut self, handle: u64, kind: nt61::DcExtent, extent: [i32; 2]) -> Option<(bool, [i32; 2])> {
        let dc = self.memory_dcs.iter().find(|(guest, _)| *guest == handle)?.1;
        self.host.set_dc_extent(dc, kind, extent)
    }
    fn logical_to_device(&mut self, handle: u64, points: &mut [u8]) -> bool {
        self.memory_dcs.iter().find(|(guest, _)| *guest == handle)
            .is_some_and(|&(_, dc)| self.host.logical_to_device(dc, points))
    }
    fn create_directory(&mut self, abi: GuestAbi, path: &str) -> Result<(), u32> {
        let path = guest_file_path(abi, path, self.fs_redirection)?;
        self.host.create_directory(&path)
    }
    fn directory_entries(&mut self, abi: GuestAbi, path: &str) -> Result<Vec<nt61::DirectoryEntry>, u32> {
        let path = guest_file_path(abi, path, self.fs_redirection)?;
        self.host.directory_entries(&path)
    }
    fn file_attributes(&mut self, abi: GuestAbi, path: &str) -> Result<u32, u32> {
        let path = guest_file_path(abi, path, self.fs_redirection)?;
        self.host.file_attributes(&path)
    }
    fn file_basic_information(&mut self, abi: GuestAbi, path: &str) -> Result<nt61::FileBasicInformation, u32> {
        let path = guest_file_path(abi, path, self.fs_redirection)?;
        self.host.file_basic_information(&path)
    }
    fn set_file_attributes(&mut self, abi: GuestAbi, path: &str, attributes: u32) -> Result<(), u32> {
        let path = guest_file_path(abi, path, self.fs_redirection)?;
        self.host.set_file_attributes(&path, attributes)
    }
    fn delete_file(&mut self, abi: GuestAbi, path: &str) -> Result<(), u32> {
        let path = guest_file_path(abi, path, self.fs_redirection)?;
        self.host.delete_file(&path)
    }
    fn remove_directory(&mut self, abi: GuestAbi, path: &str) -> Result<(), u32> {
        let path = guest_file_path(abi, path, self.fs_redirection)?;
        self.host.remove_directory(&path)
    }
    fn move_file(&mut self, abi: GuestAbi, source: &str, destination: &str) -> Result<(), u32> {
        let source = guest_file_path(abi, source, self.fs_redirection)?;
        let destination = guest_file_path(abi, destination, self.fs_redirection)?;
        self.host.move_file(&source, &destination)
    }
    fn set_file_times(&mut self, handle: u64, times: [Option<u64>; 3]) -> Result<(), u32> {
        let file = self.files.iter().find(|file| file.handle == handle).ok_or(INVALID_HANDLE)?;
        if !file.write_attributes {
            return Err(ACCESS_DENIED);
        }
        if times == [None; 3] {
            return Ok(());
        }
        self.host.set_file_times(&file.path, times)
    }
    fn application_log_available(&mut self) -> bool { self.host.application_log_available() }
    fn report_application_event(&mut self, event: nt61::ApplicationEvent) -> Result<(), u32> {
        self.host.report_application_event(event)
    }

    fn native_processor_info(&mut self) -> Result<nt61::NativeProcessorInfo, u32> { self.host.native_processor_info() }
    fn unbiased_interrupt_time(&mut self) -> Result<u64, u32> { self.host.unbiased_interrupt_time() }
    fn time_adjustment(&mut self) -> Result<nt61::TimeAdjustment, u32> {
        self.host.time_adjustment()
    }
    fn random_bytes(&mut self, bytes: &mut [u8]) -> Result<(), u32> { self.host.random_bytes(bytes) }
    fn file_type(&mut self, handle: u64) -> Result<u32, u32> {
        match handle {
            STD_INPUT | STD_OUTPUT | STD_ERROR => Ok(2), // this context owns console streams
            h if self.files.iter().any(|file| file.handle == h) => Ok(1),
            _ => Err(INVALID_HANDLE),
        }
    }
    fn file_handle_flags(&mut self, handle: u64) -> Result<u32, u32> {
        self.files.iter().find(|file| file.handle == handle)
            .map(|file| u32::from(file.inheritable)).ok_or(INVALID_HANDLE)
    }
    fn cancel_file_io(&mut self, handle: u64) -> Result<(), u32> {
        self.files.iter().any(|file| file.handle == handle).then_some(()).ok_or(INVALID_HANDLE)
    }
    fn validate_disk_file(&self, handle: u64) -> Result<(), u32> {
        let file = self.files.iter().find(|file| file.handle == handle).ok_or(INVALID_HANDLE)?;
        if file.directory { return Err(ACCESS_DENIED); }
        Ok(())
    }
    fn file_size(&mut self, handle: u64) -> Result<u64, u32> {
        let file = self.files.iter().find(|file| file.handle == handle).ok_or(INVALID_HANDLE)?;
        self.host.file_size(&file.path)
    }
    fn seek_file(&mut self, handle: u64, distance: i64, origin: u32, limit: u64) -> Result<u64, nt61::FileSeekError> {
        use nt61::FileSeekError::{NegativePosition, PositionTooLarge, SetStatus, Status};
        let file_index = self.files.iter().position(|file| file.handle == handle);
        let base = match origin {
            0 => 0,
            1 => self.files[file_index.ok_or(Status(INVALID_HANDLE))?].position,
            2 => {
                let path = &self.files[file_index.ok_or(Status(INVALID_HANDLE))?].path;
                self.host.file_size(path).map_err(Status)?
            }
            _ => return Err(Status(INVALID_PARAMETER)),
        };
        let position = (base as i64).wrapping_add(distance);
        if position < 0 { return Err(NegativePosition); }
        if position as u64 > limit { return Err(PositionTooLarge); }
        let file = &mut self.files[file_index.ok_or(SetStatus(INVALID_HANDLE))?];
        file.position = position as u64;
        Ok(file.position)
    }
    fn printer_available(&mut self) -> bool { self.host.printer_available() }
    fn print_document(&mut self, data: &[u8]) -> Result<u32,u32> { self.host.print_document(data) }
    fn message_beep(&mut self, kind: u32) -> bool { self.host.message_beep(kind) }

    fn desktop_dialog(&mut self, request: &[u8]) -> Result<Vec<u8>, u32> { self.host.desktop_dialog(request) }

    fn memory_status(&mut self) -> Result<nt61::MemoryStatus, u32> {
        self.host.memory_status()
    }
    fn processor_mask(&mut self) -> Result<u64, u32> {
        self.host.processor_mask()
    }
    fn read_library(&mut self, name: &str, mode: super::ir::DecodeMode) -> Result<Vec<u8>, u32> {
        if name.as_bytes().get(1) == Some(&b':') {
            let abi = if mode == super::ir::DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
            let path = guest_file_path(abi, name, self.fs_redirection)?;
            self.host.read_library(&path, mode)
        } else {
            self.host.read_library(name, mode)
        }
    }
    fn read_system_library(&mut self, name: &str, mode: super::ir::DecodeMode) -> Result<Vec<u8>, u32> {
        self.host.read_system_library(name, mode)
    }
    fn resolve_library(&mut self, name: &str, mode: super::ir::DecodeMode, system: bool)
        -> Result<super::pe::ResolvedLibrary, u32> {
        if !system && name.as_bytes().get(1) == Some(&b':') {
            let abi = if mode == super::ir::DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
            let path = guest_file_path(abi, name, self.fs_redirection)?;
            self.host.resolve_library(&path, mode, false)
        } else {
            self.host.resolve_library(name, mode, system)
        }
    }
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

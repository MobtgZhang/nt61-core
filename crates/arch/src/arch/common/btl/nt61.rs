//! Windows 7 SP1 guest service identities and checked argument capture.
//!
//! Service indices are per guest ISA, not per host ISA. Values cross-checked
//! against j00ru/windows-syscalls/{x86,x64}/csv/nt.csv, Windows 7 (SP1).
//! This module owns no native function pointers: structure/handle conversion
//! belongs to a typed service handler, after this ABI boundary.

use super::{abi::GuestAbi, ir::X86Reg, memory::GuestMemory};
use alloc::vec::Vec;

#[path = "../../../../../abi/src/syscall_metadata.rs"]
mod syscall_metadata;

pub const STATUS_INVALID_SYSTEM_SERVICE: u32 = 0xc000_001c;
pub const STATUS_ACCESS_VIOLATION: u32 = 0xc000_0005;
pub const STATUS_NOT_IMPLEMENTED: u32 = 0xc000_0002;
pub const MAX_ARGUMENTS: usize = 17;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Close,
    TerminateProcess,
    TerminateThread,
    DelayExecution,
    YieldExecution,
    QuerySystemTime,
    QueryPerformanceCounter,
    ReadFile,
    WriteFile,
    CreateFile,
    AllocateVirtualMemory,
    QueryInformationProcess,
    OpenProcess,
    WaitForSingleObject,
}

impl Service {
    pub const fn argument_count(self) -> usize {
        match self {
            Self::Close | Self::QuerySystemTime => 1,
            Self::TerminateProcess
            | Self::TerminateThread
            | Self::DelayExecution
            | Self::QueryPerformanceCounter => 2,
            Self::YieldExecution => 0,
            Self::WaitForSingleObject => 3,
            Self::OpenProcess => 4,
            Self::QueryInformationProcess => 5,
            Self::AllocateVirtualMemory => 6,
            Self::ReadFile | Self::WriteFile => 9,
            Self::CreateFile => 11,
        }
    }
}

pub fn resolve(abi: GuestAbi, number: u32) -> Option<Service> {
    use Service::*;
    // An unknown guest service must never fall through to a different native
    // service with the same numeric index.
    Some(match (abi, number) {
        (GuestAbi::X86_32, 0x32) | (GuestAbi::X86_64, 0x0c) => Close,
        (GuestAbi::X86_32, 0x172) | (GuestAbi::X86_64, 0x29) => TerminateProcess,
        (GuestAbi::X86_32, 0x173) | (GuestAbi::X86_64, 0x50) => TerminateThread,
        (GuestAbi::X86_32, 0x62) | (GuestAbi::X86_64, 0x31) => DelayExecution,
        (GuestAbi::X86_32, 0x190) | (GuestAbi::X86_64, 0x43) => YieldExecution,
        (GuestAbi::X86_32, 0x107) | (GuestAbi::X86_64, 0x57) => QuerySystemTime,
        (GuestAbi::X86_32, 0xfb) | (GuestAbi::X86_64, 0x2e) => QueryPerformanceCounter,
        (GuestAbi::X86_32, 0x111) | (GuestAbi::X86_64, 3) => ReadFile,
        (GuestAbi::X86_32, 0x18c) | (GuestAbi::X86_64, 5) => WriteFile,
        (GuestAbi::X86_32, 0x42) | (GuestAbi::X86_64, 0x52) => CreateFile,
        (GuestAbi::X86_32, 0x13) | (GuestAbi::X86_64, 0x15) => AllocateVirtualMemory,
        (GuestAbi::X86_32, 0xea) | (GuestAbi::X86_64, 0x16) => QueryInformationProcess,
        (GuestAbi::X86_32, 0xbe) | (GuestAbi::X86_64, 0x23) => OpenProcess,
        (GuestAbi::X86_32, 0x187) | (GuestAbi::X86_64, 1) => WaitForSingleObject,
        _ => return None,
    })
}

/// All arguments are captured as guest values. In particular, argument 9 of a
/// 32-bit call is read as u32, never from a native usize user-stack pointer.
pub fn capture_arguments(
    abi: GuestAbi,
    regs: &[u64],
    count: usize,
    memory: &mut dyn GuestMemory,
) -> Result<[u64; MAX_ARGUMENTS], u32> {
    if count > MAX_ARGUMENTS {
        return Err(STATUS_INVALID_SYSTEM_SERVICE);
    }
    let mut args = [0; MAX_ARGUMENTS];
    if count == 0 {
        return Ok(args);
    }
    let reg = |r: X86Reg| regs.get(r as usize).copied().ok_or(STATUS_ACCESS_VIOLATION);
    let (first, base, width) = match abi {
        GuestAbi::X86_64 => {
            for (slot, r) in [X86Reg::R10, X86Reg::Rdx, X86Reg::R8, X86Reg::R9]
                .iter()
                .enumerate()
                .take(count)
            {
                args[slot] = reg(*r)?;
            }
            if count <= 4 {
                return Ok(args);
            }
            (
                4,
                reg(X86Reg::Rsp)?
                    .checked_add(0x28)
                    .ok_or(STATUS_ACCESS_VIOLATION)?,
                8,
            )
        }
        GuestAbi::X86_32 => (0, reg(X86Reg::Rdx)? as u32 as u64, 4),
        GuestAbi::Arm32 => return Err(STATUS_INVALID_SYSTEM_SERVICE),
    };
    let bytes = (count - first) * width;
    let end = base
        .checked_add(bytes as u64)
        .ok_or(STATUS_ACCESS_VIOLATION)?;
    if width == 4 && end > 0x1_0000_0000 {
        return Err(STATUS_ACCESS_VIOLATION);
    }
    let mut raw = [0; MAX_ARGUMENTS * 8];
    if memory.read_virt(base, &mut raw[..bytes]) != bytes {
        return Err(STATUS_ACCESS_VIOLATION);
    }
    for slot in first..count {
        let start = (slot - first) * width;
        let mut value = [0; 8];
        value[..width].copy_from_slice(&raw[start..start + width]);
        args[slot] = u64::from_le_bytes(value);
    }
    Ok(args)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceResult {
    Return(u32),
    ExitProcess(u32),
    ExitThread(u32),
}

/// Host physical and paging-file accounting in bytes, captured as one sample.
#[derive(Clone, Copy, Debug)]
pub struct MemoryStatus {
    pub physical_total: u64,
    pub physical_available: u64,
    pub paging_total: u64,
    pub paging_available: u64,
}
/// Clock discipline and nominal tick interval, both in 100-ns units. This
/// describes the actual host clock; QPC frequency is not a timer increment.
#[derive(Clone, Copy, Debug)]
pub struct TimeAdjustment {
    pub adjustment: u32,
    pub increment: u32,
    pub disabled: bool,
}
/// Native processor identity, distinct from the emulated process's CPUID.
pub struct NativeProcessorInfo {
    pub architecture: u16,
    pub processor_type: u32,
    pub level: u16,
    pub revision: u16,
}

pub struct VolumeAllocation {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub total_clusters: u64,
    pub free_clusters: u64,
}

pub struct FileBasicInformation {
    pub attributes: u32,
    pub times: [u64; 3],
    pub size: u64,
}
/// Owned classic Application log data. UTF-16 insertion strings, full DWORD
/// event identity, SID and binary payload survive the guest ABI boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationEvent {
    pub source: alloc::vec::Vec<u16>,
    pub event_type: u16,
    pub category: u16,
    pub event_id: u32,
    pub sid: alloc::vec::Vec<u8>,
    pub strings: alloc::vec::Vec<alloc::vec::Vec<u16>>,
    pub data: alloc::vec::Vec<u8>,
}
/// Copied mounted-volume metadata. Times use NT FILETIME units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: Vec<u16>,
    pub attributes: u32,
    pub size: u64,
    pub creation_time: u64,
    pub access_time: u64,
    pub write_time: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcExtent {
    Window,
    Viewport,
}

pub type FontEnumeration = ([u8; 92], [u8; 60]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    TcpV4,
    UdpV4,
}

pub struct ChildRequest {
    pub application: Option<alloc::string::String>,
    pub command_line: Option<alloc::string::String>,
    pub directory: alloc::string::String,
    pub parent_directory: alloc::string::String,
    pub search_path: Option<alloc::string::String>,
    pub environment: Option<alloc::vec::Vec<u8>>,
    pub abi: GuestAbi,
    pub redirect: bool,
}

pub struct ChildProcess {
    pub process_id: u32,
    pub thread_id: u32,
    pub exit: alloc::sync::Arc<core::sync::atomic::AtomicU64>,
    pub thread_exit: alloc::sync::Arc<core::sync::atomic::AtomicU64>,
}

impl ChildProcess {
    pub fn exit_code(&self, thread: bool) -> Option<u32> {
        let exit = if thread { &self.thread_exit } else { &self.exit };
        u32::try_from(exit.load(core::sync::atomic::Ordering::Acquire)).ok()
    }
}

pub trait GuestServices {
    fn create_process(&mut self, _request: ChildRequest) -> Result<[u32; 4], u32> { Err(120) }
    fn child_info(&self, _handle: u64, _thread: bool) -> Result<(u32, u32, u32), u32> { Err(6) }
    fn is_child_handle(&self, _handle: u64) -> bool { false }
    fn select_file_view(&mut self, _redirection_enabled: bool) {}
    fn resolve_ipv4(&mut self, _name: &str) -> Result<[u8; 4], u32> { Err(11003) }
    fn open_socket(&mut self, _kind: SocketKind) -> Result<u64, u32> { Err(10050) }
    fn close_socket(&mut self, _handle: u64) -> Result<(), u32> { Err(10038) }
    fn connect_tcp(&mut self, _handle: u64, _ip: [u8; 4], _port: u16) -> Result<(), u32> { Err(10045) }
    fn send_tcp(&mut self, _handle: u64, _bytes: &[u8]) -> Result<usize, u32> { Err(10045) }
    fn recv_tcp(&mut self, _handle: u64, _bytes: &mut [u8]) -> Result<usize, u32> { Err(10045) }
    fn close_sockets(&mut self) {}
    /// Unnamed, non-inheritable events owned by this guest context. Handles
    /// returned here must not expose a backend/kernel handle to guest code.
    fn create_event(&mut self, _manual: bool, _signaled: bool) -> Result<u64, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn change_event(&mut self, _handle: u64, _signaled: bool) -> Result<(), u32> { Err(STATUS_NOT_IMPLEMENTED) }
    /// Non-alertable NT wait, with a captured optional 100-ns deadline.
    fn wait_event(&mut self, _handle: u64, _timeout: Option<i64>) -> Result<u32, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn validate_wait_event(&self, _handle: u64) -> Result<(), u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn poll_events(&mut self, _handles: &[u64], _all: bool) -> Result<u32, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn create_memory_dc(&mut self, _source: u64) -> Option<u64> { None }
    fn delete_memory_dc(&mut self, _handle: u64) -> bool { false }
    fn device_caps(&mut self, _handle: u64, _index: i32) -> Option<u32> { None }
    fn draw_text_layout(&mut self, _handle: u64, _clip: Option<[i32; 4]>, _lines: &[(i32, i32, &[u16])]) -> bool { false }
    fn create_font_w(&mut self, _description: &[u8; 92]) -> Option<u64> { None }
    fn select_font(&mut self, _dc: u64, _font: u64) -> Option<u64> { None }
    fn delete_font(&mut self, _font: u64) -> bool { false }
    fn enumerate_fonts_w(&mut self, _dc: u64, _family: Option<&str>) -> Option<alloc::vec::Vec<FontEnumeration>> { None }
    fn text_metrics_w(&mut self, _handle: u64) -> Option<[u8; 60]> { None }
    fn text_face_w(&mut self, _handle: u64) -> Option<alloc::vec::Vec<u16>> { None }
    fn text_extent_w(&mut self, _handle: u64, _text: &[u16]) -> Option<[i32; 2]> { None }
    fn set_bk_mode(&mut self, _handle: u64, _mode: i32) -> Option<i32> { None }
    fn set_map_mode(&mut self, _handle: u64, _mode: i32) -> Option<i32> { None }
    fn set_dc_extent(&mut self, _handle: u64, _kind: DcExtent, _extent: [i32; 2]) -> Option<(bool, [i32; 2])> { None }
    fn logical_to_device(&mut self, _handle: u64, _points: &mut [u8]) -> bool { false }
    fn create_directory(&mut self, _abi: GuestAbi, _path: &str) -> Result<(), u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn directory_entries(&mut self, _abi: GuestAbi, _path: &str) -> Result<Vec<DirectoryEntry>, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn file_attributes(&mut self, _abi: GuestAbi, _path: &str) -> Result<u32, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn file_basic_information(&mut self, _abi: GuestAbi, _path: &str) -> Result<FileBasicInformation, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn set_file_attributes(&mut self, _abi: GuestAbi, _path: &str, _attributes: u32) -> Result<(), u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn delete_file(&mut self, _abi: GuestAbi, _path: &str) -> Result<(), u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn remove_directory(&mut self, _abi: GuestAbi, _path: &str) -> Result<(), u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn move_file(&mut self, _abi: GuestAbi, _source: &str, _destination: &str) -> Result<(), u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn set_file_times(&mut self, _handle: u64, _times: [Option<u64>; 3]) -> Result<(), u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    /// Local Application log backend availability; no remote RPC support.
    fn application_log_available(&mut self) -> bool { false }
    /// Only a successful backend commit may make ReportEvent return TRUE.
    /// Errors are Win32 error codes, unlike the native call method's NTSTATUS.
    fn report_application_event(&mut self, _event: ApplicationEvent) -> Result<(), u32> { Err(120) }

    fn native_processor_info(&mut self) -> Result<NativeProcessorInfo, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    /// Running time in 100-ns units, excluding suspended and hibernated time.
    fn unbiased_interrupt_time(&mut self) -> Result<u64, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn time_adjustment(&mut self) -> Result<TimeAdjustment, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    /// Fill a kernel-owned buffer from a cryptographic entropy source. An
    /// unavailable source must return an error rather than predictable bytes.
    fn random_bytes(&mut self, _bytes: &mut [u8]) -> Result<(), u32> { Err(STATUS_NOT_IMPLEMENTED) }
    /// Win32 FILE_TYPE_* of a live handle in this guest service context.
    fn file_type(&mut self, _handle: u64) -> Result<u32, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn file_handle_flags(&mut self, _handle: u64) -> Result<u32, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn cancel_file_io(&mut self, _handle: u64) -> Result<(), u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn validate_disk_file(&self, _handle: u64) -> Result<(), u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn file_size(&mut self, _handle: u64) -> Result<u64, u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn flush_file_buffers(&mut self, _handle: u64) -> Result<(), u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn set_file_range_lock(&mut self, _handle: u64, _offset: u64, _length: u64, _lock: bool) -> Result<(), u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn volume_allocation(&mut self, _abi: GuestAbi, _directory: &str) -> Result<VolumeAllocation, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn retain_file_mapping(&mut self, _file: Option<u64>, _writable: bool) -> Result<(u64, u64), u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn release_file_mapping(&mut self, _mapping: u64) {}
    fn read_file_mapping(&mut self, _mapping: u64, _offset: u64, _length: usize) -> Result<alloc::vec::Vec<u8>, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn write_file_mapping(&mut self, _mapping: u64, _offset: u64, _bytes: &[u8]) -> Result<usize, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    fn seek_file(&mut self, _handle: u64, _distance: i64, _origin: u32, _limit: u64) -> Result<u64, FileSeekError> {
        Err(FileSeekError::Status(STATUS_NOT_IMPLEMENTED))
    }
    fn printer_available(&mut self) -> bool { false }
    fn print_document(&mut self, _data: &[u8]) -> Result<u32,u32> { Err(STATUS_NOT_IMPLEMENTED) }
    fn message_beep(&mut self, _kind: u32) -> bool { false }

    /// Copied private desktop protocol. Never accepts a guest process pointer.
    fn desktop_dialog(&mut self, _request: &[u8]) -> Result<alloc::vec::Vec<u8>, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }

    fn memory_status(&mut self) -> Result<MemoryStatus, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    /// Process/system logical processor availability, supplied by the guest host.
    fn processor_mask(&mut self) -> Result<u64, u32> {
        Err(STATUS_NOT_IMPLEMENTED)
    }
    /// Dynamic loading reads owned guest PE bytes using the same filesystem
    /// policy as static imports. Unsupported hosts fail closed.
    fn read_library(
        &mut self,
        _name: &str,
        _mode: super::ir::DecodeMode,
    ) -> Result<alloc::vec::Vec<u8>, u32> {
        Err(0xc000_0135)
    }
    fn read_system_library(
        &mut self,
        _name: &str,
        _mode: super::ir::DecodeMode,
    ) -> Result<alloc::vec::Vec<u8>, u32> {
        Err(0xc000_0135)
    }
    fn resolve_library(&mut self, name: &str, mode: super::ir::DecodeMode, system: bool)
        -> Result<super::pe::ResolvedLibrary, u32> {
        let bytes = if system { self.read_system_library(name, mode) } else { self.read_library(name, mode) }?;
        Ok(super::pe::ResolvedLibrary { bytes, path: None })
    }
    /// Guest pointers remain addresses in `memory`; handlers must convert
    /// structures by guest bitness rather than cast them to native pointers.
    fn call(
        &mut self,
        abi: GuestAbi,
        service: Service,
        args: &[u64],
        memory: &mut dyn GuestMemory,
    ) -> ServiceResult;
}

pub enum FileSeekError {
    Status(u32),
    SetStatus(u32),
    NegativePosition,
    PositionTooLarge,
}

pub fn dispatch(
    abi: GuestAbi,
    number: u32,
    regs: &[u64],
    memory: &mut dyn GuestMemory,
    services: &mut dyn GuestServices,
) -> ServiceResult {
    let Some(service) = resolve(abi, number) else {
        let known = syscall_metadata::NATIVE_SERVICE_METADATA.iter().any(|entry| match abi {
            GuestAbi::X86_32 => u32::from(entry.x86_number) == number,
            GuestAbi::X86_64 => u32::from(entry.number) == number,
            GuestAbi::Arm32 => false,
        });
        return ServiceResult::Return(if known { STATUS_NOT_IMPLEMENTED } else { STATUS_INVALID_SYSTEM_SERVICE });
    };
    let args = match capture_arguments(abi, regs, service.argument_count(), memory) {
        Ok(args) => args,
        Err(status) => return ServiceResult::Return(status),
    };
    services.call(abi, service, &args[..service.argument_count()], memory)
}

/// Minimal services with no native pointer dereferences. Other services return
/// NOT_IMPLEMENTED so the caller can replace this with its VFS/process thunks.
pub struct BasicServices;
impl GuestServices for BasicServices {
    fn call(
        &mut self,
        abi: GuestAbi,
        service: Service,
        args: &[u64],
        _: &mut dyn GuestMemory,
    ) -> ServiceResult {
        if abi == GuestAbi::Arm32 {
            return ServiceResult::Return(STATUS_INVALID_SYSTEM_SERVICE);
        }
        if args.len() != service.argument_count() {
            return ServiceResult::Return(0xc000_000d);
        }
        match service {
            Service::TerminateProcess | Service::TerminateThread => {
                let handle = if abi == GuestAbi::X86_32 {
                    args[0] as u32 as i32 as i64
                } else {
                    args[0] as i64
                };
                let expected = if service == Service::TerminateProcess {
                    -1
                } else {
                    -2
                };
                if handle != 0 && handle != expected {
                    return ServiceResult::Return(0xc000_0008);
                }
                if service == Service::TerminateProcess {
                    ServiceResult::ExitProcess(args[1] as u32)
                } else {
                    ServiceResult::ExitThread(args[1] as u32)
                }
            }
            Service::YieldExecution => ServiceResult::Return(0x4000_0024), // STATUS_NO_YIELD_PERFORMED
            _ => ServiceResult::Return(STATUS_NOT_IMPLEMENTED),
        }
    }
}

/// Compatibility for old emitters that can return only a scalar status. They
/// cannot terminate a guest process or supply typed host services. Never route
/// a raw Win7 index to the host SSDT or report a dropped exit event as success.
pub fn legacy_status(
    abi: GuestAbi,
    number: u32,
    regs: &[u64],
    memory: &mut dyn GuestMemory,
) -> u32 {
    match dispatch(abi, number, regs, memory, &mut BasicServices) {
        ServiceResult::Return(status) => status,
        ServiceResult::ExitProcess(_) | ServiceResult::ExitThread(_) => STATUS_NOT_IMPLEMENTED,
    }
}

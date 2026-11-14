//! Windows 7 SP1 guest service identities and checked argument capture.
//!
//! Service indices are per guest ISA, not per host ISA. Values cross-checked
//! against j00ru/windows-syscalls/{x86,x64}/csv/nt.csv, Windows 7 (SP1).
//! This module owns no native function pointers: structure/handle conversion
//! belongs to a typed service handler, after this ABI boundary.

use super::{abi::GuestAbi, ir::X86Reg, memory::GuestMemory};

pub const STATUS_INVALID_SYSTEM_SERVICE: u32 = 0xc000_000c;
pub const STATUS_ACCESS_VIOLATION: u32 = 0xc000_0005;
pub const STATUS_NOT_IMPLEMENTED: u32 = 0xc000_0002;
pub const MAX_ARGUMENTS: usize = 16;

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

pub trait GuestServices {
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

pub fn dispatch(
    abi: GuestAbi,
    number: u32,
    regs: &[u64],
    memory: &mut dyn GuestMemory,
    services: &mut dyn GuestServices,
) -> ServiceResult {
    let Some(service) = resolve(abi, number) else {
        return ServiceResult::Return(STATUS_INVALID_SYSTEM_SERVICE);
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

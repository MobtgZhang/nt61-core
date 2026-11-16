//! Win32 ABI services for the isolated x86/x64 runtime DLLs.
//! Addresses and handles belong to one GuestImage. No guest pointer is cast to
//! a host pointer. Unsupported APIs stop explicitly rather than return success.
#[path = "win32/advapi.rs"]
mod advapi;
#[path = "win32/gui.rs"]
mod gui;
#[path = "win32/dialogs.rs"]
mod dialogs;
#[path = "win32/printing.rs"]
mod printing;
#[path = "win32/user.rs"]
mod user;
#[path = "win32/catalog.rs"]
pub mod catalog;
use crate::arch::common::btl::{
    abi::GuestAbi,
    ir::DecodeMode,
    memory::GuestMemory,
    nt61::{self, GuestServices, Service, ServiceResult},
    pe::{GuestImage, Region, EXECUTE, PAGE, READ, WRITE},
    runtime::StopReason,
};
use alloc::{string::String, vec, vec::Vec};

const INVALID_PARAMETER: u32 = 87;
const NOACCESS: u32 = 998;
const MAX_BUFFER: usize = 1024 * 1024;
const HEAP: u64 = 0x7f00_0000;

struct VirtualAllocation {
    base: u64,
    length: usize,
    committed: Vec<bool>,
}

pub struct Runtime {
    pub modules: Vec<(String, u64)>,
    allocations: Vec<(u64, usize)>,
    heap_allocations: Vec<(u64, usize)>,
    virtual_allocations: Vec<VirtualAllocation>,
    tls: Vec<Option<u64>>,
    next_address: u64,
    pub last_api: Option<u32>,
    /// Bounded diagnostics for failed runtime calls; never changes guest results.
    pub failures: Vec<(u32, u32, String)>,
    pub last_library_error: Option<String>,
    advapi: advapi::State,
    gui: gui::State,
    printing: printing::State,
    user: user::State,
}
impl Runtime {
    pub(super) fn new(base: u64) -> Self {
        Self {
            modules: vec![(String::new(), base)],
            allocations: Vec::new(),
            heap_allocations: Vec::new(),
            virtual_allocations: Vec::new(),
            tls: Vec::new(),
            next_address: 0x2000_0000,
            last_api: None,
            failures: Vec::new(),
            last_library_error: None,
            advapi: advapi::State::new(),
            gui: gui::State::new(),
            printing: printing::State::new(),
            user: user::State::new(),
        }
    }
}
pub enum Outcome {
    Return(u64),
    Exit(u32),
    Continue,
}

fn read(m: &mut dyn GuestMemory, p: u64, n: usize) -> Result<Vec<u8>, u32> {
    if n > MAX_BUFFER {
        return Err(INVALID_PARAMETER);
    }
    let mut b = vec![0; n];
    if m.read_virt(p, &mut b) != n {
        return Err(NOACCESS);
    }
    Ok(b)
}
fn word(m: &mut dyn GuestMemory, p: u64, n: usize) -> Result<u64, u32> {
    let b = read(m, p, n)?;
    let mut v = [0; 8];
    v[..n].copy_from_slice(&b);
    Ok(u64::from_le_bytes(v))
}
fn put(m: &mut dyn GuestMemory, p: u64, v: u64, n: usize) -> Result<(), u32> {
    if m.write_virt(p, &v.to_le_bytes()[..n]) {
        Ok(())
    } else {
        Err(NOACCESS)
    }
}
fn string(m: &mut dyn GuestMemory, p: u64, wide: bool) -> Result<String, u32> {
    let width = if wide { 2 } else { 1 };
    let mut units = Vec::new();
    for i in 0..32768 {
        let v = word(m, p.checked_add(i * width).ok_or(NOACCESS)?, width as usize)? as u16;
        if v == 0 {
            return if wide {
                String::from_utf16(&units).map_err(|_| 1113)
            } else {
                Ok(units.iter().map(|v| char::from(*v as u8)).collect())
            };
        }
        if !wide && v > 0x7f {
            return Err(1113);
        }
        units.push(v)
    }
    Err(INVALID_PARAMETER)
}
fn write_string(m: &mut dyn GuestMemory, p: u64, s: &str, wide: bool) -> Result<usize, u32> {
    let b: Vec<u8> = if wide {
        s.encode_utf16()
            .chain(core::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        if !s.is_ascii() {
            return Err(1113);
        }
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        b
    };
    if !m.write_virt(p, &b) {
        return Err(NOACCESS);
    }
    Ok(if wide { b.len() / 2 - 1 } else { b.len() - 1 })
}
fn error(image: &mut GuestImage, e: u32) {
    let off = if image.thread.context.mode == DecodeMode::X86 {
        0x34
    } else {
        0x68
    };
    let _ = put(&mut image.memory, image.teb + off, e as u64, 4);
}
fn protection(p: u32) -> Result<u8, u32> {
    Ok(match p {
        1 => 0,
        2 => READ,
        4 => READ | WRITE,
        0x10 => EXECUTE,
        0x20 => READ | EXECUTE,
        0x40 => READ | WRITE | EXECUTE,
        // WRITECOPY protections apply to mapped file views, not VirtualAlloc
        // private pages. This runtime has no file-view/COW implementation;
        // accepting them as ordinary writable pages would claim false success.
        _ => return Err(INVALID_PARAMETER),
    })
}
fn win_protection(p: u8) -> u32 {
    match p {
        0 => 1,
        1 => 2,
        3 => 4,
        4 => 0x10,
        5 => 0x20,
        7 => 0x40,
        _ => 1,
    }
}
fn aligned_size(n: u64) -> Result<usize, u32> {
    let n = usize::try_from(n).map_err(|_| 8u32)?;
    if n == 0 || n > 64 * 1024 * 1024 {
        return Err(8);
    }
    n.checked_add(PAGE - 1).map(|v| v & !(PAGE - 1)).ok_or(8)
}
fn allocate(image: &mut GuestImage, requested: u64, n: u64, permissions: u8) -> Result<u64, u32> {
    let requested_base = requested & !0xffff;
    let len = aligned_size(n.checked_add(requested - requested_base).ok_or(8u32)?)?;
    let mut base = if requested == 0 {
        image.win32.next_address
    } else {
        requested_base
    };
    if requested == 0 {
        loop {
            let end = base.checked_add(len as u64).ok_or(8u32)?;
            let conflict = image
                .memory
                .regions
                .iter()
                .find(|r| base < r.base + r.bytes.len() as u64 && r.base < end);
            match conflict {
                Some(r) => base = (r.base + r.bytes.len() as u64 + 0xffff) & !0xffff,
                None => break,
            }
        }
    }
    let end = base.checked_add(len as u64).ok_or(8u32)?;
    if base < 0x10000
        || end
            > if image.thread.context.mode == DecodeMode::X86 {
                0x7f000000
            } else {
                0x0000_8000_0000_0000
            }
        || image.win32.allocations.len() >= 4096
        || image
            .memory
            .regions
            .iter()
            .map(|region| region.bytes.len())
            .sum::<usize>()
            .saturating_add(len)
            > 128 * 1024 * 1024
    {
        return Err(8);
    }
    if image
        .memory
        .regions
        .iter()
        .any(|r| base < r.base + r.bytes.len() as u64 && r.base < end)
    {
        return Err(487);
    }
    image
        .memory
        .regions
        .push(Region::new(base, len, permissions).map_err(|_| 8u32)?);
    image.win32.allocations.push((base, len));
    image.win32.next_address = (end + 0xffff) & !0xffff;
    Ok(base)
}
fn free(image: &mut GuestImage, p: u64) -> Result<(), u32> {
    let i = image
        .win32
        .allocations
        .iter()
        .position(|a| a.0 == p)
        .ok_or(INVALID_PARAMETER)?;
    image.win32.allocations.remove(i);
    let i = image
        .memory
        .regions
        .iter()
        .position(|r| r.base == p)
        .ok_or(INVALID_PARAMETER)?;
    image.memory.regions.remove(i);
    Ok(())
}
fn virtual_range(image: &GuestImage, p: u64, n: u64) -> Result<(usize, usize, usize), u32> {
    if n == 0 {
        return Err(INVALID_PARAMETER);
    }
    let end = p.checked_add(n).ok_or(INVALID_PARAMETER)?;
    let (index, allocation) = image
        .win32
        .virtual_allocations
        .iter()
        .enumerate()
        .find(|(_, a)| p >= a.base && end <= a.base + a.length as u64)
        .ok_or(487u32)?;
    Ok((
        index,
        ((p - allocation.base) as usize) / PAGE,
        ((end - 1 - allocation.base) as usize) / PAGE,
    ))
}
fn change_commit(
    image: &mut GuestImage,
    p: u64,
    n: u64,
    bits: u8,
    commit: bool,
) -> Result<(), u32> {
    let (index, first, last) = virtual_range(image, p, n)?;
    let allocation = &mut image.win32.virtual_allocations[index];
    let region = image
        .memory
        .regions
        .iter_mut()
        .find(|r| r.base == allocation.base)
        .ok_or(487u32)?;
    for page in first..=last {
        if !commit || !allocation.committed[page] {
            let end = ((page + 1) * PAGE).min(region.bytes.len());
            region.bytes[page * PAGE..end].fill(0);
        }
        allocation.committed[page] = commit;
        region.permissions[page] = if commit { bits } else { 0 };
    }
    image.thread.invalidate_code();
    Ok(())
}
fn protect(image: &mut GuestImage, p: u64, n: u64, flags: u32) -> Result<u32, u32> {
    let bits = protection(flags)?;
    if n == 0 {
        return Err(INVALID_PARAMETER);
    }
    let end = p.checked_add(n).ok_or(INVALID_PARAMETER)?;
    for a in &image.win32.virtual_allocations {
        if p < a.base + a.length as u64 && a.base < end {
            let (_, first, last) = virtual_range(image, p, n)?;
            if a.committed[first..=last].iter().any(|v| !v) {
                return Err(487);
            }
        }
    }
    let r = image
        .memory
        .regions
        .iter_mut()
        .find(|r| p >= r.base && end <= r.base + r.bytes.len() as u64)
        .ok_or(487u32)?;
    let first = ((p - r.base) as usize) / PAGE;
    let last = ((end - 1 - r.base) as usize) / PAGE;
    let old = win_protection(r.permissions[first]);
    r.permissions[first..=last].fill(bits);
    Ok(old)
}
fn heap_allocate(image: &mut GuestImage, n: u64) -> Result<u64, u32> {
    let p = allocate(image, 0, n.max(1), READ | WRITE)?;
    image.win32.heap_allocations.push((p, n as usize));
    Ok(p)
}
fn heap_free(image: &mut GuestImage, p: u64) -> Result<(), u32> {
    if p == 0 {
        return Ok(());
    }
    let index = image
        .win32
        .heap_allocations
        .iter()
        .position(|a| a.0 == p)
        .ok_or(INVALID_PARAMETER)?;
    free(image, p)?;
    image.win32.heap_allocations.remove(index);
    Ok(())
}
fn module(image: &GuestImage, name: &str) -> Option<u64> {
    let name = name.rsplit(['\\', '/']).next()?;
    let lower = name.to_ascii_lowercase();
    let lower = if !lower.contains('.') {
        alloc::format!("{lower}.dll")
    } else {
        lower
    };
    image
        .win32
        .modules
        .iter()
        .find(|m| m.0.eq_ignore_ascii_case(&lower))
        .map(|m| m.1)
}
fn load_library(
    image: &mut GuestImage,
    name: &str,
    services: &mut dyn GuestServices,
) -> Result<u64, u32> {
    struct Resolver<'a>(&'a mut dyn GuestServices);
    impl super::ImportResolver for Resolver<'_> {
        fn read_library(&mut self, name: &str, mode: DecodeMode) -> Result<Vec<u8>, u32> {
            self.0.read_library(name, mode)
        }
    }
    image
        .load_library(name, &mut Resolver(services))
        .map_err(|e| {
            image.win32.last_library_error = Some(alloc::format!("{}: {}", name, e));
            match e.ntstatus() {
                0xc0000135 => 126,
                0xc0000139 => 127,
                0xc000007b => 193,
                0xc0000017 => 8,
                _ => 120,
            }
        })
}
fn proc_address(
    image: &mut GuestImage,
    base: u64,
    name: Option<&str>,
    ordinal: u16,
    depth: usize,
    services: &mut dyn GuestServices,
) -> Result<u64, u32> {
    if depth >= 32 || !image.win32.modules.iter().any(|m| m.1 == base) {
        return Err(127);
    }
    let m = &mut image.memory;
    let pe = word(m, base + 0x3c, 4)?;
    let opt = base + pe + 24;
    let dir = opt + if word(m, opt, 2)? == 0x10b { 96 } else { 112 };
    let rva = word(m, dir, 4)?;
    let size = word(m, dir + 4, 4)?;
    if rva == 0 || size < 40 {
        return Err(127);
    }
    let t = base + rva;
    let first = word(m, t + 16, 4)?;
    let count = word(m, t + 20, 4)?;
    let named = word(m, t + 24, 4)?;
    if count > 16384 || named > 16384 {
        return Err(127);
    }
    let functions = base + word(m, t + 28, 4)?;
    let names = base + word(m, t + 32, 4)?;
    let ords = base + word(m, t + 36, 4)?;
    let index = if let Some(n) = name {
        let mut found = None;
        for i in 0..named {
            let p = base + word(m, names + i * 4, 4)?;
            if string(m, p, false)? == n {
                found = Some(word(m, ords + i * 2, 2)?);
                break;
            }
        }
        found.ok_or(127u32)?
    } else {
        (ordinal as u64).checked_sub(first).ok_or(127u32)?
    };
    if index >= count {
        return Err(127);
    }
    let target = word(m, functions + index * 4, 4)?;
    if target == 0 {
        return Err(127);
    }
    if target >= rva && target < rva + size {
        let f = string(m, base + target, false)?;
        let (dll, symbol) = f.rsplit_once('.').ok_or(127u32)?;
        let b = if let Some(base) = module(image, dll) {
            base
        } else {
            load_library(image, dll, services)?
        };
        if let Some(n) = symbol.strip_prefix('#') {
            return proc_address(
                image,
                b,
                None,
                n.parse().map_err(|_| 127u32)?,
                depth + 1,
                services,
            );
        }
        return proc_address(image, b, Some(symbol), 0, depth + 1, services);
    }
    let mut byte = [0];
    // A PE export may name readable data as well as code. Returning that
    // address does not grant execute permission; instruction fetch checks it.
    if image.memory.read_virt(base + target, &mut byte) != 1 {
        return Err(127);
    }
    Ok(base + target)
}
fn nt_service(name: &str) -> Option<Service> {
    Some(match name.trim_start_matches("Zw") {
        "NtClose" | "Close" => Service::Close,
        "NtTerminateProcess" | "TerminateProcess" => Service::TerminateProcess,
        "NtTerminateThread" | "TerminateThread" => Service::TerminateThread,
        "NtDelayExecution" | "DelayExecution" => Service::DelayExecution,
        "NtYieldExecution" | "YieldExecution" => Service::YieldExecution,
        "NtQuerySystemTime" | "QuerySystemTime" => Service::QuerySystemTime,
        "NtQueryPerformanceCounter" | "QueryPerformanceCounter" => Service::QueryPerformanceCounter,
        "NtReadFile" | "ReadFile" => Service::ReadFile,
        "NtWriteFile" | "WriteFile" => Service::WriteFile,
        "NtCreateFile" | "CreateFile" => Service::CreateFile,
        "NtAllocateVirtualMemory" | "AllocateVirtualMemory" => Service::AllocateVirtualMemory,
        "NtQueryInformationProcess" | "QueryInformationProcess" => Service::QueryInformationProcess,
        "NtOpenProcess" | "OpenProcess" => Service::OpenProcess,
        "NtWaitForSingleObject" | "WaitForSingleObject" => Service::WaitForSingleObject,
        _ => return None,
    })
}

pub fn dispatch(
    image: &mut GuestImage,
    number: u32,
    services: &mut dyn GuestServices,
) -> Result<Outcome, StopReason> {
    let id = number & 0xffff;
    if id==user::CALLBACK_RETURN{return user::callback_return(image,services);}
    let api = catalog::resolve(id).ok_or(StopReason::UnsupportedApi { id })?;
    image.win32.last_api = Some(id);
    let abi = if image.thread.context.mode == DecodeMode::X86 {
        GuestAbi::X86_32
    } else {
        GuestAbi::X86_64
    };
    let count = if abi == GuestAbi::X86_32 {
        api.x86_arguments
    } else {
        api.arguments
    } as usize;
    let a = nt61::capture_arguments(abi, &image.thread.context.regs, count, &mut image.memory)
        .map_err(|_| StopReason::DataFault {
            pc: image.thread.context.rip,
        })?;
    if api.dll == "ntdll.dll" {
        if let Some(service) = nt_service(api.name) {
            return Ok(
                match services.call(abi, service, &a[..count], &mut image.memory) {
                    ServiceResult::Return(v) => Outcome::Return(v as u64),
                    ServiceResult::ExitProcess(c) | ServiceResult::ExitThread(c) => {
                        Outcome::Exit(c)
                    }
                },
            );
        }
    }
    if api.name == "ExitProcess" || api.name == "ExitThread" {
        return Ok(Outcome::Exit(a[0] as u32));
    }
    match invoke(image, api, &a, services) {
        Ok(v) => {
            if let Some((procedure,args))=image.win32.user.callback.take(){
                user::enter_callback(image,procedure,args).map_err(|_|StopReason::DataFault{pc:image.thread.context.rip})?;
                return Ok(Outcome::Continue);
            }
            if abi == GuestAbi::X86_32 && api.name == "GetTickCount64" {
                image
                    .thread
                    .context
                    .set_reg(crate::arch::common::btl::ir::X86Reg::Rdx, v >> 32);
            }
            Ok(Outcome::Return(v))
        }
        Err(ApiError::Windows(e)) => {
            let detail = if api.name.starts_with("LoadLibrary") {
                image.win32.last_library_error.take().unwrap_or_else(|| {
                    string(&mut image.memory, a[0], api.name.ends_with('W')).unwrap_or_default()
                })
            } else if api.name == "GetProcAddress" {
                if a[1] <= 0xffff {
                    alloc::format!("ordinal {}", a[1])
                } else {
                    string(&mut image.memory, a[1], false).unwrap_or_default()
                }
            } else {
                String::new()
            };
            if image.win32.failures.len() == 16 {
                image.win32.failures.remove(0);
            }
            image.win32.failures.push((id, e, detail));
            error(image, e);
            Ok(Outcome::Return(match api.name {
                "TlsAlloc" => u32::MAX as u64,
                "GlobalFree" | "LocalFree" | "FreeSid" => a[0],
                "CreateFileA" | "CreateFileW" | "HeapSize" => {
                    if abi == GuestAbi::X86_32 {
                        u32::MAX as u64
                    } else {
                        u64::MAX
                    }
                }
                _ => 0,
            }))
        }
        Err(ApiError::Unsupported) => Err(StopReason::UnsupportedApi { id }),
        Err(ApiError::MemoryFault) => Err(StopReason::DataFault {
            pc: image.thread.context.rip,
        }),
    }
}
enum ApiError {
    Windows(u32),
    Unsupported,
    MemoryFault,
}
impl From<u32> for ApiError {
    fn from(e: u32) -> Self {
        Self::Windows(e)
    }
}
fn invoke(
    image: &mut GuestImage,
    api: &catalog::Api,
    a: &[u64],
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    if api.dll == "comdlg32.dll" || api.dll == "comctl32.dll" {
        return gui::invoke(image, api.dll, api.name, a, services);
    }
    if api.dll == "user32.dll" { return user::invoke(image, api.name, a, services); }
    if api.dll == "gdi32.dll" { return printing::invoke(image, api.name, a, services); }
    if api.dll == "advapi32.dll" {
        return advapi::invoke(image, api.name, a)
            .ok_or(ApiError::Unsupported)?
            .map_err(ApiError::Windows);
    }
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let abi = if x86 {
        GuestAbi::X86_32
    } else {
        GuestAbi::X86_64
    };
    let name = api.name;
    Ok(match name {
        "Sleep" | "SleepEx" => {
            let milliseconds = a[0] as u32;
            let mut interval = if milliseconds == u32::MAX {
                i64::MIN
            } else {
                -i64::from(milliseconds) * 10_000
            };
            let scratch = allocate(image, 0, 16, READ | WRITE)?;
            let result = (|| -> Result<u64, ApiError> {
                let start = if milliseconds != u32::MAX {
                    Some(sleep_counter(abi, services, &mut image.memory, scratch)?)
                } else {
                    None
                };
                let alertable = name == "SleepEx" && a[1] as u32 != 0;
                loop {
                    put(&mut image.memory, scratch, interval as u64, 8)?;
                    match services.call(
                        abi, Service::DelayExecution,
                        &[u64::from(alertable), scratch], &mut image.memory,
                    ) {
                        ServiceResult::Return(0) => return Ok(0),
                        ServiceResult::Return(0xc0) if alertable => return Ok(0xc0),
                        ServiceResult::Return(0x101) => {
                            if let Some((start, frequency)) = start {
                                let (now, current_frequency) = sleep_counter(
                                    abi, services, &mut image.memory, scratch,
                                )?;
                                if frequency != current_frequency || now < start {
                                    return Err(ApiError::Unsupported);
                                }
                                let elapsed = u128::from(now - start) * 10_000_000
                                    / u128::from(frequency);
                                let duration = u128::from(milliseconds) * 10_000;
                                if elapsed >= duration {
                                    return Ok(0);
                                }
                                interval = -((duration - elapsed) as i64);
                            }
                        }
                        // Sleep has no Win32 failure result. Stop execution
                        // when its backend is unavailable rather than return
                        // as if the requested delay completed.
                        _ => return Err(ApiError::Unsupported),
                    }
                }
            })();
            free(image, scratch)?;
            result?
        }
        "GlobalAlloc" => {
            if a[0]&!0x42!=0 || a[1]==0 {return Err(INVALID_PARAMETER.into());}
            heap_allocate(image,a[1])?
        }
        "GlobalFree" => {heap_free(image,a[0])?;0}
        "GlobalLock" => {
            if !image.win32.heap_allocations.iter().any(|v|v.0==a[0]){return Err(6u32.into());}a[0]
        }
        "GlobalUnlock" => {if !image.win32.heap_allocations.iter().any(|v|v.0==a[0]){return Err(6u32.into());}error(image,0);0}
        "LocalAlloc" => {
            // Movable local-memory handles need a separate lock/handle model.
            if a[0] & !0x40 != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            heap_allocate(image, a[1])?
        }
        "LocalFree" => {
            heap_free(image, a[0])?;
            0
        }
        "LocalSize" => image
            .win32
            .heap_allocations
            .iter()
            .find(|entry| entry.0 == a[0])
            .map(|entry| entry.1 as u64)
            .ok_or(INVALID_PARAMETER)?,
        "QueryPerformanceCounter"
        | "QueryPerformanceFrequency"
        | "GetTickCount"
        | "GetTickCount64"
        | "GetSystemTimeAsFileTime" => {
            let scratch = allocate(image, 0, 16, READ | WRITE)?;
            let result = (|| -> Result<u64, ApiError> {
                let system = name == "GetSystemTimeAsFileTime";
                let args = if system {
                    [scratch, 0]
                } else {
                    [scratch, scratch + 8]
                };
                let status = services.call(
                    abi,
                    if system {
                        Service::QuerySystemTime
                    } else {
                        Service::QueryPerformanceCounter
                    },
                    &args[..if system { 1 } else { 2 }],
                    &mut image.memory,
                );
                match status {
                    ServiceResult::Return(0) => {}
                    ServiceResult::Return(s) => return Err(status_error(s).into()),
                    _ => return Err(ApiError::Unsupported),
                }
                let value = word(&mut image.memory, scratch, 8)?;
                let frequency = word(&mut image.memory, scratch + 8, 8)?;
                if name == "GetTickCount" || name == "GetTickCount64" {
                    if frequency == 0 {
                        return Err(INVALID_PARAMETER.into());
                    }
                    let ticks = (value as u128 * 1000 / frequency as u128) as u64;
                    return Ok(if name == "GetTickCount" {
                        ticks as u32 as u64
                    } else {
                        ticks
                    });
                }
                put(
                    &mut image.memory,
                    a[0],
                    if name == "QueryPerformanceFrequency" {
                        frequency
                    } else {
                        value
                    },
                    8,
                )?;
                Ok(if system { 0 } else { 1 })
            })();
            free(image, scratch)?;
            result?
        }
        "GetLastError" => word(
            &mut image.memory,
            image.teb + if x86 { 0x34 } else { 0x68 },
            4,
        )?,
        "SetLastError" => {
            error(image, a[0] as u32);
            0
        }
        "GetCurrentProcess" => {
            if x86 {
                u32::MAX as u64
            } else {
                u64::MAX
            }
        }
        "GetCurrentThread" => {
            if x86 {
                u32::MAX as u64 - 1
            } else {
                u64::MAX - 1
            }
        }
        "GetCurrentProcessId" | "GetCurrentThreadId" => 1,
        "IsProcessorFeaturePresent" => {
            // Report the isolated interpreter's guarantees, never host CPUID.
            // Instruction fetch checks EXECUTE permissions (PF_NX_ENABLED=12).
            // SIMD, FPU and atomic instruction families are only partial; do
            // not advertise a family until its complete contract is supported.
            u64::from(a[0] as u32 == 12)
        }
        "IsDebuggerPresent" => 0,
        "GetVersion" => 0x1db10106,
        "GetProcessHeap" => HEAP,
        "GetStdHandle" => match a[0] as u32 {
            0xfffffff6 => crate::arch::common::btl::io::STD_INPUT,
            0xfffffff5 => crate::arch::common::btl::io::STD_OUTPUT,
            0xfffffff4 => crate::arch::common::btl::io::STD_ERROR,
            _ => return Err(6.into()),
        },
        "GetCommandLineA" | "GetCommandLineW" => {
            let p = word(
                &mut image.memory,
                image.peb + if x86 { 0x10 } else { 0x20 },
                width,
            )?;
            let ptr = word(&mut image.memory, p + if x86 { 0x44 } else { 0x78 }, width)?;
            if name.ends_with('W') {
                ptr
            } else {
                let s = string(&mut image.memory, ptr, true)?;
                let ptr = allocate(image, 0, (s.len() + 1) as u64, READ | WRITE)?;
                write_string(&mut image.memory, ptr, &s, false)?;
                ptr
            }
        }
        "GetModuleHandleA" | "GetModuleHandleW" | "LoadLibraryA" | "LoadLibraryW"
        | "LoadLibraryExA" | "LoadLibraryExW" => {
            let loading = name.starts_with("LoadLibrary");
            image.win32.last_library_error = None;
            if name.starts_with("LoadLibraryEx") && (a[1] != 0 || a[2] != 0) {
                return Err(120.into());
            }
            if a[0] == 0 {
                if loading {
                    return Err(INVALID_PARAMETER.into());
                }
                image.image_base
            } else {
                let n = string(&mut image.memory, a[0], name.ends_with('W'))?;
                if let Some(base) = module(image, &n) {
                    base
                } else if loading {
                    load_library(image, &n, services)?
                } else {
                    return Err(126.into());
                }
            }
        }
        "GlobalMemoryStatus" | "GlobalMemoryStatusEx" => {
            let extended = name.ends_with("Ex");
            let width = if extended || image.thread.context.mode != DecodeMode::X86 {
                8
            } else {
                4
            };
            let size = if extended { 64 } else { 8 + 6 * width };
            if !image.memory.probe_write(a[0], size) {
                return Err(NOACCESS.into());
            }
            if extended && word(&mut image.memory, a[0], 4)? != 64 {
                return Err(INVALID_PARAMETER.into());
            }
            let stats = services.memory_status().map_err(|_| 120u32)?;
            if stats.physical_total == 0
                || stats.physical_available > stats.physical_total
                || stats.paging_available > stats.paging_total
            {
                return Err(120.into());
            }
            let total_commit = stats.physical_total.saturating_add(stats.paging_total);
            let available_commit = stats
                .physical_available
                .saturating_add(stats.paging_available);
            // The isolated process has a documented 128 MiB mapping budget.
            // Report that real limit, rather than promising unmappable address space.
            let virtual_total = 128u64 * 1024 * 1024;
            let mapped = image
                .memory
                .regions
                .iter()
                .fold(0u64, |sum, r| sum.saturating_add(r.bytes.len() as u64));
            let virtual_available = virtual_total.saturating_sub(mapped);
            let load = (((stats.physical_total - stats.physical_available) as u128 * 100)
                / stats.physical_total as u128) as u32;
            let mut bytes = vec![0u8; size];
            bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
            bytes[4..8].copy_from_slice(&load.to_le_bytes());
            let values = [
                stats.physical_total,
                stats.physical_available,
                total_commit,
                available_commit,
                virtual_total,
                virtual_available,
            ];
            for (i, value) in values.iter().enumerate() {
                let value = if width == 4 {
                    (*value).min(u32::MAX as u64)
                } else {
                    *value
                };
                bytes[8 + i * width..8 + (i + 1) * width]
                    .copy_from_slice(&value.to_le_bytes()[..width]);
            }
            if !image.memory.write_virt(a[0], &bytes) {
                return Err(NOACCESS.into());
            }
            if extended {
                1
            } else {
                0
            }
        }
        "GetProcessAffinityMask" => {
            let width = if image.thread.context.mode == DecodeMode::X86 {
                4
            } else {
                8
            };
            let pseudo = if width == 4 {
                u32::MAX as u64
            } else {
                u64::MAX
            };
            if a[0] != pseudo {
                return Err(6.into());
            }
            if !image.memory.probe_write(a[1], width) || !image.memory.probe_write(a[2], width) {
                return Err(NOACCESS.into());
            }
            let raw = services.processor_mask().map_err(|_| 120u32)?;
            let mask = if width == 4 {
                raw & u32::MAX as u64
            } else {
                raw
            };
            if mask == 0 {
                return Err(120.into());
            }
            put(&mut image.memory, a[1], mask, width)?;
            put(&mut image.memory, a[2], mask, width)?;
            1
        }
        "FreeLibrary" => return Err(ApiError::Unsupported),
        "GetProcAddress" => {
            if a[1] <= 0xffff {
                proc_address(image, a[0], None, a[1] as u16, 0, services)?
            } else {
                let n = string(&mut image.memory, a[1], false)?;
                proc_address(image, a[0], Some(&n), 0, 0, services)?
            }
        }
        "VirtualAlloc" => {
            let kind = a[2] as u32;
            // Validate the requested length before allocation-granularity
            // padding is added. An unaligned address cannot make zero nonzero.
            if a[1] == 0 || kind & !0x3000 != 0 || kind == 0 {
                return Err(INVALID_PARAMETER.into());
            }
            let bits = protection(a[3] as u32)?;
            if a[0] != 0 && kind == 0x1000 {
                change_commit(image, a[0], a[1], bits, true)?;
                a[0] & !0xfff
            } else {
                let p = allocate(image, a[0], a[1], if kind & 0x1000 == 0 { 0 } else { bits })?;
                let length = image
                    .win32
                    .allocations
                    .iter()
                    .find(|a| a.0 == p)
                    .ok_or(INVALID_PARAMETER)?
                    .1;
                image.win32.virtual_allocations.push(VirtualAllocation {
                    base: p,
                    length,
                    committed: vec![kind & 0x1000 != 0; length / PAGE],
                });
                p
            }
        }
        "VirtualFree" => {
            match a[2] {
                0x8000 if a[1] == 0 => {
                    let i = image
                        .win32
                        .virtual_allocations
                        .iter()
                        .position(|v| v.base == a[0])
                        .ok_or(INVALID_PARAMETER)?;
                    free(image, a[0])?;
                    image.win32.virtual_allocations.remove(i);
                }
                0x4000 => {
                    let n = if a[1] == 0 {
                        image
                            .win32
                            .virtual_allocations
                            .iter()
                            .find(|v| v.base == a[0])
                            .ok_or(INVALID_PARAMETER)?
                            .length as u64
                    } else {
                        a[1]
                    };
                    change_commit(image, a[0], n, 0, false)?;
                }
                _ => return Err(INVALID_PARAMETER.into()),
            }
            1
        }
        "VirtualProtect" => {
            if !image.memory.probe_write(a[3], 4) {
                return Err(NOACCESS.into());
            }
            // lpflOldProtect may alias the changed pages. If publishing it
            // fails after permissions change, restore permissions atomically.
            let snapshot: Vec<Vec<u8>> = image
                .memory
                .regions
                .iter()
                .map(|r| r.permissions.clone())
                .collect();
            let old = protect(image, a[0], a[1], a[2] as u32)?;
            if let Err(e) = put(&mut image.memory, a[3], old as u64, 4) {
                for (region, permissions) in image.memory.regions.iter_mut().zip(snapshot) {
                    region.permissions = permissions;
                }
                return Err(e.into());
            }
            image.thread.invalidate_code();
            1
        }
        "HeapAlloc" | "HeapReAlloc" => {
            if a[0] != HEAP || a[1] & !8 != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            let n = if name == "HeapAlloc" { a[2] } else { a[3] };
            let old = if name == "HeapReAlloc" {
                Some(
                    image
                        .win32
                        .heap_allocations
                        .iter()
                        .find(|p| p.0 == a[2])
                        .copied()
                        .ok_or(INVALID_PARAMETER)?,
                )
            } else {
                None
            };
            let p = heap_allocate(image, n)?;
            if let Some((old, length)) = old {
                let count = length.min(n as usize);
                let mut cursor = 0;
                while cursor < count {
                    let chunk = (count - cursor).min(MAX_BUFFER);
                    let copied =
                        read(&mut image.memory, old + cursor as u64, chunk).and_then(|b| {
                            if image.memory.write_virt(p + cursor as u64, &b) {
                                Ok(())
                            } else {
                                Err(NOACCESS)
                            }
                        });
                    if let Err(e) = copied {
                        let _ = heap_free(image, p);
                        return Err(e.into());
                    }
                    cursor += chunk;
                }
                heap_free(image, old)?;
            }
            p
        }
        "HeapFree" => {
            if a[0] != HEAP || a[1] != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            heap_free(image, a[2])?;
            1
        }
        "HeapSize" => {
            if a[0] != HEAP || a[1] != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            image
                .win32
                .heap_allocations
                .iter()
                .find(|p| p.0 == a[2])
                .ok_or(INVALID_PARAMETER)?
                .1 as u64
        }
        "TlsAlloc" => {
            if let Some(i) = image.win32.tls.iter().position(Option::is_none) {
                image.win32.tls[i] = Some(0);
                i as u64
            } else {
                if image.win32.tls.len() >= 1088 {
                    return Err(259.into());
                }
                image.win32.tls.push(Some(0));
                image.win32.tls.len() as u64 - 1
            }
        }
        "TlsFree" => {
            let slot = image
                .win32
                .tls
                .get_mut(a[0] as usize)
                .filter(|s| s.is_some())
                .ok_or(INVALID_PARAMETER)?;
            *slot = None;
            1
        }
        "TlsSetValue" => {
            let slot = image
                .win32
                .tls
                .get_mut(a[0] as usize)
                .and_then(Option::as_mut)
                .ok_or(INVALID_PARAMETER)?;
            *slot = a[1];
            1
        }
        "TlsGetValue" => {
            let v = image
                .win32
                .tls
                .get(a[0] as usize)
                .and_then(|s| *s)
                .ok_or(INVALID_PARAMETER)?;
            error(image, 0);
            v
        }
        "EncodePointer" | "DecodePointer" => a[0],
        "lstrlenA" | "lstrlenW" => {
            if a[0] == 0 {
                0
            } else {
                let s = string(&mut image.memory, a[0], name.ends_with('W'))?;
                if name.ends_with('W') {
                    s.encode_utf16().count() as u64
                } else {
                    s.len() as u64
                }
            }
        }
        "lstrcmpA" | "lstrcmpW" | "lstrcmpiA" | "lstrcmpiW" => {
            let wide = name.ends_with('W');
            let mut x = string(&mut image.memory, a[0], wide)?;
            let mut y = string(&mut image.memory, a[1], wide)?;
            if name.contains("cmpi") {
                x.make_ascii_lowercase();
                y.make_ascii_lowercase()
            }
            match x.cmp(&y) {
                core::cmp::Ordering::Less => u32::MAX as u64,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            }
        }
        "RtlInitUnicodeString" => {
            let (n, p) = if a[1] == 0 {
                (0, 0)
            } else {
                (
                    string(&mut image.memory, a[1], true)?
                        .encode_utf16()
                        .count()
                        * 2,
                    a[1],
                )
            };
            if n > 65532 {
                return Err(INVALID_PARAMETER.into());
            }
            put(&mut image.memory, a[0], n as u64, 2)?;
            put(
                &mut image.memory,
                a[0] + 2,
                if p == 0 { 0 } else { n as u64 + 2 },
                2,
            )?;
            put(&mut image.memory, a[0] + width as u64, p, width)?;
            0
        }
        "RtlAllocateHeap" => {
            if a[0] != HEAP || a[1] & !8 != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            heap_allocate(image, a[2])?
        }
        "RtlFreeHeap" => {
            if a[0] != HEAP || a[1] != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            heap_free(image, a[2])?;
            1
        }
        "CreateFileA" | "CreateFileW" => {
            if a[3] != 0 || a[4] != 3 || a[6] != 0 {
                return Err(120.into());
            }
            if a[1] & !(0x8000_0000 | 0x0012_0089) != 0 || a[2] & !7 != 0 {
                return Err(5.into());
            }
            if a[5] & !0x80 != 0 {
                return Err(120.into());
            }
            let path = string(&mut image.memory, a[0], name.ends_with('W'))?;
            let utf16: Vec<u16> = path.encode_utf16().collect();
            if utf16.len() > 16000 {
                return Err(INVALID_PARAMETER.into());
            }
            let scratch = allocate(image, 0, (utf16.len() * 2 + 130) as u64, READ | WRITE)?;
            let result = (|| -> Result<u64, u32> {
                let us = scratch + 64;
                let buffer = scratch + 128;
                let iosb = scratch + 48;
                put(&mut image.memory, scratch, if x86 { 24 } else { 48 }, 4)?;
                put(
                    &mut image.memory,
                    scratch + if x86 { 8 } else { 16 },
                    us,
                    width,
                )?;
                put(
                    &mut image.memory,
                    scratch + if x86 { 12 } else { 24 },
                    0x40,
                    4,
                )?;
                put(&mut image.memory, us, (utf16.len() * 2) as u64, 2)?;
                put(&mut image.memory, us + 2, (utf16.len() * 2 + 2) as u64, 2)?;
                put(&mut image.memory, us + width as u64, buffer, width)?;
                write_string(&mut image.memory, buffer, &path, true)?;
                let out = scratch + 32; // overlaps zero OA security fields on x64: use separate slot below
                let out = out + 64;
                let args = [out, a[1], scratch, iosb, 0, a[5], a[2], 1, 0x60, 0, 0];
                match services.call(abi, Service::CreateFile, &args, &mut image.memory) {
                    ServiceResult::Return(0) => word(&mut image.memory, out, width),
                    ServiceResult::Return(s) => Err(status_error(s)),
                    _ => Err(120),
                }
            })();
            free(image, scratch)?;
            result?
        }
        "ReadFile" | "WriteFile" => {
            if a[4] != 0 || a[2] > 65536 {
                return Err(120.into());
            }
            if a[3] == 0 || !image.memory.probe_write(a[3], 4) {
                return Err(NOACCESS.into());
            }
            put(&mut image.memory, a[3], 0, 4)?;
            if name == "ReadFile" && a[2] != 0 && !image.memory.probe_write(a[1], a[2] as usize) {
                return Err(NOACCESS.into());
            }
            let scratch = allocate(image, 0, 16, READ | WRITE)?;
            let args = [a[0], 0, 0, 0, scratch, a[1], a[2], 0, 0];
            let result = (|| -> Result<u64, u32> {
                match services.call(
                    abi,
                    if name == "ReadFile" {
                        Service::ReadFile
                    } else {
                        Service::WriteFile
                    },
                    &args,
                    &mut image.memory,
                ) {
                    ServiceResult::Return(s) if s == 0 || s == 0xc0000011 => {
                        let n = word(&mut image.memory, scratch + width as u64, width)?;
                        put(&mut image.memory, a[3], n, 4)?;
                        Ok(1)
                    }
                    ServiceResult::Return(s) => Err(status_error(s)),
                    _ => Err(120),
                }
            })();
            free(image, scratch)?;
            result?
        }
        "memcpy" | "memmove" => {
            if a[2] == 0 {
                return Ok(a[0]);
            }
            let n = usize::try_from(a[2]).map_err(|_| ApiError::Unsupported)?;
            if n > MAX_BUFFER {
                return Err(ApiError::Unsupported);
            }
            let b = read(&mut image.memory, a[1], n).map_err(|_| ApiError::MemoryFault)?;
            if !image.memory.write_virt(a[0], &b) {
                return Err(ApiError::MemoryFault);
            }
            a[0]
        }
        "memset" => {
            if a[2] == 0 {
                return Ok(a[0]);
            }
            let n = usize::try_from(a[2]).map_err(|_| ApiError::Unsupported)?;
            if n > MAX_BUFFER {
                return Err(ApiError::Unsupported);
            }
            if !image.memory.write_virt(a[0], &vec![a[1] as u8; n]) {
                return Err(ApiError::MemoryFault);
            }
            a[0]
        }
        "memcmp" => {
            if a[2] == 0 {
                return Ok(0);
            }
            let n = usize::try_from(a[2]).map_err(|_| ApiError::Unsupported)?;
            if n > MAX_BUFFER {
                return Err(ApiError::Unsupported);
            }
            let x = read(&mut image.memory, a[0], n).map_err(|_| ApiError::MemoryFault)?;
            let y = read(&mut image.memory, a[1], n).map_err(|_| ApiError::MemoryFault)?;
            x.iter()
                .zip(y.iter())
                .find(|(x, y)| x != y)
                .map_or(0, |(x, y)| (*x as i32 - *y as i32) as u32 as u64)
        }
        "CloseHandle" => match services.call(abi, Service::Close, &a[..1], &mut image.memory) {
            ServiceResult::Return(0) => 1,
            ServiceResult::Return(s) => return Err(status_error(s).into()),
            _ => return Err(ApiError::Unsupported),
        },
        "CryptBinaryToStringA" | "CryptBinaryToStringW" => {
            binary_to_string(&mut image.memory, a, name.ends_with('W'))?
        }
        "CryptStringToBinaryA" | "CryptStringToBinaryW" => {
            string_to_binary(&mut image.memory, a, name.ends_with('W'))?
        }
        _ => return Err(ApiError::Unsupported),
    })
}

fn sleep_counter(
    abi: GuestAbi,
    services: &mut dyn GuestServices,
    memory: &mut dyn GuestMemory,
    scratch: u64,
) -> Result<(u64, u64), ApiError> {
    match services.call(abi, Service::QueryPerformanceCounter, &[scratch, scratch + 8], memory) {
        ServiceResult::Return(0) => {}
        _ => return Err(ApiError::Unsupported),
    }
    let counter = word(memory, scratch, 8)?;
    let frequency = word(memory, scratch + 8, 8)?;
    if frequency == 0 {
        return Err(ApiError::Unsupported);
    }
    Ok((counter, frequency))
}
fn status_error(s: u32) -> u32 {
    match s {
        0xc0000005 => NOACCESS,
        0xc0000008 => 6,
        0xc000000d => 87,
        0xc0000022 => 5,
        0xc0000034 | 0xc000000f => 2,
        0xc000003a => 3,
        0xc0000011 => 38,
        0xc0000002 => 120,
        _ => 1,
    }
}
const BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn encode64(b: &[u8]) -> String {
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | c.get(2).copied().unwrap_or(0) as u32;
        s.push(BASE64[(n >> 18) as usize] as char);
        s.push(BASE64[((n >> 12) & 63) as usize] as char);
        s.push(if c.len() > 1 {
            BASE64[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            BASE64[(n & 63) as usize] as char
        } else {
            '='
        })
    }
    s
}
fn binary_to_string(m: &mut dyn GuestMemory, a: &[u64], wide: bool) -> Result<u64, u32> {
    let flags = a[2] as u32;
    let kind = flags & 0x0fff_ffff;
    if flags & !0xcfff_ffff != 0 || !matches!(kind, 0 | 1 | 3 | 12) {
        return Err(INVALID_PARAMETER);
    }
    let b = read(
        m,
        a[0],
        usize::try_from(a[1]).map_err(|_| INVALID_PARAMETER)?,
    )?;
    let capacity = word(m, a[4], 4)? as usize;
    let mut s = String::new();
    if kind == 12 {
        for v in &b {
            use core::fmt::Write;
            let _ = write!(s, "{v:02x}");
        }
    } else if kind == 3 {
        return Err(120);
    } else {
        let encoded = encode64(&b);
        let newline = if flags & 0x4000_0000 != 0 {
            ""
        } else if flags & 0x8000_0000 != 0 {
            "\n"
        } else {
            "\r\n"
        };
        if kind == 0 {
            s.push_str("-----BEGIN CERTIFICATE-----");
            s.push_str(newline)
        }
        for c in encoded.as_bytes().chunks(64) {
            s.push_str(core::str::from_utf8(c).map_err(|_| 13u32)?);
            s.push_str(newline)
        }
        if kind == 0 {
            s.push_str("-----END CERTIFICATE-----");
            s.push_str(newline)
        }
    }
    let required = if wide {
        s.encode_utf16().count()
    } else {
        s.len()
    };
    if a[3] == 0 {
        put(m, a[4], (required + 1) as u64, 4)?;
        return Ok(1);
    }
    if capacity < required + 1 {
        put(m, a[4], (required + 1) as u64, 4)?;
        return Err(234);
    }
    if !m.probe_write(a[3], (required + 1) * if wide { 2 } else { 1 }) {
        return Err(NOACCESS);
    }
    write_string(m, a[3], &s, wide)?;
    put(m, a[4], required as u64, 4)?;
    Ok(1)
}
fn decode64(s: &str) -> Result<Vec<u8>, u32> {
    let b: Vec<u8> = s
        .bytes()
        .filter(|c| c.is_ascii_whitespace() == false)
        .collect();
    if b.len() % 4 != 0 {
        return Err(13);
    }
    let mut out = Vec::new();
    for (i, c) in b.chunks(4).enumerate() {
        let last = i * 4 + 4 == b.len();
        let pad = if c[2] == b'=' {
            2
        } else if c[3] == b'=' {
            1
        } else {
            0
        };
        if pad > 0 && !last || pad == 2 && c[3] != b'=' {
            return Err(13);
        }
        let mut v = [0u32; 4];
        for j in 0..4 - pad {
            v[j] = BASE64.iter().position(|x| *x == c[j]).ok_or(13u32)? as u32
        }
        if pad == 2 && v[1] & 15 != 0 || pad == 1 && v[2] & 3 != 0 {
            return Err(13);
        }
        let n = v[0] << 18 | v[1] << 12 | v[2] << 6 | v[3];
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8)
        }
        if pad < 1 {
            out.push(n as u8)
        }
    }
    Ok(out)
}
fn string_to_binary(m: &mut dyn GuestMemory, a: &[u64], wide: bool) -> Result<u64, u32> {
    let kind = a[2] as u32;
    if !matches!(kind, 1 | 12) {
        return Err(INVALID_PARAMETER);
    }
    let s = if a[1] == 0 {
        string(m, a[0], wide)?
    } else {
        let n = usize::try_from(a[1]).map_err(|_| INVALID_PARAMETER)?;
        let b = read(
            m,
            a[0],
            n.checked_mul(if wide { 2 } else { 1 })
                .ok_or(INVALID_PARAMETER)?,
        )?;
        if wide {
            String::from_utf16(
                &b.chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| 13u32)?
        } else {
            String::from_utf8(b).map_err(|_| 13u32)?
        }
    };
    let b = if kind == 1 {
        decode64(&s)?
    } else {
        let hex: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if hex.len() % 2 != 0 {
            return Err(13);
        }
        let mut b = Vec::new();
        for c in hex.chunks_exact(2) {
            let h = (c[0] as char).to_digit(16).ok_or(13u32)?;
            let l = (c[1] as char).to_digit(16).ok_or(13u32)?;
            b.push((h * 16 + l) as u8)
        }
        b
    };
    let capacity = word(m, a[4], 4)?;
    for p in [a[5], a[6]] {
        if p != 0 && !m.probe_write(p, 4) {
            return Err(NOACCESS);
        }
    }
    if !m.probe_write(a[4], 4) {
        return Err(NOACCESS);
    }
    if a[3] != 0 && capacity < b.len() as u64 {
        put(m, a[4], b.len() as u64, 4)?;
        return Err(234);
    }
    if a[3] != 0 && !m.write_virt(a[3], &b) {
        return Err(NOACCESS);
    }
    put(m, a[4], b.len() as u64, 4)?;
    if a[5] != 0 {
        put(m, a[5], 0, 4)?
    }
    if a[6] != 0 {
        put(m, a[6], kind as u64, 4)?
    }
    Ok(1)
}

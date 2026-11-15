//! Isolated guest SCM configuration database. No host services are installed or
//! started. Rights are handle capabilities, not a substitute for Windows SCM ACLs.
//! Contracts: https://learn.microsoft.com/windows/win32/api/winsvc/
//! nf-winsvc-{openscmanagerw,createservicew,queryserviceconfigw,deleteservice,
//! enumservicesstatusw,enumservicesstatusexw,getservicedisplaynamew}.
use super::super::{put, string, word};
use crate::arch::common::btl::{ir::DecodeMode, memory::GuestMemory, pe::GuestImage};
use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

const INVALID_HANDLE: u32 = 6;
const ACCESS_DENIED: u32 = 5;
const INVALID_PARAMETER: u32 = 87;
const NOACCESS: u32 = 998;
const NOT_SUPPORTED: u32 = 50;
const MANAGER_ALL: u32 = 0x000f_003f;
const SERVICE_ALL: u32 = 0x000f_01ff;
const DELETE: u32 = 0x10000;
const MAX_SERVICES: usize = 256;
const MAX_HANDLES: usize = 4096;

#[derive(Clone)]
struct Config {
    name: String,
    display: String,
    binary: String,
    group: String,
    dependencies: Vec<String>,
    account: String,
    service_type: u32,
    start: u32,
    error: u32,
    deleted: bool,
}
#[derive(Clone, Copy)]
enum Kind {
    Manager,
    Service(usize),
}
struct Handle {
    id: u64,
    rights: u32,
    kind: Kind,
}
pub struct State {
    services: Vec<Option<Config>>,
    handles: Vec<Handle>,
    next_handle: u64,
}
impl State {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            handles: Vec::new(),
            next_handle: 0x7300_0000,
        }
    }
    fn handle(&self, id: u64, rights: u32) -> Result<&Handle, u32> {
        let h = self
            .handles
            .iter()
            .find(|h| h.id == id)
            .ok_or(INVALID_HANDLE)?;
        if h.rights & rights != rights {
            return Err(ACCESS_DENIED);
        }
        Ok(h)
    }
    fn manager(&self, id: u64, rights: u32) -> Result<(), u32> {
        if !matches!(self.handle(id, 0)?.kind, Kind::Manager) {
            return Err(INVALID_HANDLE);
        }
        self.handle(id, rights)?;
        Ok(())
    }
    fn service(&self, id: u64, rights: u32) -> Result<usize, u32> {
        match self.handle(id, 0)?.kind {
            Kind::Service(i) => {
                self.handle(id, rights)?;
                Ok(i)
            }
            _ => Err(INVALID_HANDLE),
        }
    }
    fn allocate(&mut self, kind: Kind, rights: u32) -> Result<u64, u32> {
        if self.handles.len() >= MAX_HANDLES {
            return Err(8);
        }
        let id = self.next_handle;
        self.next_handle = self.next_handle.checked_add(4).ok_or(8u32)?;
        self.handles.push(Handle { id, rights, kind });
        Ok(id)
    }
    fn config(&self, i: usize) -> Result<&Config, u32> {
        self.services
            .get(i)
            .and_then(Option::as_ref)
            .ok_or(INVALID_HANDLE)
    }
    fn find(&self, name: &str) -> Option<usize> {
        self.services.iter().position(|s| {
            s.as_ref()
                .is_some_and(|s| s.name.eq_ignore_ascii_case(name))
        })
    }
    fn collect_deleted(&mut self, i: usize) {
        if self.services[i].as_ref().is_some_and(|s| s.deleted)
            && !self
                .handles
                .iter()
                .any(|h| matches!(h.kind,Kind::Service(j) if i==j))
        {
            self.services[i] = None;
        }
    }
}
fn access(mut bits: u32, manager: bool) -> Result<u32, u32> {
    let all = if manager { MANAGER_ALL } else { SERVICE_ALL };
    if bits & 0x0200_0000 != 0 {
        bits = (bits & !0x0200_0000) | all;
    }
    if bits & 0x1000_0000 != 0 {
        bits = (bits & !0x1000_0000) | all;
    }
    if bits & 0x8000_0000 != 0 {
        bits = (bits & !0x8000_0000) | if manager { 0x20014 } else { 0x2008d };
    }
    if bits & 0x4000_0000 != 0 {
        bits = (bits & !0x4000_0000) | if manager { 0x20022 } else { 0x20002 };
    }
    if bits & 0x2000_0000 != 0 {
        bits = (bits & !0x2000_0000) | if manager { 0x20009 } else { 0x20170 };
    }
    // Security-system access requires a real privilege backend.
    if bits & 0x0100_0000 != 0 {
        return Err(1314);
    }
    if bits & !all != 0 {
        return Err(ACCESS_DENIED);
    }
    Ok(bits | if manager { 1 } else { 0 })
}
fn opt(image: &mut GuestImage, p: u64, wide: bool) -> Result<Option<String>, u32> {
    if p == 0 {
        Ok(None)
    } else {
        Ok(Some(string(&mut image.memory, p, wide)?))
    }
}
fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.encode_utf16().count() <= 256 && !s.contains(['\\', '/'])
}
fn multi(image: &mut GuestImage, p: u64, wide: bool) -> Result<Vec<String>, u32> {
    if p == 0 {
        return Ok(Vec::new());
    }
    let width = if wide { 2 } else { 1 };
    let mut result = Vec::new();
    let mut at = p;
    for _ in 0..256 {
        let s = string(&mut image.memory, at, wide)?;
        if s.is_empty() {
            return Ok(result);
        }
        let count = if wide {
            s.encode_utf16().count()
        } else {
            s.len()
        };
        at = at
            .checked_add(((count + 1) * width) as u64)
            .ok_or(NOACCESS)?;
        if at - p > 65536 {
            return Err(INVALID_PARAMETER);
        }
        result.push(s);
    }
    Err(INVALID_PARAMETER)
}
fn encoded(s: &str, wide: bool) -> Result<Vec<u8>, u32> {
    if wide {
        Ok(s.encode_utf16()
            .chain(core::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect())
    } else if s.is_ascii() {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        Ok(v)
    } else {
        Err(1113)
    }
}
fn deps_bytes(s: &[String], wide: bool) -> Result<Vec<u8>, u32> {
    let mut b = Vec::new();
    for entry in s {
        b.extend(encoded(entry, wide)?);
    }
    b.extend(if wide { &[0, 0][..] } else { &[0][..] });
    if s.is_empty() {
        b.extend(if wide { &[0, 0][..] } else { &[0][..] });
    }
    Ok(b)
}
fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn putptr(b: &mut [u8], o: usize, v: u64, w: usize) {
    b[o..o + w].copy_from_slice(&v.to_le_bytes()[..w]);
}
fn writable(image: &mut GuestImage, p: u64, n: usize) -> Result<(), u32> {
    if p == 0 || !image.memory.probe_write(p, n) {
        Err(NOACCESS)
    } else {
        Ok(())
    }
}
fn config_bytes(s: &Config, p: u64, wide: bool, w: usize) -> Result<Vec<u8>, u32> {
    let size = if w == 8 { 64 } else { 36 };
    let mut out = vec![0; size];
    put32(&mut out, 0, s.service_type);
    put32(&mut out, 4, s.start);
    put32(&mut out, 8, s.error);
    let pointer_start = if w == 8 { 16 } else { 12 };
    let group_offset = pointer_start + w;
    let tag_offset = group_offset + w;
    let dep_offset = if w == 8 { 40 } else { 24 };
    for (offset, bytes) in [
        (pointer_start, encoded(&s.binary, wide)?),
        (group_offset, encoded(&s.group, wide)?),
        (dep_offset, deps_bytes(&s.dependencies, wide)?),
        (dep_offset + w, encoded(&s.account, wide)?),
        (dep_offset + 2 * w, encoded(&s.display, wide)?),
    ] {
        let pointer = p.checked_add(out.len() as u64).ok_or(NOACCESS)?;
        putptr(&mut out, offset, pointer, w);
        out.extend(bytes);
    }
    put32(&mut out, tag_offset, 0);
    Ok(out)
}
fn status(s: &Config, process: bool) -> Vec<u8> {
    let mut out = vec![0; if process { 36 } else { 28 }];
    put32(&mut out, 0, s.service_type);
    put32(&mut out, 4, 1);
    out
}
fn validate_configuration(t: u32, start: u32, error: u32) -> Result<(), u32> {
    if !matches!(t, 1 | 2 | 0x10 | 0x20 | 0x110 | 0x120) || start > 4 || error > 3 {
        return Err(INVALID_PARAMETER);
    }
    if start <= 1 && t & 0x30 != 0 {
        return Err(INVALID_PARAMETER);
    }
    Ok(())
}
fn credentials(account: Option<String>, password: Option<String>) -> Result<String, u32> {
    let account = account.unwrap_or_else(|| "LocalSystem".to_string());
    if !account.eq_ignore_ascii_case("LocalSystem")
        && !account.eq_ignore_ascii_case("NT AUTHORITY\\LocalService")
        && !account.eq_ignore_ascii_case("NT AUTHORITY\\NetworkService")
    {
        return Err(NOT_SUPPORTED);
    }
    if password.is_some_and(|s| !s.is_empty()) {
        return Err(NOT_SUPPORTED);
    }
    Ok(account)
}
fn enum_services(
    image: &mut GuestImage,
    a: &[u64],
    wide: bool,
    process: bool,
    w: usize,
) -> Result<u64, u32> {
    let (typ, state, buffer, size, needed, returned, resume, group) = if process {
        if a[1] != 0 {
            return Err(124);
        }
        (
            a[2] as u32,
            a[3] as u32,
            a[4],
            a[5] as usize,
            a[6],
            a[7],
            a[8],
            opt(image, a[9], wide)?,
        )
    } else {
        (
            a[1] as u32,
            a[2] as u32,
            a[3],
            a[4] as usize,
            a[5],
            a[6],
            a[7],
            None,
        )
    };
    image.win32.advapi.service.manager(a[0], 4)?;
    if typ == 0 || typ & !0x13b != 0 || !(1..=3).contains(&state) || size > 256 * 1024 {
        return Err(INVALID_PARAMETER);
    }
    writable(image, needed, 4)?;
    writable(image, returned, 4)?;
    let start = if resume == 0 {
        0
    } else {
        writable(image, resume, 4)?;
        word(&mut image.memory, resume, 4)? as usize
    };
    let services: Vec<(usize, Config)> = image
        .win32
        .advapi
        .service
        .services
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let s = s.as_ref()?;
            if s.deleted || i < start || s.service_type & typ == 0 || state & 2 == 0 {
                return None;
            }
            if group
                .as_ref()
                .is_some_and(|g| !g.eq_ignore_ascii_case(&s.group))
            {
                return None;
            }
            Some((i, s.clone()))
        })
        .collect();
    let struct_size = if w == 8 {
        if process {
            56
        } else {
            48
        }
    } else if process {
        44
    } else {
        36
    };
    let mut records = Vec::new();
    let mut total = 0usize;
    for (i, s) in &services {
        let n = encoded(&s.name, wide)?;
        let d = encoded(&s.display, wide)?;
        total += struct_size + n.len() + d.len();
        records.push((*i, s, n, d));
    }
    let mut count = 0usize;
    let mut used = 0usize;
    for (_, _, n, d) in &records {
        let add = struct_size + n.len() + d.len();
        if used + add > size {
            break;
        }
        used += add;
        count += 1;
    }
    if count != 0 {
        writable(image, buffer, used)?;
    }
    let mut bytes = vec![0; count * struct_size];
    for (position, (_, s, n, d)) in records.iter().take(count).enumerate() {
        let base = position * struct_size;
        let p = buffer.checked_add(bytes.len() as u64).ok_or(NOACCESS)?;
        putptr(&mut bytes, base, p, w);
        bytes.extend_from_slice(n);
        let p = buffer.checked_add(bytes.len() as u64).ok_or(NOACCESS)?;
        putptr(&mut bytes, base + w, p, w);
        bytes.extend_from_slice(d);
        let st = status(s, process);
        bytes[base + 2 * w..base + 2 * w + st.len()].copy_from_slice(&st);
    }
    if !bytes.is_empty() && !image.memory.write_virt(buffer, &bytes) {
        return Err(NOACCESS);
    }
    put(&mut image.memory, returned, count as u64, 4)?;
    put(&mut image.memory, needed, (total - used) as u64, 4)?;
    if resume != 0 {
        put(
            &mut image.memory,
            resume,
            if count < records.len() {
                records[count].0 as u64
            } else {
                0
            },
            4,
        )?;
    }
    if count < records.len() {
        Err(234)
    } else {
        Ok(1)
    }
}

pub fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Option<Result<u64, u32>> {
    let supported = matches!(
        name,
        "OpenSCManagerA"
            | "OpenSCManagerW"
            | "OpenServiceA"
            | "OpenServiceW"
            | "CloseServiceHandle"
            | "CreateServiceA"
            | "CreateServiceW"
            | "DeleteService"
            | "ChangeServiceConfigA"
            | "ChangeServiceConfigW"
            | "QueryServiceConfigA"
            | "QueryServiceConfigW"
            | "QueryServiceStatus"
            | "QueryServiceStatusEx"
            | "EnumServicesStatusA"
            | "EnumServicesStatusW"
            | "EnumServicesStatusExA"
            | "EnumServicesStatusExW"
            | "GetServiceDisplayNameA"
            | "GetServiceDisplayNameW"
            | "GetServiceKeyNameA"
            | "GetServiceKeyNameW"
            | "StartServiceA"
            | "StartServiceW"
            | "ControlService"
            | "StartServiceCtrlDispatcherA"
            | "StartServiceCtrlDispatcherW"
            | "RegisterServiceCtrlHandlerA"
            | "RegisterServiceCtrlHandlerW"
            | "RegisterServiceCtrlHandlerExA"
            | "RegisterServiceCtrlHandlerExW"
            | "SetServiceStatus"
    );
    if !supported {
        return None;
    }
    Some((|| {
        let wide = name.ends_with('W');
        let w = if image.thread.context.mode == DecodeMode::X86 {
            4
        } else {
            8
        };
        match name {
            "OpenSCManagerA" | "OpenSCManagerW" => {
                let machine = opt(image, a[0], wide)?;
                let database = opt(image, a[1], wide)?;
                if machine
                    .as_ref()
                    .is_some_and(|s| !s.is_empty() && s != "." && s != "\\\\.")
                {
                    return Err(1722);
                }
                if database
                    .as_ref()
                    .is_some_and(|s| !s.eq_ignore_ascii_case("ServicesActive"))
                {
                    return Err(1065);
                }
                let rights = access(a[2] as u32, true)?;
                image.win32.advapi.service.allocate(Kind::Manager, rights)
            }
            "OpenServiceA" | "OpenServiceW" => {
                image.win32.advapi.service.manager(a[0], 1)?;
                let n = string(&mut image.memory, a[1], wide)?;
                if !name_ok(&n) {
                    return Err(123);
                }
                let rights = access(a[2] as u32, false)?;
                let i = image.win32.advapi.service.find(&n).ok_or(1060u32)?;
                if image.win32.advapi.service.config(i)?.deleted {
                    return Err(1072);
                }
                image
                    .win32
                    .advapi
                    .service
                    .allocate(Kind::Service(i), rights)
            }
            "CloseServiceHandle" => {
                let state = &mut image.win32.advapi.service;
                let i = state
                    .handles
                    .iter()
                    .position(|h| h.id == a[0])
                    .ok_or(INVALID_HANDLE)?;
                let h = state.handles.remove(i);
                if let Kind::Service(i) = h.kind {
                    state.collect_deleted(i);
                }
                Ok(1)
            }
            "CreateServiceA" | "CreateServiceW" => {
                image.win32.advapi.service.manager(a[0], 2)?;
                let n = string(&mut image.memory, a[1], wide)?;
                if !name_ok(&n) {
                    return Err(123);
                }
                let display = opt(image, a[2], wide)?.unwrap_or_else(|| n.clone());
                if display.encode_utf16().count() > 256 {
                    return Err(INVALID_PARAMETER);
                }
                let rights = access(a[3] as u32, false)?;
                let typ = a[4] as u32;
                let start = a[5] as u32;
                let err = a[6] as u32;
                validate_configuration(typ, start, err)?;
                let binary = string(&mut image.memory, a[7], wide)?;
                if binary.is_empty() {
                    return Err(123);
                }
                let group = opt(image, a[8], wide)?.unwrap_or_default();
                // Group tag assignment needs a real load-order database.
                if a[9] != 0 {
                    return Err(NOT_SUPPORTED);
                }
                let dependencies = multi(image, a[10], wide)?;
                if dependencies.iter().any(|s| s.eq_ignore_ascii_case(&n)) {
                    return Err(1059);
                }
                let account_in = opt(image, a[11], wide)?;
                let password = opt(image, a[12], wide)?;
                let account = credentials(account_in, password)?;
                if typ & 0x100 != 0 && !account.eq_ignore_ascii_case("LocalSystem") {
                    return Err(INVALID_PARAMETER);
                }
                let state = &mut image.win32.advapi.service;
                if let Some(i) = state.find(&n) {
                    return Err(if state.config(i)?.deleted { 1072 } else { 1073 });
                }
                if state
                    .services
                    .iter()
                    .flatten()
                    .any(|s| s.display.eq_ignore_ascii_case(&display))
                {
                    return Err(1078);
                }
                if state.services.iter().flatten().count() >= MAX_SERVICES
                    || state.handles.len() >= MAX_HANDLES
                {
                    return Err(8);
                }
                let i = state
                    .services
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(state.services.len());
                if i == state.services.len() {
                    state.services.push(None);
                }
                state.services[i] = Some(Config {
                    name: n,
                    display,
                    binary,
                    group,
                    dependencies,
                    account,
                    service_type: typ,
                    start,
                    error: err,
                    deleted: false,
                });
                state.allocate(Kind::Service(i), rights)
            }
            "DeleteService" => {
                let state = &mut image.win32.advapi.service;
                let i = state.service(a[0], DELETE)?;
                let s = state.services[i].as_mut().ok_or(INVALID_HANDLE)?;
                if s.deleted {
                    return Err(1072);
                }
                s.deleted = true;
                Ok(1)
            }
            "ChangeServiceConfigA" | "ChangeServiceConfigW" => {
                let i = image.win32.advapi.service.service(a[0], 2)?;
                let mut cfg = image.win32.advapi.service.config(i)?.clone();
                if cfg.deleted {
                    return Err(1072);
                }
                if a[1] as u32 != u32::MAX {
                    cfg.service_type = a[1] as u32;
                }
                if a[2] as u32 != u32::MAX {
                    cfg.start = a[2] as u32;
                }
                if a[3] as u32 != u32::MAX {
                    cfg.error = a[3] as u32;
                }
                validate_configuration(cfg.service_type, cfg.start, cfg.error)?;
                if let Some(v) = opt(image, a[4], wide)? {
                    if v.is_empty() {
                        return Err(123);
                    }
                    cfg.binary = v;
                }
                if let Some(v) = opt(image, a[5], wide)? {
                    cfg.group = v;
                }
                if a[6] != 0 {
                    return Err(NOT_SUPPORTED);
                }
                if a[7] != 0 {
                    cfg.dependencies = multi(image, a[7], wide)?;
                }
                if cfg
                    .dependencies
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&cfg.name))
                {
                    return Err(1059);
                }
                let account = opt(image, a[8], wide)?;
                let password = opt(image, a[9], wide)?;
                if account.is_some() || password.is_some() {
                    cfg.account =
                        credentials(account.or_else(|| Some(cfg.account.clone())), password)?;
                }
                if cfg.service_type & 0x100 != 0 && !cfg.account.eq_ignore_ascii_case("LocalSystem")
                {
                    return Err(INVALID_PARAMETER);
                }
                if let Some(v) = opt(image, a[10], wide)? {
                    if v.encode_utf16().count() > 256 {
                        return Err(INVALID_PARAMETER);
                    }
                    cfg.display = v;
                }
                if image
                    .win32
                    .advapi
                    .service
                    .services
                    .iter()
                    .enumerate()
                    .any(|(j, s)| {
                        j != i
                            && s.as_ref()
                                .is_some_and(|s| s.display.eq_ignore_ascii_case(&cfg.display))
                    })
                {
                    return Err(1078);
                }
                image.win32.advapi.service.services[i] = Some(cfg);
                Ok(1)
            }
            "QueryServiceConfigA" | "QueryServiceConfigW" => {
                let i = image.win32.advapi.service.service(a[0], 1)?;
                let cfg = image.win32.advapi.service.config(i)?.clone();
                let bytes = config_bytes(&cfg, a[1], wide, w)?;
                writable(image, a[3], 4)?;
                put(&mut image.memory, a[3], bytes.len() as u64, 4)?;
                if a[2] < (bytes.len() as u64) {
                    return Err(122);
                }
                writable(image, a[1], bytes.len())?;
                if !image.memory.write_virt(a[1], &bytes) {
                    return Err(NOACCESS);
                }
                Ok(1)
            }
            "QueryServiceStatus" | "QueryServiceStatusEx" => {
                let i = image.win32.advapi.service.service(a[0], 4)?;
                let cfg = image.win32.advapi.service.config(i)?.clone();
                let ex = name == "QueryServiceStatusEx";
                let (p, size) = if ex {
                    if a[1] != 0 {
                        return Err(124);
                    }
                    writable(image, a[4], 4)?;
                    put(&mut image.memory, a[4], 36, 4)?;
                    (a[2], a[3])
                } else {
                    (a[1], 28)
                };
                let bytes = status(&cfg, ex);
                if size < (bytes.len() as u64) {
                    return Err(122);
                }
                writable(image, p, bytes.len())?;
                if !image.memory.write_virt(p, &bytes) {
                    return Err(NOACCESS);
                }
                Ok(1)
            }
            "EnumServicesStatusA"
            | "EnumServicesStatusW"
            | "EnumServicesStatusExA"
            | "EnumServicesStatusExW" => {
                enum_services(image, a, wide, name.contains("StatusEx"), w)
            }
            "GetServiceDisplayNameA"
            | "GetServiceDisplayNameW"
            | "GetServiceKeyNameA"
            | "GetServiceKeyNameW" => {
                image.win32.advapi.service.manager(a[0], 1)?;
                let n = string(&mut image.memory, a[1], wide)?;
                let by_display = name.starts_with("GetServiceKeyName");
                let cfg = image
                    .win32
                    .advapi
                    .service
                    .services
                    .iter()
                    .flatten()
                    .find(|s| {
                        !s.deleted
                            && if by_display {
                                s.display.eq_ignore_ascii_case(&n)
                            } else {
                                s.name.eq_ignore_ascii_case(&n)
                            }
                    })
                    .ok_or(1060u32)?;
                let text = if by_display {
                    cfg.name.clone()
                } else {
                    cfg.display.clone()
                };
                let b = encoded(&text, wide)?;
                writable(image, a[3], 4)?;
                let cap = word(&mut image.memory, a[3], 4)?;
                let count = (b.len() / if wide { 2 } else { 1 }) - 1;
                if cap <= count as u64 || a[2] == 0 {
                    put(&mut image.memory, a[3], count as u64 + 1, 4)?;
                    if a[2] != 0 && cap != 0 {
                        writable(image, a[2], if wide { 2 } else { 1 })?;
                        put(&mut image.memory, a[2], 0, if wide { 2 } else { 1 })?;
                    }
                    return Err(122);
                }
                writable(image, a[2], b.len())?;
                if !image.memory.write_virt(a[2], &b) {
                    return Err(NOACCESS);
                }
                put(&mut image.memory, a[3], count as u64, 4)?;
                Ok(1)
            }
            "StartServiceA" | "StartServiceW" => {
                let i = image.win32.advapi.service.service(a[0], 0x10)?;
                let cfg = image.win32.advapi.service.config(i)?;
                if cfg.deleted {
                    return Err(1072);
                }
                if cfg.start == 4 {
                    return Err(1058);
                }
                Err(NOT_SUPPORTED)
            }
            "ControlService" => {
                let right = match a[1] as u32 {
                    1 => 0x20,
                    2 | 3 => 0x40,
                    4 => 0x80,
                    128..=255 => 0x100,
                    _ => return Err(1052),
                };
                image.win32.advapi.service.service(a[0], right)?;
                Err(1062)
            }
            "StartServiceCtrlDispatcherA" | "StartServiceCtrlDispatcherW" => Err(1063),
            "RegisterServiceCtrlHandlerA"
            | "RegisterServiceCtrlHandlerW"
            | "RegisterServiceCtrlHandlerExA"
            | "RegisterServiceCtrlHandlerExW" => Err(1063),
            "SetServiceStatus" => Err(INVALID_HANDLE),
            _ => Err(NOT_SUPPORTED),
        }
    })())
}

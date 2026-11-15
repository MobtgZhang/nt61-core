//! Bounded, process-private registry, never host registry or a persistent hive.
//! No fabricated OS/version/user values are installed. Handles and values die
//! with GuestImage. ACLs, inheritance, symbolic links, HKCR's merged view,
//! notifications, transactions, remote access and persistence are unsupported.
//! Requested handle access is enforced; this is not a Windows security boundary.
//! A strings use the runtime's ASCII subset; W data preserves raw UTF-16 units.
//! Names use Unicode lowercase comparison (not Windows' full ordinal upcase table).
//! Last-write FILETIMEs are zero (unknown), not invented host timestamps.
//!
//! Microsoft contracts consulted 2026-11-14:
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regcopytreew
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regdeletekeyvaluew
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regsetkeyvaluew
//! CopyTree merges data snapshots; every key has the same private default
//! descriptor. It cannot copy Windows ACLs. Exact self-copy is a no-op; copying
//! into a descendant in the same view is rejected with ERROR_INVALID_PARAMETER.
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regenumkeyw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regqueryvaluew
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regsetvaluew
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regquerymultiplevaluesw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/ns-winreg-valentw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regcreatekeyexw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regopenkeyexw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regclosekey
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regsetvalueexw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regqueryvalueexw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regdeletekeyexw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regenumkeyexw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regenumvaluew
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regqueryinfokeyw
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-reggetvaluew
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regdeletetreew
//! https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regflushkey
//! https://learn.microsoft.com/en-us/windows/win32/sysinfo/registry-key-security-and-access-rights
//! https://learn.microsoft.com/en-us/windows/win32/winprog64/shared-registry-keys
use crate::arch::common::btl::{
    ir::DecodeMode,
    memory::GuestMemory,
    pe::{
        win32::{put, read, string, word, write_string},
        GuestImage,
    },
};
use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

const INVALID: u32 = 87;
const DENIED: u32 = 5;
const BAD_HANDLE: u32 = 6;
const NOT_FOUND: u32 = 2;
const MORE: u32 = 234;
const NO_MORE: u32 = 259;
const DELETED: u32 = 1018;
const UNSUPPORTED: u32 = 50;
const NOACCESS: u32 = 998;
const MAX_DATA: usize = 1024 * 1024;
const MAX_TOTAL: usize = 16 * MAX_DATA;
const MAX_KEYS: usize = 4096;
const MAX_HANDLES: usize = 4096;
const ALL: u32 = 0xf003f;
const QUERY: u32 = 1;
const SET: u32 = 2;
const ENUM: u32 = 8;
const DELETE: u32 = 0x10000;

#[derive(Clone)]
struct Value {
    name: String,
    kind: u32,
    data: Vec<u8>,
}
#[derive(Clone)]
struct Key {
    root: u8,
    path: String,
    view: u8,
    class: String,
    live: bool,
    values: Vec<Value>,
}
#[derive(Clone, Copy)]
struct Handle {
    value: u64,
    key: usize,
    rights: u32,
    view: u8,
}
#[derive(Clone)]
struct Location {
    root: u8,
    path: String,
    view: u8,
    key: usize,
    rights: u32,
}
#[derive(Clone)]
pub struct State {
    keys: Vec<Key>,
    handles: Vec<Handle>,
    next: u64,
    bytes: usize,
}
impl State {
    pub fn new() -> Self {
        let mut s = Self {
            keys: Vec::new(),
            handles: Vec::new(),
            next: 0x5e00_0000,
            bytes: 0,
        };
        // Structural roots only. HKCR is intentionally an isolated classes tree;
        // it does not silently claim the Windows user/machine merged behavior.
        for root in 0..5 {
            s.insert(root, "", 64, "");
        }
        for (r, p) in [
            (1, "Software"),
            (2, "Software"),
            (2, "System"),
            (3, ".DEFAULT"),
        ] {
            s.insert(r, p, 64, "");
            if r == 2 && p == "Software" {
                s.insert(r, p, 32, "");
            }
        }
        s
    }
    fn insert(&mut self, root: u8, path: &str, view: u8, class: &str) -> usize {
        let id = self.keys.len();
        self.bytes += path.len() + class.len();
        self.keys.push(Key {
            root,
            path: path.to_string(),
            view: physical_view(root, path, view),
            class: class.to_string(),
            live: true,
            values: Vec::new(),
        });
        id
    }
    fn find(&self, root: u8, path: &str, view: u8) -> Option<usize> {
        let v = physical_view(root, path, view);
        self.keys
            .iter()
            .position(|k| k.live && k.root == root && k.view == v && equal(&k.path, path))
    }
    fn locate(&self, h: u64, default: u8, need: u32) -> Result<Location, u32> {
        if let Some(root) = predefined(h) {
            let key = self.find(root, "", default).ok_or(BAD_HANDLE)?;
            return Ok(Location {
                root,
                path: String::new(),
                view: default,
                key,
                rights: ALL,
            });
        }
        let h = self
            .handles
            .iter()
            .find(|v| v.value == h)
            .ok_or(BAD_HANDLE)?;
        let k = &self.keys[h.key];
        if !k.live {
            return Err(DELETED);
        }
        if h.rights & need != need {
            return Err(DENIED);
        }
        Ok(Location {
            root: k.root,
            path: k.path.clone(),
            view: h.view,
            key: h.key,
            rights: h.rights,
        })
    }
    fn handle(&mut self, key: usize, view: u8, rights: u32) -> Result<u64, u32> {
        if self.handles.len() >= MAX_HANDLES || self.next > 0x7fff_fffc {
            return Err(8);
        }
        let h = self.next;
        self.next += 4;
        self.handles.push(Handle {
            value: h,
            key,
            view,
            rights,
        });
        Ok(h)
    }
    fn children(&self, l: &Location) -> Vec<usize> {
        self.keys
            .iter()
            .enumerate()
            .filter_map(|(i, k)| {
                if !k.live || k.root != l.root || k.view != physical_view(k.root, &k.path, l.view) {
                    return None;
                }
                let (parent, _) = k.path.rsplit_once('\\').unwrap_or(("", k.path.as_str()));
                if !k.path.is_empty() && equal(parent, &l.path) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }
    fn ensure_path(&mut self, l: &Location, sub: &str, class: &str) -> Result<(usize, bool), u32> {
        let target = joined(&l.path, sub)?;
        if let Some(id) = self.find(l.root, &target, l.view) {
            return Ok((id, false));
        }
        let parts: Vec<&str> = sub.split('\\').filter(|x| !x.is_empty()).collect();
        if parts.len() > 32 {
            return Err(161);
        }
        let mut p = l.path.clone();
        let mut missing = Vec::new();
        for part in parts {
            p = joined(&p, part)?;
            if self.find(l.root, &p, l.view).is_none() {
                missing.push(p.clone());
            }
        }
        if self.keys.len() + missing.len() > MAX_KEYS
            || self.bytes + missing.iter().map(|p| p.len()).sum::<usize>() + class.len() > MAX_TOTAL
        {
            return Err(8);
        }
        for p in missing {
            self.insert(
                l.root,
                &p,
                l.view,
                if equal(&p, &target) { class } else { "" },
            );
        }
        Ok((self.find(l.root, &target, l.view).ok_or(NOT_FOUND)?, true))
    }
    fn set_value(&mut self, key: usize, name: String, kind: u32, data: Vec<u8>) -> Result<(), u32> {
        if data.len() > MAX_DATA {
            return Err(8);
        }
        let old = self.keys[key]
            .values
            .iter()
            .position(|v| equal(&v.name, &name));
        let old_size = old
            .map(|i| {
                let v = &self.keys[key].values[i];
                v.data.len() + v.name.len()
            })
            .unwrap_or(0);
        if self.bytes - old_size + data.len() + name.len() > MAX_TOTAL
            || old.is_none() && self.keys[key].values.len() >= 1024
        {
            return Err(8);
        }
        self.bytes = self.bytes - old_size + data.len() + name.len();
        let v = Value { name, kind, data };
        if let Some(i) = old {
            self.keys[key].values[i] = v;
        } else {
            self.keys[key].values.push(v);
        }
        Ok(())
    }
    fn remove(&mut self, id: usize) {
        self.bytes -= self.keys[id]
            .values
            .iter()
            .map(|v| v.data.len() + v.name.len())
            .sum::<usize>();
        self.keys[id].values.clear();
        self.keys[id].live = false;
    }
}
fn equal(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}
fn predefined(h: u64) -> Option<u8> {
    // A PE32+ HKEY pseudo-handle is sign extended. Accept the zero-extended
    // SDK constant too, but never accept arbitrary high words sharing low bits.
    if h > u32::MAX as u64 && h >> 32 != u32::MAX as u64 {
        return None;
    }
    match h as u32 {
        0x80000000 => Some(0),
        0x80000001 => Some(1),
        0x80000002 => Some(2),
        0x80000003 => Some(3),
        0x80000005 => Some(4),
        _ => None,
    }
}
fn below(p: &str, prefix: &str) -> bool {
    p == prefix || p.strip_prefix(prefix).is_some_and(|r| r.starts_with('\\'))
}
fn physical_view(root: u8, path: &str, view: u8) -> u8 {
    let p = path.to_lowercase();
    // Windows 7's most common shared/redirected boundaries. Shared children
    // remain shared even beneath the redirected HKLM\\Software parent.
    let class_path = if root == 0 {
        Some(p.as_str())
    } else if root == 1 || root == 2 {
        p.strip_prefix("software\\classes").and_then(|x| {
            if x.is_empty() {
                Some("")
            } else {
                x.strip_prefix('\\')
            }
        })
    } else {
        None
    };
    if let Some(c) = class_path {
        return if [
            "clsid",
            "directshow",
            "interface",
            "media type",
            "mediafoundation",
        ]
        .iter()
        .any(|x| below(c, x))
        {
            view
        } else {
            0
        };
    }
    if root == 2 && below(&p, "software") {
        const SHARED: &[&str] = &[
            "software\\clients",
            "software\\policies",
            "software\\registeredapplications",
            "software\\microsoft\\com3",
            "software\\microsoft\\cryptography\\calais\\current",
            "software\\microsoft\\cryptography\\calais\\readers",
            "software\\microsoft\\cryptography\\services",
            "software\\microsoft\\ctf\\systemshared",
            "software\\microsoft\\ctf\\tip",
            "software\\microsoft\\dfs",
            "software\\microsoft\\driver signing",
            "software\\microsoft\\enterprisecertificates",
            "software\\microsoft\\eventsystem",
            "software\\microsoft\\msmq",
            "software\\microsoft\\non-driver signing",
            "software\\microsoft\\notepad\\defaultfonts",
            "software\\microsoft\\ole",
            "software\\microsoft\\ras",
            "software\\microsoft\\rpc",
            "software\\microsoft\\shared tools\\msinfo",
            "software\\microsoft\\systemcertificates",
            "software\\microsoft\\termservlicensing",
            "software\\microsoft\\transactionserver",
            "software\\microsoft\\windows\\currentversion\\app paths",
            "software\\microsoft\\windows\\currentversion\\control panel\\cursors\\schemes",
            "software\\microsoft\\windows\\currentversion\\explorer\\autoplayhandlers",
            "software\\microsoft\\windows\\currentversion\\explorer\\driveicons",
            "software\\microsoft\\windows\\currentversion\\explorer\\kindmap",
            "software\\microsoft\\windows\\currentversion\\group policy",
            "software\\microsoft\\windows\\currentversion\\policies",
            "software\\microsoft\\windows\\currentversion\\previewhandlers",
            "software\\microsoft\\windows\\currentversion\\setup",
            "software\\microsoft\\windows\\currentversion\\telephony\\locations",
        ];
        if SHARED.iter().any(|x| below(&p, x)) {
            return 0;
        }
        if let Some(nt) = p.strip_prefix("software\\microsoft\\windows nt\\currentversion\\") {
            if [
                "console",
                "fontdpi",
                "fontlink",
                "fontmapper",
                "fonts",
                "fontsubstitutes",
                "gre_initialize",
                "image file execution options",
                "language pack",
                "networkcards",
                "perflib",
                "ports",
                "print",
                "profilelist",
                "time zones",
            ]
            .iter()
            .any(|x| below(nt, x))
            {
                return 0;
            }
        }
        return view;
    }
    0
}
fn requested_view(mask: u32, fallback: u8) -> Result<u8, u32> {
    match mask & 0x300 {
        0 => Ok(fallback),
        0x100 => Ok(64),
        0x200 => Ok(32),
        _ => Err(INVALID),
    }
}
fn rights(mask: u32) -> Result<u32, u32> {
    // MAXIMUM_ALLOWED and generic rights are normalized to this private model.
    if mask & !(ALL | 0x300 | 0x0200_0000 | 0xf000_0000) != 0 {
        return Err(DENIED);
    }
    let mut r = mask & ALL;
    if mask & 0x1200_0000 != 0 {
        r |= ALL
    }
    if mask & 0x8000_0000 != 0 {
        r |= 0x20019
    }
    if mask & 0x4000_0000 != 0 {
        r |= 0x20006
    }
    if mask & 0x2000_0000 != 0 {
        r |= 0x20019
    }
    Ok(r)
}
fn optional_name(image: &mut GuestImage, p: u64, wide: bool) -> Result<String, u32> {
    let s = if p == 0 {
        String::new()
    } else {
        string(&mut image.memory, p, wide)?
    };
    if s.encode_utf16().count() > 16383 {
        return Err(INVALID);
    }
    Ok(s)
}
fn path(image: &mut GuestImage, p: u64, wide: bool) -> Result<String, u32> {
    let s = if p == 0 {
        String::new()
    } else {
        string(&mut image.memory, p, wide)?
    };
    if s.starts_with('\\')
        || s.ends_with('\\')
        || s.contains("\\\\")
        || s.split('\\').any(|x| x.encode_utf16().count() > 255)
    {
        return Err(161);
    }
    Ok(s)
}
fn joined(base: &str, part: &str) -> Result<String, u32> {
    let mut p = base.to_string();
    if !part.is_empty() {
        if !p.is_empty() {
            p.push('\\')
        }
        p.push_str(part)
    }
    if p.len() > 32768 || p.split('\\').count() > 512 {
        return Err(161);
    }
    Ok(p)
}
fn check_output(image: &mut GuestImage, p: u64, n: usize) -> Result<(), u32> {
    if p == 0 || !image.memory.probe_write(p, n) {
        Err(NOACCESS)
    } else {
        Ok(())
    }
}
fn count(s: &str, wide: bool) -> Result<usize, u32> {
    if wide {
        Ok(s.encode_utf16().count())
    } else if s.is_ascii() {
        Ok(s.len())
    } else {
        Err(1113)
    }
}
fn string_kind(k: u32) -> bool {
    matches!(k, 1 | 2 | 7)
}
fn store_data(data: Vec<u8>, kind: u32, wide: bool) -> Result<Vec<u8>, u32> {
    if !string_kind(kind) {
        return Ok(data);
    }
    if wide {
        if data.len() % 2 != 0 {
            return Err(INVALID);
        }
        return Ok(data);
    }
    if !data.is_ascii() {
        return Err(1113);
    }
    Ok(data
        .into_iter()
        .flat_map(|x| (x as u16).to_le_bytes())
        .collect())
}
fn load_data(v: &Value, wide: bool) -> Result<Vec<u8>, u32> {
    if !string_kind(v.kind) || wide {
        return Ok(v.data.clone());
    }
    let mut b = Vec::with_capacity(v.data.len() / 2);
    for c in v.data.chunks_exact(2) {
        let x = u16::from_le_bytes([c[0], c[1]]);
        if x > 127 {
            return Err(1113);
        }
        b.push(x as u8)
    }
    Ok(b)
}
fn output_value(
    image: &mut GuestImage,
    v: &Value,
    wide: bool,
    typ: u64,
    data: u64,
    size: u64,
    get: bool,
    flags: u32,
) -> Result<u64, u32> {
    if data != 0 && size == 0 {
        return Err(INVALID);
    }
    let capacity = if size != 0 {
        word(&mut image.memory, size, 4)? as usize
    } else {
        0
    };
    let mut b = load_data(v, wide)?;
    let kind = v.kind;
    if get {
        let filter = flags & 0xffff;
        let bit = match kind {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            4 => 16,
            7 => 32,
            11 => 64,
            _ => 0,
        };
        if filter != 0 && filter != 0xffff && filter & bit == 0 {
            return Err(1630);
        }
        if (filter == 0x18 && b.len() != 4) || (filter == 0x48 && b.len() != 8) {
            return Err(13);
        }
        if kind == 2 && flags & 0x1000_0000 == 0 {
            // No host environment is ever substituted. Expansion is not yet
            // backed by a guest environment provider; fail explicitly.
            return Err(UNSUPPORTED);
        }
        if string_kind(kind) {
            let unit = if wide { 2 } else { 1 };
            let needed = if kind == 7 { 2 } else { 1 };
            let trailing = b
                .chunks_exact(unit)
                .rev()
                .take_while(|c| c.iter().all(|x| *x == 0))
                .count();
            for _ in trailing..needed {
                b.extend(core::iter::repeat_n(0, unit))
            }
        }
    }
    if typ != 0 {
        put(&mut image.memory, typ, kind as u64, 4)?
    }
    if size != 0 {
        put(&mut image.memory, size, b.len() as u64, 4)?
    }
    if data != 0 {
        if capacity < b.len() {
            return Err(MORE);
        }
        if !b.is_empty() && !image.memory.write_virt(data, &b) {
            return Err(NOACCESS);
        }
    }
    Ok(0)
}
fn output_name(
    image: &mut GuestImage,
    s: &str,
    wide: bool,
    p: u64,
    size: u64,
    required: bool,
) -> Result<(), u32> {
    if size == 0 {
        return if p == 0 && !required {
            Ok(())
        } else {
            Err(INVALID)
        };
    }
    let cap = word(&mut image.memory, size, 4)? as usize;
    let n = count(s, wide)?;
    put(&mut image.memory, size, n as u64, 4)?;
    if p == 0 {
        return if required { Err(INVALID) } else { Ok(()) };
    }
    if cap <= n {
        return Err(MORE);
    }
    write_string(&mut image.memory, p, s, wide)?;
    Ok(())
}
fn execute(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64, u32> {
    let wide = name.ends_with('W');
    let op = name
        .strip_suffix('W')
        .or_else(|| name.strip_suffix('A'))
        .unwrap_or(name);
    let default = if image.thread.context.mode == DecodeMode::X64 {
        64
    } else {
        32
    };
    let width = if default == 64 { 8 } else { 4 };
    let arg = |i: usize| a.get(i).copied().unwrap_or(0);
    let h = arg(0);
    match op {
        "RegCreateKey" | "RegCreateKeyEx" | "RegOpenKey" | "RegOpenKeyEx" => {
            let create = op.starts_with("RegCreate");
            let ex = op.ends_with("Ex");
            let sub = path(image, arg(1), wide)?;
            if create && arg(1) == 0 {
                return Err(INVALID);
            }
            let output = if ex {
                if create {
                    arg(7)
                } else {
                    arg(4)
                }
            } else {
                arg(2)
            };
            check_output(image, output, width)?;
            let disposition = if create && ex { arg(8) } else { 0 };
            if disposition != 0 {
                check_output(image, disposition, 4)?
            }
            let mask = if ex {
                if create {
                    arg(5) as u32
                } else {
                    arg(3) as u32
                }
            } else {
                ALL
            };
            let r = rights(mask)?;
            let mut l = image.win32.advapi.registry.locate(h, default, 0)?;
            l.view = requested_view(mask, l.view)?;
            let class = if create && ex && arg(3) != 0 {
                string(&mut image.memory, arg(3), wide)?
            } else {
                String::new()
            };
            if class.encode_utf16().count() > 255 {
                return Err(INVALID);
            }
            if ex {
                if arg(2) != 0 {
                    return Err(INVALID);
                }
                if create {
                    if arg(4) & !1 != 0 || arg(6) != 0 {
                        return Err(UNSUPPORTED);
                    }
                }
            }
            let (id, created) = if create {
                if image.win32.advapi.registry.handles.len() >= MAX_HANDLES {
                    return Err(8);
                }
                image.win32.advapi.registry.ensure_path(&l, &sub, &class)?
            } else {
                let target = joined(&l.path, &sub)?;
                (
                    image
                        .win32
                        .advapi
                        .registry
                        .find(l.root, &target, l.view)
                        .ok_or(NOT_FOUND)?,
                    false,
                )
            };
            let handle = if !create && sub.is_empty() && predefined(h).is_some() {
                h
            } else {
                image.win32.advapi.registry.handle(id, l.view, r)?
            };
            put(&mut image.memory, output, handle, width)?;
            if disposition != 0 {
                put(
                    &mut image.memory,
                    disposition,
                    if created { 1 } else { 2 },
                    4,
                )?
            }
            Ok(0)
        }
        "RegEnumKey" => {
            let l = image.win32.advapi.registry.locate(h, default, ENUM)?;
            let children = image.win32.advapi.registry.children(&l);
            let id = *children.get(arg(1) as u32 as usize).ok_or(NO_MORE)?;
            let name = image.win32.advapi.registry.keys[id]
                .path
                .rsplit('\\')
                .next()
                .unwrap_or("")
                .to_string();
            if arg(2) == 0 {
                return Err(INVALID);
            }
            if (arg(3) as u32 as usize) <= count(&name, wide)? {
                return Err(MORE);
            }
            write_string(&mut image.memory, arg(2), &name, wide)?;
            Ok(0)
        }
        "RegQueryValue" => {
            let mut l = image.win32.advapi.registry.locate(h, default, QUERY)?;
            let sub = path(image, arg(1), wide)?;
            if !sub.is_empty() {
                let target = joined(&l.path, &sub)?;
                l.key = image
                    .win32
                    .advapi
                    .registry
                    .find(l.root, &target, l.view)
                    .ok_or(NOT_FOUND)?;
            }
            let v = image.win32.advapi.registry.keys[l.key]
                .values
                .iter()
                .find(|v| v.name.is_empty())
                .cloned()
                .unwrap_or(Value {
                    name: String::new(),
                    kind: 1,
                    data: vec![0, 0],
                });
            if v.kind != 1 {
                return Err(13);
            }
            output_value(image, &v, wide, 0, arg(2), arg(3), false, 0)
        }
        "RegSetValue" => {
            let l = image.win32.advapi.registry.locate(h, default, SET)?;
            if arg(2) as u32 != 1 || arg(3) == 0 {
                return Err(INVALID);
            }
            let sub = path(image, arg(1), wide)?;
            // Legacy cbData is explicitly ignored: the NUL-terminated string
            // determines the complete data including its terminator.
            let text = string(&mut image.memory, arg(3), wide)?;
            let data: Vec<u8> = text
                .encode_utf16()
                .chain(core::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            let (key, _) = image.win32.advapi.registry.ensure_path(&l, &sub, "")?;
            image
                .win32
                .advapi
                .registry
                .set_value(key, String::new(), 1, data)?;
            Ok(0)
        }
        "RegQueryMultipleValues" => {
            let l = image.win32.advapi.registry.locate(h, default, QUERY)?;
            let number = arg(2) as u32 as usize;
            let stride = if width == 8 { 32usize } else { 16usize };
            let pointer_offset = if width == 8 { 16u64 } else { 8u64 };
            let type_offset = if width == 8 { 24u64 } else { 12u64 };
            if number == 0 || arg(1) == 0 || arg(4) == 0 {
                return Err(INVALID);
            }
            let list_bytes = number.checked_mul(stride).ok_or(8u32)?;
            if list_bytes > MAX_DATA {
                return Err(8);
            }
            // Stage names, payloads and ABI metadata before publishing anything.
            // A missing value or inaccessible output cannot partially update
            // this process-private snapshot's VALENT array or value buffer.
            check_output(image, arg(1), list_bytes)?;
            check_output(image, arg(4), 4)?;
            let capacity = word(&mut image.memory, arg(4), 4)? as usize;
            if arg(3) == 0 && capacity != 0 {
                return Err(INVALID);
            }
            let mut values = Vec::new();
            let mut total = 0usize;
            for i in 0..number {
                let entry = arg(1).checked_add((i * stride) as u64).ok_or(NOACCESS)?;
                let namep = word(&mut image.memory, entry, width)?;
                let value_name = optional_name(image, namep, wide)?;
                let value = image.win32.advapi.registry.keys[l.key]
                    .values
                    .iter()
                    .find(|v| equal(&v.name, &value_name))
                    .cloned()
                    .ok_or(NOT_FOUND)?;
                let data = load_data(&value, wide)?;
                let offset = total;
                total = total.checked_add(data.len()).ok_or(8u32)?;
                if list_bytes + total > MAX_DATA {
                    return Err(8);
                }
                values.push((entry, value.kind, offset, data));
            }
            if arg(3) == 0 || capacity < total {
                put(&mut image.memory, arg(4), total as u64, 4)?;
                return Err(MORE);
            }
            if total != 0 {
                check_output(image, arg(3), total)?;
            }
            // Ensure all output pointers fit the guest ABI before any writes.
            for (_, _, offset, _) in &values {
                let valuep = arg(3).checked_add(*offset as u64).ok_or(NOACCESS)?;
                if width == 4 && valuep > u32::MAX as u64 {
                    return Err(NOACCESS);
                }
            }
            let mut bytes = Vec::with_capacity(total);
            for (_, _, _, data) in &values {
                bytes.extend_from_slice(data);
            }
            if total != 0 && !image.memory.write_virt(arg(3), &bytes) {
                return Err(NOACCESS);
            }
            for (entry, kind, offset, data) in values {
                put(
                    &mut image.memory,
                    entry + width as u64,
                    data.len() as u64,
                    4,
                )?;
                put(
                    &mut image.memory,
                    entry + pointer_offset,
                    arg(3) + offset as u64,
                    width,
                )?;
                put(&mut image.memory, entry + type_offset, kind as u64, 4)?;
            }
            put(&mut image.memory, arg(4), total as u64, 4)?;
            Ok(0)
        }
        "RegCloseKey" => {
            if predefined(h).is_some() {
                return Ok(0);
            }
            let s = &mut image.win32.advapi.registry;
            let i = s
                .handles
                .iter()
                .position(|x| x.value == h)
                .ok_or(BAD_HANDLE)?;
            s.handles.remove(i);
            Ok(0)
        }
        "RegSetKeyValue" => {
            let l = image.win32.advapi.registry.locate(h, default, SET)?;
            let sub = path(image, arg(1), wide)?;
            let value_name = optional_name(image, arg(2), wide)?;
            let kind = arg(3) as u32;
            let n = arg(5) as u32 as usize;
            if n > MAX_DATA || arg(4) == 0 && n != 0 {
                return Err(INVALID);
            }
            // Validate and copy every input before creating any missing key.
            let data = if n == 0 {
                Vec::new()
            } else {
                read(&mut image.memory, arg(4), n)?
            };
            let data = store_data(data, kind, wide)?;
            if data.len() > MAX_DATA {
                return Err(8);
            }
            let mut staged = image.win32.advapi.registry.clone();
            let (key, _) = staged.ensure_path(&l, &sub, "")?;
            staged.set_value(key, value_name, kind, data)?;
            image.win32.advapi.registry = staged;
            Ok(0)
        }
        "RegDeleteKeyValue" => {
            let mut l = image.win32.advapi.registry.locate(h, default, SET)?;
            let sub = path(image, arg(1), wide)?;
            let value_name = optional_name(image, arg(2), wide)?;
            if !sub.is_empty() {
                let target = joined(&l.path, &sub)?;
                l.key = image
                    .win32
                    .advapi
                    .registry
                    .find(l.root, &target, l.view)
                    .ok_or(NOT_FOUND)?;
            }
            let s = &mut image.win32.advapi.registry;
            let i = s.keys[l.key]
                .values
                .iter()
                .position(|v| equal(&v.name, &value_name))
                .ok_or(NOT_FOUND)?;
            let removed = s.keys[l.key].values.remove(i);
            s.bytes -= removed.name.len() + removed.data.len();
            Ok(0)
        }
        "RegCopyTree" => {
            let mut source = image.win32.advapi.registry.locate(h, default, 0x20019)?;
            let sub = path(image, arg(1), wide)?;
            let destination = image.win32.advapi.registry.locate(arg(2), default, 4)?;
            if !sub.is_empty() {
                source.path = joined(&source.path, &sub)?;
                source.key = image
                    .win32
                    .advapi
                    .registry
                    .find(source.root, &source.path, source.view)
                    .ok_or(NOT_FOUND)?;
            }
            if source.key == destination.key {
                return Ok(0);
            }
            let s = &image.win32.advapi.registry;
            if source.root == destination.root
                && (source.path.is_empty()
                    || below(
                        &destination.path.to_lowercase(),
                        &source.path.to_lowercase(),
                    ))
                && s.find(source.root, &destination.path, source.view) == Some(destination.key)
            {
                return Err(INVALID);
            }
            // Enumerate the complete source view before mutating the destination.
            // This also makes copying into an ancestor finite and independent
            // of the new entries created while the snapshot is merged.
            let mut pending = vec![(source.clone(), String::new())];
            let mut snapshot = Vec::new();
            while let Some((location, relative)) = pending.pop() {
                let key = &s.keys[location.key];
                snapshot.push((relative.clone(), key.class.clone(), key.values.clone()));
                for id in s.children(&location) {
                    let child = &s.keys[id];
                    let component = child.path.rsplit('\\').next().unwrap_or("");
                    let rel = joined(&relative, component)?;
                    pending.push((
                        Location {
                            path: child.path.clone(),
                            key: id,
                            ..location.clone()
                        },
                        rel,
                    ));
                }
            }
            let mut staged = s.clone();
            for (relative, class, values) in snapshot {
                // Build one component at a time; a tree may exceed the 32-level
                // limit of a single RegCreateKeyEx path without being invalid.
                let mut cursor = destination.clone();
                let parts: Vec<&str> = relative.split('\\').filter(|x| !x.is_empty()).collect();
                for (index, part) in parts.iter().enumerate() {
                    let last = index + 1 == parts.len();
                    let (id, _) =
                        staged.ensure_path(&cursor, part, if last { &class } else { "" })?;
                    cursor.path = joined(&cursor.path, part)?;
                    cursor.key = id;
                }
                for value in values {
                    staged.set_value(cursor.key, value.name, value.kind, value.data)?;
                }
            }
            image.win32.advapi.registry = staged;
            Ok(0)
        }
        "RegSetValueEx" => {
            if arg(2) != 0 {
                return Err(INVALID);
            }
            let l = image.win32.advapi.registry.locate(h, default, SET)?;
            let name = optional_name(image, arg(1), wide)?;
            let n = usize::try_from(arg(5)).map_err(|_| INVALID)?;
            if n > MAX_DATA || arg(4) == 0 && n != 0 {
                return Err(INVALID);
            }
            let data = if n == 0 {
                Vec::new()
            } else {
                read(&mut image.memory, arg(4), n)?
            };
            let data = store_data(data, arg(3) as u32, wide)?;
            if data.len() > MAX_DATA {
                return Err(8);
            }
            image
                .win32
                .advapi
                .registry
                .set_value(l.key, name, arg(3) as u32, data)?;
            Ok(0)
        }
        "RegQueryValueEx" | "RegGetValue" => {
            let get = op == "RegGetValue";
            let mut l = image.win32.advapi.registry.locate(h, default, QUERY)?;
            let (namep, flags, typ, data, size) = if get {
                let flags = arg(3) as u32;
                if flags & !0x3003_ffff != 0 {
                    return Err(INVALID);
                }
                l.view = requested_view((flags >> 8) & 0x300, l.view)?;
                let sub = path(image, arg(1), wide)?;
                if !sub.is_empty() {
                    let p = joined(&l.path, &sub)?;
                    l.key = image
                        .win32
                        .advapi
                        .registry
                        .find(l.root, &p, l.view)
                        .ok_or(NOT_FOUND)?;
                }
                (arg(2), flags, arg(4), arg(5), arg(6))
            } else {
                if arg(2) != 0 {
                    return Err(INVALID);
                }
                (arg(1), 0, arg(3), arg(4), arg(5))
            };
            let name = optional_name(image, namep, wide)?;
            let v = image.win32.advapi.registry.keys[l.key]
                .values
                .iter()
                .find(|v| equal(&v.name, &name))
                .cloned()
                .ok_or(NOT_FOUND)?;
            output_value(image, &v, wide, typ, data, size, get, flags)
        }
        "RegDeleteValue" => {
            let l = image.win32.advapi.registry.locate(h, default, SET)?;
            let name = optional_name(image, arg(1), wide)?;
            let s = &mut image.win32.advapi.registry;
            let i = s.keys[l.key]
                .values
                .iter()
                .position(|v| equal(&v.name, &name))
                .ok_or(NOT_FOUND)?;
            let removed = s.keys[l.key].values.remove(i);
            s.bytes -= removed.data.len() + removed.name.len();
            Ok(0)
        }
        "RegDeleteKey" | "RegDeleteKeyEx" | "RegDeleteTree" => {
            let tree = op == "RegDeleteTree";
            let need = if tree { DELETE | ENUM | QUERY } else { 0 };
            let mut l = image.win32.advapi.registry.locate(h, default, need)?;
            if op == "RegDeleteKeyEx" {
                if arg(3) != 0 || arg(2) & !0x300 != 0 {
                    return Err(INVALID);
                }
                l.view = requested_view(arg(2) as u32, l.view)?;
            }
            let sub = path(image, arg(1), wide)?;
            if !tree && (arg(1) == 0 || sub.is_empty()) {
                return Err(INVALID);
            }
            let target = joined(&l.path, &sub)?;
            let id = image
                .win32
                .advapi
                .registry
                .find(l.root, &target, l.view)
                .ok_or(NOT_FOUND)?;
            let dest = Location {
                path: target.clone(),
                key: id,
                ..l.clone()
            };
            if tree {
                if !image.win32.advapi.registry.keys[id].values.is_empty() && l.rights & SET == 0 {
                    return Err(DENIED);
                }
                let mut pending = vec![dest];
                let mut victims = Vec::new();
                while let Some(p) = pending.pop() {
                    for i in image.win32.advapi.registry.children(&p) {
                        let k = &image.win32.advapi.registry.keys[i];
                        pending.push(Location {
                            path: k.path.clone(),
                            key: i,
                            ..p.clone()
                        });
                        victims.push(i)
                    }
                }
                let s = &mut image.win32.advapi.registry;
                for i in victims {
                    s.remove(i)
                }
                if !sub.is_empty() {
                    s.remove(id)
                } else {
                    let n = s.keys[id]
                        .values
                        .iter()
                        .map(|v| v.data.len() + v.name.len())
                        .sum::<usize>();
                    s.bytes -= n;
                    s.keys[id].values.clear()
                }
            } else {
                if !image.win32.advapi.registry.children(&dest).is_empty() {
                    return Err(DENIED);
                }
                image.win32.advapi.registry.remove(id)
            }
            Ok(0)
        }
        "RegEnumKeyEx" => {
            if arg(4) != 0 {
                return Err(INVALID);
            }
            let l = image.win32.advapi.registry.locate(h, default, ENUM)?;
            let list = image.win32.advapi.registry.children(&l);
            let id = *list
                .get(usize::try_from(arg(1)).map_err(|_| NO_MORE)?)
                .ok_or(NO_MORE)?;
            let key = &image.win32.advapi.registry.keys[id];
            let name = key.path.rsplit('\\').next().unwrap_or("").to_string();
            let class = key.class.clone();
            // Probe capacities before writing either string.
            let n = count(&name, wide)?;
            let c = count(&class, wide)?;
            if arg(3) == 0 || arg(2) == 0 || arg(5) != 0 && arg(6) == 0 {
                return Err(INVALID);
            }
            let nc = word(&mut image.memory, arg(3), 4)? as usize;
            let cc = if arg(6) != 0 {
                word(&mut image.memory, arg(6), 4)? as usize
            } else {
                0
            };
            if nc <= n || arg(5) != 0 && cc <= c {
                put(&mut image.memory, arg(3), n as u64, 4)?;
                if arg(6) != 0 {
                    put(&mut image.memory, arg(6), c as u64, 4)?
                }
                return Err(MORE);
            }
            output_name(image, &name, wide, arg(2), arg(3), true)?;
            output_name(image, &class, wide, arg(5), arg(6), false)?;
            if arg(7) != 0 {
                put(&mut image.memory, arg(7), 0, 8)?
            }
            Ok(0)
        }
        "RegEnumValue" => {
            if arg(4) != 0 {
                return Err(INVALID);
            }
            let l = image.win32.advapi.registry.locate(h, default, QUERY)?;
            let v = image.win32.advapi.registry.keys[l.key]
                .values
                .get(usize::try_from(arg(1)).map_err(|_| NO_MORE)?)
                .cloned()
                .ok_or(NO_MORE)?;
            output_name(image, &v.name, wide, arg(2), arg(3), true)?;
            output_value(image, &v, wide, arg(5), arg(6), arg(7), false, 0)
        }
        "RegQueryInfoKey" => {
            if arg(3) != 0 {
                return Err(INVALID);
            }
            let l = image.win32.advapi.registry.locate(h, default, QUERY)?;
            // No ACL provider exists; do not invent a descriptor length.
            if arg(10) != 0 {
                return Err(UNSUPPORTED);
            }
            let s = &image.win32.advapi.registry;
            let key = &s.keys[l.key];
            let children = s.children(&l);
            let stats = [
                children.len(),
                children
                    .iter()
                    .map(|i| {
                        s.keys[*i]
                            .path
                            .rsplit('\\')
                            .next()
                            .unwrap_or("")
                            .encode_utf16()
                            .count()
                    })
                    .max()
                    .unwrap_or(0),
                children
                    .iter()
                    .map(|i| s.keys[*i].class.encode_utf16().count())
                    .max()
                    .unwrap_or(0),
                key.values.len(),
                key.values
                    .iter()
                    .map(|v| v.name.encode_utf16().count())
                    .max()
                    .unwrap_or(0),
                key.values.iter().map(|v| v.data.len()).max().unwrap_or(0),
            ];
            let class = key.class.clone();
            for (i, n) in stats.iter().enumerate() {
                if arg(4 + i) != 0 {
                    put(&mut image.memory, arg(4 + i), *n as u64, 4)?
                }
            }
            if arg(11) != 0 {
                put(&mut image.memory, arg(11), 0, 8)?
            }
            output_name(image, &class, wide, arg(1), arg(2), false)?;
            Ok(0)
        }
        "RegFlushKey" => {
            image.win32.advapi.registry.locate(h, default, QUERY)?;
            Err(120)
        }
        _ => Err(120),
    }
}
/// All Reg* APIs return LSTATUS directly, including pointer/permission errors.
/// In particular an error is never propagated to the BOOL/LastError gateway.
pub fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Option<Result<u64, u32>> {
    if !name.starts_with("Reg") || name.starts_with("Register") {
        return None;
    }
    let zero = name.starts_with("RegGetValue") && a.get(3).copied().unwrap_or(0) & 0x2000_0000 != 0;
    let buffer = a.get(5).copied().unwrap_or(0);
    let capacity = if zero && buffer != 0 {
        a.get(6)
            .and_then(|p| word(&mut image.memory, *p, 4).ok())
            .filter(|n| *n <= MAX_DATA as u64)
    } else {
        None
    };
    let result = if zero && buffer != 0 {
        match a.get(6).copied().filter(|p| *p != 0) {
            Some(p) => match word(&mut image.memory, p, 4) {
                Ok(n) if n <= MAX_DATA as u64 => execute(image, name, a),
                Ok(_) => Err(INVALID),
                Err(e) => Err(e),
            },
            None => Err(INVALID),
        }
    } else {
        execute(image, name, a)
    };
    if result.is_err() && zero {
        if let Some(n) = capacity {
            let zeros = vec![0; n as usize];
            if !image.memory.write_virt(buffer, &zeros) {
                return Some(Ok(NOACCESS as u64));
            }
        }
    }
    Some(Ok(result.unwrap_or_else(|e| e as u64)))
}

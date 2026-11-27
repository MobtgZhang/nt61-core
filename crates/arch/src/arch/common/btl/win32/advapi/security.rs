//! Checked guest SID/ACL/security descriptor manipulation, not authorization.
//! Contracts consulted 2026-11-14:
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-allocateandinitializesid
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-equalprefixsid
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-initializesid
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-initializeacl
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-addace
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-addaccessallowedaceex
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-makeselfrelativesd
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsecuritydescriptorlength
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsecuritydescriptordacl
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-setaclinformation
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createwellknownsid
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-iswellknownsid
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getwindowsaccountdomainsid
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-equaldomainsid
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-getsecuritydescriptorrmcontrol
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-setsecuritydescriptorrmcontrol
//! https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-well_known_sid_type
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-mapgenericmask
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-areallaccessesgranted
//! https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-areanyaccessesgranted
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-lookupprivilegevaluew
//! https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-lookupprivilegenamew
//! https://learn.microsoft.com/en-us/windows/win32/secauthz/privilege-constants
//! https://learn.microsoft.com/en-us/windows/win32/api/sddl/nf-sddl-convertstringsidtosidw
//! Guest objects are decoded as bytes, never cast. No token/account services or
//! access checks are synthesized. SDDL domain-relative aliases are unsupported.
//! Original SP1 x64 evidence: ntdll RtlSetInformationAcl RVA 0xa9580,
//! RtlGet/SetSecurityDescriptorRMControl 0xa9920/0xa9880; KernelBase
//! GetWindowsAccountDomainSid 0x31490, CreateWellKnownSid 0x9af0 with its
//! identifier-authority/type tables at 0x507e0. The shipped table supports
//! types 79, 81 and 82 despite the current docs' Windows 7 availability note.
//! Original ntdll mask arithmetic: RtlMapGenericMask 0xf530 (ordered read,
//! write, execute, all), RtlAreAllAccessesGranted 0xf590, and
//! RtlAreAnyAccessesGranted 0xa9940. LUID 36 is a later Windows privilege.
use super::super::{heap_allocate, heap_free, put, read, string, word, write_string};
use crate::arch::common::btl::{ir::DecodeMode, memory::GuestMemory, pe::GuestImage};
use alloc::{format, string::String, vec, vec::Vec};

pub struct State {
    allocated_sids: Vec<u64>,
}
impl State {
    pub fn new() -> Self {
        Self {
            allocated_sids: Vec::new(),
        }
    }
}
const PARAM: u32 = 87;
const SID: u32 = 1337;
const ACL: u32 = 1336;
const SD: u32 = 1338;
// NT 6.1 well-known LUID LowParts 2..=35, HighPart zero. These names identify
// privileges; resolving a name neither assigns nor enables that privilege.
const PRIVILEGES: [&str; 34] = [
    "SeCreateTokenPrivilege",
    "SeAssignPrimaryTokenPrivilege",
    "SeLockMemoryPrivilege",
    "SeIncreaseQuotaPrivilege",
    "SeMachineAccountPrivilege",
    "SeTcbPrivilege",
    "SeSecurityPrivilege",
    "SeTakeOwnershipPrivilege",
    "SeLoadDriverPrivilege",
    "SeSystemProfilePrivilege",
    "SeSystemtimePrivilege",
    "SeProfileSingleProcessPrivilege",
    "SeIncreaseBasePriorityPrivilege",
    "SeCreatePagefilePrivilege",
    "SeCreatePermanentPrivilege",
    "SeBackupPrivilege",
    "SeRestorePrivilege",
    "SeShutdownPrivilege",
    "SeDebugPrivilege",
    "SeAuditPrivilege",
    "SeSystemEnvironmentPrivilege",
    "SeChangeNotifyPrivilege",
    "SeRemoteShutdownPrivilege",
    "SeUndockPrivilege",
    "SeSyncAgentPrivilege",
    "SeEnableDelegationPrivilege",
    "SeManageVolumePrivilege",
    "SeImpersonatePrivilege",
    "SeCreateGlobalPrivilege",
    "SeTrustedCredManAccessPrivilege",
    "SeRelabelPrivilege",
    "SeIncreaseWorkingSetPrivilege",
    "SeTimeZonePrivilege",
    "SeCreateSymbolicLinkPrivilege",
];
fn local_privileges(image: &mut GuestImage, p: u64, wide: bool) -> Result<(), u32> {
    if p != 0 {
        let system = string(&mut image.memory, p, wide)?;
        if !system.is_empty() && system != "." && system != "\\\\." {
            // No remote LSA/RPC backend exists for the isolated process.
            return Err(1722);
        }
    }
    Ok(())
}
fn addr(p: u64, offset: usize) -> Result<u64, u32> {
    p.checked_add(offset as u64).ok_or(998)
}
fn write(m: &mut dyn GuestMemory, p: u64, b: &[u8]) -> Result<(), u32> {
    if p == 0 || !m.write_virt(p, b) {
        Err(998)
    } else {
        Ok(())
    }
}
fn u16le(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes([b[n], b[n + 1]])
}
fn u32le(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes([b[n], b[n + 1], b[n + 2], b[n + 3]])
}
fn ptr_size(image: &GuestImage) -> usize {
    if image.thread.context.mode == DecodeMode::X86 {
        4
    } else {
        8
    }
}
fn sid(m: &mut dyn GuestMemory, p: u64) -> Result<Vec<u8>, u32> {
    if p == 0 {
        return Err(SID);
    }
    let h = read(m, p, 8)?;
    if h[0] != 1 || h[1] > 15 {
        return Err(SID);
    }
    read(m, p, 8 + usize::from(h[1]) * 4)
}
fn sid_text(b: &[u8]) -> String {
    let authority = b[2..8].iter().fold(0u64, |v, &n| (v << 8) | u64::from(n));
    let mut s = if authority > u64::from(u32::MAX) {
        format!("S-1-0x{authority:012x}")
    } else {
        format!("S-1-{authority}")
    };
    for n in b[8..].chunks_exact(4) {
        s.push_str(&format!("-{}", u32le(n, 0)));
    }
    s
}
fn parse_sid(s: &str) -> Result<Vec<u8>, u32> {
    // Domain-relative aliases need an actual account/domain database and are
    // deliberately rejected. Only fixed, well-known SID aliases are resolved.
    let s = match s {
        "WD" => "S-1-1-0",
        "CO" => "S-1-3-0",
        "CG" => "S-1-3-1",
        "NU" => "S-1-5-2",
        "IU" => "S-1-5-4",
        "SU" => "S-1-5-6",
        "AN" => "S-1-5-7",
        "ED" => "S-1-5-9",
        "PS" => "S-1-5-10",
        "AU" => "S-1-5-11",
        "RC" => "S-1-5-12",
        "SY" => "S-1-5-18",
        "LS" => "S-1-5-19",
        "NS" => "S-1-5-20",
        "BA" => "S-1-5-32-544",
        "BU" => "S-1-5-32-545",
        "BG" => "S-1-5-32-546",
        "PU" => "S-1-5-32-547",
        "AO" => "S-1-5-32-548",
        "SO" => "S-1-5-32-549",
        "PO" => "S-1-5-32-550",
        "BO" => "S-1-5-32-551",
        "RE" => "S-1-5-32-552",
        "RU" => "S-1-5-32-554",
        "RD" => "S-1-5-32-555",
        "NO" => "S-1-5-32-556",
        _ => s,
    };
    fn number(s: &str) -> Option<u64> {
        if s.is_empty() {
            None
        } else if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u64::from_str_radix(h, 16).ok()
        } else if s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    }
    let mut parts = s.split('-');
    if parts.next() != Some("S") || parts.next() != Some("1") {
        return Err(SID);
    }
    let authority = number(parts.next().ok_or(SID)?)
        .filter(|&n| n <= 0xffff_ffff_ffff)
        .ok_or(SID)?;
    let mut b = vec![1, 0];
    b.extend_from_slice(&authority.to_be_bytes()[2..]);
    for part in parts {
        let n = number(part)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(SID)?;
        if b[1] == 15 {
            return Err(SID);
        }
        b[1] += 1;
        b.extend_from_slice(&n.to_le_bytes());
    }
    Ok(b)
}

fn binary_sid(authority: u8, sub: &[u32]) -> Vec<u8> {
    let mut b = vec![1, sub.len() as u8, 0, 0, 0, 0, 0, authority];
    for value in sub {
        b.extend_from_slice(&value.to_le_bytes());
    }
    b
}
fn account_rid(kind: u64) -> Option<u32> {
    Some(match kind {
        38..=40 => 500 + (kind - 38) as u32,
        41..=49 => 512 + (kind - 41) as u32,
        50 => 553,
        72 => 571,
        73 => 572,
        75 => 521,
        77 => 498,
        _ => return None,
    })
}
fn domain_sid(b: &[u8], builtin: bool) -> Result<Vec<u8>, u32> {
    let error = if builtin { 1258 } else { 1257 };
    if b[2..8] != [0, 0, 0, 0, 0, 5] || b[1] == 0 {
        return Err(error); // ERROR_NON_DOMAIN_SID / ERROR_NON_ACCOUNT_SID
    }
    let (count, n) = match u32le(b, 8) {
        21 if b[1] >= 4 => (4, 24),
        32 if builtin => (1, 12),
        _ => return Err(error),
    };
    let mut domain = b[..n].to_vec();
    domain[1] = count;
    Ok(domain)
}
fn well_known_sid(kind: u64, domain: Option<&[u8]>) -> Result<Vec<u8>, u32> {
    if let Some(rid) = account_rid(kind) {
        // A local machine/domain SID cannot be inferred from the guest process.
        // Require an explicit account-domain SID rather than inventing one.
        let domain = domain.ok_or(PARAM)?;
        // Like the shipped routine, append the RID to a structurally valid
        // supplied SID; its count need not be the usual four-domain layout.
        if domain[1] == 15 {
            return Err(PARAM);
        }
        let mut b = domain.to_vec();
        b[1] += 1;
        b.extend_from_slice(&rid.to_le_bytes());
        return Ok(b);
    }
    let (authority, sub): (u8, Vec<u32>) = match kind {
        0 => (0, vec![0]),
        1 => (1, vec![0]),
        2 => (2, vec![0]),
        3..=6 => (3, vec![(kind - 3) as u32]),
        7 => (5, vec![]),
        8 => (5, vec![1]),
        9..=11 => (5, vec![(kind - 7) as u32]),
        12..=20 => (5, vec![(kind - 6) as u32]),
        22..=24 => (5, vec![(kind - 4) as u32]),
        25 => (5, vec![32]),
        26..=34 => (5, vec![32, 544 + (kind - 26) as u32]),
        35..=37 => (5, vec![32, 554 + (kind - 35) as u32]),
        51 => (5, vec![64, 10]),
        52 => (5, vec![64, 21]),
        53 => (5, vec![64, 14]),
        54 => (5, vec![15]),
        55 => (5, vec![1000]),
        56 => (5, vec![32, 557]),
        57 => (5, vec![32, 558]),
        58 => (5, vec![32, 559]),
        59 => (5, vec![32, 560]),
        60 => (5, vec![32, 561]),
        61 => (5, vec![32, 562]),
        62 => (5, vec![32, 568]),
        63 => (5, vec![17]),
        64 => (5, vec![32, 569]),
        65..=69 => (16, vec![(kind - 65) as u32 * 0x1000]),
        70 => (5, vec![33]),
        71 => (3, vec![4]),
        74 => (5, vec![22]),
        76 => (5, vec![32, 573]),
        78 => (5, vec![32, 574]),
        79 => (16, vec![0x2100]),
        81 => (2, vec![1]),
        82 => (5, vec![65, 1]),
        // WinLogonIdsSid contains a per-logon LUID and cannot be created.
        // Type 80 is absent from the original table; package types start at 83.
        _ => return Err(PARAM),
    };
    Ok(binary_sid(authority, &sub))
}
fn sid_output(image: &mut GuestImage, p: u64, size: u64, b: &[u8]) -> Result<u64, u32> {
    if size == 0 {
        return Err(PARAM);
    }
    let capacity = word(&mut image.memory, size, 4)?;
    put(&mut image.memory, size, b.len() as u64, 4)?;
    if capacity < b.len() as u64 {
        return Err(122);
    }
    if p == 0 {
        return Err(PARAM);
    }
    write(&mut image.memory, p, b)?;
    Ok(1)
}

struct Acl {
    bytes: Vec<u8>,
    offsets: Vec<usize>,
    used: usize,
}
fn ace_list(bytes: &[u8], revision: u8) -> Result<Vec<usize>, u32> {
    let mut offsets = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes.len() - at < 4 {
            return Err(ACL);
        }
        let n = usize::from(u16le(bytes, at + 2));
        if n < 4 || n % 4 != 0 || n > bytes.len() - at {
            return Err(ACL);
        }
        let kind = bytes[at];
        // NT 6.1 ACE layouts. Object and callback ACEs can be copied without
        // interpreting access policy, but their embedded SID must still fit.
        let callback = matches!(kind, 9..=16);
        let sid_offset = match kind {
            0..=3 | 9 | 10 | 13 | 14 | 17 => 8,
            4 => {
                if n < 12 || u16le(bytes, at + 8) != 1 {
                    return Err(ACL);
                }
                12
            }
            5..=8 | 11 | 12 | 15 | 16 => {
                if revision != 4 || n < 12 {
                    return Err(ACL);
                }
                let flags = u32le(bytes, at + 8);
                if flags & !3 != 0 {
                    return Err(ACL);
                }
                12 + if flags & 1 != 0 { 16 } else { 0 } + if flags & 2 != 0 { 16 } else { 0 }
            }
            _ => return Err(ACL),
        };
        if n < sid_offset + 8 || bytes[at + sid_offset] != 1 || bytes[at + sid_offset + 1] > 15 {
            return Err(ACL);
        }
        let sid_end = sid_offset + 8 + usize::from(bytes[at + sid_offset + 1]) * 4;
        if sid_end > n || (!callback && sid_end != n) {
            return Err(ACL);
        }
        offsets.push(at);
        at += n;
    }
    Ok(offsets)
}
fn acl(m: &mut dyn GuestMemory, p: u64) -> Result<Acl, u32> {
    if p == 0 {
        return Err(ACL);
    }
    let h = read(m, p, 8)?;
    if !matches!(h[0], 2..=4) || u16le(&h, 2) < 8 || u16le(&h, 2) % 4 != 0 {
        return Err(ACL);
    }
    let b = read(m, p, usize::from(u16le(&h, 2)))?;
    let mut at = 8;
    let mut offsets = Vec::new();
    for _ in 0..u16le(&h, 4) {
        if at + 4 > b.len() {
            return Err(ACL);
        }
        let n = usize::from(u16le(&b, at + 2));
        if n < 4 || n > b.len() - at {
            return Err(ACL);
        }
        ace_list(&b[at..at + n], h[0])?;
        offsets.push(at);
        at += n;
    }
    Ok(Acl {
        bytes: b,
        offsets,
        used: at,
    })
}
fn add_aces(
    m: &mut dyn GuestMemory,
    p: u64,
    rev: u64,
    index: u64,
    list: &[u8],
) -> Result<u64, u32> {
    if !matches!(rev, 2..=4) {
        return Err(1305);
    }
    let mut acl = acl(m, p)?;
    let count = ace_list(list, rev as u8)?.len();
    let index = if index == u32::MAX as u64 {
        acl.offsets.len()
    } else {
        usize::try_from(index).map_err(|_| PARAM)?
    };
    if index > acl.offsets.len() {
        return Err(PARAM);
    }
    let new_count = acl
        .offsets
        .len()
        .checked_add(count)
        .filter(|&n| n <= u16::MAX as usize)
        .ok_or(122u32)?;
    if list.len() > acl.bytes.len() - acl.used {
        return Err(122);
    }
    let at = acl.offsets.get(index).copied().unwrap_or(acl.used);
    acl.bytes.copy_within(at..acl.used, at + list.len());
    acl.bytes[at..at + list.len()].copy_from_slice(list);
    acl.bytes[4..6].copy_from_slice(&(new_count as u16).to_le_bytes());
    acl.bytes[0] = acl.bytes[0].max(rev as u8);
    write(m, p, &acl.bytes)?;
    Ok(1)
}

#[derive(Clone, Copy)]
enum ObjectAceKind { Allowed, Denied, Audit, Mandatory }

/// Append the SP1 object ACE, or its ordinary ACE fallback.
/// Native references: scripts/fixtures/sp1-{object,denied-object,audit-object,mandatory}-ace.tsv.
/// This only edits ACL bytes; it does not authorize the caller or emit audits.
fn add_object_ace(m: &mut dyn GuestMemory, a: &[u64], kind: ObjectAceKind) -> Result<u64, u32> {
    let s = sid(m, a[6])?;
    let mandatory = matches!(kind, ObjectAceKind::Mandatory);
    // SP1 checks SID validity and the complete authority before ACL revision.
    // It accepts any valid subauthority count and RID for authority 16.
    if mandatory && s[2..8] != [0, 0, 0, 0, 0, 16] { return Err(PARAM); }
    let mut h = read(m, a[0], 8)?;
    let object = a[4] != 0 || a[5] != 0;
    if h[0] > 4 || a[1] > 4 || (object && a[1] != 4) {
        return Err(1306); // ERROR_REVISION_MISMATCH, not UNKNOWN_REVISION.
    }
    let valid_flags = if matches!(kind, ObjectAceKind::Audit) { 0xdf } else { 0x1f };
    if a[2] & !valid_flags != 0 || (mandatory && a[3] & !7 != 0) {
        return Err(if mandatory { PARAM } else { 1004 });
    }
    if h[0] < 2 || u16le(&h, 2) < 8 || a[0] & 1 != 0 {
        return Err(ACL);
    }
    let capacity = usize::from(u16le(&h, 2));
    let count = u16le(&h, 4);
    let mut at = 8usize;
    // Original NTDLL RtlValidAcl (RVA 0x29600, 0x6317b, 0x63237):
    // capacity need not be aligned; ordinary/object ACEs allow trailing bytes;
    // types >=9 only need a bounded header. The generic ace_list parser has a
    // different contract and must not reject those SP1 append inputs.
    for _ in 0..count {
        if at + 4 >= capacity || addr(a[0], at)? & 1 != 0 {
            return Err(ACL);
        }
        let ace = read(m, addr(a[0], at)?, 4)?;
        let size = usize::from(u16le(&ace, 2));
        if size < 4 || size > capacity - at {
            return Err(ACL);
        }
        if ace[0] <= 8 {
            let bytes = read(m, addr(a[0], at)?, size)?;
            let sid_at = match ace[0] {
                0..=3 => 8,
                4 => 12,
                _ => {
                    if h[0] < 4 || size < 12 { return Err(ACL); }
                    let flags = u32le(&bytes, 8);
                    12 + if flags & 1 != 0 { 16 } else { 0 }
                        + if flags & 2 != 0 { 16 } else { 0 }
                }
            };
            let minimum = match ace[0] { 0..=3 => 16, 4 => 24, _ => sid_at + 12 };
            if size % 4 != 0 || size < minimum || (ace[0] == 4 &&
                (h[0] < 3 || u16le(&bytes, 8) != 1)) {
                return Err(ACL);
            }
            let s = &bytes[sid_at..];
            if s[0] != 1 || s[1] > 15 || 8 + usize::from(s[1]) * 4 > s.len() {
                return Err(ACL);
            }
            if ace[0] == 4 {
                let next = sid_at + 8 + usize::from(s[1]) * 4;
                if next + 12 > size { return Err(ACL); }
                let s = &bytes[next..];
                if s[0] != 1 || s[1] > 15 || 8 + usize::from(s[1]) * 4 > s.len() {
                    return Err(ACL);
                }
            }
        }
        at += size;
    }
    let (ordinary, object_type) = match kind {
        ObjectAceKind::Allowed => (0, 5),
        ObjectAceKind::Denied => (1, 6),
        ObjectAceKind::Audit => (2, 7),
        ObjectAceKind::Mandatory => (17, 17),
    };
    let mut flags = a[2] as u8;
    if matches!(kind, ObjectAceKind::Audit) {
        // SP1 KernelBase narrows the BOOLs to RTL BOOLEAN before the call:
        // 2 enables auditing while 256 does not (native fixtures, all ABIs).
        if a[7] as u8 != 0 { flags |= 0x40; }
        if a[8] as u8 != 0 { flags |= 0x80; }
    }
    let mut b = vec![if object { object_type } else { ordinary }, flags, 0, 0];
    b.extend_from_slice(&(a[3] as u32).to_le_bytes());
    if object {
        let flags = u32::from(a[4] != 0) | (u32::from(a[5] != 0) << 1);
        b.extend_from_slice(&flags.to_le_bytes());
        for p in [a[4], a[5]] {
            if p != 0 {
                b.extend_from_slice(&read(m, p, 16)?);
            }
        }
    }
    b.extend_from_slice(&s);
    let size = b.len() as u16; // bounded by two GUIDs and a validated SID.
    b[2..4].copy_from_slice(&size.to_le_bytes());
    if b.len() > capacity - at || count == u16::MAX {
        return Err(1344); // ERROR_ALLOTTED_SPACE_EXCEEDED.
    }
    // Read the complete destination before any mutation. GuestMemory checks
    // page protections for the single final write; untouched bytes survive.
    let mut bytes = read(m, a[0], at + b.len())?;
    h[0] = h[0].max(a[1] as u8);
    h[4..6].copy_from_slice(&(count + 1).to_le_bytes());
    bytes[..8].copy_from_slice(&h);
    bytes[at..].copy_from_slice(&b);
    if !m.probe_write(a[0], bytes.len()) { return Err(998); }
    write(m, a[0], &bytes)?;
    Ok(1)
}

pub(super) struct Descriptor {
    pub(super) control: u16,
    pub(super) rm: u8,
    pub(super) pointers: [u64; 4],
    pub(super) blobs: [Vec<u8>; 4],
    pub(super) relative: bool,
}
pub(super) fn descriptor(image: &mut GuestImage, p: u64) -> Result<Descriptor, u32> {
    if p == 0 {
        return Err(SD);
    }
    let h = read(&mut image.memory, p, 4)?;
    if h[0] != 1 {
        return Err(SD);
    }
    let control = u16le(&h, 2);
    let relative = control & 0x8000 != 0;
    let width = if relative { 4 } else { ptr_size(image) };
    let first = if width == 8 { 8 } else { 4 };
    // Validate the complete fixed-size header, including alignment padding.
    read(&mut image.memory, p, first + 4 * width)?;
    let mut pointers = [0; 4];
    let mut blobs: [Vec<u8>; 4] = core::array::from_fn(|_| Vec::new());
    for i in 0..4 {
        let value = word(&mut image.memory, addr(p, first + i * width)?, width)?;
        let present = i < 2 || control & if i == 2 { 0x10 } else { 4 } != 0;
        if !present || value == 0 {
            continue;
        }
        if relative && (value < 20 || value % 4 != 0) {
            return Err(SD);
        }
        pointers[i] = if relative {
            p.checked_add(value).ok_or(SD)?
        } else {
            value
        };
        blobs[i] = if i < 2 {
            sid(&mut image.memory, pointers[i])?
        } else {
            acl(&mut image.memory, pointers[i])?.bytes
        };
    }
    Ok(Descriptor {
        control,
        rm: h[1],
        pointers,
        blobs,
        relative,
    })
}
pub(super) fn relative_sd(d: &Descriptor) -> Vec<u8> {
    let mut b = vec![1, d.rm, 0, 0];
    b[2..4].copy_from_slice(&(d.control | 0x8000).to_le_bytes());
    b.resize(20, 0);
    for i in 0..4 {
        if d.blobs[i].is_empty() {
            continue;
        }
        let offset = b.len() as u32;
        b[4 + i * 4..8 + i * 4].copy_from_slice(&offset.to_le_bytes());
        b.extend_from_slice(&d.blobs[i]);
        while b.len() % 4 != 0 {
            b.push(0);
        }
    }
    b
}
fn set_component(image: &mut GuestImage, a: &[u64], i: usize) -> Result<u64, u32> {
    let d = descriptor(image, a[0])?;
    if d.relative {
        return Err(SD);
    }
    let (value, defaulted, present) = if i < 2 {
        (a[1], a[2] != 0, true)
    } else {
        (a[2], a[3] != 0, a[1] != 0)
    };
    if present && value != 0 {
        if i < 2 {
            sid(&mut image.memory, value)?;
        } else {
            acl(&mut image.memory, value)?;
        }
    }
    let mut control = d.control;
    let default_bit = [1, 2, 0x20, 8][i];
    if defaulted {
        control |= default_bit;
    } else {
        control &= !default_bit;
    }
    if i >= 2 {
        let bit = if i == 2 { 0x10 } else { 4 };
        if present {
            control |= bit;
        } else {
            control &= !bit;
        }
    }
    let width = ptr_size(image);
    let first = if width == 8 { 8 } else { 4 };
    let mut h = read(&mut image.memory, a[0], first + 4 * width)?;
    h[2..4].copy_from_slice(&control.to_le_bytes());
    h[first + i * width..first + (i + 1) * width].copy_from_slice(&value.to_le_bytes()[..width]);
    write(&mut image.memory, a[0], &h)?;
    Ok(1)
}

pub fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Option<Result<u64, u32>> {
    const NAMES: &[&str] = &[
        "MapGenericMask",
        "AreAllAccessesGranted",
        "AreAnyAccessesGranted",
        "LookupPrivilegeValueA",
        "LookupPrivilegeValueW",
        "LookupPrivilegeNameA",
        "LookupPrivilegeNameW",
        "IsValidSid",
        "GetLengthSid",
        "GetSidLengthRequired",
        "InitializeSid",
        "GetSidIdentifierAuthority",
        "GetSidSubAuthority",
        "GetSidSubAuthorityCount",
        "CopySid",
        "EqualSid",
        "EqualPrefixSid",
        "CreateWellKnownSid",
        "IsWellKnownSid",
        "GetWindowsAccountDomainSid",
        "EqualDomainSid",
        "AllocateAndInitializeSid",
        "FreeSid",
        "ConvertSidToStringSidA",
        "ConvertSidToStringSidW",
        "ConvertStringSidToSidA",
        "ConvertStringSidToSidW",
        "InitializeAcl",
        "IsValidAcl",
        "GetAclInformation",
        "SetAclInformation",
        "GetAce",
        "AddAce",
        "DeleteAce",
        "AddAccessAllowedAce",
        "AddAccessAllowedAceEx",
        "AddAccessAllowedObjectAce",
        "AddAccessDeniedObjectAce",
        "AddAuditAccessObjectAce",
        "AddMandatoryAce",
        "AddAccessDeniedAce",
        "AddAccessDeniedAceEx",
        "AddAuditAccessAce",
        "AddAuditAccessAceEx",
        "InitializeSecurityDescriptor",
        "IsValidSecurityDescriptor",
        "GetSecurityDescriptorLength",
        "GetSecurityDescriptorControl",
        "SetSecurityDescriptorControl",
        "GetSecurityDescriptorRMControl",
        "SetSecurityDescriptorRMControl",
        "GetSecurityDescriptorOwner",
        "GetSecurityDescriptorGroup",
        "GetSecurityDescriptorDacl",
        "GetSecurityDescriptorSacl",
        "SetSecurityDescriptorOwner",
        "SetSecurityDescriptorGroup",
        "SetSecurityDescriptorDacl",
        "SetSecurityDescriptorSacl",
        "MakeSelfRelativeSD",
    ];
    if !NAMES.contains(&name) {
        return None;
    }
    let argc = match name {
        "IsValidSid"
        | "GetLengthSid"
        | "GetSidLengthRequired"
        | "GetSidIdentifierAuthority"
        | "GetSidSubAuthorityCount"
        | "FreeSid"
        | "IsValidAcl"
        | "IsValidSecurityDescriptor"
        | "GetSecurityDescriptorLength" => 1,
        "GetSidSubAuthority"
        | "MapGenericMask"
        | "AreAllAccessesGranted"
        | "AreAnyAccessesGranted"
        | "EqualSid"
        | "EqualPrefixSid"
        | "IsWellKnownSid"
        | "GetSecurityDescriptorRMControl"
        | "SetSecurityDescriptorRMControl"
        | "ConvertSidToStringSidA"
        | "ConvertSidToStringSidW"
        | "ConvertStringSidToSidA"
        | "ConvertStringSidToSidW"
        | "DeleteAce"
        | "InitializeSecurityDescriptor" => 2,
        "AllocateAndInitializeSid" => 11,
        "GetAclInformation"
        | "LookupPrivilegeNameA"
        | "LookupPrivilegeNameW"
        | "SetAclInformation"
        | "CreateWellKnownSid"
        | "SetSecurityDescriptorDacl"
        | "SetSecurityDescriptorSacl"
        | "GetSecurityDescriptorDacl"
        | "GetSecurityDescriptorSacl"
        | "AddAccessAllowedAce"
        | "AddAccessDeniedAce" => 4,
        "AddAce" | "AddAccessAllowedAceEx" | "AddAccessDeniedAceEx" => 5,
        "AddAuditAccessAce" => 6,
        "AddAuditAccessObjectAce" => 9,
        "AddMandatoryAce" => 5,
        "AddAuditAccessAceEx" | "AddAccessAllowedObjectAce" | "AddAccessDeniedObjectAce" => 7,
        _ => 3,
    };
    let rm_control = matches!(
        name,
        "GetSecurityDescriptorRMControl" | "SetSecurityDescriptorRMControl"
    );
    if a.len() < argc {
        return Some(if rm_control {
            Ok(u64::from(PARAM))
        } else {
            Err(PARAM)
        });
    }
    if rm_control {
        // These APIs return a DWORD status directly; they do not return BOOL
        // and do not use the gateway's BOOL/GetLastError error conversion.
        let result: Result<u64, u32> = (|| {
            let d = descriptor(image, a[0])?;
            if name == "GetSecurityDescriptorRMControl" {
                if d.control & 0x4000 == 0 {
                    put(&mut image.memory, a[1], 0, 1)?;
                    return Ok(13); // ERROR_INVALID_DATA, as the shipped RTL routine.
                }
                put(&mut image.memory, a[1], u64::from(d.rm), 1)?;
            } else {
                let mut h = read(&mut image.memory, a[0], 4)?;
                let c = if a[1] == 0 {
                    h[1] = 0;
                    d.control & !0x4000
                } else {
                    h[1] = word(&mut image.memory, a[1], 1)? as u8;
                    d.control | 0x4000
                };
                h[2..4].copy_from_slice(&c.to_le_bytes());
                write(&mut image.memory, a[0], &h)?;
            }
            Ok(0)
        })();
        return Some(Ok(result.unwrap_or_else(u64::from)));
    }
    Some((|| {
        let width = ptr_size(image);
        match name {
            "MapGenericMask" => {
                let mut mask = word(&mut image.memory, a[0], 4)? as u32;
                let mapping = read(&mut image.memory, a[1], 16)?;
                // Apply in the shipped RtlMapGenericMask order; a mapping may
                // introduce a later generic bit, which is then mapped too.
                for (index, bit) in [0x8000_0000, 0x4000_0000, 0x2000_0000, 0x1000_0000]
                    .iter()
                    .enumerate()
                {
                    if mask & bit != 0 {
                        mask |= u32le(&mapping, index * 4);
                    }
                }
                put(&mut image.memory, a[0], u64::from(mask & 0x0fff_ffff), 4)?;
                Ok(0) // VOID
            }
            "AreAllAccessesGranted" => {
                let granted = a[0] as u32;
                let desired = a[1] as u32;
                Ok(u64::from(granted & desired == desired))
            }
            "AreAnyAccessesGranted" => Ok(u64::from((a[0] as u32) & (a[1] as u32) != 0)),
            "LookupPrivilegeValueA" | "LookupPrivilegeValueW" => {
                let wide = name.ends_with('W');
                local_privileges(image, a[0], wide)?;
                if a[1] == 0 {
                    return Err(1313); // ERROR_NO_SUCH_PRIVILEGE
                }
                let name = string(&mut image.memory, a[1], wide)?;
                let index = PRIVILEGES
                    .iter()
                    .position(|s| s.eq_ignore_ascii_case(&name))
                    .ok_or(1313u32)?;
                write(&mut image.memory, a[2], &(index as u64 + 2).to_le_bytes())?;
                Ok(1)
            }
            "LookupPrivilegeNameA" | "LookupPrivilegeNameW" => {
                let wide = name.ends_with('W');
                local_privileges(image, a[0], wide)?;
                let luid = word(&mut image.memory, a[1], 8)?;
                if !(2..=35).contains(&luid) {
                    return Err(1313);
                }
                let privilege = PRIVILEGES[(luid - 2) as usize];
                let capacity = word(&mut image.memory, a[3], 4)? as u32;
                let required = privilege.len() as u32 + 1;
                if a[2] == 0 || capacity < required {
                    put(&mut image.memory, a[3], u64::from(required), 4)?;
                    return Err(122);
                }
                write_string(&mut image.memory, a[2], privilege, wide)?;
                put(&mut image.memory, a[3], u64::from(required - 1), 4)?;
                Ok(1)
            }
            "IsValidSid" => Ok(u64::from(sid(&mut image.memory, a[0]).is_ok())),
            "GetLengthSid" => Ok(sid(&mut image.memory, a[0])?.len() as u64),
            "GetSidLengthRequired" => Ok(8 + 4 * u64::from(a[0] as u8)),
            "InitializeSid" => {
                let count = a[2] as u8;
                if count > 15 {
                    return Err(SID);
                }
                let authority = read(&mut image.memory, a[1], 6)?;
                // InitializeSid initializes the header, not subauthority data.
                let mut b = vec![1, count];
                b.extend_from_slice(&authority);
                write(&mut image.memory, a[0], &b)?;
                Ok(1)
            }
            "GetSidIdentifierAuthority" | "GetSidSubAuthority" | "GetSidSubAuthorityCount" => {
                let b = sid(&mut image.memory, a[0])?;
                let offset = match name {
                    "GetSidIdentifierAuthority" => 2,
                    "GetSidSubAuthorityCount" => 1,
                    _ => {
                        if a[1] >= u64::from(b[1]) {
                            return Err(PARAM);
                        }
                        8 + a[1] as usize * 4
                    }
                };
                addr(a[0], offset)
            }
            "CopySid" => {
                let b = sid(&mut image.memory, a[2])?;
                if a[0] < b.len() as u64 {
                    return Err(122);
                }
                write(&mut image.memory, a[1], &b)?;
                Ok(1)
            }
            "EqualSid" | "EqualPrefixSid" => {
                let x = sid(&mut image.memory, a[0])?;
                let y = sid(&mut image.memory, a[1])?;
                let n = if name == "EqualPrefixSid" {
                    x.len().saturating_sub(4).max(8)
                } else {
                    x.len()
                };
                Ok(u64::from(x.len() == y.len() && x[..n] == y[..n]))
            }
            "CreateWellKnownSid" => {
                let domain = if a[1] != 0 {
                    Some(
                        sid(&mut image.memory, a[1])
                            .map_err(|e| if e == SID { PARAM } else { e })?,
                    )
                } else {
                    None
                };
                let b = well_known_sid(a[0], domain.as_deref())?;
                sid_output(image, a[2], a[3], &b)
            }
            "IsWellKnownSid" => {
                let b = sid(&mut image.memory, a[0])?;
                let matches = if a[1] == 21 {
                    b[1] == 3 && b[2..8] == [0, 0, 0, 0, 0, 5] && u32le(&b, 8) == 5
                } else if let Some(rid) = account_rid(a[1]) {
                    b[1] == 5 && domain_sid(&b, false).is_ok() && u32le(&b, 24) == rid
                } else {
                    well_known_sid(a[1], None).is_ok_and(|expected| b == expected)
                };
                Ok(u64::from(matches))
            }
            "GetWindowsAccountDomainSid" => {
                let b = sid(&mut image.memory, a[0])?;
                if a[2] == 0 {
                    return Err(PARAM);
                }
                let domain = domain_sid(&b, false)?;
                sid_output(image, a[1], a[2], &domain)
            }
            "EqualDomainSid" => {
                let x = sid(&mut image.memory, a[0])?;
                let y = sid(&mut image.memory, a[1])?;
                if a[2] == 0 {
                    return Err(PARAM);
                }
                let x = domain_sid(&x, true)?;
                let y = domain_sid(&y, true)?;
                put(&mut image.memory, a[2], u64::from(x == y), 4)?;
                Ok(1)
            }
            "AllocateAndInitializeSid" => {
                let count = a[1] as u8;
                if !(1..=8).contains(&count) {
                    return Err(SID);
                }
                let authority = read(&mut image.memory, a[0], 6)?;
                let mut b = vec![1, count];
                b.extend_from_slice(&authority);
                for &value in &a[2..2 + usize::from(count)] {
                    b.extend_from_slice(&(value as u32).to_le_bytes());
                }
                let p = heap_allocate(image, b.len() as u64)?;
                if let Err(e) = write(&mut image.memory, p, &b)
                    .and_then(|_| put(&mut image.memory, a[10], p, width))
                {
                    let _ = heap_free(image, p);
                    return Err(e);
                }
                image.win32.advapi.security.allocated_sids.push(p);
                Ok(1)
            }
            "FreeSid" => {
                let index = image
                    .win32
                    .advapi
                    .security
                    .allocated_sids
                    .iter()
                    .position(|&p| p == a[0])
                    .ok_or(PARAM)?;
                heap_free(image, a[0])?;
                image.win32.advapi.security.allocated_sids.remove(index);
                Ok(0)
            }
            "ConvertSidToStringSidA" | "ConvertSidToStringSidW" => {
                let b = sid(&mut image.memory, a[0])?;
                let s = sid_text(&b);
                let wide = name.ends_with('W');
                let p = heap_allocate(image, ((s.len() + 1) * if wide { 2 } else { 1 }) as u64)?;
                if let Err(e) = write_string(&mut image.memory, p, &s, wide)
                    .map(|_| ())
                    .and_then(|_| put(&mut image.memory, a[1], p, width))
                {
                    let _ = heap_free(image, p);
                    return Err(e);
                }
                Ok(1)
            }
            "ConvertStringSidToSidA" | "ConvertStringSidToSidW" => {
                let s = string(&mut image.memory, a[0], name.ends_with('W'))?;
                let b = parse_sid(&s)?;
                let p = heap_allocate(image, b.len() as u64)?;
                if let Err(e) = write(&mut image.memory, p, &b)
                    .and_then(|_| put(&mut image.memory, a[1], p, width))
                {
                    let _ = heap_free(image, p);
                    return Err(e);
                }
                Ok(1)
            }
            "InitializeAcl" => {
                if !matches!(a[2], 2..=4) {
                    return Err(1305);
                }
                if a[1] < 8 || a[1] > u16::MAX as u64 || a[1] % 4 != 0 {
                    return Err(PARAM);
                }
                let mut b = vec![0; 8];
                b[0] = a[2] as u8;
                b[2..4].copy_from_slice(&(a[1] as u16).to_le_bytes());
                write(&mut image.memory, a[0], &b)?;
                Ok(1)
            }
            "IsValidAcl" => Ok(u64::from(acl(&mut image.memory, a[0]).is_ok())),
            "GetAclInformation" => {
                let acl = acl(&mut image.memory, a[0])?;
                let b = match a[3] {
                    1 => (acl.bytes[0] as u32).to_le_bytes().to_vec(),
                    2 => {
                        let mut b = Vec::new();
                        for n in [acl.offsets.len(), acl.used, acl.bytes.len() - acl.used] {
                            b.extend_from_slice(&(n as u32).to_le_bytes());
                        }
                        b
                    }
                    _ => return Err(PARAM),
                };
                if a[2] < b.len() as u64 {
                    return Err(122);
                }
                write(&mut image.memory, a[1], &b)?;
                Ok(1)
            }
            "SetAclInformation" => {
                let old = acl(&mut image.memory, a[0])?;
                if a[3] != 1 {
                    return Err(PARAM);
                }
                if a[2] < 4 {
                    return Err(122);
                }
                let revision = word(&mut image.memory, a[1], 4)?;
                // Original RtlSetInformationAcl compares the full DWORD with
                // the current revision and stores its low byte. It allows an
                // equal revision and does not validate the new upper bound.
                if revision < u64::from(old.bytes[0]) {
                    return Err(PARAM);
                }
                put(&mut image.memory, a[0], revision, 1)?;
                Ok(1)
            }
            "GetAce" => {
                let acl = acl(&mut image.memory, a[0])?;
                let offset = acl.offsets.get(a[1] as usize).copied().ok_or(PARAM)?;
                put(&mut image.memory, a[2], addr(a[0], offset)?, width)?;
                Ok(1)
            }
            "DeleteAce" => {
                let mut acl = acl(&mut image.memory, a[0])?;
                let at = acl.offsets.get(a[1] as usize).copied().ok_or(PARAM)?;
                let n = usize::from(u16le(&acl.bytes, at + 2));
                acl.bytes.copy_within(at + n..acl.used, at);
                acl.bytes[acl.used - n..acl.used].fill(0);
                acl.bytes[4..6].copy_from_slice(&((acl.offsets.len() - 1) as u16).to_le_bytes());
                write(&mut image.memory, a[0], &acl.bytes)?;
                Ok(1)
            }
            "AddAce" => {
                let n = usize::try_from(a[4]).map_err(|_| PARAM)?;
                let b = read(&mut image.memory, a[3], n)?;
                add_aces(&mut image.memory, a[0], a[1], a[2], &b)
            }
            "AddAccessAllowedObjectAce" => add_object_ace(&mut image.memory, a, ObjectAceKind::Allowed),
            "AddAccessDeniedObjectAce" => add_object_ace(&mut image.memory, a, ObjectAceKind::Denied),
            "AddAuditAccessObjectAce" => add_object_ace(&mut image.memory, a, ObjectAceKind::Audit),
            "AddMandatoryAce" => add_object_ace(&mut image.memory,
                &[a[0], a[1], a[2], a[3], 0, 0, a[4]], ObjectAceKind::Mandatory),
            "AddAccessAllowedAce"
            | "AddAccessAllowedAceEx"
            | "AddAccessDeniedAce"
            | "AddAccessDeniedAceEx"
            | "AddAuditAccessAce"
            | "AddAuditAccessAceEx" => {
                let ex = name.ends_with("Ex");
                let audit = name.starts_with("AddAudit");
                let shift = usize::from(ex);
                let mask = a[2 + shift] as u32;
                let s = sid(&mut image.memory, a[3 + shift])?;
                let mut flags = if ex { a[2] } else { 0 };
                if flags & !0x1f != 0 {
                    return Err(1004);
                }
                if audit {
                    if a[4 + shift] != 0 {
                        flags |= 0x40;
                    }
                    if a[5 + shift] != 0 {
                        flags |= 0x80;
                    }
                }
                let kind = if audit {
                    2
                } else if name.starts_with("AddAccessDenied") {
                    1
                } else {
                    0
                };
                let mut b = vec![kind, flags as u8];
                b.extend_from_slice(&((8 + s.len()) as u16).to_le_bytes());
                b.extend_from_slice(&mask.to_le_bytes());
                b.extend_from_slice(&s);
                add_aces(&mut image.memory, a[0], a[1], u32::MAX as u64, &b).map_err(|e| {
                    if e == 122 {
                        1344
                    } else {
                        e
                    }
                })
            }
            "InitializeSecurityDescriptor" => {
                if a[1] != 1 {
                    return Err(1305);
                }
                let mut b = vec![0; if width == 8 { 40 } else { 20 }];
                b[0] = 1;
                write(&mut image.memory, a[0], &b)?;
                Ok(1)
            }
            "IsValidSecurityDescriptor" => Ok(u64::from(descriptor(image, a[0]).is_ok())),
            "GetSecurityDescriptorLength" => {
                let d = descriptor(image, a[0])?;
                let header = if d.relative {
                    20
                } else if width == 8 {
                    40
                } else {
                    20
                };
                Ok((header
                    + d.blobs
                        .iter()
                        .map(|b| b.len().div_ceil(4) * 4)
                        .sum::<usize>()) as u64)
            }
            "GetSecurityDescriptorControl" => {
                let d = descriptor(image, a[0])?;
                put(&mut image.memory, a[1], u64::from(d.control), 2)?;
                put(&mut image.memory, a[2], 1, 4)?;
                Ok(1)
            }
            "SetSecurityDescriptorControl" => {
                let d = descriptor(image, a[0])?;
                if a[1] & !0x3f00 != 0 {
                    return Err(PARAM);
                }
                let c = (d.control & !(a[1] as u16)) | ((a[2] & a[1]) as u16);
                put(&mut image.memory, addr(a[0], 2)?, u64::from(c), 2)?;
                Ok(1)
            }
            "GetSecurityDescriptorOwner"
            | "GetSecurityDescriptorGroup"
            | "GetSecurityDescriptorDacl"
            | "GetSecurityDescriptorSacl" => {
                let d = descriptor(image, a[0])?;
                let i = match name {
                    "GetSecurityDescriptorOwner" => 0,
                    "GetSecurityDescriptorGroup" => 1,
                    "GetSecurityDescriptorSacl" => 2,
                    _ => 3,
                };
                let bit = [1, 2, 0x20, 8][i];
                if i < 2 {
                    put(&mut image.memory, a[1], d.pointers[i], width)?;
                    put(&mut image.memory, a[2], u64::from(d.control & bit != 0), 4)?;
                } else {
                    let present = d.control & if i == 2 { 0x10 } else { 4 } != 0;
                    put(&mut image.memory, a[1], u64::from(present), 4)?;
                    if present {
                        put(&mut image.memory, a[2], d.pointers[i], width)?;
                        put(&mut image.memory, a[3], u64::from(d.control & bit != 0), 4)?;
                    }
                }
                Ok(1)
            }
            "SetSecurityDescriptorOwner" => set_component(image, a, 0),
            "SetSecurityDescriptorGroup" => set_component(image, a, 1),
            "SetSecurityDescriptorSacl" => set_component(image, a, 2),
            "SetSecurityDescriptorDacl" => set_component(image, a, 3),
            "MakeSelfRelativeSD" => {
                let d = descriptor(image, a[0])?;
                if d.relative {
                    return Err(PARAM);
                }
                let b = relative_sd(&d);
                let capacity = word(&mut image.memory, a[2], 4)?;
                put(&mut image.memory, a[2], b.len() as u64, 4)?;
                if a[1] == 0 || capacity < b.len() as u64 {
                    return Err(122);
                }
                write(&mut image.memory, a[1], &b)?;
                Ok(1)
            }
            _ => Err(120),
        }
    })())
}

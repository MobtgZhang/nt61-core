//! ADVAPI32 services for an isolated guest process. Guest buffers and ABI-sized
//! pointers are checked by the same memory layer as the PE interpreter.
//! Registry and service configuration live in this process, never in host OS
//! state. Security data operations do not grant host or kernel permissions.

use super::super::GuestImage;
use super::{directory, process, put, status_error, ApiError, GuestAbi, GuestMemory, GuestServices, NOACCESS};

#[path = "advapi/absolute.rs"]
mod absolute;
#[path = "advapi/crypto.rs"]
mod crypto;
#[path = "advapi/registry.rs"]
mod registry;
#[path = "advapi/security.rs"]
mod security;
#[path = "advapi/service.rs"]
mod service;
#[path = "advapi/text.rs"]
mod text;
#[path = "advapi/token.rs"]
pub(super) mod token;

pub(super) fn file_security(
    image: &mut GuestImage,
    name: &str,
    args: &[u64],
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    let abi = match image.thread.context.mode {
        super::DecodeMode::X86 => GuestAbi::X86_32,
        super::DecodeMode::X64 => GuestAbi::X86_64,
    };
    let path = process::process_string(&mut image.memory, args[0], true)?.ok_or(87u32)?;
    if path.is_empty() {
        return Err(123u32.into());
    }
    let path = directory::path(image, &path)?;
    let information = args[1] as u32;
    if information == 0 {
        return Err(5u32.into());
    }
    if name == "SetFileSecurityW" {
        if args[2] == 0 {
            return Err(NOACCESS.into());
        }
        let descriptor = security::descriptor(image, args[2])?;
        let bytes = security::relative_sd(&descriptor);
        services.set_file_security(abi, &path, information, &bytes).map_err(status_error)?;
    } else {
        let bytes = services.query_file_security(abi, &path, information).map_err(status_error)?;
        let capacity = args[3] as u32 as usize;
        if capacity < bytes.len() {
            put(&mut image.memory, args[4], bytes.len() as u64, 4)?;
            return Err(122u32.into());
        }
        if !image.memory.probe_write(args[4], 4) || !image.memory.probe_write(args[2], bytes.len()) {
            return Err(NOACCESS.into());
        }
        if !image.memory.write_virt(args[2], &bytes) {
            return Err(NOACCESS.into());
        }
        put(&mut image.memory, args[4], bytes.len() as u64, 4)?;
    }
    Ok(1)
}

pub(super) struct State {
    registry: registry::State,
    security: security::State,
    service: service::State,
    crypto: crypto::State,
}

impl State {
    pub(super) fn new() -> Self {
        Self {
            registry: registry::State::new(),
            security: security::State::new(),
            service: service::State::new(),
            crypto: crypto::State::new(),
        }
    }
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Option<Result<u64, u32>> {
    // The Win64 ABI does not require the upper register bits of DWORD/BOOL/INT
    // arguments to be clear. Preserve pointer-sized handles and addresses while
    // narrowing documented scalar parameters before any validation or indexing.
    let scalars: u32 = match name {
        "RegCreateKeyExA" | "RegCreateKeyExW" => (1 << 2) | (1 << 4) | (1 << 5),
        "RegOpenKeyExA" | "RegOpenKeyExW" => (1 << 2) | (1 << 3),
        "RegDeleteKeyExA" | "RegDeleteKeyExW" => (1 << 2) | (1 << 3),
        "RegSetValueExA" | "RegSetValueExW" => (1 << 2) | (1 << 3) | (1 << 5),
        "RegSetKeyValueA" | "RegSetKeyValueW" => (1 << 3) | (1 << 5),
        "AreAllAccessesGranted" | "AreAnyAccessesGranted" => 3,
        "RegSetValueA" | "RegSetValueW" => (1 << 2) | (1 << 4),
        "RegEnumKeyA" | "RegEnumKeyW" => (1 << 1) | (1 << 3),
        "RegEnumKeyExA" | "RegEnumKeyExW" | "RegEnumValueA" | "RegEnumValueW" => 1 << 1,
        "RegQueryMultipleValuesA" | "RegQueryMultipleValuesW" => 1 << 2,
        "RegGetValueA" | "RegGetValueW" => 1 << 3,
        "OpenSCManagerA" | "OpenSCManagerW" | "OpenServiceA" | "OpenServiceW" => 1 << 2,
        "CreateServiceA" | "CreateServiceW" => (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6),
        "ChangeServiceConfigA" | "ChangeServiceConfigW" => (1 << 1) | (1 << 2) | (1 << 3),
        "QueryServiceConfigA" | "QueryServiceConfigW" => 1 << 2,
        "QueryServiceStatusEx" => (1 << 1) | (1 << 3),
        "EnumServicesStatusA" | "EnumServicesStatusW" => (1 << 1) | (1 << 2) | (1 << 4),
        "EnumServicesStatusExA" | "EnumServicesStatusExW" => {
            (1 << 1) | (1 << 2) | (1 << 3) | (1 << 5)
        }
        "StartServiceA" | "StartServiceW" | "ControlService" => 1 << 1,
        "CryptAcquireContextA" | "CryptAcquireContextW" => (1 << 3) | (1 << 4),
        "CryptReleaseContext" => 1 << 1,
        "CryptContextAddRef" => 1 << 2,
        "CryptCreateHash" => (1 << 1) | (1 << 3),
        "CryptHashData" => (1 << 2) | (1 << 3),
        "CryptGetHashParam" | "CryptGetProvParam" => (1 << 1) | (1 << 4),
        "CryptSetHashParam" => (1 << 1) | (1 << 3),
        "CryptDuplicateHash" => 1 << 2,
        "CryptGenRandom" => 1 << 1,
        "GetSidLengthRequired" | "CopySid" | "CreateWellKnownSid" => 1,
        "IsWellKnownSid" | "GetSidSubAuthority" | "DeleteAce" => 1 << 1,
        "InitializeSid" | "GetAce" => {
            if name == "GetAce" {
                1 << 1
            } else {
                1 << 2
            }
        }
        "InitializeAcl" => (1 << 1) | (1 << 2),
        "InitializeSecurityDescriptor" => 1 << 1,
        "GetAclInformation" | "SetAclInformation" => (1 << 2) | (1 << 3),
        "AddAce" => (1 << 1) | (1 << 2) | (1 << 4),
        "AddAccessAllowedAce" | "AddAccessDeniedAce" => (1 << 1) | (1 << 2),
        "AddMandatoryAce" | "AddAccessAllowedAceEx" | "AddAccessDeniedAceEx" | "AddAccessAllowedObjectAce" | "AddAccessDeniedObjectAce" => (1 << 1) | (1 << 2) | (1 << 3),
        "AddAuditAccessObjectAce" => (1 << 1) | (1 << 2) | (1 << 3) | (1 << 7) | (1 << 8),
        "AddAuditAccessAce" => (1 << 1) | (1 << 2) | (1 << 4) | (1 << 5),
        "AddAuditAccessAceEx" => (1 << 1) | (1 << 2) | (1 << 3) | (1 << 5) | (1 << 6),
        "SetSecurityDescriptorOwner" | "SetSecurityDescriptorGroup" => 1 << 2,
        "SetSecurityDescriptorDacl" | "SetSecurityDescriptorSacl" => (1 << 1) | (1 << 3),
        "SetSecurityDescriptorControl" => (1 << 1) | (1 << 2),
        "AllocateAndInitializeSid" => 0x3fe,
        "IsTextUnicode" => 1 << 1,
        _ => 0,
    };
    let mut narrowed = [0; crate::arch::common::btl::nt61::MAX_ARGUMENTS];
    if a.len() > narrowed.len() {
        return Some(Err(87));
    }
    for (index, value) in a.iter().enumerate() {
        narrowed[index] = if name == "SetSecurityDescriptorControl" && index > 0 {
            u64::from(*value as u16)
        } else if scalars & (1 << index) != 0 {
            u64::from(*value as u32)
        } else {
            *value
        };
    }
    let a = &narrowed[..a.len()];
    registry::invoke(image, name, a)
        .or_else(|| security::invoke(image, name, a))
        .or_else(|| absolute::invoke(image, name, a))
        .or_else(|| service::invoke(image, name, a))
        .or_else(|| crypto::invoke(image, name, a))
        .or_else(|| text::invoke(image, name, a))
}

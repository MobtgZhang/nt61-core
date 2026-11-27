use super::super::status::{STATUS_INVALID_PARAMETER, STATUS_NOT_SUPPORTED, STATUS_NO_MEMORY};
use super::super::types::NTSTATUS;
use crate::fs::ntfs::relative_security_descriptor::RelativeDescriptor;
use crate::fs::ntfs::security_target::FileSecurityTarget;
use crate::fs::system_volume;
use alloc::vec::Vec;

const OWNER: u32 = 1;
const GROUP: u32 = 2;
const DACL: u32 = 4;
const SACL: u32 = 8;
const BASIC_INFORMATION: u32 = OWNER | GROUP | DACL | SACL;
const PROTECTED_DACL: u32 = 0x8000_0000;
const UNPROTECTED_DACL: u32 = 0x2000_0000;
const DACL_PROTECTION: u32 = PROTECTED_DACL | UNPROTECTED_DACL;
pub(crate) const STATUS_INVALID_SECURITY_DESCR: NTSTATUS = 0xc000_0079u32 as i32;

pub(crate) fn query_access(information: u32) -> Result<u32, NTSTATUS> {
    use crate::se::seaccess::{ACCESS_READ_CONTROL, ACCESS_SYSTEM_SECURITY};
    if information & !BASIC_INFORMATION != 0 {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut access = 0;
    if information & (OWNER | GROUP | DACL) != 0 {
        access |= ACCESS_READ_CONTROL;
    }
    if information & SACL != 0 {
        access |= ACCESS_SYSTEM_SECURITY;
    }
    Ok(access)
}

pub(crate) fn set_access(information: u32) -> Result<u32, NTSTATUS> {
    use crate::se::seaccess::{ACCESS_SYSTEM_SECURITY, ACCESS_WRITE_DAC, ACCESS_WRITE_OWNER};
    let protection = information & DACL_PROTECTION;
    if information & !(BASIC_INFORMATION | DACL_PROTECTION) != 0
        || protection == DACL_PROTECTION
        || protection != 0 && information & DACL == 0
    {
        return Err(STATUS_INVALID_PARAMETER);
    }
    let mut access = 0;
    if information & (OWNER | GROUP) != 0 {
        access |= ACCESS_WRITE_OWNER;
    }
    if information & DACL != 0 {
        access |= ACCESS_WRITE_DAC;
    }
    if information & SACL != 0 {
        access |= ACCESS_SYSTEM_SECURITY;
    }
    Ok(access)
}

pub(crate) fn query(target: FileSecurityTarget, information: u32) -> Result<Vec<u8>, NTSTATUS> {
    let bytes = system_volume::read_file_security(target).map_err(|status| status as NTSTATUS)?;
    RelativeDescriptor::parse(&bytes)
        .ok_or(STATUS_INVALID_SECURITY_DESCR)?
        .select(information)
        .encode()
        .ok_or(STATUS_NO_MEMORY)
}

pub(crate) fn set(target: FileSecurityTarget, information: u32, bytes: &[u8]) -> Result<(), NTSTATUS> {
    if information == 0 {
        return Ok(());
    }
    if information & !(DACL | PROTECTED_DACL) != 0 {
        return Err(STATUS_NOT_SUPPORTED);
    }
    let mut replacement = RelativeDescriptor::parse(bytes).ok_or(STATUS_INVALID_SECURITY_DESCR)?;
    if replacement.control & 4 == 0 {
        return Err(STATUS_INVALID_SECURITY_DESCR);
    }
    replacement.validate_discretionary_acl().ok_or(STATUS_INVALID_SECURITY_DESCR)?;
    system_volume::update_file_security(target, |current| {
        let current = RelativeDescriptor::parse(current).ok_or(STATUS_INVALID_SECURITY_DESCR as u32)?;
        let protection = if information & PROTECTED_DACL != 0 { 0x1000 } else { current.control & 0x1000 };
        replacement.control = (replacement.control & !0x1000) | protection;
        current.merge(replacement, DACL).encode().ok_or(STATUS_NO_MEMORY as u32)
    }).map_err(|status| status as NTSTATUS)
}

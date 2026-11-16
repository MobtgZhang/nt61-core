//! Copy descriptors before consulting token state. No Rust reference aliases
//! caller-controlled descriptor, SID or ACL storage.

use super::*;
use alloc::vec::Vec;

#[derive(Clone, Copy)]
pub(super) enum Memory<'a> {
    Caller(u64),
    Trusted(&'a [u8]),
}

impl Memory<'_> {
    fn read(self, offset: usize, length: usize) -> Result<Vec<u8>, NTSTATUS> {
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| STATUS_NO_MEMORY)?;
        output.resize(length, 0);
        match self {
            Self::Caller(base) => copy_from_user(
                &mut output,
                base.checked_add(offset as u64)
                    .ok_or(STATUS_ACCESS_VIOLATION)?,
            )?,
            Self::Trusted(bytes) => {
                let end = offset
                    .checked_add(length)
                    .ok_or(STATUS_INVALID_SECURITY_DESCR)?;
                output.copy_from_slice(
                    bytes
                        .get(offset..end)
                        .ok_or(STATUS_INVALID_SECURITY_DESCR)?,
                );
            }
        }
        Ok(output)
    }

    fn field(
        self,
        header: &[u8],
        relative: bool,
        index: usize,
    ) -> Result<Option<(Self, usize)>, NTSTATUS> {
        if relative {
            let offset =
                u32::from_le_bytes(header[4 + index * 4..8 + index * 4].try_into().unwrap())
                    as usize;
            if offset == 0 {
                return Ok(None);
            }
            if offset < 20 || offset % 4 != 0 {
                return Err(STATUS_INVALID_SECURITY_DESCR);
            }
            Ok(Some((self, offset)))
        } else {
            let address =
                u64::from_le_bytes(header[8 + index * 8..16 + index * 8].try_into().unwrap());
            Ok((address != 0).then_some((Self::Caller(address), 0)))
        }
    }
}

pub(super) struct Ace {
    pub kind: u8,
    pub flags: u8,
    pub mask: u32,
    pub sid: Vec<u8>,
}

pub(super) struct Descriptor {
    pub owner: Vec<u8>,
    pub dacl: Option<Vec<Ace>>,
}

fn sid_length(bytes: &[u8]) -> Result<usize, NTSTATUS> {
    if bytes.len() < 8 || bytes[0] != 1 || bytes[1] > 15 {
        return Err(STATUS_INVALID_SID);
    }
    Ok(8 + bytes[1] as usize * 4)
}

fn sid(memory: Memory<'_>, offset: usize) -> Result<Vec<u8>, NTSTATUS> {
    let header = memory.read(offset, 8)?;
    let length = sid_length(&header)?;
    let value = memory.read(offset, length)?;
    // Recheck after capture: the header may have changed between copies.
    if sid_length(&value)? != length {
        return Err(STATUS_INVALID_SID);
    }
    Ok(value)
}

fn acl(memory: Memory<'_>, offset: usize) -> Result<Vec<Ace>, NTSTATUS> {
    let header = memory.read(offset, 8)?;
    let length = u16::from_le_bytes([header[2], header[3]]) as usize;
    if length < 8 {
        return Err(STATUS_INVALID_ACL);
    }
    let bytes = memory.read(offset, length)?;
    if !matches!(bytes[0], 2 | 4) || u16::from_le_bytes([bytes[2], bytes[3]]) as usize != length {
        return Err(STATUS_INVALID_ACL);
    }
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    if count > (length - 8) / 4 {
        return Err(STATUS_INVALID_ACL);
    }
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| STATUS_NO_MEMORY)?;
    let mut offset = 8;
    for _ in 0..count {
        let header = bytes.get(offset..offset + 4).ok_or(STATUS_INVALID_ACL)?;
        let size = u16::from_le_bytes([header[2], header[3]]) as usize;
        if size < 4 || size % 4 != 0 {
            return Err(STATUS_INVALID_ACL);
        }
        let ace = bytes.get(offset..offset + size).ok_or(STATUS_INVALID_ACL)?;
        offset += size;
        if !matches!(ace[0], 0 | 1) {
            // Conditions, GUIDs and compound identities need their own models.
            // Never reinterpret these as an unconditional grant.
            return Err(STATUS_NOT_SUPPORTED);
        }
        let sid_bytes = ace.get(8..).ok_or(STATUS_INVALID_ACL)?;
        let sid_size = sid_length(sid_bytes).map_err(|_| STATUS_INVALID_ACL)?;
        if sid_size != sid_bytes.len() {
            return Err(STATUS_INVALID_ACL);
        }
        let mut captured_sid = Vec::new();
        captured_sid
            .try_reserve_exact(sid_size)
            .map_err(|_| STATUS_NO_MEMORY)?;
        captured_sid.extend_from_slice(sid_bytes);
        entries.push(Ace {
            kind: ace[0],
            flags: ace[1],
            mask: u32::from_le_bytes(ace[4..8].try_into().unwrap()),
            sid: captured_sid,
        });
    }
    // ACLs may reserve unused capacity after their last ACE.
    Ok(entries)
}

pub(super) fn descriptor(memory: Memory<'_>) -> Result<Descriptor, NTSTATUS> {
    let prefix = memory.read(0, 4)?;
    let relative = u16::from_le_bytes([prefix[2], prefix[3]]) & 0x8000 != 0;
    let header = memory.read(0, if relative { 20 } else { 40 })?;
    let control = u16::from_le_bytes([header[2], header[3]]);
    if header[0] != 1 || (control & 0x8000 != 0) != relative {
        return Err(STATUS_INVALID_SECURITY_DESCR);
    }
    let (owner_memory, owner_offset) = memory
        .field(&header, relative, 0)?
        .ok_or(STATUS_INVALID_SECURITY_DESCR)?;
    let (group_memory, group_offset) = memory
        .field(&header, relative, 1)?
        .ok_or(STATUS_INVALID_SECURITY_DESCR)?;
    let owner = sid(owner_memory, owner_offset)?;
    // An invalid group also makes the descriptor invalid, even though this
    // check does not use group ownership to grant access.
    sid(group_memory, group_offset)?;
    let dacl = if control & 4 == 0 {
        None
    } else if let Some((acl_memory, acl_offset)) = memory.field(&header, relative, 3)? {
        Some(acl(acl_memory, acl_offset)?)
    } else {
        None
    };
    Ok(Descriptor { owner, dacl })
}

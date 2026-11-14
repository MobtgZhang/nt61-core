//! Read-only kernel process observations, decoded without native struct casts.
use alloc::{string::String, vec::Vec};
use nt61_abi::desktop_bridge::{MAX_PATH, MAX_PROCESSES, PROCESS_HEADER, STATUS_INVALID_PARAMETER};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: u64,
    pub working_set: u64,
    pub cpu_ticks: u64,
    pub session: u32,
    pub name: String,
    pub user: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub cpu_count: u32,
    pub processes: Vec<Process>,
}

impl Snapshot {
    pub fn decode(bytes: &[u8]) -> Result<Self, u32> {
        let invalid = STATUS_INVALID_PARAMETER;
        let cpu_count = u32_at(bytes, 0)?;
        let count = u32_at(bytes, 4)? as usize;
        if cpu_count == 0 || count > MAX_PROCESSES || count > bytes.len().saturating_sub(8) / PROCESS_HEADER {
            return Err(invalid);
        }
        let mut processes = Vec::with_capacity(count);
        let mut offset = 8;
        for _ in 0..count {
            let header = bytes.get(offset..offset + PROCESS_HEADER).ok_or(invalid)?;
            let name_len = u32_at(header, 28)? as usize;
            let user_len = u32_at(header, 32)? as usize;
            if name_len > MAX_PATH || user_len > MAX_PATH { return Err(invalid); }
            offset += PROCESS_HEADER;
            let name = bytes.get(offset..offset + name_len).ok_or(invalid)?;
            offset += name_len;
            let user = bytes.get(offset..offset + user_len).ok_or(invalid)?;
            offset += user_len;
            processes.push(Process {
                pid: u64_at(header, 0)?, working_set: u64_at(header, 8)?,
                cpu_ticks: u64_at(header, 16)?, session: u32_at(header, 24)?,
                name: core::str::from_utf8(name).map_err(|_| invalid)?.into(),
                user: core::str::from_utf8(user).map_err(|_| invalid)?.into(),
            });
        }
        if offset != bytes.len() { return Err(invalid); }
        processes.sort_by_key(|p| p.pid);
        Ok(Self { cpu_count, processes })
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, u32> {
    Ok(u32::from_le_bytes(bytes.get(offset..offset + 4)
        .and_then(|b| b.try_into().ok()).ok_or(STATUS_INVALID_PARAMETER)?))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, u32> {
    Ok(u64::from_le_bytes(bytes.get(offset..offset + 8)
        .and_then(|b| b.try_into().ok()).ok_or(STATUS_INVALID_PARAMETER)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(4u32.to_le_bytes()); bytes.extend(1u32.to_le_bytes());
        for value in [104u64, 123456, 789] { bytes.extend(value.to_le_bytes()); }
        for value in [1u32, 12, 13] { bytes.extend(value.to_le_bytes()); }
        bytes.extend(b"explorer.exeAdministrator"); bytes
    }
    #[test]
    fn decodes_real_wire_fields_and_rejects_truncated_or_unbounded_records() {
        let bytes = packet(); let snapshot = Snapshot::decode(&bytes).unwrap();
        assert_eq!(snapshot.cpu_count, 4);
        assert_eq!(snapshot.processes[0], Process { pid:104, working_set:123456,
            cpu_ticks:789, session:1, name:"explorer.exe".into(), user:"Administrator".into() });
        for end in 0..bytes.len() { assert!(Snapshot::decode(&bytes[..end]).is_err()); }
        let mut bad = bytes.clone(); bad.extend([0]); assert!(Snapshot::decode(&bad).is_err());
        let mut bad = bytes.clone(); bad[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Snapshot::decode(&bad).is_err());
        let mut bad = bytes.clone(); bad[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Snapshot::decode(&bad).is_err());
        let mut bad = bytes; bad[44] = 0xFF; assert!(Snapshot::decode(&bad).is_err());
    }
}

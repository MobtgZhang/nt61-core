use super::NtfsData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileSecurityTarget {
    pub(super) record: u64,
    volume_serial: u64,
    partition_start: u64,
    sequence: u16,
}

fn active_sequence(record: &[u8]) -> Option<u16> {
    let header = record.get(..0x30)?;
    if header.get(..4)? != b"FILE" || header[0x16] & 1 == 0
        || header[0x20..0x28] != [0; 8]
    {
        return None;
    }
    let sequence = u16::from_le_bytes(header[0x10..0x12].try_into().ok()?);
    (sequence != 0).then_some(sequence)
}

impl FileSecurityTarget {
    pub(crate) fn record(self) -> u64 {
        self.record
    }

    #[cfg(test)]
    pub(crate) fn capture(ntfs: &NtfsData, record: u64) -> Option<Self> {
        let data = super::read_mft_record(ntfs, record)?;
        Self::from_record(ntfs, record, &data)
    }

    pub(super) fn from_record(ntfs: &NtfsData, record: u64, data: &[u8]) -> Option<Self> {
        Some(Self {
            record,
            volume_serial: ntfs.volume_serial,
            partition_start: ntfs.hidden_sectors,
            sequence: active_sequence(data)?,
        })
    }

    pub(super) fn matches(self, ntfs: &NtfsData, data: &[u8]) -> bool {
        self.volume_serial == ntfs.volume_serial
            && self.partition_start == ntfs.hidden_sectors
            && active_sequence(data) == Some(self.sequence)
    }
}

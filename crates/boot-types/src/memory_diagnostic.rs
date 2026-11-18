//! Versioned persistent memory diagnostic result, shared by bootmgr/winload/kernel.
//! Explicit encoding avoids handing firmware-owned pointers or Rust layouts across boots.
pub const VARIABLE_NAME: &str = "NT61MemoryDiagnostic";
pub const VENDOR_GUID: &str = "9d0f7e8a-8d21-4f7f-987f-3c9d0c5f4b61";
pub const SIZE: usize = 88;
pub const SIGNATURE: u32 = u32::from_le_bytes(*b"MTES");
pub const PASSED: u32 = 2;
pub const FAILED: u32 = 3;
pub const CANCELLED: u32 = 4;
pub const INCONCLUSIVE: u32 = 5;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResultRecord {
    /// Persistent sequence distinguishes identical results from separate runs.
    pub run_id: u64,
    pub status: u32,
    pub passes: u32,
    /// Unique bytes tested per completed pass, not pattern traffic.
    pub tested: u64,
    pub available: u64,
    pub excluded: u64,
    pub errors: u64,
    pub address: u64,
    pub expected: u64,
    pub actual: u64,
}
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |v, &b| {
        (v ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}
impl ResultRecord {
    pub fn acknowledged_by(self, bytes: &[u8]) -> bool {
        self.run_id != 0 && bytes == self.encode()
    }
    pub fn encode(self) -> [u8; SIZE] {
        let mut b = [0u8; SIZE];
        b[..4].copy_from_slice(b"MTES");
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        b[8..12].copy_from_slice(&self.status.to_le_bytes());
        b[12..16].copy_from_slice(&self.passes.to_le_bytes());
        for (chunk, value) in b[16..72].chunks_exact_mut(8).zip([
            self.tested,
            self.available,
            self.excluded,
            self.errors,
            self.address,
            self.expected,
            self.actual,
        ]) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        b[72..80].copy_from_slice(&self.run_id.to_le_bytes());
        let sum = checksum(&b[..80]);
        b[80..].copy_from_slice(&sum.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() != SIZE
            || &b[..4] != b"MTES"
            || b[4..8] != 2u32.to_le_bytes()
            || u64::from_le_bytes(b[80..88].try_into().ok()?) != checksum(&b[..80])
        {
            return None;
        }
        let word = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let r = Self {
            run_id: word(72),
            status: u32::from_le_bytes(b[8..12].try_into().ok()?),
            passes: u32::from_le_bytes(b[12..16].try_into().ok()?),
            tested: word(16),
            available: word(24),
            excluded: word(32),
            errors: word(40),
            address: word(48),
            expected: word(56),
            actual: word(64),
        };
        if !matches!(r.status, PASSED | FAILED | CANCELLED | INCONCLUSIVE)
            || r.tested > r.available
            || r.excluded > r.available
        {
            return None;
        }
        if r.status == PASSED && (r.tested == 0 || r.passes == 0 || r.errors != 0) {
            return None;
        }
        if r.status == FAILED && r.errors == 0 {
            return None;
        }
        Some(r)
    }
    pub const fn summary(self) -> &'static str {
        match self.status {
        PASSED => "No memory errors were detected in the tested memory.",
        FAILED => "Your computer has a memory problem. Contact the computer manufacturer for assistance.",
        CANCELLED => "The memory diagnostic was cancelled. Testing was not completed.",
        _ => "The memory diagnostic could not complete. No passing result is available.",
    }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn passed() -> ResultRecord {
        ResultRecord {
            status: PASSED,
            passes: 2,
            tested: 32 * 1048576,
            available: 48 * 1048576,
            excluded: 16 * 1048576,
            ..ResultRecord::default()
        }
    }
    #[test]
    fn roundtrip_and_checksum_rejects_corruption() {
        let r = passed();
        let b = r.encode();
        assert_eq!(ResultRecord::decode(&b), Some(r));
        for i in 0..SIZE {
            let mut bad = b;
            bad[i] ^= 1;
            assert_eq!(ResultRecord::decode(&bad), None, "byte {}", i);
        }
        assert_eq!(ResultRecord::decode(&b[..SIZE - 1]), None);
    }
    #[test]
    fn zero_bytes_errors_and_incomplete_passes_cannot_be_success() {
        let mut r = passed();
        r.tested = 0;
        assert!(ResultRecord::decode(&r.encode()).is_none());
        r = passed();
        r.passes = 0;
        assert!(ResultRecord::decode(&r.encode()).is_none());
        r = passed();
        r.errors = 1;
        assert!(ResultRecord::decode(&r.encode()).is_none());
        r = passed();
        r.tested = r.available + 1;
        assert!(ResultRecord::decode(&r.encode()).is_none());
        r = passed();
        r.status = CANCELLED;
        r.passes = 0;
        r.tested = 0;
        assert_eq!(ResultRecord::decode(&r.encode()), Some(r));
    }
    #[test]
    fn acknowledgement_cannot_consume_another_run_or_an_unsequenced_result() {
        let mut old = passed();
        old.run_id = 41;
        let ack = old.encode();
        assert!(old.acknowledged_by(&ack));
        let mut next = old;
        next.run_id = 42;
        assert!(!next.acknowledged_by(&ack));
        next.run_id = 0;
        assert!(!next.acknowledged_by(&next.encode()));
        assert!(!old.acknowledged_by(&ack[..SIZE - 1]));
    }
}

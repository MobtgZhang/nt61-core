//! Bounded decoding of the desktop telemetry protocol. All counters are cumulative.
use alloc::{string::String, vec::Vec};
use nt61_abi::desktop_bridge::{MAX_PATH, STATUS_INVALID_PARAMETER as INVALID};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Network {
    pub index: u64,
    pub up: bool,
    pub speed: u64,
    pub sent: u64,
    pub received: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub uptime: u64,
    pub total: u64,
    pub available: u64,
    pub threads: u64,
    pub pool: u64,
    pub ticks: u64,
    pub idle: u64,
    pub network: Vec<Network>,
}
fn number(b: &[u8], i: usize) -> Result<u64, u32> {
    Ok(u64::from_le_bytes(
        b.get(i..i + 8)
            .ok_or(INVALID)?
            .try_into()
            .map_err(|_| INVALID)?,
    ))
}
fn word(b: &[u8], i: usize) -> Result<u32, u32> {
    Ok(u32::from_le_bytes(
        b.get(i..i + 4)
            .ok_or(INVALID)?
            .try_into()
            .map_err(|_| INVALID)?,
    ))
}
impl Sample {
    pub fn decode(b: &[u8]) -> Result<Self, u32> {
        let count = word(b, 56)? as usize;
        if count > 256 || b.len() != 60 + count * 40 {
            return Err(INVALID);
        }
        let mut sample = Self {
            uptime: number(b, 0)?,
            total: number(b, 8)?,
            available: number(b, 16)?,
            threads: number(b, 24)?,
            pool: number(b, 32)?,
            ticks: number(b, 40)?,
            idle: number(b, 48)?,
            network: Vec::new(),
        };
        if sample.available > sample.total || sample.idle > sample.ticks {
            return Err(INVALID);
        }
        for i in 0..count {
            let n = 60 + i * 40;
            let up = number(b, n + 8)?;
            if up > 1 {
                return Err(INVALID);
            }
            sample.network.push(Network {
                index: number(b, n)?,
                up: up != 0,
                speed: number(b, n + 16)?,
                sent: number(b, n + 24)?,
                received: number(b, n + 32)?,
            });
        }
        Ok(sample)
    }
    pub fn cpu_since(&self, old: &Self) -> Option<u32> {
        let total = self.ticks.checked_sub(old.ticks)?;
        let idle = self.idle.checked_sub(old.idle)?;
        if total == 0 || idle > total {
            return None;
        }
        Some(((total - idle) as u128 * 100 / total as u128) as u32)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub state: u32,
    pub pid: u32,
    pub description: String,
    pub group: String,
    pub start_type: u32,
    pub accepted: u32,
    pub exit_code: u32,
}
pub fn decode_services(b: &[u8]) -> Result<Vec<Service>, u32> {
    let mut out = Vec::new();
    let mut n = 0;
    if b.starts_with(b"SVC2") {
        let count = word(b, 4)? as usize;
        if count > 4096 {
            return Err(INVALID);
        }
        n = 8;
        for _ in 0..count {
            let lengths = [
                word(b, n)? as usize,
                word(b, n + 4)? as usize,
                word(b, n + 8)? as usize,
            ];
            if lengths[0] == 0 || lengths.iter().any(|v| *v > MAX_PATH) {
                return Err(INVALID);
            }
            let state = word(b, n + 12)?;
            let pid = word(b, n + 16)?;
            let start_type = word(b, n + 20)?;
            let accepted = word(b, n + 24)?;
            let exit_code = word(b, n + 28)?;
            if !(1..=7).contains(&state) || start_type > 4 {
                return Err(INVALID);
            }
            n += 32;
            let mut texts = Vec::new();
            for len in lengths {
                let text =
                    core::str::from_utf8(b.get(n..n + len).ok_or(INVALID)?).map_err(|_| INVALID)?;
                if text.contains('\0') {
                    return Err(INVALID);
                }
                texts.push(String::from(text));
                n += len;
            }
            out.push(Service {
                name: texts.remove(0),
                description: texts.remove(0),
                group: texts.remove(0),
                state,
                pid,
                start_type,
                accepted,
                exit_code,
            });
        }
        return if n == b.len() { Ok(out) } else { Err(INVALID) };
    }

    while n < b.len() {
        let len = word(b, n)? as usize;
        let state = word(b, n + 4)?;
        let pid = word(b, n + 8)?;
        n += 12;
        if len == 0 || len > MAX_PATH || !(1..=7).contains(&state) || out.len() >= 4096 {
            return Err(INVALID);
        }
        let name = core::str::from_utf8(b.get(n..n + len).ok_or(INVALID)?).map_err(|_| INVALID)?;
        if name.contains('\0') {
            return Err(INVALID);
        }
        out.push(Service {
            name: name.into(),
            state,
            pid,
            description: name.into(),
            group: String::new(),
            start_type: 3,
            accepted: 0,
            exit_code: 0,
        });
        n += len;
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packets_are_strict() {
        let mut b = alloc::vec![0;60];
        let sample = Sample::decode(&b).unwrap();
        assert_eq!(sample.cpu_since(&sample), None);
        for n in 0..60 {
            assert!(Sample::decode(&b[..n]).is_err());
        }
        b.push(0);
        assert!(Sample::decode(&b).is_err());
        let mut s = Vec::new();
        s.extend(3u32.to_le_bytes());
        s.extend(4u32.to_le_bytes());
        s.extend(42u32.to_le_bytes());
        s.extend(b"svc");
        assert_eq!(decode_services(&s).unwrap()[0].pid, 42);
        for n in 1..s.len() {
            assert!(decode_services(&s[..n]).is_err());
        }
    }
    #[test]
    fn cpu_uses_deltas_and_rejects_reset() {
        let a = Sample {
            ticks: 100,
            idle: 40,
            ..Sample::default()
        };
        let b = Sample {
            ticks: 200,
            idle: 65,
            ..Sample::default()
        };
        assert_eq!(b.cpu_since(&a), Some(75));
        assert_eq!(a.cpu_since(&b), None);
    }
}

#[cfg(test)]
mod service_metadata_tests {
    use super::*;
    #[test]
    fn service_metadata_is_versioned_and_strict() {
        let mut b = b"SVC2".to_vec();
        b.extend(1u32.to_le_bytes());
        for n in [3u32, 10, 8, 4, 42, 2, 1, 0] {
            b.extend(n.to_le_bytes());
        }
        b.extend(b"dnsDNS ClientLocalSvc");
        let records = decode_services(&b).unwrap();
        assert_eq!(records[0].description, "DNS Client");
        assert_eq!(records[0].group, "LocalSvc");
        assert_eq!(records[0].accepted, 1);
        for n in 4..b.len() {
            assert!(decode_services(&b[..n]).is_err());
        }
        let mut invalid = b.clone();
        invalid[28..32].copy_from_slice(&5u32.to_le_bytes());
        assert!(decode_services(&invalid).is_err());
        b.push(0);
        assert!(decode_services(&b).is_err());
    }
}

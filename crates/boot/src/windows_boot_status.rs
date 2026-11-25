extern crate alloc;

use alloc::vec::Vec;

pub const CAPACITY: usize = 64 * 1024;
pub const FILE_HEADER_SIZE: usize = 16;
const RECORD_HEADER_SIZE: usize = 40;
const VERSION: u32 = 2;
const LOAD_FAILURE: u32 = 0xc000_000f;
const RECOVERY_REASON_LOAD_FAILURE: u32 = 2;

pub fn is_v2_header(header: &[u8; FILE_HEADER_SIZE]) -> bool {
    let next = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    header[..4] == VERSION.to_le_bytes()
        && header[4..8] == (FILE_HEADER_SIZE as u32).to_le_bytes()
        && header[8..12] == (CAPACITY as u32).to_le_bytes()
        && (FILE_HEADER_SIZE..=CAPACITY).contains(&next)
}

pub fn automatic_recovery_log(
    time: [u16; 7],
    failed_path: &str,
    recovery_guid: [u8; 16],
    recovery_path: &str,
) -> Option<Vec<u8>> {
    let fixed = FILE_HEADER_SIZE + 3 * RECORD_HEADER_SIZE + 24 + 4 + 20 + 4;
    let length =
        fixed + 2 * (failed_path.encode_utf16().count() + recovery_path.encode_utf16().count());
    if length > CAPACITY
        || [failed_path, recovery_path]
            .iter()
            .any(|path| path.is_empty() || path.contains('\0'))
    {
        return None;
    }

    let mut log = Vec::new();
    log.try_reserve_exact(length).ok()?;
    for field in [
        VERSION,
        FILE_HEADER_SIZE as u32,
        CAPACITY as u32,
        length as u32,
    ] {
        log.extend_from_slice(&field.to_le_bytes());
    }
    let clock = 3600 + 60 * u64::from(time[4]) + u64::from(time[5]);
    let start = begin_record(&mut log, clock, 1, 1);
    for field in time.into_iter().chain([7]) {
        log.extend_from_slice(&field.to_le_bytes());
    }
    log.extend_from_slice(&1_u64.to_le_bytes());
    finish_record(&mut log, start);

    let start = begin_record(&mut log, clock + 1, 3, 0x13);
    log.extend_from_slice(&LOAD_FAILURE.to_le_bytes());
    push_path(&mut log, failed_path);
    finish_record(&mut log, start);

    let start = begin_record(&mut log, clock + 2, 1, 0x11);
    log.extend_from_slice(&crate::bcd_parser::windows_guid(recovery_guid));
    log.extend_from_slice(&RECOVERY_REASON_LOAD_FAILURE.to_le_bytes());
    push_path(&mut log, recovery_path);
    finish_record(&mut log, start);
    debug_assert_eq!(log.len(), length);
    Some(log)
}

fn begin_record(log: &mut Vec<u8>, clock: u64, category: u32, event: u32) -> usize {
    let start = log.len();
    log.extend_from_slice(&clock.to_le_bytes());
    log.extend_from_slice(&[0; 16]);
    for field in [0, category, VERSION, event] {
        log.extend_from_slice(&field.to_le_bytes());
    }
    start
}

fn finish_record(log: &mut [u8], start: usize) {
    let length = (log.len() - start) as u32;
    log[start + 24..start + 28].copy_from_slice(&length.to_le_bytes());
}

fn push_path(log: &mut Vec<u8>, path: &str) {
    for unit in path.encode_utf16().chain([0]) {
        log.extend_from_slice(&unit.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_stock_recovery_record_layout() {
        let recovery_guid = [
            0xda, 0xc4, 0x2e, 0x20, 0xe3, 0xd1, 0x11, 0xf1, 0x9f, 0x5b, 0xcf, 0x2a, 0xe4, 0x3d,
            0xcc, 0x1a,
        ];
        let log = automatic_recovery_log(
            [2026, 11, 19, 14, 11, 58, 0],
            r"\EFI\Microsoft\Boot\missing-nt61-loader.efi",
            recovery_guid,
            r"\windows\system32\winload.efi",
        )
        .unwrap();
        let number = |offset| u32::from_le_bytes(log[offset..offset + 4].try_into().unwrap());
        assert_eq!(log.len(), 0x14c);
        assert!(is_v2_header(log[..16].try_into().unwrap()));
        assert_eq!(number(12), 0x14c);
        assert_eq!((number(0x28), number(0x34)), (0x40, 1));
        assert_eq!((number(0x68), number(0x6c), number(0x74)), (0x84, 3, 0x13));
        assert_eq!(number(0x78), 0xc000_000f);
        assert_eq!((number(0xec), number(0xf0), number(0xf8)), (0x78, 1, 0x11));
        assert_eq!(
            log[0xfc..0x10c],
            [
                0x20, 0x2e, 0xc4, 0xda, 0xd1, 0xe3, 0xf1, 0x11, 0x9f, 0x5b, 0xcf, 0x2a, 0xe4, 0x3d,
                0xcc, 0x1a,
            ]
        );
        assert_eq!(number(0x10c), 2);
        assert_eq!(log[0x110..0x114], [b'\\', 0, b'w', 0]);
        assert_eq!(log[0x14a..], [0; 2]);
    }

    #[test]
    fn validates_existing_header_and_bounds_paths() {
        let time = [2026, 11, 19, 14, 11, 58, 0];
        assert!(automatic_recovery_log(time, "missing\0path", [0; 16], "winload").is_none());
        assert!(
            automatic_recovery_log(time, &"x".repeat(CAPACITY / 2), [0; 16], "winload").is_none()
        );
        let log = automatic_recovery_log(time, "é", [0; 16], "winload").unwrap();
        assert_eq!(log[0x7c..0x80], [0xe9, 0, 0, 0]);
        let mut header: [u8; FILE_HEADER_SIZE] = log[..16].try_into().unwrap();
        header[12..16].copy_from_slice(&(CAPACITY as u32 + 1).to_le_bytes());
        assert!(!is_v2_header(&header));
        assert!(!is_v2_header(&[0; FILE_HEADER_SIZE]));
    }
}

//! Device pen/brush catalog returned by Win7 SP1 NtGdiEnumObjects.
//! This enumerates supported LOGPEN/LOGBRUSH descriptions, not allocated GDI
//! handles. Original x64 instructions and tables are retained in the test corpus.
use super::dc;
use crate::{arch::common::syscall_frame::SyscallFrame, mm::user_copy};

const RECORD_BYTES: usize = 16;
const MAX_RECORDS: usize = 140;
const COLORS: [u32; 20] = [
    0x000000, 0xffffff, 0x0000ff, 0x00ff00, 0x00ffff,
    0xff0000, 0xff00ff, 0xffff00, 0x808080, 0xc0c0c0,
    0x000080, 0x008000, 0x008080, 0x800000, 0x800080,
    0x808000, 0xc0dcc0, 0xf0c8a4, 0xf0fbff, 0xa4a0a0,
];

fn color_count(device_colors: u32) -> usize {
    match device_colors {
        20.. => 20, // includes NUMCOLORS=-1 for a true-color device
        16..=19 => 16,
        8..=15 => 8,
        2..=7 => 2,
        count => count as usize,
    }
}

fn record_count(device_colors: u32, kind: u32) -> Option<usize> {
    let multiplier = match kind { 1 => 5, 2 => 7, _ => return None };
    Some(color_count(device_colors) * multiplier)
}

fn snapshot(device_colors: u32, kind: u32) -> Option<([u8; MAX_RECORDS * RECORD_BYTES], usize)> {
    let count = record_count(device_colors, kind)?;
    let colors = &COLORS[..color_count(device_colors)];
    let mut bytes = [0; MAX_RECORDS * RECORD_BYTES];
    let mut index = 0;
    if kind == 1 {
        for style in 0u32..5 {
            for color in colors {
                let record = &mut bytes[index * RECORD_BYTES..(index + 1) * RECORD_BYTES];
                record[..4].copy_from_slice(&style.to_le_bytes());
                // LOGPEN width.x and width.y both remain zero.
                record[12..16].copy_from_slice(&color.to_le_bytes());
                index += 1;
            }
        }
    } else {
        for style in 0..7 {
            for color in colors {
                let record = &mut bytes[index * RECORD_BYTES..(index + 1) * RECORD_BYTES];
                let (brush_style, hatch) = if style == 0 { (0u32, 0u64) } else { (2, style - 1) };
                record[..4].copy_from_slice(&brush_style.to_le_bytes());
                record[4..8].copy_from_slice(&color.to_le_bytes());
                record[8..16].copy_from_slice(&hatch.to_le_bytes());
                index += 1;
            }
        }
    }
    Some((bytes, count))
}

pub extern "C" fn nt_gdi_enum_objects(frame: &SyscallFrame) -> u64 {
    let size = frame.args[2] as u32 as usize;
    let output = frame.args[3];
    // Win7 rejects mismatched query arguments before locking the DC.
    if (size == 0) != (output == 0) { return 0; }
    let Some(dc) = dc::get_dc(frame.args[0]) else { return 0; };
    // Only display and compatible memory DCs have a device backend here.
    if !matches!(dc.dc_type, 1 | 2) { return 0; }
    let device_colors = dc::caps::device_color_count();
    drop(dc);
    let kind = frame.args[1] as u32;
    let Some(count) = record_count(device_colors, kind) else { return 0; };
    if count == 0 { return 0; }
    // Original code treats every size below one record as a count query,
    // without probing the nonnull pointer paired with sizes 1..15.
    if size / RECORD_BYTES == 0 { return count as u64; }
    if size / RECORD_BYTES < count { return 0; }
    // Probe the entire supplied range, including any unwritten tail. Original
    // ProbeForWrite requires DWORD alignment for both native record layouts.
    if output & 3 != 0 || user_copy::probe_user_write(output, size).is_err() { return 0; }
    let Some((bytes, count)) = snapshot(device_colors, kind) else { return 0; };
    if user_copy::copy_to_user(output, &bytes[..count * RECORD_BYTES]).is_err() { return 0; }
    count as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_catalog_bytes_match_executed_win7_instructions() {
        let original = include_bytes!("../../tests/data/win7-enumobjects-x64.bin");
        let mut offset = 0;
        for colors in [1, 2, 8, 16, 20] {
            for kind in [1, 2] {
                let (actual, count) = snapshot(colors, kind).unwrap();
                let length = count * RECORD_BYTES;
                assert_eq!(&actual[..length], &original[offset..offset + length]);
                assert!(actual[length..].iter().all(|&byte| byte == 0));
                offset += length;
            }
        }
        assert_eq!(offset, original.len());
        for (colors, count) in [(0, 0), (3, 2), (7, 2), (9, 8), (15, 8), (17, 16),
            (19, 16), (21, 20), (u32::MAX, 20)] {
            assert_eq!(snapshot(colors, 1).unwrap().1, count * 5);
            assert_eq!(snapshot(colors, 2).unwrap().1, count * 7);
        }
    }
}

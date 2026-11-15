//! NT 6.1 text heuristic in the process's non-DBCS code-page mode.
//! The 256-word scan bound and result-mask behavior were checked against the
//! supplied Windows 7 SP1 AMD64 ntdll!RtlIsTextUnicode, not a UTF-16 validator.

/// `bytes` contains at least `min(size & !1, 512)` bytes. `size` retains the
/// original DWORD bit pattern, including an INT caller's negative values.
pub fn classify(bytes: &[u8], size: u32, mask: Option<u32>) -> (bool, u32) {
    let mut count = (size as usize / 2).min(256);
    if count == 0 || (size == 2 && bytes[0] != 0 && bytes[1] == 0) {
        return (false, 5);
    }
    if size > 2 && size <= 512 && size & 1 == 0 && bytes[count * 2 - 1] == 0 {
        count -= 1;
    }
    let mut low_diff = 0u32;
    let mut high_diff = 0u32;
    let mut previous_low = 0u32;
    let mut previous_high = 0u32;
    let mut zero_bytes = 0u32;
    let mut controls = false;
    let mut reverse_controls = false;
    let mut illegal = false;
    let mut packed_controls = 0u32;
    for pair in bytes[..count * 2].chunks_exact(2) {
        let low = u32::from(pair[0]);
        let high = u32::from(pair[1]);
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        controls |= matches!(value, 0x20 | 9 | 10 | 13 | 0x3000);
        reverse_controls |= matches!(value, 0x2000 | 0x900 | 0xa00 | 0xd00);
        illegal |= matches!(value, 0 | 0xa0d | 0xfffe | 0xffff);
        if (low == 13 && previous_high == 10) || (low == 10 && previous_high == 13) {
            packed_controls += 1;
        }
        zero_bytes += u32::from(low == 0) + u32::from(high == 0);
        low_diff += low.abs_diff(previous_low);
        high_diff += high.abs_diff(previous_high);
        previous_low = low;
        previous_high = high;
    }
    if (previous_low == 13 && previous_high == 10) || (previous_low == 10 && previous_high == 13) {
        packed_controls += 1;
    }
    if previous_high == 0 {
        zero_bytes -= 1;
    } else if previous_high == 0x1a {
        packed_controls += 1;
    }
    let mut flags = 0u32;
    if low_diff < 127 && high_diff == 0 {
        flags = 1;
    } else if low_diff == 0 && high_diff != 0 {
        flags = 0x10;
    }
    if 3 * high_diff < low_diff {
        flags |= 2;
    }
    if 3 * low_diff < high_diff {
        flags |= 0x20;
    }
    if controls {
        flags |= 4;
    }
    if reverse_controls {
        flags |= 0x40;
    }
    if illegal || (packed_controls != 0 && packed_controls >= size.min(512) / 40) {
        flags |= 0x100;
    }
    if size & 1 != 0 {
        flags |= 0x200;
    }
    if zero_bytes != 0 {
        flags |= 0x1000;
    }
    match u16::from_le_bytes([bytes[0], bytes[1]]) {
        0xfeff => flags |= 8,
        0xfffe => flags |= 0x80,
        _ => {}
    }
    if let Some(requested) = mask {
        flags &= requested;
    }
    let result = if flags & 8 != 0 && flags & 0xb00 == 0 {
        true
    } else if flags & 0xff0 != 0 {
        false
    } else {
        flags & 0xf00f != 0
    };
    (result, flags)
}

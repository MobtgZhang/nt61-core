//! Capabilities of the current framebuffer device, not a printer or remote DC.
use super::{get_dc, mapping::DisplayMetrics};
use crate::{arch::common::syscall_frame::SyscallFrame, mm::user_copy};

fn snapshot() -> [u32; 36] {
    let metrics = DisplayMetrics::current();
    let mut caps = [0u32; 36];
    caps[0] = 0x400; // GDI driver version
    caps[1] = 1; // DT_RASDISPLAY
    caps[2] = metrics.micrometers.cx as u32;
    caps[3] = metrics.micrometers.cy as u32;
    caps[4] = caps[2] / 1000;
    caps[5] = caps[3] / 1000;
    caps[6] = metrics.pixels.cx as u32;
    caps[7] = metrics.pixels.cy as u32;
    caps[8] = 32;
    caps[9] = 1;
    caps[10] = u32::MAX; // pens are not limited by a hardware palette
    caps[12] = u32::MAX; // true-color device
    caps[13] = 1 | 8 | 0x800; // RC_BITBLT | RC_BITMAP64 | RC_STRETCHBLT
    caps[14] = 36;
    caps[15] = 36;
    caps[16] = 51;
    caps[17] = 96;
    caps[18] = 96;
    caps[20] = 24;
    caps[27] = caps[6];
    caps[28] = caps[7];
    // No palette, printer geometry, refresh/EDID, ICM or alpha acceleration.
    caps
}
pub extern "C" fn nt_gdi_get_device_caps(frame: &SyscallFrame) -> u64 {
    let Some(_dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    let caps = snapshot();
    let slot = match frame.args[1] as i32 {
        0 => 0,
        2 => 1,
        4 => 4,
        6 => 5,
        8 => 6,
        10 => 7,
        12 => 8,
        14 => 9,
        18 => 10,
        22 => 11,
        24 => 12,
        38 => 13,
        40 => 14,
        42 => 15,
        44 => 16,
        88 => 17,
        90 => 18,
        104 => 19,
        108 => 20,
        110 => 21,
        111 => 22,
        112 => 23,
        113 => 24,
        34 => 25,
        116 => 26,
        118 => 27,
        117 => 28,
        119 => 29,
        120 => 34,
        121 => 35,
        _ => return 0,
    };
    caps[slot] as u64
}
pub extern "C" fn nt_gdi_get_device_caps_all(frame: &SyscallFrame) -> u64 {
    let Some(_dc) = get_dc(frame.args[0]) else {
        return 0;
    };
    user_copy::write_user_object(frame.args[1], &snapshot()).is_ok() as u64
}

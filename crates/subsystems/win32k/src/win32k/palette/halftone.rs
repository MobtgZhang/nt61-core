//! Native halftone format palettes, independent of a device or palette handle.

use crate::{arch::common::syscall_frame::SyscallFrame, mm::user_copy};

const FORMAT_COLOR_COUNT: usize = 6 * 6 * 6;
const PALETTE_ENTRY_SIZE: usize = 4;

fn fill_format_palette(bytes: &mut [u8]) {
    for (index, entry) in bytes.chunks_exact_mut(PALETTE_ENTRY_SIZE).enumerate() {
        entry[0] = ((index % 6) * 51) as u8;
        entry[1] = (((index / 6) % 6) * 51) as u8;
        entry[2] = ((index / 36) * 51) as u8;
        entry[3] = 0;
    }
}

/// Return the Win7 8-bit halftone format's 216 PALETTEENTRY values.
///
/// A null output queries the count. The Win7 SP1 implementation accepts three
/// USHORT gamma arguments but does not apply them to this format: red varies
/// fastest across a six-level RGB cube, and every entry has zero flags.
pub extern "C" fn nt_gdi_ht_get_8bpp_format_palette(frame: &SyscallFrame) -> u64 {
    let output = frame.args[0];
    if output == 0 {
        return FORMAT_COLOR_COUNT as u64;
    }

    let mut bytes = [0u8; FORMAT_COLOR_COUNT * PALETTE_ENTRY_SIZE];
    fill_format_palette(&mut bytes);
    if user_copy::copy_to_user(output, &bytes).is_err() {
        return 0;
    }
    FORMAT_COLOR_COUNT as u64
}

fn mask_levels(mask: u8) -> Option<[u8; 3]> {
    let levels = match mask {
        0 => return Some([0; 3]), // A 256-entry grayscale ramp.
        1 => [4; 3],
        2 => [5; 3],
        _ => [mask >> 5, (mask >> 2) & 7, mask & 3],
    };
    levels.iter().all(|&level| level != 0).then_some(levels)
}

fn subtractive_channel(step: u8, maximum: u8) -> u8 {
    if step >= maximum {
        return 0;
    }
    // Win7 first rounds the ratio to six decimal places, then rounds the
    // complementary 8-bit channel. Combining these divisions changes ties.
    let maximum = u32::from(maximum);
    let fixed = (u32::from(step) * 1_000_000 + maximum / 2) / maximum;
    ((255_500_000 - fixed * 255) / 1_000_000) as u8
}

fn set_mask_color(bytes: &mut [u8], reverse: bool, index: usize, color: [u8; 3]) {
    let physical_index = if reverse { 255 - index } else { index };
    let offset = physical_index * PALETTE_ENTRY_SIZE;
    bytes[offset..offset + 3].copy_from_slice(&color);
}

fn fill_mask_palette(bytes: &mut [u8], mask: u8, levels: [u8; 3]) {
    let reverse = bytes[..4] == *b"RGB0";
    bytes.fill(0);
    if mask == 0 {
        for index in 0..256 {
            set_mask_color(bytes, reverse, index, [255 - index as u8; 3]);
        }
        return;
    }

    let color_count = levels
        .iter()
        .map(|&level| usize::from(level) + 1)
        .product::<usize>();
    let prefix = if reverse { (256 - color_count) / 2 } else { 0 };
    for index in 0..prefix {
        set_mask_color(bytes, reverse, index, [255; 3]);
    }
    // An odd-sized centered cube repeats its middle entry so the white and
    // black padding have equal lengths in the 256-entry output.
    let duplicate_at = if reverse && color_count % 2 != 0 {
        prefix + color_count / 2
    } else {
        256
    };
    let dimensions = if reverse || mask == 1 || mask == 2 {
        levels
    } else {
        [7, 7, 3]
    };
    let mut index = prefix;
    for cyan in 0..=dimensions[0] {
        for magenta in 0..=dimensions[1] {
            for yellow in 0..=dimensions[2] {
                let color = [
                    subtractive_channel(cyan, levels[0]),
                    subtractive_channel(magenta, levels[1]),
                    subtractive_channel(yellow, levels[2]),
                ];
                set_mask_color(bytes, reverse, index, color);
                if index == duplicate_at {
                    index += 1;
                    set_mask_color(bytes, reverse, index, color);
                }
                index += 1;
            }
        }
    }
}

/// Build a masked halftone palette, capturing its input layout marker first.
///
/// Parameters are an in/out PALETTEENTRY pointer, BOOL Use8BPPMaskPal, BYTE
/// CMYMask, and three USHORT gamma values. Win7 ignores gamma here. With the
/// BOOL clear this produces the 216-entry format palette; otherwise it returns
/// 256 entries. The marker `RGB0` requests reversed, centered cube ordering.
pub extern "C" fn nt_gdi_ht_get_8bpp_mask_palette(frame: &SyscallFrame) -> u64 {
    let use_mask = frame.args[1] as u32 != 0;
    let mask = frame.args[2] as u8;
    let levels = if use_mask {
        let Some(levels) = mask_levels(mask) else {
            return 0;
        };
        levels
    } else {
        [0; 3]
    };
    let count = if use_mask { 256 } else { FORMAT_COLOR_COUNT };
    let output = frame.args[0];
    if output == 0 {
        return count as u64;
    }
    let mut storage = [0u8; 256 * PALETTE_ENTRY_SIZE];
    let bytes = &mut storage[..count * PALETTE_ENTRY_SIZE];
    if user_copy::copy_from_user(bytes, output).is_err() {
        return 0;
    }
    if use_mask {
        fill_mask_palette(bytes, mask, levels);
    } else {
        fill_format_palette(bytes);
    }
    if user_copy::copy_to_user(output, bytes).is_err() {
        return 0;
    }
    count as u64
}

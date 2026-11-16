//! Captured, uncompressed RGB DIB transfers. Decode before taking the DC lock;
//! malformed headers, palettes and buffers cannot cause partial destination writes.
//! RLE, JPEG/PNG, palette indices and color-management transforms need separate
//! engines and are rejected until those engines are available.

use super::*;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy;

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn dword(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn capture(address: u64, length: usize, limit: usize) -> Option<Vec<u8>> {
    if length > limit {
        return None;
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).ok()?;
    bytes.resize(length, 0);
    user_copy::copy_from_user(&mut bytes, address).ok()?;
    Some(bytes)
}

#[derive(Clone, Copy)]
struct Channels([u32; 3]);

impl Channels {
    fn new(masks: [u32; 3], depth: u16) -> Option<Self> {
        for (index, &mask) in masks.iter().enumerate() {
            if mask == 0 || (depth == 16 && mask > 0xffff) {
                return None;
            }
            let normalized = mask >> mask.trailing_zeros();
            if normalized & normalized.wrapping_add(1) != 0
                || masks[..index].iter().any(|other| mask & other != 0)
            {
                return None;
            }
        }
        Some(Self(masks))
    }

    fn rgb(self, value: u32) -> u32 {
        let mut rgb = 0;
        for (index, mask) in self.0.into_iter().enumerate() {
            let shift = mask.trailing_zeros();
            let width = mask.count_ones();
            let component = (value & mask) >> shift;
            let channel = if width >= 8 {
                component >> (width - 8)
            } else {
                // Expand the high bits by repetition (RGB555: abcdeabc,
                // RGB565 green: abcdefab), rather than linear rescaling.
                let mut expanded = component;
                let mut bits = width;
                while bits < 8 {
                    expanded = (expanded << width) | component;
                    bits += width;
                }
                expanded >> (bits - 8)
            };
            rgb |= channel << (16 - index * 8);
        }
        rgb
    }
}

struct Dib {
    surface: GdiSurface,
    bottom_up: bool,
    first_scan: u32,
}

impl Dib {
    fn capture(frame: &SyscallFrame, band: Option<(u32, u32)>) -> Option<Self> {
        if frame.args[11] as u32 != 0 || frame.args[15] != 0 {
            return None;
        }
        let info_address = frame.args[10];
        let info_limit = frame.args[13] as u32 as usize;
        let bits_limit = frame.args[if band.is_some() { 12 } else { 14 }] as u32 as usize;
        let prefix = capture(info_address, 4, info_limit)?;
        let header_size = dword(&prefix, 0)? as usize;
        if !matches!(header_size, 12 | 40 | 108 | 124) {
            return None;
        }
        let header = capture(info_address, header_size, info_limit)?;
        let core = header_size == 12;
        let (width, signed_height, planes, depth, compression, used) = if core {
            (
                i32::from(word(&header, 4)?),
                i32::from(word(&header, 6)?),
                word(&header, 8)?,
                word(&header, 10)?,
                0,
                0,
            )
        } else {
            (
                dword(&header, 4)? as i32,
                dword(&header, 8)? as i32,
                word(&header, 12)?,
                word(&header, 14)?,
                dword(&header, 16)?,
                dword(&header, 32)?,
            )
        };
        if width <= 0
            || signed_height == 0
            || planes != 1
            || !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32)
            || !matches!(compression, 0 | 3)
            || (compression == 3 && !matches!(depth, 16 | 32))
        {
            return None;
        }
        let full_height = signed_height.checked_abs()?;
        let (first_scan, height) = if let Some((first, count)) = band {
            if first >= full_height as u32 || count == 0 {
                return None;
            }
            (first, count.min(full_height as u32 - first) as i32)
        } else {
            (0, full_height)
        };
        let pixel_count = (width as usize).checked_mul(height as usize)?;
        if pixel_count > MAX_CAPTURE_PIXELS {
            return None;
        }
        let mut palette_offset = header_size;
        let channels = if compression == 3 {
            let extra_masks = if header_size == 40 {
                palette_offset = 52;
                Some(capture(info_address, palette_offset, info_limit)?)
            } else {
                None
            };
            let masks = extra_masks.as_deref().unwrap_or(&header);
            Channels::new(
                [dword(masks, 40)?, dword(masks, 44)?, dword(masks, 48)?],
                depth,
            )?
        } else if depth == 16 {
            Channels::new([0x7c00, 0x03e0, 0x001f], depth)?
        } else {
            Channels::new([0xff0000, 0xff00, 0xff], 32)?
        };
        let palette_count = if depth <= 8 {
            let maximum = 1usize << depth;
            if used == 0 {
                maximum
            } else if used as usize <= maximum {
                used as usize
            } else {
                return None;
            }
        } else {
            0
        };
        let entry_size = if core { 3 } else { 4 };
        let info_length = palette_offset.checked_add(palette_count.checked_mul(entry_size)?)?;
        let info = capture(info_address, info_length, info_limit)?;
        // Use only captured metadata from here onward; caller mutations of bmi
        // cannot change our validated dimensions or palette bounds.
        let stride = (width as usize)
            .checked_mul(depth as usize)?
            .checked_add(31)?
            / 32
            * 4;
        let bits_length = stride.checked_mul(height as usize)?;
        let bits = capture(frame.args[9], bits_length, bits_limit)?;
        let mut surface = GdiSurface::new(width, height, PIXEL_FORMAT_32BPP)?;
        for y in 0..height as usize {
            let row = if signed_height > 0 {
                height as usize - y - 1
            } else {
                y
            };
            let scan = &bits[row * stride..(row + 1) * stride];
            for x in 0..width as usize {
                let color = if depth <= 8 {
                    let index = match depth {
                        1 => (scan[x / 8] >> (7 - x % 8)) & 1,
                        4 => (scan[x / 2] >> (4 * (1 - x % 2))) & 15,
                        _ => scan[x],
                    } as usize;
                    if index >= palette_count {
                        return None;
                    }
                    let offset = palette_offset + index * entry_size;
                    u32::from(info[offset])
                        | (u32::from(info[offset + 1]) << 8)
                        | (u32::from(info[offset + 2]) << 16)
                } else {
                    let value = match depth {
                        16 => u32::from(word(scan, x * 2)?),
                        24 => {
                            u32::from(scan[x * 3])
                                | (u32::from(scan[x * 3 + 1]) << 8)
                                | (u32::from(scan[x * 3 + 2]) << 16)
                        }
                        _ => dword(scan, x * 4)?,
                    };
                    channels.rgb(value)
                };
                surface.set_pixel(x as i32, y as i32, color);
            }
        }
        Some(Self {
            surface,
            bottom_up: signed_height > 0,
            first_scan,
        })
    }
}

/// Win7 native argument order, including both buffer bounds and xform handle.
pub extern "C" fn nt_gdi_stretch_dibits_internal(frame: &SyscallFrame) -> u64 {
    let operation = frame.args[12] as u32;
    if operation & 0x4000_0000 != 0 {
        return 0;
    }
    let Some(dib) = Dib::capture(frame, None) else {
        return 0;
    };
    let target = [
        frame.args[1] as i32,
        frame.args[2] as i32,
        frame.args[3] as i32,
        frame.args[4] as i32,
    ];
    let mut input = [
        frame.args[5] as i32,
        frame.args[6] as i32,
        frame.args[7] as i32,
        frame.args[8] as i32,
    ];
    if dib.bottom_up {
        let Some(y) = dib
            .surface
            .height
            .checked_sub(input[1])
            .and_then(|y| y.checked_sub(input[3]))
        else {
            return 0;
        };
        input[1] = y;
    }
    if input[2] == 0 || input[3] == 0 || target[2] == 0 || target[3] == 0 {
        return 0;
    }
    let Some(right) = input[0].checked_add(input[2]) else {
        return 0;
    };
    let Some(bottom) = input[1].checked_add(input[3]) else {
        return 0;
    };
    if input[0].max(right) <= 0
        || input[0].min(right) >= dib.surface.width
        || input[1].max(bottom) <= 0
        || input[1].min(bottom) >= dib.surface.height
    {
        return 0;
    }
    let mut table = super::super::dc::get_dc_table();
    let Some(destination) = table.get(frame.args[0]) else {
        return 0;
    };
    if super::stretch::draw_dib(destination, &dib.surface, target, input, operation).is_none() {
        return 0;
    }
    let scans = if operation & 0x00ff_ffff != 0x00cc_0020 && !dib.bottom_up {
        -dib.surface.height
    } else {
        dib.surface.height
    };
    u64::from(scans as u32)
}

/// Win7's 16-argument unscaled DIB transfer, including the two native buffer
/// bounds and the coordinate-transform flag. The supplied pixel buffer starts
/// at the requested scan band, rather than at the beginning of a full bitmap.
pub extern "C" fn nt_gdi_set_dibits_to_device_internal(frame: &SyscallFrame) -> u64 {
    let width = frame.args[3] as u32;
    let height = frame.args[4] as u32;
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return 0;
    }
    let Some(dib) = Dib::capture(frame, Some((frame.args[7] as u32, frame.args[8] as u32))) else {
        return 0;
    };
    // Both row directions use a bottom-relative source origin; only the band
    // itself is decoded according to the sign of biHeight.
    let source_y = i64::from(dib.first_scan) + i64::from(dib.surface.height)
        - i64::from(frame.args[6] as i32)
        - i64::from(height);
    let Ok(source_y) = i32::try_from(source_y) else {
        return 0;
    };
    let source_x = frame.args[5] as i32;
    let right = i64::from(source_x) + i64::from(width);
    let bottom = i64::from(source_y) + i64::from(height);
    let mut table = super::super::dc::get_dc_table();
    let Some(destination) = table.get(frame.args[0]) else {
        return 0;
    };
    let mut scans = dib.surface.height;
    if source_y > 0 {
        if source_y >= scans {
            return if dib.bottom_up { 0 } else { scans as u64 };
        }
        if dib.bottom_up {
            scans -= source_y;
        }
    }
    if source_x >= dib.surface.width || right <= 0 || bottom <= 0 {
        return 0;
    }
    let target = [
        frame.args[1] as i32,
        frame.args[2] as i32,
        width as i32,
        height as i32,
    ];
    if super::stretch::draw_dib_device(
        destination,
        &dib.surface,
        target,
        PointL::new(source_x, source_y),
        frame.args[14] as u32 != 0,
    )
    .is_none()
    {
        return 0;
    }
    scans as u64
}

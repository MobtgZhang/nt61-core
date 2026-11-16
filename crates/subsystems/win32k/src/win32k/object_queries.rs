//! GDI object descriptions returned by NtGdiExtGetObjectW.
//!
//! Original Win7 SP1 x64 Nt (0x89208), Gre (0xb8cf0), and brush helper
//! (0x1fba08) are covered by an independently executed machine-code corpus.
//! DIBSECTION remains a separate dependency. Current bitmaps own kernel
//! storage, not a DIB section;
//! their BITMAP queries must never expose that pool address as bmBits.

use super::objects::{self, BrushStyle, GdiBitmap, GdiBrush, GdiObjectType, GdiPalette};
use crate::{arch::common::syscall_frame::SyscallFrame, mm::user_copy};

#[derive(Clone, Copy, Debug)]
pub(super) enum Description {
    Brush { style: u32, color: u32, hatch: u64 },
    Bitmap { width: i32, height: i32, depth: u16 },
    Palette { entries: u16 },
}

fn snapshot(handle: u64) -> Option<Description> {
    objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        if entry.obj_ptr == 0
            || entry.process_id != 0
                && entry.process_id != super::window::input_context::caller_identity().0 as u32
        {
            return None;
        }
        match entry.get_type()? {
            GdiObjectType::Brush => {
                // SAFETY: validated type and owner; table lock prevents deletion.
                let brush = unsafe { &*(entry.obj_ptr as *const GdiBrush) };
                let (color, hatch) = match brush.style {
                    x if x == BrushStyle::Solid as u32 => (brush.color, 0),
                    x if x == BrushStyle::Null as u32 => (0, 0),
                    x if x == BrushStyle::Hatched as u32 => {
                        (brush.color, u64::from(brush.hatch as u32))
                    }
                    // No owned pattern or DIB brush objects exist yet.
                    _ => return None,
                };
                Some(Description::Brush {
                    style: brush.style,
                    color,
                    hatch,
                })
            }
            GdiObjectType::Bitmap => {
                // SAFETY: validated type and owner; copy scalar state under lock.
                let bitmap = unsafe { &*(entry.obj_ptr as *const GdiBitmap) };
                Some(Description::Bitmap {
                    width: bitmap.width,
                    height: bitmap.height.checked_abs()?,
                    depth: bitmap.bit_count,
                })
            }
            GdiObjectType::Palette => {
                // SAFETY: palette mutations and deletion also hold this lock.
                let palette = unsafe { &*(entry.obj_ptr as *const GdiPalette) };
                Some(Description::Palette {
                    entries: u16::try_from(palette.num_entries).ok()?,
                })
            }
            _ => None,
        }
    })
}

/// Return zero-initialized ABI bytes, the native return value and bytes copied.
/// The separate copy length preserves the native brush prefix-copy behavior.
/// BITMAP alignment padding is deliberately zero rather than kernel stack data.
pub(super) fn serialize(
    object: Description,
    pointer_bytes: usize,
    supplied: usize,
    query: bool,
) -> Option<([u8; 32], usize, usize)> {
    if !matches!(pointer_bytes, 4 | 8) {
        return None;
    }
    let mut bytes = [0; 32];
    let (size, copied) = match object {
        Description::Brush {
            style,
            color,
            hatch,
        } => {
            let size = 8 + pointer_bytes;
            if !query && supplied == 0 {
                return None;
            }
            bytes[..4].copy_from_slice(&style.to_le_bytes());
            bytes[4..8].copy_from_slice(&color.to_le_bytes());
            bytes[8..size].copy_from_slice(&hatch.to_le_bytes()[..pointer_bytes]);
            (size, supplied.min(size))
        }
        Description::Bitmap {
            width,
            height,
            depth,
        } => {
            let size = if pointer_bytes == 8 { 32 } else { 24 };
            if !query && supplied < size {
                return None;
            }
            if width <= 0 || height <= 0 || !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32) {
                return None;
            }
            let bits = u64::try_from(width).ok()?.checked_mul(u64::from(depth))?;
            let stride =
                i32::try_from(bits.checked_add(15)?.checked_div(16)?.checked_mul(2)?).ok()?;
            bytes[4..8].copy_from_slice(&width.to_le_bytes());
            bytes[8..12].copy_from_slice(&height.to_le_bytes());
            bytes[12..16].copy_from_slice(&stride.to_le_bytes());
            bytes[16..18].copy_from_slice(&1u16.to_le_bytes());
            bytes[18..20].copy_from_slice(&depth.to_le_bytes());
            // bmType and bmBits remain zero for DDBs, regardless of cbBuffer.
            (size, size)
        }
        Description::Palette { entries } => {
            if !query && supplied < 2 {
                return None;
            }
            bytes[..2].copy_from_slice(&entries.to_le_bytes());
            (2, 2)
        }
    };
    Some((bytes, size, if query { 0 } else { copied }))
}

/// Only brush, DDB bitmap and palette branches; not a complete service endpoint.
pub extern "C" fn nt_gdi_ext_get_simple_object_w(frame: &SyscallFrame) -> u64 {
    let handle = frame.args[0];
    let count = frame.args[1] as i32;
    let supplied = if (0..=0x250).contains(&count) {
        count as usize
    } else {
        0x250
    };
    let output = frame.args[2];
    // Original bitmap/palette NULL-output size queries precede object locking.
    // Translate that behavior using this repository's synthetic type prefixes.
    let prefix = (handle >> 16) as u32 & !0x80;
    if output == 0 {
        match prefix {
            0x4002 => {
                return if core::mem::size_of::<usize>() == 8 {
                    32
                } else {
                    24
                }
            }
            0x4003 => return 2,
            _ => {}
        }
    }
    let Some(description) = snapshot(handle) else {
        if prefix == 0x4007 {
            super::input_services::set_last_error(87);
        }
        return 0;
    };
    let Some((bytes, result, copied)) = serialize(
        description,
        core::mem::size_of::<usize>(),
        supplied,
        output == 0,
    ) else {
        return 0;
    };
    if output != 0
        && (output & 1 != 0 || user_copy::copy_to_user(output, &bytes[..copied]).is_err())
    {
        return 0;
    }
    result as u64
}

/// Shared dispatcher for the implemented object families. Unsupported classes
/// return zero without changing last error, as does the original Gre dispatcher.
/// DIBSECTION queries remain incomplete until mapped section state exists.
pub extern "C" fn nt_gdi_ext_get_object_w(frame: &SyscallFrame) -> u64 {
    match (frame.args[0] >> 16) as u32 & !0x80 {
        0x4005 | 0x4006 => super::pen_services::nt_gdi_ext_get_pen_object_w(frame),
        0x4004 => query_description(frame, GdiObjectType::Font),
        0x400c => query_description(frame, GdiObjectType::ColorSpace),
        _ => nt_gdi_ext_get_simple_object_w(frame),
    }
}

/// Native LFONTOBJ keeps356+4*axis_count bytes (at most420). NtHfontCreate
/// clears axis_count before publication, so its current objects always have356.
pub(super) fn serialize_font(
    payload: &[u8],
    supplied: usize,
    query: bool,
) -> Option<([u8; 420], usize)> {
    if !(356..=420).contains(&payload.len()) || (payload.len() - 356) % 4 != 0 {
        return None;
    }
    let mut bytes = [0; 420];
    let length = if query {
        payload.len()
    } else {
        supplied.min(payload.len())
    };
    if !query {
        bytes[..length].copy_from_slice(&payload[..length]);
    }
    Some((bytes, length))
}

/// Original helper copies only260 filename bytes, leaving bytes328..588
/// unspecified. Initialize that range instead of returning kernel stack data.
pub(super) fn serialize_color_space(
    payload: &[u8; 592],
    supplied: usize,
    query: bool,
) -> Option<([u8; 592], usize)> {
    if query || supplied < 588 {
        return None;
    }
    let mut bytes = [0; 592];
    bytes[..328].copy_from_slice(&payload[..328]);
    let length = if supplied >= 592 {
        bytes[588..].copy_from_slice(&payload[588..]);
        592
    } else {
        588
    };
    Some((bytes, length))
}

fn query_description(frame: &SyscallFrame, kind: GdiObjectType) -> u64 {
    let count = frame.args[1] as i32;
    let supplied = if (0..=0x250).contains(&count) {
        count as usize
    } else {
        0x250
    };
    let output = frame.args[2];
    // Color-space size queries and short output fail before object acquisition.
    if kind == GdiObjectType::ColorSpace && (output == 0 || supplied < 588) {
        return 0;
    }
    let mut captured = [0; 592];
    let length = objects::with_handle_table(|table| {
        let entry = table.get(frame.args[0])?;
        if entry.get_type()? != kind
            || entry.obj_ptr == 0
            || entry.process_id != 0
                && entry.process_id != super::window::input_context::caller_identity().0 as u32
        {
            return None;
        }
        match kind {
            GdiObjectType::Font => {
                // SAFETY: validated allocation stays live under the table lock.
                let font = unsafe { &*(entry.obj_ptr as *const objects::GdiFont) };
                captured[..356].copy_from_slice(&font.logical_description());
                Some(356)
            }
            GdiObjectType::ColorSpace => {
                // SAFETY: color_space::nt_gdi_create_color_space publishes an
                // allocation of exactly592 captured bytes under this type.
                captured = unsafe { *(entry.obj_ptr as *const [u8; 592]) };
                Some(592)
            }
            _ => None,
        }
    });
    let Some(length) = length else {
        return 0;
    };
    let mut bytes = [0; 592];
    let result = if kind == GdiObjectType::Font {
        let Some((font_bytes, length)) = serialize_font(&captured[..length], supplied, output == 0)
        else {
            return 0;
        };
        bytes[..length].copy_from_slice(&font_bytes[..length]);
        length
    } else {
        let Some((color_bytes, length)) = serialize_color_space(&captured, supplied, output == 0)
        else {
            return 0;
        };
        bytes = color_bytes;
        length
    };
    if output != 0
        && result != 0
        && (output & 1 != 0 || user_copy::copy_to_user(output, &bytes[..result]).is_err())
    {
        return 0;
    }
    result as u64
}

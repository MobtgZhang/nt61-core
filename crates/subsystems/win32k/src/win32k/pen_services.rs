//! Captured extended-pen state and pen-family object queries.
//!
//! Win7 SP1 x64 GreExtCreatePen (RVA 0x204bb8) and cjGetBrushOrPen
//! (0x1fba08) establish validation and serialization. Pattern/DIB brush
//! realization and wide/styled rasterization remain separate dependencies;
//! unsupported creation backends fail rather than publish incomplete objects.
//! The final internal Brush argument (and CreatePen fourth argument) is not
//! implemented: supplied handles are neither reused nor adopted. Supported
//! pens allocate fresh objects with embedded brush descriptions, so original
//! internal brush reuse/attribute semantics are an explicit remaining gap.

use super::objects::{self, GdiObjectType, GdiPen, PenStyle};
use crate::{arch::common::syscall_frame::SyscallFrame, mm::user_copy};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct PenParameters {
    pub style: u32,
    pub width: u32,
    pub brush_style: u32,
    pub color: u32,
    pub client_hatch: u64,
    pub hatch: u64,
    pub style_count: u32,
    pub old_style: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenError {
    InvalidParameter,
    BrushFailure,
}

/// None represents the stock NULL_PEN; all object state is otherwise embedded.
/// The optional slice distinguishes a NULL style pointer from captured data.
pub fn validate_pen(
    input: PenParameters,
    captured: Option<&[u32]>,
) -> Result<Option<GdiPen>, PenError> {
    let low = input.style & 0xf;
    if low == 5 {
        return Ok(None);
    }
    let width = (input.width as i32)
        .checked_abs()
        .ok_or(PenError::InvalidParameter)?;
    let geometric = input.style & 0xf0000;
    let join = (input.style >> 12) & 0xf;
    let cap = (input.style >> 8) & 0xf;
    if input.style & 0xfff0_00f0 != 0
        || !matches!(geometric, 0 | 0x10000)
        || join > 2
        || cap > 2
        || geometric == 0 && !input.old_style && width != 1
        || geometric == 0
            && !(input.brush_style == 0 || input.brush_style == 2 && matches!(input.hatch, 8 | 10))
        || if low == 7 {
            !(1..=16).contains(&input.style_count)
        } else {
            input.style_count != 0
        }
        || if geometric != 0 {
            !matches!(low, 0..=4 | 6 | 7)
        } else {
            !(matches!(low, 0..=4 | 7 | 8) || low == 6 && input.old_style)
        }
    {
        return Err(PenError::InvalidParameter);
    }

    let mut pen = GdiPen::new(PenStyle::Solid, width, input.color);
    pen.type_ = if input.old_style {
        GdiObjectType::Pen
    } else {
        GdiObjectType::ExtPen
    } as u8;
    pen.style = input.style;
    pen.legacy = input.old_style;
    pen.brush_style = input.brush_style;
    pen.client_hatch = input.client_hatch;
    pen.hatch = input.hatch;
    let template: &[u32] = match (geometric != 0, low) {
        (_, 7) => {
            let count = input.style_count as usize;
            let Some(entries) = captured.and_then(|v| v.get(..count)) else {
                return Err(PenError::InvalidParameter);
            };
            pen.user_style_count = count as u8;
            pen.user_styles[..count].copy_from_slice(entries);
            entries
        }
        (true, 1) => &[3, 1],
        (true, 2) => &[1, 1],
        (true, 3) => &[3, 1, 1, 1],
        (true, 4) => &[3, 1, 1, 1, 1, 1],
        (false, 1) => &[6, 2],
        (false, 2) => &[1, 1, 1, 1, 1, 1, 1, 1],
        (false, 3) => &[3, 2, 1, 2],
        (false, 4) => &[3, 1, 1, 1, 1, 1],
        _ => &[],
    };
    let mut sum = 0i32;
    for (index, &entry) in template.iter().enumerate() {
        let mut value = entry as i32;
        if geometric != 0 && !input.old_style {
            if low != 7 {
                if cap != 2 {
                    value = value.wrapping_add(if index % 2 == 0 { -1 } else { 1 });
                }
                value = value.wrapping_mul(width);
            }
            if value < 0 {
                return Err(PenError::InvalidParameter);
            }
        } else if low == 7 && !(1..=0x3fff).contains(&value) {
            return Err(PenError::InvalidParameter);
        }
        sum = sum.wrapping_add(value);
        pen.realized_styles[index] = value as f32;
    }
    if !template.is_empty()
        && (sum <= 0 || (geometric == 0 || input.old_style) && low == 7 && sum > 0x3fff)
    {
        return Err(PenError::InvalidParameter);
    }
    pen.realized_style_count = template.len() as u8;
    match input.brush_style {
        1 => Ok(None),
        0 | 3 | 6 => Ok(Some(pen)),
        2 if input.hatch as u32 <= 11 => Ok(Some(pen)),
        2 => Err(PenError::BrushFailure),
        _ => Err(PenError::InvalidParameter),
    }
}

fn fail(error: u32) -> u64 {
    super::input_services::set_last_error(error);
    0
}

fn capture(pointer: u64, length: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).ok()?;
    bytes.resize(length, 0);
    user_copy::copy_from_user(&mut bytes, pointer).ok()?;
    Some(bytes)
}

/// Eleven native parameters; this is not the public five-argument GDI API.
pub extern "C" fn nt_gdi_ext_create_pen(frame: &SyscallFrame) -> u64 {
    let count = frame.args[6] as u32;
    let pointer = frame.args[7];
    let styles = if pointer == 0 {
        None
    } else {
        if count == 0 || count > 0x9c4000 {
            return 0;
        }
        let Some(bytes) = capture(pointer, count as usize * 4) else {
            return 0;
        };
        Some(bytes)
    };
    let brush_style = frame.args[2] as u32;
    let _dib = if brush_style == 6 {
        let size = frame.args[8] as u32;
        if size > 0x2710000 {
            return 0;
        }
        let Some(bytes) = capture(frame.args[5], size as usize) else {
            return 0;
        };
        Some(bytes)
    } else {
        None
    };
    let mut entries = [0u32; 16];
    if let Some(bytes) = &styles {
        for (slot, bytes) in entries.iter_mut().zip(bytes.chunks_exact(4)) {
            *slot = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
    }
    let input = PenParameters {
        style: frame.args[0] as u32,
        width: frame.args[1] as u32,
        brush_style,
        color: frame.args[3] as u32,
        client_hatch: frame.args[4],
        hatch: frame.args[5],
        style_count: count,
        old_style: frame.args[9] as u32 != 0,
    };
    match validate_pen(input, styles.as_ref().map(|_| entries.as_slice())) {
        Ok(None) => objects::gdi_get_stock_object(8),
        Ok(Some(pen)) => {
            // Pattern/DIB pens must own a realized brush/bitmap snapshot. Their
            // backend is not present yet; never publish a dangling user handle.
            if matches!(pen.brush_style, 3 | 6) {
                return fail(50);
            }
            let handle = objects::gdi_publish_pen(pen);
            if handle == 0 {
                fail(8)
            } else {
                handle
            }
        }
        Err(PenError::InvalidParameter) => fail(87),
        Err(PenError::BrushFailure) => 0,
    }
}

/// Strict legacy style validation before using the shared native constructor.
pub extern "C" fn nt_gdi_create_pen(frame: &SyscallFrame) -> u64 {
    let style = frame.args[0] as i32;
    if !(0..=6).contains(&style) {
        return fail(87);
    }
    let input = PenParameters {
        style: style as u32,
        width: frame.args[1] as u32,
        brush_style: 0,
        color: frame.args[2] as u32,
        client_hatch: 0,
        hatch: 0,
        style_count: 0,
        old_style: true,
    };
    match validate_pen(input, None) {
        Ok(None) => objects::gdi_get_stock_object(8),
        Ok(Some(pen)) => {
            let handle = objects::gdi_publish_pen(pen);
            if handle == 0 {
                fail(8)
            } else {
                handle
            }
        }
        Err(_) => fail(87),
    }
}

/// Serialize a pen into a zeroed fixed-size buffer using Windows field widths.
/// Query size depends on pointer width, never the layout of the Rust struct.
pub fn serialize_pen(
    pen: &GdiPen,
    pointer_bytes: usize,
    supplied: usize,
    query: bool,
) -> Option<([u8; 96], usize)> {
    if !matches!(pointer_bytes, 4 | 8) {
        return None;
    }
    let mut bytes = [0; 96];
    let special_null = pen.legacy
        && pen.style == 5
        && supplied == if pointer_bytes == 8 { 32 } else { 28 }
        && !query;
    let count = if pen.style & 0xf == 7 {
        usize::from(pen.user_style_count)
    } else {
        0
    };
    let base = 20 + pointer_bytes;
    let length = if special_null {
        if pointer_bytes == 8 {
            32
        } else {
            28
        }
    } else if pen.legacy {
        16
    } else {
        base + count * 4
    };
    if !query && supplied < length {
        return None;
    }
    bytes[..4].copy_from_slice(&pen.style.to_le_bytes());
    if special_null {
        return Some((bytes, length));
    }
    bytes[4..8].copy_from_slice(&pen.width.to_le_bytes());
    if pen.legacy {
        bytes[12..16].copy_from_slice(&pen.color.to_le_bytes());
    } else {
        bytes[8..12].copy_from_slice(&pen.brush_style.to_le_bytes());
        bytes[12..16].copy_from_slice(&pen.color.to_le_bytes());
        bytes[16..16 + pointer_bytes]
            .copy_from_slice(&pen.client_hatch.to_le_bytes()[..pointer_bytes]);
        bytes[16 + pointer_bytes..base].copy_from_slice(&(count as u32).to_le_bytes());
        for index in 0..count {
            // All captured USERSTYLE inputs are nonnegative LONG integers.
            // Their f32 realization remains integral; bFToL(mode6) therefore
            // has the same bit result, including rounding above 2^24.
            // Original bFToL mode6 differential fixtures also cover f32(0x7fffffff)
            // becoming 2^31 and returning DWORD 0x80000000 (success), not overflow.
            let value = if pen.style & 0xf0000 != 0 {
                pen.realized_styles[index] as u32
            } else {
                pen.user_styles[index]
            };
            bytes[base + index * 4..base + (index + 1) * 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Some((bytes, length))
}

/// Pen-family portion of GetObject. Other object classes need their own query
/// implementations before the complete NtGdiExtGetObjectW can be registered.
pub extern "C" fn nt_gdi_ext_get_pen_object_w(frame: &SyscallFrame) -> u64 {
    let Some(pen) = objects::gdi_get_pen(frame.args[0]) else {
        return fail(87);
    };
    let count = frame.args[1] as i32;
    let supplied = if !(0..=0x250).contains(&count) {
        0x250
    } else {
        count as usize
    };
    let output = frame.args[2];
    let Some((bytes, length)) =
        serialize_pen(&pen, core::mem::size_of::<usize>(), supplied, output == 0)
    else {
        return 0;
    };
    if output != 0
        && (output & 1 != 0 || user_copy::copy_to_user(output, &bytes[..length]).is_err())
    {
        return 0;
    }
    length as u64
}

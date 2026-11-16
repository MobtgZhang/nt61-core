//! Page mapping for the currently supported display device.
//!
//! The framebuffer supplies resolution but no physical monitor dimensions.
//! Until the display driver publishes those dimensions, use the system's
//! 96-DPI display model. This is not EDID or printer-device support.

use super::{
    DcObject, SizeL, MM_ANISOTROPIC, MM_HIENGLISH, MM_HIMETRIC, MM_ISOTROPIC, MM_LOENGLISH,
    MM_LOMETRIC, MM_TEXT, MM_TWIPS,
};

#[derive(Clone, Copy)]
pub(super) struct DisplayMetrics {
    pub pixels: SizeL,
    pub micrometers: SizeL,
}

impl DisplayMetrics {
    pub fn current() -> Self {
        let info = crate::hal::common::framebuffer::info();
        // Headless DCs use the same default display model as gdi32.
        let width = i32::try_from(info.width)
            .ok()
            .filter(|v| *v > 0)
            .unwrap_or(1024);
        let height = i32::try_from(info.height)
            .ok()
            .filter(|v| *v > 0)
            .unwrap_or(768);
        Self {
            pixels: SizeL::new(width, height),
            micrometers: SizeL::new(
                ((i64::from(width) * 25_400 + 48) / 96).min(i64::from(i32::MAX)) as i32,
                ((i64::from(height) * 25_400 + 48) / 96).min(i64::from(i32::MAX)) as i32,
            ),
        }
    }
}

fn logical_extent(micrometers: i64, mode: i32) -> i32 {
    let value = micrometers;
    let (numerator, denominator) = match mode {
        MM_LOMETRIC | MM_ISOTROPIC => (1, 100),
        MM_HIMETRIC => (1, 10),
        MM_LOENGLISH => (1, 254),
        MM_HIENGLISH => (10, 254),
        MM_TWIPS => (144, 2540),
        _ => return 1,
    };
    ((value * numerator + denominator / 2) / denominator).clamp(1, i64::from(i32::MAX)) as i32
}

pub(super) fn set_map_mode(dc: &mut DcObject, mode: i32) -> i32 {
    if !(MM_TEXT..=MM_ANISOTROPIC).contains(&mode) {
        return 0;
    }
    let layout = dc.attributes.layout;
    if mode != MM_ANISOTROPIC && layout & 7 != 0 {
        dc.set_layout(-1, 0);
    }
    let previous = dc.map_mode;
    if mode == previous {
        return previous;
    }
    match mode {
        MM_TEXT => {
            dc.window_ext = SizeL::new(1, 1);
            dc.viewport_ext = SizeL::new(1, 1);
        }
        MM_ANISOTROPIC => {}
        _ => {
            let metrics = DisplayMetrics::current();
            let virtual_values = dc.attributes.virtual_resolution;
            let (pixels, micrometers) = if virtual_values == [0; 4] {
                (metrics.pixels, [i64::from(metrics.micrometers.cx), i64::from(metrics.micrometers.cy)])
            } else {
                (SizeL::new(virtual_values[0] as i32, virtual_values[1] as i32),
                    [i64::from(virtual_values[2]) * 1000, i64::from(virtual_values[3]) * 1000])
            };
            dc.viewport_ext = SizeL::new(pixels.cx, pixels.cy.wrapping_neg());
            dc.window_ext = SizeL::new(
                logical_extent(micrometers[0], mode),
                logical_extent(micrometers[1], mode),
            );
        }
    }
    dc.map_mode = mode;
    if mode != MM_ANISOTROPIC && layout & 7 != 0 {
        dc.set_layout(-1, layout);
    }
    previous
}

/// Win7 EngMulDiv normalizes signed DWORD operands with wrapping negation,
/// rounds the quotient, and saturates an out-of-range magnitude.
fn eng_mul_div(mut value: i32, mut numerator: i32, mut denominator: i32) -> i32 {
    let mut sign = 1;
    if value < 0 { sign = -sign; value = value.wrapping_neg(); }
    if numerator < 0 { sign = -sign; numerator = numerator.wrapping_neg(); }
    if denominator < 0 { sign = -sign; denominator = denominator.wrapping_neg(); }
    let saturated = || if sign > 0 { i32::MAX } else { i32::MIN };
    if denominator == 0 { return saturated(); }
    let half = denominator.wrapping_sub(denominator >> 31) >> 1;
    let quotient = (i64::from(value) * i64::from(numerator) + i64::from(half))
        / i64::from(denominator);
    if quotient as u64 > i32::MAX as u64 { return saturated(); }
    if sign > 0 { quotient as i32 } else { (quotient as i32).wrapping_neg() }
}

/// The original bFToL(flags=6) rounds the float significand using integer
/// shifts. Preserve its wraparound and untouched-output overflow behavior,
/// including values outside the range of a Rust saturating float cast.
fn float_to_long(value: f32, previous: i32) -> i32 {
    let bits = value.to_bits();
    let shift = ((bits >> 23) & 0xff) as i32 - 118;
    if shift > 40 { return previous; }
    let significand = u64::from((bits & 0x7f_ffff) | 0x80_0000);
    let fixed = if shift >= 0 {
        significand.wrapping_shl(shift as u32)
    } else {
        significand.wrapping_shr(shift.wrapping_neg() as u32)
    };
    let magnitude = (fixed.wrapping_add(0x8000_0000) >> 32) as i32;
    if (bits as i32) < 0 { magnitude.wrapping_neg() } else { magnitude }
}

/// Compute the pending Win7 DC::vMakeIso adjustment. All arithmetic steps
/// use single precision in the same order as the original x64 instructions.
/// Native extent queries commit this result to the saved drawing state.
pub(crate) fn effective_viewport(dc: &DcObject) -> Option<SizeL> {
    let viewport = dc.viewport_ext;
    let window = dc.window_ext;
    if window.cx == 0 || window.cy == 0 {
        return None;
    }
    if dc.map_mode != MM_ISOTROPIC {
        return Some(viewport);
    }
    let [pixels_x, pixels_y, mm_x, mm_y] = dc.attributes.virtual_resolution.map(|v| v as i32);
    let device = dc.attributes.size_device;
    let ratio = if pixels_x != 0 {
        if device.cx != 0 && device.cy != 0 {
            eng_mul_div(pixels_y, device.cx, pixels_x) as f32 / device.cy as f32
        } else {
            mm_x.wrapping_mul(pixels_y) as f32 / pixels_x as f32 / mm_y as f32
        }
    } else {
        // The supported display's LOGPIXELSY / LOGPIXELSX is 96 / 96.
        1.0
    };
    let candidate_y = float_to_long(
        window.cy as f32 * viewport.cx as f32 / window.cx as f32 * ratio,
        ratio.to_bits() as i32,
    ).wrapping_abs();
    if viewport.cy.wrapping_abs() >= candidate_y {
        Some(SizeL::new(viewport.cx,
            if viewport.cy > 0 { candidate_y } else { candidate_y.wrapping_neg() }))
    } else {
        let candidate_x = float_to_long(
            window.cx as f32 * viewport.cy as f32 / window.cy as f32 / ratio,
            candidate_y,
        ).wrapping_abs();
        Some(SizeL::new(
            if viewport.cx > 0 { candidate_x } else { candidate_x.wrapping_neg() },
            viewport.cy,
        ))
    }
}

pub(super) fn set_extent(dc: &mut DcObject, size: SizeL, viewport: bool) -> bool {
    if dc.map_mode <= MM_TWIPS {
        return true;
    }
    if size.cx == 0 || size.cy == 0 {
        return false;
    }
    if viewport {
        dc.viewport_ext = size;
    } else {
        dc.window_ext = size;
    }
    dc.mirror_window_origin()
}

/// The original x64 implementation uses IMUL r32 followed by signed IDIV.
/// Preserve the 32-bit product; checked division prevents a kernel panic for
/// MIN / -1, for which this implementation deliberately returns failure.
pub(super) fn scale_extent(size: SizeL, factors: [i32; 4]) -> Option<SizeL> {
    let [x_num, x_den, y_num, y_den] = factors;
    let x = size.cx.wrapping_mul(x_num).checked_div(x_den)?;
    let y = size.cy.wrapping_mul(y_num).checked_div(y_den)?;
    (x != 0 && y != 0).then_some(SizeL::new(x, y))
}

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

fn logical_extent(micrometers: i32, mode: i32) -> i32 {
    let value = i64::from(micrometers);
    let (numerator, denominator) = match mode {
        MM_LOMETRIC | MM_ISOTROPIC => (1, 100),
        MM_HIMETRIC => (1, 10),
        MM_LOENGLISH => (1, 254),
        MM_HIENGLISH => (10, 254),
        MM_TWIPS => (144, 2540),
        _ => return 1,
    };
    ((value * numerator + denominator / 2) / denominator).max(1) as i32
}

pub(super) fn set_map_mode(dc: &mut DcObject, mode: i32) -> i32 {
    if !(MM_TEXT..=MM_ANISOTROPIC).contains(&mode) {
        return 0;
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
            dc.viewport_ext = SizeL::new(metrics.pixels.cx, -metrics.pixels.cy);
            dc.window_ext = SizeL::new(
                logical_extent(metrics.micrometers.cx, mode),
                logical_extent(metrics.micrometers.cy, mode),
            );
        }
    }
    dc.map_mode = mode;
    previous
}

/// Isotropic mapping reduces the larger physical scale, preserving signs.
/// Compute the effective extent without accumulating rounding on every query.
pub(crate) fn effective_viewport(dc: &DcObject) -> Option<SizeL> {
    let viewport = dc.viewport_ext;
    let window = dc.window_ext;
    if window.cx == 0 || window.cy == 0 || viewport.cx == 0 || viewport.cy == 0 {
        return None;
    }
    if dc.map_mode != MM_ISOTROPIC {
        return Some(viewport);
    }
    // The supported display reports equal 96-DPI axes. Win7 uses the
    // device's LOGPIXELSY / LOGPIXELSX ratio here, not monitor millimeters.
    let horizontal = (f64::from(viewport.cx) / f64::from(window.cx)).abs();
    let vertical = (f64::from(viewport.cy) / f64::from(window.cy)).abs();
    let signed_extent = |original: i32, ratio: f64| -> Option<i32> {
        let magnitude = (f64::from(original).abs() * ratio + 0.5) as i64;
        let magnitude = magnitude.max(1);
        i32::try_from(if original < 0 { -magnitude } else { magnitude }).ok()
    };
    if horizontal > vertical {
        Some(SizeL::new(
            signed_extent(viewport.cx, vertical / horizontal)?,
            viewport.cy,
        ))
    } else {
        Some(SizeL::new(
            viewport.cx,
            signed_extent(viewport.cy, horizontal / vertical)?,
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
    true
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

//! Integer midpoint scan conversion for native rounded and elliptic regions.
//! GDI excludes the right/bottom outline; odd and even diameters have different
//! central rows. Intermediates are wide enough for the full region coordinate
//! range, and corner storage/work are bounded before any allocation.

use super::super::{objects::Rect, region_geometry::*};
use alloc::vec::Vec;

pub(super) fn rounded(mut bounds: Rect, diameter: [i64; 2]) -> Result<Region, RegionError> {
    if !valid_rect(&bounds) {
        return Err(RegionError::InvalidCoordinate);
    }
    bounds.normalize();
    bounds.right = bounds.right.checked_sub(1).ok_or(RegionError::InvalidCoordinate)?;
    bounds.bottom = bounds.bottom.checked_sub(1).ok_or(RegionError::InvalidCoordinate)?;
    let width = (i64::from(bounds.right) - i64::from(bounds.left)).min(diameter[0].abs());
    let height = (i64::from(bounds.bottom) - i64::from(bounds.top)).min(diameter[1].abs());
    if width < 2 || height < 2 {
        return Region::rectangle(bounds);
    }
    if height as usize > MAX_REGION_RECTS || width + height > 8_388_608 {
        return Err(RegionError::ResourceLimit);
    }
    let height = height as usize;
    let middle = height / 2;
    let mut inset = Vec::new();
    inset.try_reserve_exact(height).map_err(|_| RegionError::ResourceLimit)?;
    inset.resize(height, 0i32);

    // Midpoint error for the ellipse's lower-right quadrant. Reflect its
    // horizontal insets to construct the upper half after the recurrence.
    let a = i128::from(width - 1);
    let b = height as i128 - 1;
    let step_x = 8 * b * b;
    let step_y = 8 * a * a;
    let mut delta_x = 4 * b * b * (1 - a);
    let mut delta_y = 4 * a * a * (1 + b % 2);
    let mut error = delta_x + delta_y + a * a * (b % 2);
    let mut x = 0;
    let mut y = middle;
    while x <= width / 2 {
        let doubled = 2 * error;
        if doubled >= delta_x {
            x += 1;
            delta_x += step_x;
            error += delta_x;
        }
        if doubled <= delta_y {
            y += 1;
            delta_y += step_y;
            error += delta_y;
            if let Some(value) = inset.get_mut(y) {
                *value = x as i32;
            }
        }
    }
    for row in 0..middle {
        inset[row] = inset[height - 1 - row];
    }
    let mut bands = Vec::new();
    bands.try_reserve_exact(height).map_err(|_| RegionError::ResourceLimit)?;
    for (row, inset) in inset.into_iter().enumerate() {
        let top = if row <= middle {
            bounds.top + row as i32
        } else {
            bounds.bottom - height as i32 + row as i32
        };
        let bottom = if row == middle {
            bounds.bottom - height as i32 + row as i32 + 1
        } else { top + 1 };
        let band = Rect::new(bounds.left + inset, top, bounds.right - inset, bottom);
        if !band.is_empty() { bands.push(band); }
    }
    Region::from_bands(&bands)
}

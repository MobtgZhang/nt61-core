//! Bounded scan conversion of multiple contours. Filling closes every contour;
//! outline rendering retains the caller's explicit open/closed figures.

extern crate alloc;

use super::dc::PointL;
use super::objects::Rect;
use super::region_geometry::{Region, RegionError, MAX_COORD, MIN_COORD};
use alloc::vec::Vec;

#[path = "polygon/services.rs"]
pub mod services;

const MAX_SCAN_WORK: usize = 8_388_608;
const MAX_SPANS: usize = 131_072;

/// Alternate (1) counts crossings modulo two; winding (2) counts directed edges.
/// Intersections use exact rational arithmetic, including negative coordinates.
pub fn region_from_contours(contours: &[&[PointL]], fill_mode: i32) -> Result<Region, RegionError> {
    if !matches!(fill_mode, 1 | 2) {
        return Err(RegionError::InvalidData);
    }
    let mut edges = Vec::new();
    let mut top = MAX_COORD;
    let mut bottom = MIN_COORD;
    for contour in contours {
        if contour.len() < 2 {
            continue;
        }
        for (first, second) in contour
            .iter()
            .zip(contour.iter().cycle().skip(1))
            .take(contour.len())
        {
            if [first.x, first.y, second.x, second.y]
                .iter()
                .any(|coordinate| !(MIN_COORD..=MAX_COORD).contains(coordinate))
            {
                return Err(RegionError::InvalidCoordinate);
            }
            if first.y == second.y {
                continue;
            }
            let (low, high, direction) = if first.y < second.y {
                (*first, *second, 1)
            } else {
                (*second, *first, -1)
            };
            edges
                .try_reserve(1)
                .map_err(|_| RegionError::ResourceLimit)?;
            edges.push((low, high, direction));
            top = top.min(low.y);
            bottom = bottom.max(high.y);
        }
    }
    if edges.is_empty() {
        return Ok(Region::default());
    }
    if (bottom - top) as usize > MAX_SCAN_WORK / edges.len() {
        return Err(RegionError::ResourceLimit);
    }
    let mut intersections: Vec<(i128, i128, i32)> = Vec::new();
    intersections
        .try_reserve_exact(edges.len())
        .map_err(|_| RegionError::ResourceLimit)?;
    let mut rows: Vec<Rect> = Vec::new();
    for y in top..bottom {
        intersections.clear();
        for &(low, high, direction) in &edges {
            if y < low.y || y >= high.y {
                continue;
            }
            // GDI polygon regions intersect integer scanlines. Rounding each
            // crossing upward includes the left boundary and excludes the
            // right one; sampling at y + 0.5 would shift sloping edges.
            let denominator = i128::from(high.y - low.y);
            let numerator = i128::from(low.x) * denominator
                + i128::from(y - low.y) * i128::from(high.x - low.x);
            intersections.push((numerator, denominator, direction));
        }
        intersections.sort_unstable_by(|a, b| (a.0 * b.1).cmp(&(b.0 * a.1)));
        let mut winding = 0;
        let mut left = None;
        let mut index = 0;
        while index < intersections.len() {
            let (numerator, denominator, _) = intersections[index];
            let was_inside = if fill_mode == 1 {
                winding % 2 != 0
            } else {
                winding != 0
            };
            while index < intersections.len()
                && intersections[index].0 * denominator == numerator * intersections[index].1
            {
                winding += intersections[index].2;
                index += 1;
            }
            let is_inside = if fill_mode == 1 {
                winding % 2 != 0
            } else {
                winding != 0
            };
            // Exact ceil, including crossings left of the device origin.
            let pixel = -(-numerator).div_euclid(denominator);
            let pixel = i32::try_from(pixel).map_err(|_| RegionError::InvalidCoordinate)?;
            if !was_inside && is_inside {
                left = Some(pixel);
            } else if was_inside && !is_inside {
                if let Some(start) = left.take().filter(|start| *start < pixel) {
                    if let Some(last) = rows
                        .last_mut()
                        .filter(|last| last.top == y && last.right == start)
                    {
                        last.right = pixel;
                    } else {
                        if rows.len() == MAX_SPANS {
                            return Err(RegionError::ResourceLimit);
                        }
                        rows.try_reserve(1)
                            .map_err(|_| RegionError::ResourceLimit)?;
                        rows.push(Rect::new(start, y, pixel, y + 1));
                    }
                }
            }
        }
    }
    Region::from_scanlines(&rows)
}

//! Integer, half-open GDI regions, stored as canonical horizontal bands.
//!
//! Rectangles in a band are disjoint and sorted by x. Adjacent bands with the
//! same spans are coalesced. This makes equality independent of construction
//! order and preserves holes during every Boolean operation.

extern crate alloc;

use super::objects::Rect;
use alloc::vec::Vec;

pub const MIN_COORD: i32 = -(1 << 26);
pub const MAX_COORD: i32 = (1 << 26) - 1;
pub const MAX_REGION_RECTS: usize = 16_384;
const MAX_SWEEP_WORK: usize = 8_388_608;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum RegionCombineMode {
    Intersection = 1,
    Union = 2,
    Xor = 3,
    Difference = 4,
    Copy = 5,
}

impl RegionCombineMode {
    pub fn from_raw(mode: i32) -> Option<Self> {
        match mode {
            1 => Some(Self::Intersection),
            2 => Some(Self::Union),
            3 => Some(Self::Xor),
            4 => Some(Self::Difference),
            5 => Some(Self::Copy),
            _ => None,
        }
    }

    fn includes(self, first: bool, second: bool) -> bool {
        match self {
            Self::Intersection => first && second,
            Self::Union => first || second,
            Self::Xor => first != second,
            Self::Difference => first && !second,
            Self::Copy => first,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum RegionType {
    Null = 1,
    Simple = 2,
    Complex = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionError {
    InvalidCoordinate,
    InvalidData,
    ResourceLimit,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Region {
    rects: Vec<Rect>,
}

fn push<T>(items: &mut Vec<T>, value: T) -> Result<(), RegionError> {
    items
        .try_reserve(1)
        .map_err(|_| RegionError::ResourceLimit)?;
    items.push(value);
    Ok(())
}

pub fn valid_rect(rect: &Rect) -> bool {
    rect.to_array()
        .iter()
        .all(|coordinate| (MIN_COORD..=MAX_COORD).contains(coordinate))
}

impl Region {
    pub fn rectangle(mut rect: Rect) -> Result<Self, RegionError> {
        if !valid_rect(&rect) {
            return Err(RegionError::InvalidCoordinate);
        }
        rect.normalize();
        let mut region = Self::default();
        if !rect.is_empty() {
            push(&mut region.rects, rect)?;
        }
        Ok(region)
    }

    /// RGNDATA rectangles must have ordered edges. Ignore empty rectangles.
    pub fn from_rectangles(rects: &[Rect]) -> Result<Self, RegionError> {
        if rects.len() > MAX_REGION_RECTS {
            return Err(RegionError::ResourceLimit);
        }
        for rect in rects {
            if !valid_rect(rect) || rect.left > rect.right || rect.top > rect.bottom {
                return Err(RegionError::InvalidData);
            }
        }
        Self::sweep(rects, &[], RegionCombineMode::Copy)
    }

    /// Scan converters produce sorted, disjoint spans with one-pixel height.
    /// Canonicalize those directly instead of running a quadratic Boolean sweep.
    pub(super) fn from_scanlines(rows: &[Rect]) -> Result<Self, RegionError> {
        if rows.iter().any(|row| row.top.checked_add(1) != Some(row.bottom)) {
            return Err(RegionError::InvalidData);
        }
        Self::from_bands(rows)
    }

    /// Canonicalize ordered, nonoverlapping horizontal bands.
    pub(super) fn from_bands(rows: &[Rect]) -> Result<Self, RegionError> {
        let mut result = Self::default();
        let mut previous_start = 0;
        let mut previous_count = 0;
        let mut index = 0;
        while index < rows.len() {
            let first = rows[index];
            if !valid_rect(&first) || first.is_empty() {
                return Err(RegionError::InvalidData);
            }
            if index > 0 && rows[index - 1].bottom > first.top {
                return Err(RegionError::InvalidData);
            }
            let start = index;
            while index < rows.len() && rows[index].top == first.top {
                let rect = rows[index];
                if !valid_rect(&rect) || rect.is_empty() || rect.bottom != first.bottom
                    || (index > start && rows[index - 1].right >= rect.left)
                {
                    return Err(RegionError::InvalidData);
                }
                index += 1;
            }
            let band = &rows[start..index];
            let matches_previous = previous_count == band.len()
                && band.iter().enumerate().all(|(offset, rect)| {
                    let previous = result.rects[previous_start + offset];
                    previous.bottom == rect.top
                        && previous.left == rect.left && previous.right == rect.right
                });
            if matches_previous {
                for rect in &mut result.rects[previous_start..] {
                    rect.bottom = first.bottom;
                }
            } else {
                if result.rects.len() + band.len() > MAX_REGION_RECTS {
                    return Err(RegionError::ResourceLimit);
                }
                result.rects.try_reserve(band.len()).map_err(|_| RegionError::ResourceLimit)?;
                previous_start = result.rects.len();
                previous_count = band.len();
                result.rects.extend_from_slice(band);
            }
        }
        Ok(result)
    }

    pub fn try_clone(&self) -> Result<Self, RegionError> {
        let mut rects = Vec::new();
        rects
            .try_reserve_exact(self.rects.len())
            .map_err(|_| RegionError::ResourceLimit)?;
        rects.extend_from_slice(&self.rects);
        Ok(Self { rects })
    }

    pub fn rectangles(&self) -> &[Rect] {
        &self.rects
    }

    /// Subtract the interior remaining after a rectangular erosion. Diagonal
    /// translations preserve the corner behavior of the NT region frame.
    pub(super) fn frame(&self, width: i32, height: i32) -> Result<Self, RegionError> {
        let width = i64::from(width).abs();
        let height = i64::from(height).abs();
        if width > i64::from(MAX_COORD) || height > i64::from(MAX_COORD) {
            return Err(RegionError::InvalidCoordinate);
        }
        if self.rects.is_empty() {
            return Err(RegionError::InvalidData);
        }
        let (width, height) = (width as i32, height as i32);
        if self.rects.len() == 1 {
            let bounds = self.bounds();
            if i64::from(bounds.width()) <= 2 * i64::from(width)
                || i64::from(bounds.height()) <= 2 * i64::from(height)
            {
                return self.try_clone();
            }
            let inner = Self::rectangle(Rect::new(
                bounds.left + width, bounds.top + height,
                bounds.right - width, bounds.bottom - height,
            ))?;
            return self.combine(&inner, RegionCombineMode::Difference);
        }
        let mut inner = self.try_clone()?;
        for (dx, dy) in [(width, height), (-width, height), (-width, -height), (width, -height)] {
            let mut shifted = self.try_clone()?;
            shifted.offset(dx, dy)?;
            inner = inner.combine(&shifted, RegionCombineMode::Intersection)?;
        }
        self.combine(&inner, RegionCombineMode::Difference)
    }

    pub fn complexity(&self) -> i32 {
        match self.rects.len() {
            0 => RegionType::Null as i32,
            1 => RegionType::Simple as i32,
            _ => RegionType::Complex as i32,
        }
    }

    pub fn bounds(&self) -> Rect {
        let Some(first) = self.rects.first() else {
            return Rect::default();
        };
        self.rects.iter().skip(1).fold(*first, |bounds, rect| {
            Rect::new(
                bounds.left.min(rect.left),
                bounds.top.min(rect.top),
                bounds.right.max(rect.right),
                bounds.bottom.max(rect.bottom),
            )
        })
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        self.rects.iter().any(|rect| rect.contains_point(x, y))
    }

    pub fn intersects_rect(&self, rect: &Rect) -> bool {
        !rect.is_empty() && self.rects.iter().any(|span| span.intersect(rect).is_some())
    }

    pub fn offset(&mut self, dx: i32, dy: i32) -> Result<(), RegionError> {
        // Validate the entire operation before changing any coordinate.
        for rect in &self.rects {
            for (coordinate, delta) in [
                (rect.left, dx),
                (rect.right, dx),
                (rect.top, dy),
                (rect.bottom, dy),
            ] {
                let value = i64::from(coordinate) + i64::from(delta);
                if !(i64::from(MIN_COORD)..=i64::from(MAX_COORD)).contains(&value) {
                    return Err(RegionError::InvalidCoordinate);
                }
            }
        }
        for rect in &mut self.rects {
            rect.left += dx;
            rect.right += dx;
            rect.top += dy;
            rect.bottom += dy;
        }
        Ok(())
    }

    pub fn combine(&self, other: &Self, mode: RegionCombineMode) -> Result<Self, RegionError> {
        if mode == RegionCombineMode::Copy {
            return self.try_clone();
        }
        Self::sweep(&self.rects, &other.rects, mode)
    }

    fn sweep(
        first: &[Rect],
        second: &[Rect],
        mode: RegionCombineMode,
    ) -> Result<Self, RegionError> {
        let count = first
            .len()
            .checked_add(second.len())
            .ok_or(RegionError::ResourceLimit)?;
        if count > MAX_REGION_RECTS * 2 {
            return Err(RegionError::ResourceLimit);
        }
        let mut edges = Vec::new();
        edges
            .try_reserve_exact(count * 2)
            .map_err(|_| RegionError::ResourceLimit)?;
        for rect in first.iter().chain(second) {
            if !rect.is_empty() {
                edges.extend_from_slice(&[rect.top, rect.bottom]);
            }
        }
        edges.sort_unstable();
        edges.dedup();
        if edges.len().saturating_mul(count) > MAX_SWEEP_WORK {
            return Err(RegionError::ResourceLimit);
        }

        let mut result = Self::default();
        let mut events: Vec<(i32, i32, i32)> = Vec::new();
        events
            .try_reserve_exact(count * 2)
            .map_err(|_| RegionError::ResourceLimit)?;
        let mut previous_start = 0;
        let mut previous_count = 0;
        for band in edges.windows(2) {
            events.clear();
            for (rects, source) in [(first, 0), (second, 1)] {
                for rect in rects {
                    if rect.top <= band[0] && rect.bottom >= band[1] && !rect.is_empty() {
                        let delta = if source == 0 { (1, 0) } else { (0, 1) };
                        events.push((rect.left, delta.0, delta.1));
                        events.push((rect.right, -delta.0, -delta.1));
                    }
                }
            }
            events.sort_unstable_by_key(|event| event.0);
            let start = result.rects.len();
            let (mut inside_first, mut inside_second) = (0, 0);
            let mut left = None;
            let mut index = 0;
            while index < events.len() {
                let x = events[index].0;
                let was_inside = mode.includes(inside_first > 0, inside_second > 0);
                while index < events.len() && events[index].0 == x {
                    inside_first += events[index].1;
                    inside_second += events[index].2;
                    index += 1;
                }
                let is_inside = mode.includes(inside_first > 0, inside_second > 0);
                if !was_inside && is_inside {
                    left = Some(x);
                } else if was_inside && !is_inside {
                    if let Some(left) = left.take() {
                        if result.rects.len() >= MAX_REGION_RECTS {
                            return Err(RegionError::ResourceLimit);
                        }
                        push(&mut result.rects, Rect::new(left, band[0], x, band[1]))?;
                    }
                }
            }
            let band_count = result.rects.len() - start;
            let same_spans = band_count > 0
                && band_count == previous_count
                && (0..band_count).all(|offset| {
                    let previous = result.rects[previous_start + offset];
                    let current = result.rects[start + offset];
                    previous.left == current.left
                        && previous.right == current.right
                        && previous.bottom == current.top
                });
            if same_spans {
                for rect in &mut result.rects[previous_start..start] {
                    rect.bottom = band[1];
                }
                result.rects.truncate(start);
            } else {
                previous_start = start;
                previous_count = band_count;
            }
        }
        Ok(result)
    }
}

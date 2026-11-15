//! DC-owned GDI paths. Points are captured in device coordinates at draw time,
//! while the DC's current position remains in logical coordinates.

extern crate alloc;

use super::dc::{DcObject, PointL};
use super::{clipping, objects::Rect, polygon, region_geometry::Region, transforms};
use alloc::vec::Vec;

#[path = "path/services.rs"]
pub mod services;

pub const PT_CLOSEFIGURE: u8 = 1;
pub const PT_LINETO: u8 = 2;
pub const PT_BEZIERTO: u8 = 4;
pub const PT_MOVETO: u8 = 6;
pub const MAX_PATH_POINTS: usize = 65_536;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PathState {
    #[default]
    None,
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub point: PointL,
    pub kind: u8,
}

#[derive(Debug, Default)]
pub struct GdiPath {
    pub state: PathState,
    pub entries: Vec<Entry>,
    position: PointL,
    new_figure: bool,
}

impl GdiPath {
    pub fn try_clone(&self) -> Option<Self> {
        let mut entries = Vec::new();
        entries.try_reserve_exact(self.entries.len()).ok()?;
        entries.extend_from_slice(&self.entries);
        Some(Self {
            state: self.state,
            entries,
            position: self.position,
            new_figure: self.new_figure,
        })
    }

    fn reserve(&mut self, count: usize) -> bool {
        self.entries
            .len()
            .checked_add(count)
            .is_some_and(|total| total <= MAX_PATH_POINTS)
            && self.entries.try_reserve(count).is_ok()
    }

    pub fn begin(&mut self, position: PointL) {
        if self.state == PathState::Open {
            return;
        }
        self.entries.clear();
        self.state = PathState::Open;
        self.position = position;
        self.new_figure = true;
    }

    pub fn move_to(&mut self, position: PointL) {
        self.position = position;
        self.new_figure = true;
    }

    pub fn figure_start(&self) -> PointL {
        if self.new_figure {
            return self.position;
        }
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.kind & !PT_CLOSEFIGURE == PT_MOVETO)
            .map_or(self.position, |entry| entry.point)
    }

    pub fn append(&mut self, points: &[PointL], kind: u8) -> bool {
        if !self.reserve(points.len() + usize::from(self.new_figure)) {
            return false;
        }
        if self.new_figure {
            self.entries.push(Entry {
                point: self.position,
                kind: PT_MOVETO,
            });
            self.new_figure = false;
        }
        for point in points {
            self.entries.push(Entry {
                point: *point,
                kind,
            });
            self.position = *point;
        }
        true
    }

    pub fn close_figure(&mut self) {
        if let Some(last) = self.entries.last_mut() {
            last.kind |= PT_CLOSEFIGURE;
            self.new_figure = true;
        }
    }

    pub fn add_figure(&mut self, points: &[PointL], closed: bool) -> bool {
        if points.is_empty() {
            return true;
        }
        if !self.reserve(points.len()) {
            return false;
        }
        for (index, point) in points.iter().enumerate() {
            self.entries.push(Entry {
                point: *point,
                kind: if index == 0 { PT_MOVETO } else { PT_LINETO },
            });
        }
        if closed {
            self.close_figure();
        }
        // Standalone Polygon/Polyline/Rectangle do not change current position.
        self.new_figure = true;
        true
    }

    pub fn flattened(&self) -> Option<Self> {
        let mut result = Self {
            state: self.state,
            position: self.position,
            new_figure: self.new_figure,
            entries: Vec::new(),
        };
        let mut index = 0;
        while index < self.entries.len() {
            let entry = self.entries[index];
            if entry.kind & !PT_CLOSEFIGURE != PT_BEZIERTO {
                if !result.reserve(1) {
                    return None;
                }
                result.entries.push(entry);
                index += 1;
                continue;
            }
            let start = result.entries.last()?.point;
            let controls = self.entries.get(index..index + 3)?;
            if controls[..2].iter().any(|point| point.kind != PT_BEZIERTO)
                || controls[2].kind & !PT_CLOSEFIGURE != PT_BEZIERTO
            {
                return None;
            }
            flatten_cubic(
                &mut result,
                [
                    start,
                    controls[0].point,
                    controls[1].point,
                    controls[2].point,
                ],
            )?;
            result.entries.last_mut()?.kind |= controls[2].kind & PT_CLOSEFIGURE;
            index += 3;
        }
        Some(result)
    }

    pub fn region(&self, fill_mode: i32) -> Option<Region> {
        let flattened = self.flattened()?;
        let mut points = Vec::new();
        points.try_reserve_exact(flattened.entries.len()).ok()?;
        points.extend(flattened.entries.iter().map(|entry| entry.point));
        let mut contours = Vec::new();
        contours.try_reserve_exact(points.len()).ok()?;
        let mut start = 0;
        for index in 1..points.len() {
            if flattened.entries[index].kind & !PT_CLOSEFIGURE == PT_MOVETO {
                contours.push(&points[start..index]);
                start = index;
            }
        }
        if !points.is_empty() {
            contours.push(&points[start..]);
        }
        polygon::region_from_contours(&contours, fill_mode).ok()
    }
}

// Subdivide with de Casteljau in floating point. Each leaf deviates by at most
// a quarter device pixel from its chord; the depth and point limits bound work.
fn flatten_cubic(output: &mut GdiPath, points: [PointL; 4]) -> Option<()> {
    let points = points.map(|point| (f64::from(point.x), f64::from(point.y)));
    fn recurse(output: &mut GdiPath, points: [(f64, f64); 4], depth: u8) -> Option<()> {
        let [a, b, c, d] = points;
        let dx = d.0 - a.0;
        let dy = d.1 - a.1;
        let distance = |p: (f64, f64)| {
            let cross = (p.0 - a.0) * dy - (p.1 - a.1) * dx;
            cross * cross
        };
        // Also test longitudinal control displacement, so cusps and loops with
        // collinear endpoints cannot collapse to a single point.
        let chord = dx * dx + dy * dy;
        let flat = if chord == 0.0 {
            [b, c]
                .iter()
                .all(|p| (p.0 - a.0).abs().max((p.1 - a.1).abs()) <= 0.25)
        } else {
            [b, c].iter().all(|p| {
                let projection = (p.0 - a.0) * dx + (p.1 - a.1) * dy;
                if projection < 0.0 {
                    (p.0 - a.0) * (p.0 - a.0) + (p.1 - a.1) * (p.1 - a.1) <= 0.0625
                } else if projection > chord {
                    (p.0 - d.0) * (p.0 - d.0) + (p.1 - d.1) * (p.1 - d.1) <= 0.0625
                } else {
                    distance(*p) <= 0.0625 * chord
                }
            })
        };
        if flat {
            let point = PointL::new(
                transforms::nearest_integer(d.0)?,
                transforms::nearest_integer(d.1)?,
            );
            if !output.reserve(1) {
                return None;
            }
            output.entries.push(Entry {
                point,
                kind: PT_LINETO,
            });
            return Some(());
        }
        if depth == 24 {
            return None;
        }
        let middle = |p: (f64, f64), q: (f64, f64)| ((p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5);
        let ab = middle(a, b);
        let bc = middle(b, c);
        let cd = middle(c, d);
        let abc = middle(ab, bc);
        let bcd = middle(bc, cd);
        let center = middle(abc, bcd);
        recurse(output, [a, ab, abc, center], depth + 1)?;
        recurse(output, [center, bcd, cd, d], depth + 1)
    }
    recurse(output, points, 0)
}

pub fn move_to(dc: &mut DcObject, point: PointL) -> bool {
    if dc.path.state == PathState::Open {
        let Some(device) = clipping::logical_point(dc, point) else {
            return false;
        };
        dc.path.move_to(device);
    }
    dc.cur_x = point.x;
    dc.cur_y = point.y;
    true
}

pub fn line_to(dc: &mut DcObject, point: PointL) -> bool {
    let Some(device) = clipping::logical_point(dc, point) else {
        return false;
    };
    if !dc.path.append(&[device], PT_LINETO) {
        return false;
    }
    dc.cur_x = point.x;
    dc.cur_y = point.y;
    true
}

pub fn rectangle(dc: &mut DcObject, mut rect: Rect) -> bool {
    rect.normalize();
    if rect.is_empty() {
        return true;
    }
    let mut points = [
        PointL::new(rect.right, rect.top),
        PointL::new(rect.left, rect.top),
        PointL::new(rect.left, rect.bottom),
        PointL::new(rect.right, rect.bottom),
    ];
    for point in &mut points {
        let Some(device) = clipping::logical_point(dc, *point) else {
            return false;
        };
        *point = device;
    }
    // GM_COMPATIBLE excludes the right/bottom edge in the recorded outline.
    // General affine rectangles in GM_ADVANCED retain all four transformed corners.
    if dc.graphics_mode == 1 && dc.world_transform.m12 == 0.0 && dc.world_transform.m21 == 0.0 {
        let max_x = points.iter().map(|p| p.x).max().unwrap_or(0);
        let max_y = points.iter().map(|p| p.y).max().unwrap_or(0);
        let (Some(right), Some(bottom)) = (max_x.checked_sub(1), max_y.checked_sub(1)) else {
            return false;
        };
        for point in &mut points {
            if point.x == max_x {
                point.x = right;
            }
            if point.y == max_y {
                point.y = bottom;
            }
        }
    }
    dc.path.add_figure(&points, true)
}

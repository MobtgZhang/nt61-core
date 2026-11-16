//! Color-key copies, mesh gradients and connected flood fills on bound DCs.
//! Capture all user input and fallible drawing work before changing the surface.
use super::*;
use crate::mm::user_copy;

pub(super) fn positive_rect(dc: &DcObject, x: i32, y: i32, width: i32, height: i32) -> Option<Rect> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let first = clipping::logical_point(dc, PointL::new(x, y))?;
    let last = clipping::logical_point(dc, PointL::new(x.checked_add(width)?, y.checked_add(height)?))?;
    if dc.world_transform.m12 != 0.0 || dc.world_transform.m21 != 0.0
        || last.x <= first.x || last.y <= first.y
    {
        return None;
    }
    let rect = Rect::new(first.x, first.y, last.x, last.y);
    super::super::region_geometry::valid_rect(&rect).then_some(rect)
}

pub(super) fn pixel_count(region: &Region) -> Option<usize> {
    region.rectangles().iter().try_fold(0usize, |total, rect| {
        let area = (rect.right - rect.left) as usize * (rect.bottom - rect.top) as usize;
        total.checked_add(area).filter(|&count| count <= MAX_CAPTURE_PIXELS)
    })
}

pub extern "C" fn nt_gdi_transparent_blt(frame: &SyscallFrame) -> u64 {
    let mut table = super::super::dc::get_dc_table();
    let Some(source_dc) = table.get(frame.args[5]) else {
        return 0;
    };
    let mut source = DcObject::new();
    source.surface = source_dc.surface;
    source.world_transform = source_dc.world_transform;
    source.viewport_ext = source_dc.viewport_ext;
    source.window_ext = source_dc.window_ext;
    source.viewport_org = source_dc.viewport_org;
    source.window_org = source_dc.window_org;
    let Some(destination) = table.get(frame.args[0]) else {
        return 0;
    };
    let result = (|| {
        let destination_rect = positive_rect(destination, frame.args[1] as i32, frame.args[2] as i32,
            frame.args[3] as i32, frame.args[4] as i32)?;
        let source_rect = positive_rect(&source, frame.args[6] as i32, frame.args[7] as i32,
            frame.args[8] as i32, frame.args[9] as i32)?;
        let destination_surface = bound_surface(destination)?;
        let source_surface = bound_surface(&source)?;
        let clip = clipping::effective_region(destination).ok()?;
        let draw = Region::rectangle(destination_rect).ok()?
            .combine(&clip, RegionCombineMode::Intersection).ok()?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(pixel_count(&draw)?).ok()?;
        let key = swap_red_blue(frame.args[10] as u32);
        for rect in draw.rectangles() {
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    let sx = i64::from(source_rect.left)
                        + i64::from(x - destination_rect.left) * i64::from(source_rect.right - source_rect.left)
                        / i64::from(destination_rect.right - destination_rect.left);
                    let sy = i64::from(source_rect.top)
                        + i64::from(y - destination_rect.top) * i64::from(source_rect.bottom - source_rect.top)
                        / i64::from(destination_rect.bottom - destination_rect.top);
                    let pixel = read_pixel(source_surface, i32::try_from(sx).ok()?, i32::try_from(sy).ok()?);
                    pixels.push(pixel.filter(|&color| color & 0xffffff != key));
                }
            }
        }
        // A shared source/destination surface is safe: capture precedes writes.
        let mut samples = pixels.into_iter();
        for rect in draw.rectangles() {
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    if let Some(Some(color)) = samples.next() {
                        write_pixel(destination, destination_surface, x, y, color);
                    }
                }
            }
        }
        Some(())
    })();
    result.is_some() as u64
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vertex {
    x: i32,
    y: i32,
    red: u16,
    green: u16,
    blue: u16,
    alpha: u16,
}

fn capture<T: Copy + Default>(pointer: u64, count: usize) -> Option<Vec<T>> {
    let size = count.checked_mul(core::mem::size_of::<T>())?;
    if size > 4 * 1024 * 1024 {
        return None;
    }
    let mut data = Vec::new();
    data.try_reserve_exact(count).ok()?;
    data.resize(count, T::default());
    // SAFETY: all T used here have a C layout and admit every input bit pattern.
    let bytes = unsafe { core::slice::from_raw_parts_mut(data.as_mut_ptr().cast::<u8>(), size) };
    if size != 0 {
        user_copy::copy_from_user(bytes, pointer).ok()?;
    }
    Some(data)
}

fn determinant(a: Vertex, b: Vertex, x: i32, y: i32) -> i128 {
    (i128::from(b.x) - i128::from(a.x)) * (i128::from(y) - i128::from(a.y))
        - (i128::from(b.y) - i128::from(a.y)) * (i128::from(x) - i128::from(a.x))
}

fn blend(vertices: &[Vertex], weights: &[i128], divisor: i128) -> u32 {
    let channel = |get: fn(Vertex) -> u16| -> u32 {
        let total = vertices.iter().zip(weights).map(|(&v, &w)| i128::from(get(v)) * w).sum::<i128>();
        (total / divisor / 256).clamp(0, 255) as u32
    };
    // GDI GradientFill ignores the Alpha field.
    (channel(|v| v.red) << 16) | (channel(|v| v.green) << 8) | channel(|v| v.blue)
}

pub extern "C" fn nt_gdi_gradient_fill(frame: &SyscallFrame) -> u64 {
    let mode = frame.args[5] as u32;
    if mode > 2 {
        return 0;
    }
    let result = (|| {
        let mut vertices = capture::<Vertex>(frame.args[1], frame.args[2] as u32 as usize)?;
        let mesh_count = frame.args[4] as u32 as usize;
        let stride = if mode == 2 { 3 } else { 2 };
        let indices = capture::<u32>(frame.args[3], mesh_count.checked_mul(stride)?)?;
        if vertices.is_empty() || mesh_count == 0 || indices.iter().any(|&i| i as usize >= vertices.len()) {
            return None;
        }
        let dc = super::super::dc::get_dc(frame.args[0])?;
        let surface = bound_surface(&dc)?;
        let clip = clipping::effective_region(&dc).ok()?;
        for vertex in &mut vertices {
            let point = clipping::logical_point(&dc, PointL::new(vertex.x, vertex.y))?;
            vertex.x = point.x;
            vertex.y = point.y;
        }
        if mode != 2 && (dc.world_transform.m12 != 0.0 || dc.world_transform.m21 != 0.0) {
            return None;
        }
        let mut pixels = Vec::new();
        for mesh in indices.chunks_exact(stride) {
            let mut v = [Vertex::default(); 3];
            for (slot, &index) in mesh.iter().enumerate() {
                v[slot] = vertices[index as usize];
            }
            let shape = if mode == 2 {
                let points = v.map(|vertex| PointL::new(vertex.x, vertex.y));
                super::super::polygon::region_from_contours(&[&points], 1).ok()?
            } else {
                Region::rectangle(Rect::new(v[0].x, v[0].y, v[1].x, v[1].y)).ok()?
            };
            let draw = shape.combine(&clip, RegionCombineMode::Intersection).ok()?;
            let count = pixel_count(&draw)?;
            if pixels.len().checked_add(count)? > MAX_CAPTURE_PIXELS {
                return None;
            }
            pixels.try_reserve(count).ok()?;
            let divisor = match mode {
                0 => i128::from(v[1].x) - i128::from(v[0].x),
                1 => i128::from(v[1].y) - i128::from(v[0].y),
                _ => determinant(v[0], v[1], v[2].x, v[2].y),
            };
            if divisor == 0 {
                continue;
            }
            for rect in draw.rectangles() {
                for y in rect.top..rect.bottom {
                    for x in rect.left..rect.right {
                        let weights = if mode == 2 {
                            let first = determinant(v[1], v[2], x, y);
                            let second = determinant(v[2], v[0], x, y);
                            [first, second, divisor - first - second]
                        } else {
                            let position = if mode == 0 { i128::from(x) - i128::from(v[0].x) }
                                else { i128::from(y) - i128::from(v[0].y) };
                            [divisor - position, position, 0]
                        };
                        pixels.push((x, y, blend(&v[..stride], &weights[..stride], divisor)));
                    }
                }
            }
        }
        for (x, y, color) in pixels {
            write_pixel(&dc, surface, x, y, color);
        }
        Some(())
    })();
    result.is_some() as u64
}

pub extern "C" fn nt_gdi_ext_flood_fill(frame: &SyscallFrame) -> u64 {
    let mode = frame.args[4] as u32;
    if mode > 1 {
        return 0;
    }
    let result = (|| {
        let dc = super::super::dc::get_dc(frame.args[0])?;
        let start = clipping::logical_point(&dc, PointL::new(frame.args[1] as i32, frame.args[2] as i32))?;
        let surface = bound_surface(&dc)?;
        let clip = clipping::effective_region(&dc).ok()?;
        if !clip.contains_point(start.x, start.y) {
            return None;
        }
        let color = swap_red_blue(frame.args[3] as u32);
        let eligible = |x, y| clip.contains_point(x, y)
            && read_pixel(surface, x, y).is_some_and(|p| (p & 0xffffff == color) == (mode == 1));
        if !eligible(start.x, start.y) {
            return None;
        }
        let brush = brush_sample(&dc, dc.brush)?;
        let Some(brush) = brush else { return Some(()) };
        let operation = rop2_operation(dc.rop_mode)?;
        let bounds = clip.bounds();
        let width = (bounds.right - bounds.left) as usize;
        let count = width.checked_mul((bounds.bottom - bounds.top) as usize)?;
        if count > MAX_CAPTURE_PIXELS {
            return None;
        }
        let mut visited = Vec::new();
        visited.try_reserve_exact(count).ok()?;
        visited.resize(count, false);
        let mut queue = Vec::new();
        queue.try_reserve_exact(count).ok()?;
        queue.push((start.x, start.y));
        visited[(start.y - bounds.top) as usize * width + (start.x - bounds.left) as usize] = true;
        let mut next = 0;
        while next < queue.len() {
            let (x, y) = queue[next];
            next += 1;
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if nx < bounds.left || nx >= bounds.right || ny < bounds.top || ny >= bounds.bottom {
                    continue;
                }
                let index = (ny - bounds.top) as usize * width + (nx - bounds.left) as usize;
                if !visited[index] && eligible(nx, ny) {
                    visited[index] = true;
                    queue.push((nx, ny));
                }
            }
        }
        for (x, y) in queue {
            if let (Some(old), Some(color)) = (read_pixel(surface, x, y), brush.at(x, y)) {
                write_pixel(&dc, surface, x, y, rop3(old, 0, color, operation));
            }
        }
        Some(())
    })();
    result.is_some() as u64
}

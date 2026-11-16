//! DDB readback into uncompressed RGB DIB buffers. Indexed conversion and color
//! management require palette/color-space support beyond this true-color path.

use super::*;
use alloc::vec::Vec;

const MAX_BYTES: usize = 64 * 1024 * 1024;

fn buffer(length: usize) -> Option<Vec<u8>> {
    if length > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).ok()?;
    bytes.resize(length, 0);
    Some(bytes)
}

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

#[derive(Clone, Copy)]
struct Dimensions {
    width: i32,
    height: i32,
    depth: u16,
}

fn bitmap_dimensions(bitmap: &GdiBitmap) -> Option<Dimensions> {
    let height = bitmap.height.checked_abs()?;
    (bitmap.width > 0 && height > 0 && bitmap.planes == 1).then_some(Dimensions {
        width: bitmap.width,
        height,
        depth: bitmap.bit_count,
    })
}

fn stride(width: i32, depth: u16) -> Option<usize> {
    (usize::try_from(width)
        .ok()?
        .checked_mul(usize::from(depth))?
        .checked_add(31)?
        / 32)
        .checked_mul(4)
}

fn pixel(bitmap: &GdiBitmap, x: usize, y: usize) -> Option<u32> {
    let depth = bitmap.bit_count;
    if !matches!(depth, 16 | 24 | 32) || bitmap.bits.is_null() {
        return None;
    }
    let pitch = usize::try_from(bitmap.width_bytes).ok()?;
    let offset = y
        .checked_mul(pitch)?
        .checked_add(x.checked_mul(usize::from(depth / 8))?)?;
    let end = offset.checked_add(usize::from(depth / 8))?;
    if end > usize::try_from(bitmap.size).ok()? {
        return None;
    }
    // SAFETY: the bitmap guard excludes writes/deletion and the complete pixel
    // lies within the owned allocation. Unaligned public bitmap rows are legal.
    let raw = unsafe {
        let pointer = bitmap.bits.add(offset);
        match depth {
            16 => u32::from(pointer.cast::<u16>().read_unaligned()),
            24 => {
                u32::from(pointer.read())
                    | (u32::from(pointer.add(1).read()) << 8)
                    | (u32::from(pointer.add(2).read()) << 16)
            }
            _ => pointer.cast::<u32>().read_unaligned(),
        }
    };
    if depth != 16 {
        return Some(raw & 0xffffff);
    }
    let red = (raw >> 10) & 31;
    let green = (raw >> 5) & 31;
    let blue = raw & 31;
    Some(
        (((red << 3) | (red >> 2)) << 16)
            | (((green << 3) | (green >> 2)) << 8)
            | (blue << 3)
            | (blue >> 2),
    )
}

fn readback(
    bitmap: &GdiBitmap,
    dimensions: Dimensions,
    depth: u16,
    start: usize,
    count: usize,
    top_down: bool,
) -> Option<Vec<u8>> {
    let pitch = stride(dimensions.width, depth)?;
    let mut bytes = buffer(pitch.checked_mul(count)?)?;
    if usize::try_from(bitmap.width_bytes).ok()? < stride(dimensions.width, dimensions.depth)? {
        return None;
    }
    for row in 0..count {
        let y = if top_down {
            // StartScan counts from the bitmap's bottom even when the output
            // DIB requests top-down rows. Reverse only the copied scan band.
            dimensions.height as usize - start - count + row
        } else {
            dimensions.height as usize - 1 - start - row
        };
        for x in 0..dimensions.width as usize {
            let rgb = pixel(bitmap, x, y)?;
            let offset = row * pitch + x * usize::from(depth / 8);
            match depth {
                16 => {
                    let value =
                        (((rgb >> 19) & 31) << 10) | (((rgb >> 11) & 31) << 5) | ((rgb >> 3) & 31);
                    bytes[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
                }
                24 => bytes[offset..offset + 3].copy_from_slice(&rgb.to_le_bytes()[..3]),
                _ => bytes[offset..offset + 4].copy_from_slice(&rgb.to_le_bytes()),
            }
        }
    }
    Some(bytes)
}

fn get_bits(frame: &SyscallFrame) -> Option<u64> {
    if frame.args[6] as u32 != 0 {
        return None;
    }
    // This path exposes the current true-color device. Validate its HDC even
    // when the caller only requests bitmap metadata.
    let dc = super::super::dc::get_dc(frame.args[0])?;
    drop(dc);
    let info_address = frame.args[5];
    let info_limit = frame.args[8] as u32 as usize;
    if info_limit < 4 {
        return None;
    }
    let mut prefix = [0; 4];
    user_copy::copy_from_user(&mut prefix, info_address).ok()?;
    let size = dword(&prefix, 0)? as usize;
    if !matches!(size, 12 | 40 | 108 | 124) || size > info_limit {
        return None;
    }
    let mut info = buffer(size)?;
    user_copy::copy_from_user(&mut info, info_address).ok()?;
    user_copy::probe_user_write(info_address, size).ok()?;
    let core = size == 12;
    let requested_depth = word(&info, if core { 10 } else { 14 })?;
    let dimensions = with_bitmap(frame.args[1], |bitmap| bitmap_dimensions(bitmap))??;
    let depth = if requested_depth == 0 {
        dimensions.depth
    } else {
        requested_depth
    };
    if !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32) {
        return None;
    }
    let height = if core {
        i32::from(word(&info, 6)?)
    } else {
        dword(&info, 8)? as i32
    };
    let query = requested_depth == 0;
    let top_down = !core && !query && height < 0;
    let pitch = stride(dimensions.width, depth)?;
    let full_size = pitch.checked_mul(dimensions.height as usize)?;
    let mut count = if query { dimensions.height as usize } else { 1 };
    let mut output = None;
    let compression = if core {
        0
    } else if query && matches!(depth, 16 | 32) {
        3
    } else {
        dword(&info, 16)?
    };
    if !query {
        let width = if core {
            i32::from(word(&info, 4)?)
        } else {
            dword(&info, 4)? as i32
        };
        let planes = word(&info, if core { 8 } else { 12 })?;
        if width != dimensions.width
            || height.checked_abs()? != dimensions.height
            || planes != 1
            || depth < 16
            || !matches!(compression, 0 | 3)
            || compression == 3 && !matches!(depth, 16 | 32)
        {
            return None;
        }
        let start = frame.args[2] as u32 as usize;
        if frame.args[4] != 0 && frame.args[3] as u32 != 0 && start < dimensions.height as usize {
            count = (frame.args[3] as u32 as usize).min(dimensions.height as usize - start);
            let length = pitch.checked_mul(count)?;
            if length > frame.args[7] as u32 as usize {
                return None;
            }
            user_copy::probe_user_write(frame.args[4], length).ok()?;
            output = Some(with_bitmap(frame.args[1], |bitmap| {
                // The second lookup may see a deleted/reused handle, but the
                // generation and dimensions must still match our capture.
                let current = bitmap_dimensions(bitmap)?;
                if current.width != dimensions.width
                    || current.height != dimensions.height
                    || current.depth != dimensions.depth
                {
                    return None;
                }
                readback(bitmap, current, depth, start, count, top_down)
            })??);
        }
    }
    if core {
        info[4..6].copy_from_slice(&u16::try_from(dimensions.width).ok()?.to_le_bytes());
        info[6..8].copy_from_slice(&u16::try_from(dimensions.height).ok()?.to_le_bytes());
        info[8..10].copy_from_slice(&1u16.to_le_bytes());
        info[10..12].copy_from_slice(&depth.to_le_bytes());
    } else {
        info[4..8].copy_from_slice(&dimensions.width.to_le_bytes());
        let height = if top_down {
            -dimensions.height
        } else {
            dimensions.height
        };
        info[8..12].copy_from_slice(&height.to_le_bytes());
        info[12..14].copy_from_slice(&1u16.to_le_bytes());
        info[14..16].copy_from_slice(&depth.to_le_bytes());
        info[16..20].copy_from_slice(&compression.to_le_bytes());
        info[20..24].copy_from_slice(&u32::try_from(full_size).ok()?.to_le_bytes());
        if query {
            info[24..32].fill(0);
        }
        info[32..40].fill(0);
    }
    if !core && !query && compression == 3 {
        let length = if size == 40 { 52 } else { size };
        if length > info_limit {
            return None;
        }
        user_copy::probe_user_write(info_address, length).ok()?;
        info.resize(length, 0);
        let masks = if depth == 16 {
            [0x7c00u32, 0x03e0, 0x001f]
        } else {
            [0xff0000, 0xff00, 0xff]
        };
        for (index, mask) in masks.into_iter().enumerate() {
            info[40 + index * 4..44 + index * 4].copy_from_slice(&mask.to_le_bytes());
        }
    }
    if let Some(output) = output {
        user_copy::copy_to_user(frame.args[4], &output).ok()?;
    }
    user_copy::copy_to_user(info_address, &info).ok()?;
    Some(count as u64)
}

/// HDC, HBITMAP, first scan, scan count, pixels, info, usage, pixel limit, info limit.
pub extern "C" fn nt_gdi_get_dibits_internal(frame: &SyscallFrame) -> u64 {
    get_bits(frame).unwrap_or(0)
}

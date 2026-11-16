//! Device-dependent bitmap transfers. The public byte stream has WORD-aligned
//! scanlines, while the internal allocation uses DWORD-aligned scanlines.

use super::*;
use alloc::vec::Vec;

const MAX_TRANSFER_BYTES: usize = 64 * 1024 * 1024;

struct Layout {
    stride: usize,
    pitch: usize,
    size: usize,
}

impl Layout {
    fn of(bitmap: &GdiBitmap) -> Option<Self> {
        if bitmap.width <= 0
            || bitmap.height == 0
            || bitmap.bits.is_null()
            || bitmap.planes != 1
            || !matches!(bitmap.bit_count, 1 | 4 | 8 | 16 | 24 | 32)
        {
            return None;
        }
        let bits = (bitmap.width as usize).checked_mul(usize::from(bitmap.bit_count))?;
        let stride = bits.checked_add(15)?.checked_div(16)?.checked_mul(2)?;
        let pitch = usize::try_from(bitmap.width_bytes).ok()?;
        let rows = bitmap.height.checked_abs()? as usize;
        if stride > pitch || pitch.checked_mul(rows)? > usize::try_from(bitmap.size).ok()? {
            return None;
        }
        Some(Self {
            stride,
            pitch,
            size: stride.checked_mul(rows)?,
        })
    }
}

fn buffer(count: usize) -> Option<Vec<u8>> {
    if count > MAX_TRANSFER_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(count).ok()?;
    bytes.resize(count, 0);
    Some(bytes)
}

fn set_bits(bitmap: &mut GdiBitmap, count: usize, address: u64) -> Option<usize> {
    let layout = Layout::of(bitmap)?;
    let count = count.min(layout.size);
    if count == 0 || address == 0 {
        return Some(0);
    }
    let mut bytes = buffer(count)?;
    user_copy::copy_from_user(&mut bytes, address).ok()?;
    for (row, input) in bytes.chunks(layout.stride).enumerate() {
        // SAFETY: Layout validated every internal scanline against bitmap.size.
        // Input is an independent captured buffer, and the object guard gives
        // exclusive access to the bitmap, including a partial final scanline.
        unsafe {
            core::ptr::copy_nonoverlapping(
                input.as_ptr(),
                bitmap.bits.add(row * layout.pitch),
                input.len(),
            );
        }
    }
    Some(count)
}

/// NtGdiGetBitmapBits(HBITMAP, ULONG byte_count, void *optional_output).
pub extern "C" fn nt_gdi_get_bitmap_bits(frame: &SyscallFrame) -> u64 {
    with_bitmap(frame.args[0], |bitmap| {
        let layout = Layout::of(bitmap)?;
        if frame.args[2] == 0 {
            return Some(layout.size);
        }
        let count = (frame.args[1] as u32 as usize).min(layout.size);
        if count == 0 {
            return Some(0);
        }
        let mut bytes = buffer(count)?;
        for (row, output) in bytes.chunks_mut(layout.stride).enumerate() {
            // SAFETY: validated scanline bounds and disjoint destination buffer;
            // the table guard prevents deletion or writes during capture.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bitmap.bits.add(row * layout.pitch),
                    output.as_mut_ptr(),
                    output.len(),
                );
            }
        }
        user_copy::copy_to_user(frame.args[2], &bytes).ok()?;
        Some(count)
    })
    .flatten()
    .unwrap_or(0) as u64
}

/// NtGdiSetBitmapBits(HBITMAP, ULONG byte_count, const void *input).
pub extern "C" fn nt_gdi_set_bitmap_bits(frame: &SyscallFrame) -> u64 {
    with_bitmap(frame.args[0], |bitmap| {
        set_bits(bitmap, frame.args[1] as u32 as usize, frame.args[2])
    })
    .flatten()
    .unwrap_or(0) as u64
}

/// CreateBitmap's fifth argument supplies WORD-aligned initial scanlines.
pub extern "C" fn nt_gdi_create_bitmap(frame: &SyscallFrame) -> u64 {
    let width = frame.args[0] as i32;
    let height = frame.args[1] as i32;
    let planes = frame.args[2] as u32;
    let depth = frame.args[3] as u32;
    if planes != 1 || width <= 0 || height <= 0 || !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32) {
        return 0;
    }
    let handle = objects::gdi_create_bitmap(width, height, depth as u16);
    if handle == 0 || frame.args[4] == 0 {
        return handle;
    }
    let initialized = with_bitmap(handle, |bitmap| {
        let layout = Layout::of(bitmap)?;
        set_bits(bitmap, layout.size, frame.args[4])
    })
    .flatten()
    .is_some();
    if !initialized {
        objects::gdi_delete_object(handle);
        return 0;
    }
    handle
}

//! ntdll — RTL bitmap operations
//
//! Bitmap manipulation functions for efficient bit-level operations.
//! Used by the memory manager and other kernel subsystems.
//
//! References: MSDN Library "Windows 7" — RTL Bitmap.

use core::ptr;

#[repr(C)]
pub struct RtlBitmap {
    pub size_of_bit_map: u32,
    pub buffer: *mut u32,
}

impl RtlBitmap {
    pub const fn new() -> Self {
        Self {
            size_of_bit_map: 0,
            buffer: ptr::null_mut(),
        }
    }
}

impl Default for RtlBitmap {
    fn default() -> Self {
        Self::new()
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_initialize_bit_map(
    bitmap_header: *mut RtlBitmap,
    bitmap_buffer: *mut u32,
    size_of_bit_map: u32,
) {
    if bitmap_header.is_null() {
        return;
    }
    (*bitmap_header).size_of_bit_map = size_of_bit_map;
    (*bitmap_header).buffer = bitmap_buffer;
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_set_bit(bitmap_header: *mut RtlBitmap, bit_number: u32) {
    if bitmap_header.is_null() {
        return;
    }
    let bm = &*bitmap_header;
    if bit_number >= bm.size_of_bit_map {
        return;
    }
    let dword_index = bit_number / 32;
    let bit_index = bit_number % 32;
    *bm.buffer.add(dword_index as usize) |= 1u32 << bit_index;
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_clear_bit(bitmap_header: *mut RtlBitmap, bit_number: u32) {
    if bitmap_header.is_null() {
        return;
    }
    let bm = &*bitmap_header;
    if bit_number >= bm.size_of_bit_map {
        return;
    }
    let dword_index = bit_number / 32;
    let bit_index = bit_number % 32;
    *bm.buffer.add(dword_index as usize) &= !(1u32 << bit_index);
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_test_bit(bitmap_header: *mut RtlBitmap, bit_number: u32) -> u8 {
    if bitmap_header.is_null() {
        return 0;
    }
    let bm = &*bitmap_header;
    if bit_number >= bm.size_of_bit_map {
        return 0;
    }
    let dword_index = bit_number / 32;
    let bit_index = bit_number % 32;
    if (*bm.buffer.add(dword_index as usize) & (1u32 << bit_index)) != 0 {
        1
    } else {
        0
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_clear_all_bits(bitmap_header: *mut RtlBitmap) {
    if bitmap_header.is_null() {
        return;
    }
    let bm = &*bitmap_header;
    let dword_count = bm.size_of_bit_map.div_ceil(32);
    ptr::write_bytes(bm.buffer, 0, dword_count as usize);
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_set_all_bits(bitmap_header: *mut RtlBitmap) {
    if bitmap_header.is_null() {
        return;
    }
    let bm = &*bitmap_header;
    let dword_count = bm.size_of_bit_map.div_ceil(32);
    ptr::write_bytes(bm.buffer, 0xFF, dword_count as usize);
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_find_clear_bits(
    bitmap_header: *mut RtlBitmap,
    number_to_find: u32,
    hint_index: u32,
) -> u32 {
    if bitmap_header.is_null() {
        return 0xFFFF_FFFF;
    }
    find_bits(&*bitmap_header, number_to_find, hint_index, false)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_find_set_bits(
    bitmap_header: *mut RtlBitmap,
    number_to_find: u32,
    hint_index: u32,
) -> u32 {
    if bitmap_header.is_null() {
        return 0xFFFF_FFFF;
    }
    find_bits(&*bitmap_header, number_to_find, hint_index, true)
}

/// Search ordered, linear ranges. A second pass may cross the hint but must
/// never join the last bit of the bitmap to bit zero.
///
/// # Safety
/// `bitmap.buffer` must address ceil(size_of_bit_map / 32) initialized ULONGs.
unsafe fn find_bits(bitmap: &RtlBitmap, count: u32, hint: u32, set: bool) -> u32 {
    let size = bitmap.size_of_bit_map;
    let hint = if hint < size { hint } else { 0 };
    // NT 6.1.7601's zero-count path does not inspect the backing buffer and
    // returns a byte-aligned hint (RVA 0xd7f95 / 0xd7e0b in the supplied kernel).
    if count == 0 {
        return hint & !7;
    }
    if count > size {
        return u32::MAX;
    }
    let first = find_bits_in_range(bitmap, count, hint, size, set);
    if first != u32::MAX || hint == 0 {
        return first;
    }
    // A run beginning before the hint can extend beyond it. The original
    // backscan includes up to count bits after the hint, capped at bitmap end.
    let end = hint.saturating_add(count).min(size);
    find_bits_in_range(bitmap, count, 0, end, set)
}

/// # Safety
/// `start <= end <= size_of_bit_map` and the bitmap buffer is valid.
unsafe fn find_bits_in_range(
    bitmap: &RtlBitmap,
    count: u32,
    start: u32,
    end: u32,
    set: bool,
) -> u32 {
    let mut run = 0;
    for bit in start..end {
        // SAFETY: The caller supplies a ULONG-aligned buffer covering every
        // logical bit. Division indexes only the word containing this bit.
        let word = unsafe { bitmap.buffer.add((bit / 32) as usize).read() };
        if ((word >> (bit % 32)) & 1 != 0) == set {
            run += 1;
            if run == count {
                return bit + 1 - count;
            }
        } else {
            run = 0;
        }
    }
    u32::MAX
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_set_bits(
    bitmap_header: *mut RtlBitmap,
    starting_index: u32,
    number_to_set: u32,
) {
    if bitmap_header.is_null() {
        return;
    }
    for i in 0..number_to_set {
        rtl_set_bit(bitmap_header, starting_index + i);
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_clear_bits(
    bitmap_header: *mut RtlBitmap,
    starting_index: u32,
    number_to_clear: u32,
) {
    if bitmap_header.is_null() {
        return;
    }
    for i in 0..number_to_clear {
        rtl_clear_bit(bitmap_header, starting_index + i);
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_number_of_set_bits(bitmap_header: *mut RtlBitmap) -> u32 {
    if bitmap_header.is_null() {
        return 0;
    }
    let bm = &*bitmap_header;
    let mut count = 0u32;
    for i in 0..bm.size_of_bit_map {
        if rtl_test_bit(bitmap_header, i) != 0 {
            count += 1;
        }
    }
    count
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_number_of_clear_bits(bitmap_header: *mut RtlBitmap) -> u32 {
    if bitmap_header.is_null() {
        return 0;
    }
    let bm = &*bitmap_header;
    bm.size_of_bit_map - rtl_number_of_set_bits(bitmap_header)
}

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

#[derive(Clone, Copy)]
#[repr(C)]
pub struct RtlBitmapRun {
    pub starting_index: u32,
    pub number_of_bits: u32,
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
    if dword_count != 0 {
        ptr::write_bytes(bm.buffer, 0, dword_count as usize);
    }
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
    if dword_count != 0 {
        ptr::write_bytes(bm.buffer, 0xFF, dword_count as usize);
    }
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
    if let Some(bitmap) = bitmap_header.as_ref() {
        let end = starting_index.saturating_add(number_to_set).min(bitmap.size_of_bit_map);
        for (index, mask) in word_masks(starting_index, end) {
            *bitmap.buffer.add(index) |= mask;
        }
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
    if let Some(bitmap) = bitmap_header.as_ref() {
        let end = starting_index.saturating_add(number_to_clear).min(bitmap.size_of_bit_map);
        for (index, mask) in word_masks(starting_index, end) {
            *bitmap.buffer.add(index) &= !mask;
        }
    }
}

/// # Safety
/// The header and its backing buffer must be readable.
pub unsafe fn rtl_are_bits_clear(bitmap_header: *const RtlBitmap, start: u32, count: u32) -> u8 {
    rtl_are_bits(bitmap_header, start, count, false)
}

/// # Safety
/// The header and its backing buffer must be readable.
pub unsafe fn rtl_are_bits_set(bitmap_header: *const RtlBitmap, start: u32, count: u32) -> u8 {
    rtl_are_bits(bitmap_header, start, count, true)
}

unsafe fn rtl_are_bits(bitmap_header: *const RtlBitmap, start: u32, count: u32, set: bool) -> u8 {
    let Some(bitmap) = bitmap_header.as_ref() else { return 0; };
    let Some(end) = start.checked_add(count) else { return 0; };
    if count == 0 || end > bitmap.size_of_bit_map {
        return 0;
    }
    u8::from(word_masks(start, end).all(|(index, mask)| {
        *bitmap.buffer.add(index) & mask == if set { mask } else { 0 }
    }))
}

fn word_masks(start: u32, end: u32) -> impl DoubleEndedIterator<Item = (usize, u32)> {
    let first = start / 32;
    let past_last = if end > start { end.div_ceil(32) } else { first };
    (first..past_last).map(move |index| {
        let word_start = index * 32;
        let first_bit = start.saturating_sub(word_start);
        let past_last_bit = (end - word_start).min(32);
        let mask = (u32::MAX << first_bit) & (u32::MAX >> (32 - past_last_bit));
        (index as usize, mask)
    })
}

pub(crate) unsafe fn next_clear_run(bitmap: &RtlBitmap, from: u32) -> (u32, u32) {
    let size = bitmap.size_of_bit_map;
    if from >= size {
        return (from, 0);
    }
    let start = unsafe { next_bit(bitmap, from, false) };
    let end = unsafe { next_bit(bitmap, start, true) };
    (start, end - start)
}

unsafe fn next_bit(bitmap: &RtlBitmap, from: u32, set: bool) -> u32 {
    for (index, mask) in word_masks(from, bitmap.size_of_bit_map) {
        let word = unsafe { bitmap.buffer.add(index).read() };
        let candidates = (if set { word } else { !word }) & mask;
        if candidates != 0 {
            return index as u32 * 32 + candidates.trailing_zeros();
        }
    }
    bitmap.size_of_bit_map
}

pub(crate) unsafe fn previous_clear_run(bitmap: &RtlBitmap, from: u32) -> (u32, u32) {
    if from >= bitmap.size_of_bit_map {
        return (from, 0);
    }
    let Some(end) = (unsafe { previous_bit(bitmap, from + 1, false) }) else {
        return (0, 0);
    };
    let start = unsafe { previous_bit(bitmap, end, true) }.map_or(0, |bit| bit + 1);
    (start, end + 1 - start)
}

unsafe fn previous_bit(bitmap: &RtlBitmap, end: u32, set: bool) -> Option<u32> {
    for (index, mask) in word_masks(0, end).rev() {
        let word = unsafe { bitmap.buffer.add(index).read() };
        let candidates = (if set { word } else { !word }) & mask;
        if candidates != 0 {
            return Some(index as u32 * 32 + 31 - candidates.leading_zeros());
        }
    }
    None
}

pub(crate) unsafe fn longest_clear_run(bitmap: &RtlBitmap) -> (u32, u32) {
    let (mut best_start, mut best_length) = (0, 0);
    let mut cursor = 0;
    while bitmap.size_of_bit_map - cursor > best_length {
        let (start, length) = unsafe { next_clear_run(bitmap, cursor) };
        if length > best_length {
            best_start = start;
            best_length = length;
        }
        cursor = start + length;
    }
    (best_start, best_length)
}

pub(crate) unsafe fn find_clear_runs(
    bitmap: &RtlBitmap,
    runs: *mut RtlBitmapRun,
    capacity: usize,
    longest_first: bool,
) -> u32 {
    if capacity == 0 {
        return 0;
    }
    let scan = unsafe { clear_runs(bitmap) };
    if !longest_first {
        return unsafe { first_clear_runs(scan, runs, capacity) };
    }
    let mut count = 0;
    for run in scan {
        let mut index = count.min(capacity - 1);
        if count == capacity && unsafe { (*runs.add(index)).number_of_bits } >= run.number_of_bits {
            continue;
        }
        while index > 0 {
            let previous = unsafe { runs.add(index - 1).read() };
            if previous.number_of_bits >= run.number_of_bits {
                break;
            }
            unsafe { runs.add(index).write(previous) };
            index -= 1;
        }
        unsafe { runs.add(index).write(run) };
        count = (count + 1).min(capacity);
    }
    count as u32
}

unsafe fn clear_runs(bitmap: &RtlBitmap) -> impl Iterator<Item = RtlBitmapRun> + '_ {
    let mut cursor = 0;
    core::iter::from_fn(move || {
        let (starting_index, number_of_bits) = unsafe { next_clear_run(bitmap, cursor) };
        if number_of_bits == 0 {
            return None;
        }
        cursor = starting_index + number_of_bits;
        Some(RtlBitmapRun { starting_index, number_of_bits })
    })
}

unsafe fn first_clear_runs(
    scan: impl Iterator<Item = RtlBitmapRun>,
    runs: *mut RtlBitmapRun,
    capacity: usize,
) -> u32 {
    let mut scan = scan.peekable();
    let mut count = 0;
    while let Some(first) = scan.next() {
        let byte = (first.starting_index + first.number_of_bits) / 8;
        let mut batch = [first; 4];
        let mut length = 1;
        while scan.peek().is_some_and(|run| (run.starting_index + run.number_of_bits) / 8 == byte) {
            batch[length] = scan.next().unwrap();
            length += 1;
        }
        let leading = usize::from(first.starting_index <= byte * 8);
        batch[leading..length].sort_unstable_by_key(|run| {
            (core::cmp::Reverse(run.number_of_bits), run.starting_index)
        });
        let written = length.min(capacity - count);
        unsafe { ptr::copy_nonoverlapping(batch.as_ptr(), runs.add(count), written) };
        count += written;
        if count == capacity {
            break;
        }
    }
    count as u32
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
pub unsafe extern "C" fn rtl_number_of_set_bits(bitmap_header: *mut RtlBitmap) -> u32 {
    let Some(bitmap) = bitmap_header.as_ref() else { return 0; };
    word_masks(0, bitmap.size_of_bit_map)
        .map(|(index, mask)| (*bitmap.buffer.add(index) & mask).count_ones())
        .sum()
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

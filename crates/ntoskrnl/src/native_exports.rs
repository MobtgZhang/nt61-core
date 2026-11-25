//! Small, fully implemented native PE API surface. `efiapi` selects Microsoft
//! x64 on x86_64 (the kernel's internal C ABI is SysV), and the supported platform
//! calling convention elsewhere. Each pointer must obey the WDK caller contract.
#![allow(non_snake_case)]

// Compile the same allocation-free bitmap implementation used by ntdll. This
// module also remains independently host-testable without kernel hardware code.
#[path = "../../libs/ntdll/src/ntdll/rtl_bitmap.rs"]
#[allow(dead_code)]
mod bitmap;
pub use bitmap::{RtlBitmap, RtlBitmapRun};
#[path = "../../libs/ntdll/src/ntdll/rtl_integer.rs"]
mod integer;
#[path = "../../libs/ntdll/src/ntdll/rtl_epoch.rs"]
mod epoch;

unsafe fn time_to_seconds(time: *const i64, seconds: *mut u32, epoch: i64) -> u8 {
    let Some(value) = epoch::to_seconds(unsafe { time.read_unaligned() }, epoch) else {
        return 0;
    };
    unsafe { seconds.write_unaligned(value) };
    1
}

/// # Safety
/// `time` is readable and `seconds` is writable on a successful conversion.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlTimeToSecondsSince1970(time: *const i64, seconds: *mut u32) -> u8 {
    unsafe { time_to_seconds(time, seconds, epoch::EPOCH_1970) }
}

/// # Safety
/// `time` is readable and `seconds` is writable on a successful conversion.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlTimeToSecondsSince1980(time: *const i64, seconds: *mut u32) -> u8 {
    unsafe { time_to_seconds(time, seconds, epoch::EPOCH_1980) }
}

/// # Safety
/// `time` must be writable for eight bytes.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlSecondsSince1970ToTime(seconds: u32, time: *mut i64) {
    unsafe { time.write_unaligned(epoch::from_seconds(seconds, epoch::EPOCH_1970)) };
}

/// # Safety
/// `time` must be writable for eight bytes.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlSecondsSince1980ToTime(seconds: u32, time: *mut i64) {
    unsafe { time.write_unaligned(epoch::from_seconds(seconds, epoch::EPOCH_1980)) };
}

/// # Safety
/// The header must be writable and the ULONG-aligned buffer must cover `size`
/// bits. The caller synchronizes later bitmap access.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlInitializeBitMap(
    header: *mut RtlBitmap,
    buffer: *mut u32,
    size: u32,
) {
    unsafe { bitmap::rtl_initialize_bit_map(header, buffer, size) }
}

/// # Safety
/// The initialized header and backing buffer must be readable and synchronized.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindClearBits(
    header: *mut RtlBitmap,
    count: u32,
    hint: u32,
) -> u32 {
    unsafe { bitmap::rtl_find_clear_bits(header, count, hint) }
}

/// # Safety
/// The initialized header and backing buffer must be readable and synchronized.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindSetBits(
    header: *mut RtlBitmap,
    count: u32,
    hint: u32,
) -> u32 {
    unsafe { bitmap::rtl_find_set_bits(header, count, hint) }
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindClearRuns(
    header: *const RtlBitmap,
    runs: *mut RtlBitmapRun,
    capacity: u32,
    longest_first: u8,
) -> u32 {
    unsafe { bitmap::find_clear_runs(&*header, runs, capacity as usize, longest_first != 0) }
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindFirstRunClear(
    header: *const RtlBitmap,
    start: *mut u32,
) -> u32 {
    unsafe { nt61_export_RtlFindNextForwardRunClear(header, 0, start) }
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindNextForwardRunClear(
    header: *const RtlBitmap,
    from: u32,
    start: *mut u32,
) -> u32 {
    let (index, length) = unsafe { bitmap::next_clear_run(&*header, from) };
    unsafe { start.write(index) };
    length
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindLastBackwardRunClear(
    header: *const RtlBitmap,
    from: u32,
    start: *mut u32,
) -> u32 {
    let (index, length) = unsafe { bitmap::previous_clear_run(&*header, from) };
    unsafe { start.write(index) };
    length
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindLongestRunClear(
    header: *const RtlBitmap,
    start: *mut u32,
) -> u32 {
    let (index, length) = unsafe { bitmap::longest_clear_run(&*header) };
    unsafe { start.write(index) };
    length
}

#[no_mangle]
#[inline(never)]
pub extern "efiapi" fn nt61_export_RtlFindLeastSignificantBit(value: u64) -> i8 {
    if value == 0 { -1 } else { value.trailing_zeros() as i8 }
}

#[no_mangle]
#[inline(never)]
pub extern "efiapi" fn nt61_export_RtlFindMostSignificantBit(value: u64) -> i8 {
    63 - value.leading_zeros() as i8
}

/// # Safety
/// The header and its synchronized bitmap buffer must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlAreBitsClear(header: *const RtlBitmap, start: u32, count: u32) -> u8 {
    unsafe { bitmap::rtl_are_bits_clear(header, start, count) }
}

/// # Safety
/// The header must be readable and its synchronized bitmap buffer writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlClearAllBits(header: *mut RtlBitmap) {
    unsafe { bitmap::rtl_clear_all_bits(header) }
}

/// # Safety
/// The header must be readable and its synchronized bitmap buffer writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlClearBits(header: *mut RtlBitmap, start: u32, count: u32) {
    unsafe { bitmap::rtl_clear_bits(header, start, count) }
}

/// # Safety
/// The header must be readable and the caller must synchronize the bitmap search and update.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindClearBitsAndSet(header: *mut RtlBitmap, count: u32, hint: u32) -> u32 {
    let start = unsafe { bitmap::rtl_find_clear_bits(header, count, hint) };
    if start != u32::MAX && count != 0 {
        unsafe { bitmap::rtl_set_bits(header, start, count) };
    }
    start
}

/// # Safety
/// The header must be readable and the caller must synchronize the bitmap search and update.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFindSetBitsAndClear(header: *mut RtlBitmap, count: u32, hint: u32) -> u32 {
    let start = unsafe { bitmap::rtl_find_set_bits(header, count, hint) };
    if start != u32::MAX && count != 0 {
        unsafe { bitmap::rtl_clear_bits(header, start, count) };
    }
    start
}

/// # Safety
/// The header must be readable and its synchronized bitmap buffer writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlSetBits(header: *mut RtlBitmap, start: u32, count: u32) {
    unsafe { bitmap::rtl_set_bits(header, start, count) }
}

/// # Safety
/// The header and its synchronized bitmap buffer must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlTestBit(header: *mut RtlBitmap, index: u32) -> u8 {
    unsafe { bitmap::rtl_test_bit(header, index) }
}

/// # Safety
/// The header and its synchronized bitmap buffer must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlAreBitsSet(header: *const RtlBitmap, start: u32, count: u32) -> u8 {
    unsafe { bitmap::rtl_are_bits_set(header, start, count) }
}

/// # Safety
/// The header must be readable and its synchronized bitmap buffer writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlClearBit(header: *mut RtlBitmap, index: u32) {
    unsafe { bitmap::rtl_clear_bit(header, index) }
}

/// # Safety
/// The header and its synchronized bitmap buffer must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlNumberOfClearBits(header: *mut RtlBitmap) -> u32 {
    unsafe { bitmap::rtl_number_of_clear_bits(header) }
}

/// # Safety
/// The header and its synchronized bitmap buffer must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlNumberOfSetBits(header: *mut RtlBitmap) -> u32 {
    unsafe { bitmap::rtl_number_of_set_bits(header) }
}

#[no_mangle]
#[inline(never)]
pub extern "efiapi" fn nt61_export_RtlNumberOfSetBitsUlongPtr(value: usize) -> u32 {
    value.count_ones()
}

/// # Safety
/// The header must be readable and its synchronized bitmap buffer writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlSetAllBits(header: *mut RtlBitmap) {
    unsafe { bitmap::rtl_set_all_bits(header) }
}

/// # Safety
/// The header must be readable and its synchronized bitmap buffer writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlSetBit(header: *mut RtlBitmap, index: u32) {
    unsafe { bitmap::rtl_set_bit(header, index) }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UnicodeString {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *const u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AnsiString {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *const u8,
}

const STATUS_BUFFER_TOO_SMALL: i32 = 0xc0000023u32 as i32;

/// # Safety
/// The descriptors must be valid. Nonempty source bytes must be readable and
/// the destination buffer must be writable through its maximum length.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlAppendStringToString(
    destination: *mut AnsiString,
    source: *const AnsiString,
) -> i32 {
    let source = unsafe { source.read() };
    if source.length == 0 { return 0; }
    let length = unsafe { (*destination).length };
    let total = u32::from(length) + u32::from(source.length);
    if total > u32::from(unsafe { (*destination).maximum_length }) {
        return STATUS_BUFFER_TOO_SMALL;
    }
    unsafe {
        let start = (*destination).buffer.cast_mut().add(usize::from(length));
        core::ptr::copy(source.buffer, start, usize::from(source.length));
        (*destination).length = total as u16;
    }
    0
}

unsafe fn append_unicode(destination: *mut UnicodeString, source: *const u16, bytes: u16) -> i32 {
    let length = unsafe { (*destination).length };
    let maximum = u32::from(unsafe { (*destination).maximum_length });
    let total = u32::from(length) + u32::from(bytes);
    if total > maximum { return STATUS_BUFFER_TOO_SMALL; }
    unsafe {
        let buffer = (*destination).buffer.cast_mut();
        if bytes != 0 {
            let start = buffer.add(usize::from(length / 2)).cast::<u8>();
            core::ptr::copy(source.cast::<u8>(), start, usize::from(bytes));
        }
        (*destination).length = total as u16;
        if total + 2 <= maximum {
            buffer.add(usize::from(length / 2) + usize::from(bytes / 2)).write(0);
        }
    }
    0
}

/// # Safety
/// The descriptors must be valid. Nonempty source bytes must be readable and
/// the destination buffer must be writable through its maximum length.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlAppendUnicodeStringToString(
    destination: *mut UnicodeString,
    source: *const UnicodeString,
) -> i32 {
    let source = unsafe { source.read() };
    if source.length == 0 { return 0; }
    unsafe { append_unicode(destination, source.buffer, source.length) }
}

/// # Safety
/// The destination descriptor and its indicated buffer must be writable.
/// Source may be null or must point to a readable NUL-terminated UTF-16 string.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlAppendUnicodeToString(
    destination: *mut UnicodeString,
    source: *const u16,
) -> i32 {
    if source.is_null() { return 0; }
    for units in 0..=0x7ffe {
        if unsafe { source.add(units).read() } == 0 {
            return unsafe { append_unicode(destination, source, (units * 2) as u16) };
        }
    }
    STATUS_BUFFER_TOO_SMALL
}

unsafe fn compare_ansi_bytes(left: &AnsiString, right: &AnsiString, count: u16, ignore_case: u8) -> i32 {
    for offset in 0..usize::from(count) {
        let mut first = unsafe { left.buffer.add(offset).read() };
        let mut second = unsafe { right.buffer.add(offset).read() };
        if ignore_case != 0 {
            first = first.to_ascii_uppercase();
            second = second.to_ascii_uppercase();
        }
        if first != second {
            return i32::from(first) - i32::from(second);
        }
    }
    0
}

/// # Safety
/// Both descriptors and their nonempty counted buffers must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlCompareString(
    left: *const AnsiString,
    right: *const AnsiString,
    ignore_case: u8,
) -> i32 {
    let (left, right) = unsafe { (&*left, &*right) };
    let difference = unsafe { compare_ansi_bytes(left, right, left.length.min(right.length), ignore_case) };
    if difference != 0 {
        difference
    } else {
        i32::from(left.length) - i32::from(right.length)
    }
}

/// # Safety
/// Both descriptors and their nonempty counted buffers must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlEqualString(
    left: *const AnsiString,
    right: *const AnsiString,
    ignore_case: u8,
) -> u8 {
    let (left, right) = unsafe { (&*left, &*right) };
    u8::from(left.length == right.length
        && unsafe { compare_ansi_bytes(left, right, left.length, ignore_case) } == 0)
}

/// # Safety
/// Both descriptors and their nonempty counted buffers must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlPrefixString(
    prefix: *const AnsiString,
    string: *const AnsiString,
    ignore_case: u8,
) -> u8 {
    let (prefix, string) = unsafe { (&*prefix, &*string) };
    u8::from(prefix.length <= string.length
        && unsafe { compare_ansi_bytes(prefix, string, prefix.length, ignore_case) } == 0)
}

/// # Safety
/// The descriptor must be writable and source must be null or a readable
/// NUL-terminated byte string. The descriptor borrows source's storage.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlInitAnsiString(
    dst: *mut AnsiString,
    source: *const u8,
) {
    // NT 6.1 scans the complete source, then clamps the byte lengths. Setting
    // Buffer and clearing lengths precedes scanning in the original routine.
    dst.write(AnsiString {
        length: 0,
        maximum_length: 0,
        buffer: source,
    });
    if !source.is_null() {
        let mut length = 0usize;
        while source.add(length).read() != 0 {
            length += 1;
        }
        let length = length.min(0xfffe) as u16;
        (*dst).length = length;
        (*dst).maximum_length = length + 1;
    }
}

/// # Safety
/// The destination descriptor must be writable. Source must be null or a
/// readable NUL-terminated byte string that outlives the descriptor's use.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlInitString(
    dst: *mut AnsiString,
    source: *const u8,
) {
    unsafe { nt61_export_RtlInitAnsiString(dst, source) }
}

/// # Safety
/// The destination descriptor and its indicated buffer must be writable.
/// Source may be null. Otherwise its descriptor and indicated buffer must be readable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlCopyString(
    dst: *mut AnsiString,
    source: *const AnsiString,
) {
    unsafe { (*dst).length = 0 };
    if !source.is_null() {
        let length = unsafe { (*source).length.min((*dst).maximum_length) };
        unsafe { (*dst).length = length };
        if length != 0 {
            unsafe { core::ptr::copy((*source).buffer, (*dst).buffer.cast_mut(), usize::from(length)) };
        }
    }
}

/// # Safety
/// Descriptors must be valid, with a writable destination buffer covering
/// MaximumLength bytes and a readable source buffer covering Length bytes.
/// Source may be null; buffers must not overlap. Buffer alignment must permit
/// UTF-16 writes. This follows the supplied NT 6.1 image even for odd byte counts.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlCopyUnicodeString(
    dst: *mut UnicodeString,
    source: *const UnicodeString,
) {
    if source.is_null() {
        // A null descriptor empties Length without touching the buffer.
        (*dst).length = 0;
        return;
    }
    let length = (*source).length.min((*dst).maximum_length);
    let source_buffer = (*source).buffer.cast::<u8>();
    let destination = (*dst).buffer.cast::<u8>().cast_mut();
    (*dst).length = length;
    if length != 0 {
        core::ptr::copy_nonoverlapping(source_buffer, destination, usize::from(length));
    }
    if usize::from(length) + 2 <= usize::from((*dst).maximum_length) {
        // Original shifts Length right by 1 for WCHAR indexing. An odd byte
        // count therefore has its final copied byte overwritten by this NUL.
        destination
            .add(usize::from(length & !1))
            .cast::<u16>()
            .write(0);
    }
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlCompareMemory(
    a: *const u8,
    b: *const u8,
    len: usize,
) -> usize {
    for i in 0..len {
        if a.add(i).read() != b.add(i).read() {
            return i;
        }
    }
    len
}

/// # Safety
/// `source` must be readable through the last complete ULONG in `length`.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlCompareMemoryUlong(
    source: *const u8,
    length: usize,
    pattern: u32,
) -> usize {
    let length = length & !3;
    for offset in (0..length).step_by(4) {
        if unsafe { source.add(offset).cast::<u32>().read_unaligned() } != pattern {
            return offset;
        }
    }
    length
}

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut remainder = index as u32;
        let mut bit = 0;
        while bit < 8 {
            remainder = (remainder >> 1) ^ (0xedb8_8320 & (remainder & 1).wrapping_neg());
            bit += 1;
        }
        table[index] = remainder;
        index += 1;
    }
    table
};

/// # Safety
/// For nonzero length, `buffer` must be readable for `length` bytes.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlComputeCrc32(initial: u32, buffer: *const u8, length: u32) -> u32 {
    if length == 0 { return initial; }
    let bytes = unsafe { core::slice::from_raw_parts(buffer, length as usize) };
    let mut crc = !initial;
    for &byte in bytes {
        crc = (crc >> 8) ^ CRC32_TABLE[usize::from(crc as u8 ^ byte)];
    }
    !crc
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlCopyMemory(dst: *mut u8, src: *const u8, len: usize) {
    if len != 0 {
        core::ptr::copy_nonoverlapping(src, dst, len);
    }
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlMoveMemory(dst: *mut u8, src: *const u8, len: usize) {
    if len != 0 {
        core::ptr::copy(src, dst, len);
    }
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlFillMemory(dst: *mut u8, len: usize, value: u8) {
    if len != 0 {
        core::ptr::write_bytes(dst, value, len);
    }
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlZeroMemory(dst: *mut u8, len: usize) {
    if len != 0 {
        core::ptr::write_bytes(dst, 0, len);
    }
}

unsafe fn string_length(source: *const u16) -> usize {
    if source.is_null() {
        return 0;
    }
    let mut units = 0;
    while source.add(units).read() != 0 {
        units += 1;
    }
    units
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlInitUnicodeString(
    dst: *mut UnicodeString,
    source: *const u16,
) {
    // NT's void initializer truncates lengths exceeding USHORT_MAX - WCHAR.
    let units = string_length(source).min(0x7ffe);
    dst.write(UnicodeString {
        length: (units * 2) as u16,
        maximum_length: if source.is_null() {
            0
        } else {
            (units * 2 + 2) as u16
        },
        buffer: source,
    });
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlInitUnicodeStringEx(
    dst: *mut UnicodeString,
    source: *const u16,
) -> i32 {
    // The supplied 6.1.7601 image zeros lengths and stores Buffer even on
    // STATUS_NAME_TOO_LONG (0xc0000106), not STATUS_INVALID_PARAMETER.
    dst.write(UnicodeString {
        length: 0,
        maximum_length: 0,
        buffer: source,
    });
    let units = string_length(source);
    if units > 0x7ffe {
        return 0xc000_0106u32 as i32;
    }
    dst.write(UnicodeString {
        length: (units * 2) as u16,
        maximum_length: if source.is_null() {
            0
        } else {
            (units * 2 + 2) as u16
        },
        buffer: source,
    });
    0
}

/// # Safety
/// A non-null descriptor must be readable. Its buffer is never dereferenced.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlValidateUnicodeString(
    flags: u32,
    string: *const UnicodeString,
) -> u32 {
    const INVALID_PARAMETER: u32 = 0xc000000d;
    if flags != 0 {
        return INVALID_PARAMETER;
    }
    let Some(string) = (unsafe { string.as_ref() }) else {
        return 0;
    };
    if (string.length | string.maximum_length) & 1 != 0
        || string.length > string.maximum_length
        || (string.buffer.is_null() && string.maximum_length != 0)
    {
        return INVALID_PARAMETER;
    }
    0
}

/// # Safety
/// Source must be NUL-terminated and readable. Value must be writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlCharToInteger(
    mut source: *const u8,
    radix: u32,
    value: *mut u32,
) -> u32 {
    let units = core::iter::from_fn(|| {
        let unit = unsafe { source.read() };
        source = unsafe { source.add(1) };
        Some(unit)
    });
    match integer::parse_ansi(units, radix) {
        Ok(result) => {
            unsafe { value.write(result) };
            0
        }
        Err(status) => status,
    }
}

/// # Safety
/// Destination must cover the indicated field in bytes when the conversion fits.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlIntegerToChar(
    value: u32,
    radix: u32,
    capacity: i32,
    destination: *mut u8,
) -> u32 {
    let mut storage = [0; 32];
    let digits = match integer::digits(value, radix, &mut storage) {
        Ok(digits) => digits,
        Err(status) => return status,
    };
    let (padding, terminate) = match integer::field(capacity, digits.len()) {
        Ok(field) => field,
        Err(status) => return status,
    };
    unsafe {
        destination.write_bytes(b'0', padding);
        let start = destination.add(padding);
        core::ptr::copy_nonoverlapping(digits.as_ptr(), start, digits.len());
        if terminate {
            start.add(digits.len()).write(0);
        }
    }
    0
}

/// # Safety
/// Destination must cover the indicated field in UTF-16 units when it fits.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlIntegerToUnicode(
    value: u32,
    radix: u32,
    capacity: i32,
    destination: *mut u16,
) -> u32 {
    let mut storage = [0; 32];
    let digits = match integer::digits(value, radix, &mut storage) {
        Ok(digits) => digits,
        Err(status) => return status,
    };
    let (padding, terminate) = match integer::field(capacity, digits.len()) {
        Ok(field) => field,
        Err(status) => return status,
    };
    let output = core::iter::repeat_n(b'0', padding)
        .chain(digits.iter().copied())
        .chain(terminate.then_some(0));
    for (index, unit) in output.enumerate() {
        unsafe { destination.add(index).write(u16::from(unit)) };
    }
    0
}

/// # Safety
/// Destination must be a writable descriptor backed by its indicated buffer.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlIntegerToUnicodeString(
    value: u32,
    radix: u32,
    destination: *mut UnicodeString,
) -> u32 {
    let mut storage = [0; 32];
    let digits = match integer::digits(value, radix, &mut storage) {
        Ok(digits) => digits,
        Err(status) => return status,
    };
    let length = (digits.len() * 2) as u16;
    unsafe { (*destination).length = length };
    if unsafe { (*destination).maximum_length } < length + 2 {
        return integer::BUFFER_OVERFLOW;
    }
    let buffer = unsafe { (*destination).buffer.cast_mut() };
    for (index, unit) in digits.iter().copied().chain(Some(0)).enumerate() {
        unsafe { buffer.add(index).write(u16::from(unit)) };
    }
    0
}

/// # Safety
/// The descriptor and its counted UTF-16 buffer must be readable, and value writable.
#[no_mangle]
#[inline(never)]
pub unsafe extern "efiapi" fn nt61_export_RtlUnicodeStringToInteger(
    string: *const UnicodeString,
    base: u32,
    value: *mut u32,
) -> u32 {
    let length = unsafe { (*string).length };
    if length == 0 || length & 1 != 0 || !matches!(base, 0 | 2 | 8 | 10 | 16) {
        unsafe { value.write(0) };
        return integer::INVALID_PARAMETER;
    }
    let units = unsafe { core::slice::from_raw_parts((*string).buffer, usize::from(length / 2)) };
    let result = integer::parse(units.iter().copied().skip_while(|&unit| unit <= 0x20), base);
    unsafe { value.write(result.unwrap_or_default()) };
    result.err().unwrap_or_default()
}

// A reached volatile read roots these relocations through LTO and --gc-sections.
// The common erased pointer type is used for retention only, never for calling.
struct ExportRoot(*const ());
unsafe impl Sync for ExportRoot {}
#[used]
static EXPORT_ROOTS: [ExportRoot; 53] = [
    ExportRoot(nt61_export_RtlAppendStringToString as *const ()),
    ExportRoot(nt61_export_RtlAppendUnicodeStringToString as *const ()),
    ExportRoot(nt61_export_RtlAppendUnicodeToString as *const ()),
    ExportRoot(nt61_export_RtlAreBitsClear as *const ()),
    ExportRoot(nt61_export_RtlAreBitsSet as *const ()),
    ExportRoot(nt61_export_RtlCharToInteger as *const ()),
    ExportRoot(nt61_export_RtlClearAllBits as *const ()),
    ExportRoot(nt61_export_RtlClearBit as *const ()),
    ExportRoot(nt61_export_RtlClearBits as *const ()),
    ExportRoot(nt61_export_RtlCompareMemory as *const ()),
    ExportRoot(nt61_export_RtlCompareMemoryUlong as *const ()),
    ExportRoot(nt61_export_RtlCompareString as *const ()),
    ExportRoot(nt61_export_RtlComputeCrc32 as *const ()),
    ExportRoot(nt61_export_RtlCopyMemory as *const ()),
    ExportRoot(nt61_export_RtlCopyString as *const ()),
    ExportRoot(nt61_export_RtlCopyUnicodeString as *const ()),
    ExportRoot(nt61_export_RtlEqualString as *const ()),
    ExportRoot(nt61_export_RtlFillMemory as *const ()),
    ExportRoot(nt61_export_RtlFindClearBits as *const ()),
    ExportRoot(nt61_export_RtlFindClearBitsAndSet as *const ()),
    ExportRoot(nt61_export_RtlFindClearRuns as *const ()),
    ExportRoot(nt61_export_RtlFindFirstRunClear as *const ()),
    ExportRoot(nt61_export_RtlFindLastBackwardRunClear as *const ()),
    ExportRoot(nt61_export_RtlFindLeastSignificantBit as *const ()),
    ExportRoot(nt61_export_RtlFindLongestRunClear as *const ()),
    ExportRoot(nt61_export_RtlFindMostSignificantBit as *const ()),
    ExportRoot(nt61_export_RtlFindNextForwardRunClear as *const ()),
    ExportRoot(nt61_export_RtlFindSetBits as *const ()),
    ExportRoot(nt61_export_RtlFindSetBitsAndClear as *const ()),
    ExportRoot(nt61_export_RtlInitializeBitMap as *const ()),
    ExportRoot(nt61_export_RtlInitAnsiString as *const ()),
    ExportRoot(nt61_export_RtlInitString as *const ()),
    ExportRoot(nt61_export_RtlInitUnicodeString as *const ()),
    ExportRoot(nt61_export_RtlInitUnicodeStringEx as *const ()),
    ExportRoot(nt61_export_RtlIntegerToChar as *const ()),
    ExportRoot(nt61_export_RtlIntegerToUnicode as *const ()),
    ExportRoot(nt61_export_RtlIntegerToUnicodeString as *const ()),
    ExportRoot(nt61_export_RtlMoveMemory as *const ()),
    ExportRoot(nt61_export_RtlNumberOfClearBits as *const ()),
    ExportRoot(nt61_export_RtlNumberOfSetBits as *const ()),
    ExportRoot(nt61_export_RtlNumberOfSetBitsUlongPtr as *const ()),
    ExportRoot(nt61_export_RtlPrefixString as *const ()),
    ExportRoot(nt61_export_RtlSecondsSince1970ToTime as *const ()),
    ExportRoot(nt61_export_RtlSecondsSince1980ToTime as *const ()),
    ExportRoot(nt61_export_RtlSetAllBits as *const ()),
    ExportRoot(nt61_export_RtlSetBit as *const ()),
    ExportRoot(nt61_export_RtlSetBits as *const ()),
    ExportRoot(nt61_export_RtlTestBit as *const ()),
    ExportRoot(nt61_export_RtlTimeToSecondsSince1970 as *const ()),
    ExportRoot(nt61_export_RtlTimeToSecondsSince1980 as *const ()),
    ExportRoot(nt61_export_RtlUnicodeStringToInteger as *const ()),
    ExportRoot(nt61_export_RtlValidateUnicodeString as *const ()),
    ExportRoot(nt61_export_RtlZeroMemory as *const ()),
];

pub fn retain() {
    for root in &EXPORT_ROOTS {
        unsafe {
            core::ptr::read_volatile(&root.0);
        }
    }
}

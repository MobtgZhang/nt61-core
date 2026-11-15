//! Small, fully implemented native PE API surface. `efiapi` selects Microsoft
//! x64 on x86_64 (the kernel's internal C ABI is SysV), and the supported platform
//! calling convention elsewhere. Each pointer must obey the WDK caller contract.
#![allow(non_snake_case)]

// Compile the same allocation-free bitmap implementation used by ntdll. This
// module also remains independently host-testable without kernel hardware code.
#[path = "../../libs/ntdll/src/ntdll/rtl_bitmap.rs"]
#[allow(dead_code)]
mod bitmap;
pub use bitmap::RtlBitmap;

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

// A reached volatile read roots these relocations through LTO and --gc-sections.
// The common erased pointer type is used for retention only, never for calling.
struct ExportRoot(*const ());
unsafe impl Sync for ExportRoot {}
#[used]
static EXPORT_ROOTS: [ExportRoot; 12] = [
    ExportRoot(nt61_export_RtlCompareMemory as *const ()),
    ExportRoot(nt61_export_RtlCopyMemory as *const ()),
    ExportRoot(nt61_export_RtlCopyUnicodeString as *const ()),
    ExportRoot(nt61_export_RtlFillMemory as *const ()),
    ExportRoot(nt61_export_RtlFindClearBits as *const ()),
    ExportRoot(nt61_export_RtlFindSetBits as *const ()),
    ExportRoot(nt61_export_RtlInitializeBitMap as *const ()),
    ExportRoot(nt61_export_RtlInitAnsiString as *const ()),
    ExportRoot(nt61_export_RtlInitUnicodeString as *const ()),
    ExportRoot(nt61_export_RtlInitUnicodeStringEx as *const ()),
    ExportRoot(nt61_export_RtlMoveMemory as *const ()),
    ExportRoot(nt61_export_RtlZeroMemory as *const ()),
];

pub fn retain() {
    for root in &EXPORT_ROOTS {
        unsafe {
            core::ptr::read_volatile(&root.0);
        }
    }
}

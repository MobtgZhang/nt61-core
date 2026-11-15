//! Shared behavior tests for native string export implementations.
use super::api::{AnsiString, UnicodeString};
use core::ptr::null;
type InitAnsi = unsafe extern "efiapi" fn(*mut AnsiString, *const u8);
type CopyUnicode = unsafe extern "efiapi" fn(*mut UnicodeString, *const UnicodeString);
const INIT: InitAnsi = super::api::nt61_export_RtlInitAnsiString;
const COPY: CopyUnicode = super::api::nt61_export_RtlCopyUnicodeString;

#[test]
fn ansi_64_bit_descriptor_layout_and_byte_semantics() {
    assert_eq!(core::mem::size_of::<AnsiString>(), 16);
    assert_eq!(core::mem::offset_of!(AnsiString, buffer), 8);
    let source = [0x80, 0xff, b'A', 0, b'B'];
    let saved = source;
    let mut out = AnsiString {
        length: 99,
        maximum_length: 99,
        buffer: null(),
    };
    unsafe {
        INIT(&mut out, source.as_ptr());
    }
    assert_eq!((out.length, out.maximum_length), (3, 4));
    assert_eq!(out.buffer, source.as_ptr());
    assert_eq!(source, saved);
    unsafe {
        INIT(&mut out, null());
    }
    assert_eq!((out.length, out.maximum_length), (0, 0));
    assert!(out.buffer.is_null());
    unsafe {
        INIT(&mut out, source.as_ptr().add(3));
    }
    assert_eq!((out.length, out.maximum_length), (0, 1));
    assert_eq!(out.buffer, unsafe { source.as_ptr().add(3) });
}
#[test]
fn ansi_long_strings_clamp_without_replacing_or_changing_buffer() {
    for length in [0xfffdusize, 0xfffe, 0xffff, 0x10000, 0x10101] {
        let mut source = vec![0x81; length + 1];
        source[length] = 0;
        let mut out = AnsiString {
            length: 99,
            maximum_length: 99,
            buffer: null(),
        };
        unsafe {
            INIT(&mut out, source.as_ptr());
        }
        assert_eq!(usize::from(out.length), length.min(0xfffe));
        assert_eq!(out.maximum_length, out.length + 1);
        assert_eq!(out.buffer, source.as_ptr());
        assert!(source[..length].iter().all(|&b| b == 0x81));
        assert_eq!(source[length], 0);
    }
}
fn copy_case(source_length: u16, maximum: u16) -> (UnicodeString, [u16; 6]) {
    let input = [0x2211u16, 0x4433, 0x6655, 0x8877];
    let mut output = [0xaaaau16; 6];
    let source = UnicodeString {
        length: source_length,
        maximum_length: 8,
        buffer: input.as_ptr(),
    };
    let mut destination = UnicodeString {
        length: 77,
        maximum_length: maximum,
        buffer: output.as_mut_ptr(),
    };
    let buffer = destination.buffer;
    unsafe {
        COPY(&mut destination, &source);
    }
    assert_eq!(destination.maximum_length, maximum);
    assert_eq!(destination.buffer, buffer);
    (destination, output)
}
#[test]
fn copy_unicode_copies_bytes_truncates_and_only_terminates_with_space() {
    assert_eq!(
        copy_case(4, 8).1,
        [0x2211, 0x4433, 0, 0xaaaa, 0xaaaa, 0xaaaa]
    );
    let (dst, output) = copy_case(8, 4);
    assert_eq!(dst.length, 4);
    assert_eq!(output, [0x2211, 0x4433, 0xaaaa, 0xaaaa, 0xaaaa, 0xaaaa]);
    let (dst, output) = copy_case(8, 3);
    assert_eq!(dst.length, 3);
    assert_eq!(output, [0x2211, 0xaa33, 0xaaaa, 0xaaaa, 0xaaaa, 0xaaaa]);
    assert_eq!(
        copy_case(4, 5).1,
        [0x2211, 0x4433, 0xaaaa, 0xaaaa, 0xaaaa, 0xaaaa]
    );
}
#[test]
fn copy_unicode_odd_source_length_retains_original_wchar_terminator_position() {
    let (dst, output) = copy_case(3, 8);
    assert_eq!(dst.length, 3);
    // Original copies 11 22 33 then writes WCHAR NUL at byte offset 2.
    assert_eq!(output, [0x2211, 0, 0xaaaa, 0xaaaa, 0xaaaa, 0xaaaa]);
    assert_eq!(
        copy_case(3, 4).1,
        [0x2211, 0xaa33, 0xaaaa, 0xaaaa, 0xaaaa, 0xaaaa]
    );
    assert_eq!(
        copy_case(1, 3).1,
        [0, 0xaaaa, 0xaaaa, 0xaaaa, 0xaaaa, 0xaaaa]
    );
}
#[test]
fn null_source_descriptor_and_empty_nonnull_descriptor_have_distinct_effects() {
    let mut output = [0xaaaau16; 2];
    let mut destination = UnicodeString {
        length: 3,
        maximum_length: 4,
        buffer: output.as_mut_ptr(),
    };
    unsafe {
        COPY(&mut destination, null());
    }
    assert_eq!(destination.length, 0);
    assert_eq!(output, [0xaaaa; 2]);
    let empty = UnicodeString {
        length: 0,
        maximum_length: 0,
        buffer: null(),
    };
    unsafe {
        COPY(&mut destination, &empty);
    }
    assert_eq!(output, [0, 0xaaaa]);
    destination.buffer = null();
    destination.maximum_length = 0;
    unsafe {
        COPY(&mut destination, &empty);
    }
    assert_eq!(destination.length, 0);
}

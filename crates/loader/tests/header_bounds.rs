//! Header parsing must use the COFF-declared extent, including on host tooling paths.
use nt61_loader::loader::{get_sections, load_pe, parse_headers, read_data_directory_pe32plus};

fn header() -> Vec<u8> {
    let mut bytes = vec![0; 0x200];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
    bytes[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());
    bytes
}

#[test]
fn declared_optional_header_and_directory_count_bound_all_consumers() {
    let mut raw = header();
    assert!(parse_headers(&raw).is_some());
    // Bytes exist in the buffer, but no longer belong to the declared header.
    raw[0x94..0x96].copy_from_slice(&2u16.to_le_bytes());
    assert!(parse_headers(&raw).is_none());
    assert!(load_pe(&raw).is_none());
    assert_eq!(read_data_directory_pe32plus(&raw, 0x98, 0), (0, 0));
    assert_eq!(get_sections(&raw, &mut []), 0);
    raw = header();
    raw[0x104..0x108].copy_from_slice(&17u32.to_le_bytes());
    assert!(parse_headers(&raw).is_none());
}

#[test]
fn negative_offsets_and_truncated_headers_never_panic() {
    let mut raw = header();
    for length in 0..0x188 {
        assert!(parse_headers(&raw[..length]).is_none());
        assert!(load_pe(&raw[..length]).is_none());
    }
    raw[0x3c..0x40].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(parse_headers(&raw).is_none());
    assert!(load_pe(&raw).is_none());
}

#[test]
fn truncated_section_table_and_extreme_directory_indices_are_rejected() {
    let mut raw = header();
    raw[0x86..0x88].copy_from_slice(&4u16.to_le_bytes());
    assert!(parse_headers(&raw).is_none());
    let raw = header();
    assert_eq!(read_data_directory_pe32plus(&raw, 0x98, usize::MAX), (0, 0));
    assert_eq!(read_data_directory_pe32plus(&raw, usize::MAX, 0), (0, 0));
}

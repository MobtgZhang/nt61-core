use super::*;
use bitmap::dib::nt_gdi_get_dibits_internal as readback;

fn info(width: i32, height: i32, depth: u16) -> Vec<u8> {
    let mut bytes = vec![0; 40];
    bytes[..4].copy_from_slice(&40u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&width.to_le_bytes());
    bytes[8..12].copy_from_slice(&height.to_le_bytes());
    bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&depth.to_le_bytes());
    bytes
}

fn create_bitmap(width: i32, height: i32, depth: u16, bits: &[u8]) -> u64 {
    mm::user_copy::allow(bits.as_ptr() as u64, bits.len(), false);
    let handle = call(
        bitmap::storage::nt_gdi_create_bitmap,
        &[
            width as u64,
            height as u64,
            1,
            depth as u64,
            bits.as_ptr() as u64,
        ],
    );
    assert_ne!(handle, 0);
    handle
}

fn arguments(dc: u64, bitmap: u64, info: &mut [u8], output: &mut [u8]) -> [u64; 9] {
    mm::user_copy::allow(info.as_mut_ptr() as u64, info.len(), true);
    mm::user_copy::allow(output.as_mut_ptr() as u64, output.len(), true);
    [
        dc,
        bitmap,
        0,
        100,
        output.as_mut_ptr() as u64,
        info.as_mut_ptr() as u64,
        0,
        output.len() as u64,
        info.len() as u64,
    ]
}

#[test]
fn dib_query_reports_real_metadata_without_touching_pixel_output() {
    let target = DrawingFixture::new();
    let bitmap = objects::gdi_create_bitmap(3, 2, 24);
    let mut header = info(0, 0, 0);
    let mut output = [0xab; 8];
    let args = arguments(target.handle, bitmap, &mut header, &mut output);
    assert_eq!(call(readback, &args), 2);
    assert_eq!(i32::from_le_bytes(header[4..8].try_into().unwrap()), 3);
    assert_eq!(i32::from_le_bytes(header[8..12].try_into().unwrap()), 2);
    assert_eq!(u16::from_le_bytes(header[14..16].try_into().unwrap()), 24);
    assert_eq!(u32::from_le_bytes(header[20..24].try_into().unwrap()), 24);
    assert_eq!(output, [0xab; 8]);
    assert!(objects::gdi_delete_object(bitmap));
}

#[test]
fn dib_readback_orders_scanlines_and_clamps_partial_reads() {
    let target = DrawingFixture::new();
    let bits = [
        0, 0, 0xff, 0, 0, 0xff, 0, 0, 0xff, 0, 0, 0, 0xff, 0xff, 0xff, 0,
    ];
    let bitmap = create_bitmap(2, 2, 32, &bits);
    for (height, start, expected, count) in [
        (2, 0, [&bits[8..16], &bits[..8]].concat(), 2),
        (2, 1, bits[..8].to_vec(), 1),
        (-2, 0, bits.to_vec(), 2),
        (-2, 1, bits[..8].to_vec(), 1),
    ] {
        let mut header = info(2, height, 32);
        let mut output = [0xab; 20];
        let mut args = arguments(target.handle, bitmap, &mut header, &mut output[..16]);
        args[2] = start;
        assert_eq!(call(readback, &args), count);
        assert_eq!(&output[..expected.len()], expected);
        assert!(output[expected.len()..].iter().all(|&byte| byte == 0xab));
    }
    assert!(objects::gdi_delete_object(bitmap));
}

#[test]
fn dib_readback_converts_rgb555_and_zeros_public_padding_and_alpha() {
    let target = DrawingFixture::new();
    let bitmap = create_bitmap(1, 1, 32, &[0x56, 0x34, 0x12, 0xff]);
    for (depth, expected) in [
        (32, [0x56, 0x34, 0x12, 0]),
        (24, [0x56, 0x34, 0x12, 0]),
        (16, [0xca, 0x08, 0, 0]),
    ] {
        let mut header = info(1, 1, depth);
        let mut output = [0xab; 4];
        let args = arguments(target.handle, bitmap, &mut header, &mut output);
        assert_eq!(call(readback, &args), 1);
        assert_eq!(output, expected);
    }
    assert!(objects::gdi_delete_object(bitmap));
    let bitmap = bitmap::storage::nt_gdi_create_bitmap;
    let input = [0x84u8, 0x10];
    mm::user_copy::allow(input.as_ptr() as u64, input.len(), false);
    let handle = call(bitmap, &[1, 1, 1, 16, input.as_ptr() as u64]);
    let mut header = info(1, 1, 32);
    let mut output = [0; 4];
    let args = arguments(target.handle, handle, &mut header, &mut output);
    assert_eq!(call(readback, &args), 1);
    assert_eq!(output, [0x21, 0x21, 0x21, 0]);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn dib_core_header_query_uses_16_bit_dimensions() {
    let target = DrawingFixture::new();
    let bitmap = objects::gdi_create_bitmap(2, 3, 32);
    let mut header = [12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut output = [0xab; 4];
    let args = arguments(target.handle, bitmap, &mut header, &mut output);
    assert_eq!(call(readback, &args), 3);
    assert_eq!(header, [12, 0, 0, 0, 2, 0, 3, 0, 1, 0, 32, 0]);
    assert_eq!(output, [0xab; 4]);
    assert!(objects::gdi_delete_object(bitmap));
}

#[test]
fn dib_readback_rejects_bad_native_bounds_handles_and_unsupported_conversions() {
    let target = DrawingFixture::new();
    let bitmap = objects::gdi_create_bitmap(1, 1, 32);
    let mut header = info(1, 1, 32);
    let mut output = [0xab; 4];
    let args = arguments(target.handle, bitmap, &mut header, &mut output);
    for (index, value) in [
        (0, u64::MAX),
        (1, u64::MAX),
        (4, 1),
        (5, 1),
        (6, 1),
        (7, 3),
        (8, 39),
    ] {
        let mut invalid = args;
        invalid[index] = value;
        assert_eq!(call(readback, &invalid), 0, "argument {index}");
        assert_eq!(output, [0xab; 4]);
    }
    for (width, height, depth) in [(2, 1, 32), (1, 2, 32), (1, i32::MIN, 32), (1, 1, 8)] {
        let mut header = info(width, height, depth);
        let args = arguments(target.handle, bitmap, &mut header, &mut output);
        assert_eq!(call(readback, &args), 0);
        assert_eq!(output, [0xab; 4]);
    }
    assert!(objects::gdi_delete_object(bitmap));
    assert_eq!(call(readback, &args), 0);
}

#[test]
fn dib_bitfield_query_can_be_used_for_a_subsequent_native_read() {
    let target = DrawingFixture::new();
    let bitmap = create_bitmap(1, 2, 32, &[0, 0, 0xff, 0, 0xff, 0, 0, 0]);
    let mut header = info(0, 0, 0);
    header.resize(52, 0xab);
    let mut output = [0xab; 8];
    let mut args = arguments(target.handle, bitmap, &mut header, &mut output);
    args[4] = 0;
    assert_eq!(call(readback, &args), 2);
    assert_eq!(u32::from_le_bytes(header[16..20].try_into().unwrap()), 3);
    assert_eq!(&header[40..], &[0xab; 12]);
    args[4] = output.as_mut_ptr() as u64;
    assert_eq!(call(readback, &args), 2);
    assert_eq!(output, [0xff, 0, 0, 0, 0, 0, 0xff, 0]);
    assert_eq!(
        &header[40..],
        &[0, 0, 0xff, 0, 0, 0xff, 0, 0, 0xff, 0, 0, 0]
    );
    assert!(objects::gdi_delete_object(bitmap));
}

#[test]
fn dib_empty_scan_band_queries_format_without_writing_pixels() {
    let target = DrawingFixture::new();
    let bitmap = objects::gdi_create_bitmap(1, 1, 32);
    for (start, lines) in [(0, 0), (1, 1), (u32::MAX as u64, 1)] {
        let mut header = info(1, 1, 32);
        let mut output = [0xab; 4];
        let mut args = arguments(target.handle, bitmap, &mut header, &mut output);
        args[2] = start;
        args[3] = lines;
        assert_eq!(call(readback, &args), 1);
        assert_eq!(output, [0xab; 4]);
    }
    assert!(objects::gdi_delete_object(bitmap));
}

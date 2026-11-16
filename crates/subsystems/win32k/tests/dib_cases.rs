use super::*;
use raster::dib::nt_gdi_stretch_dibits_internal as transfer;

const SRCCOPY: u64 = 0x00cc_0020;

fn header(width: i32, height: i32, depth: u16) -> Vec<u8> {
    let mut info = vec![0; 40];
    info[0..4].copy_from_slice(&40u32.to_le_bytes());
    info[4..8].copy_from_slice(&width.to_le_bytes());
    info[8..12].copy_from_slice(&height.to_le_bytes());
    info[12..14].copy_from_slice(&1u16.to_le_bytes());
    info[14..16].copy_from_slice(&depth.to_le_bytes());
    info
}

fn arguments(
    destination: u64,
    info: &[u8],
    bits: &[u8],
    target: [i32; 4],
    source: [i32; 4],
) -> [u64; 16] {
    mm::user_copy::allow(info.as_ptr() as u64, info.len(), false);
    mm::user_copy::allow(bits.as_ptr() as u64, bits.len(), false);
    [
        destination,
        target[0] as u64,
        target[1] as u64,
        target[2] as u64,
        target[3] as u64,
        source[0] as u64,
        source[1] as u64,
        source[2] as u64,
        source[3] as u64,
        bits.as_ptr() as u64,
        info.as_ptr() as u64,
        0,
        SRCCOPY,
        info.len() as u64,
        bits.len() as u64,
        0,
    ]
}

#[test]
fn dib_decodes_bottom_up_rgb24_with_dword_padding_and_crops_from_bottom() {
    let target = DrawingFixture::new();
    let info = header(1, 2, 24);
    let bits = [0xff, 0, 0, 0xab, 0, 0, 0xff, 0xcd];
    let args = arguments(target.handle, &info, &bits, [1, 2, 2, 4], [0, 0, 1, 2]);
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(1, 2), 0xff0000);
    assert_eq!(target.pixel(2, 3), 0xff0000);
    assert_eq!(target.pixel(1, 4), 0xff);
    assert_eq!(target.pixel(2, 5), 0xff);
    let args = arguments(target.handle, &info, &bits, [5, 0, 1, 1], [0, 0, 1, 1]);
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(5, 0), 0xff);
}

#[test]
fn dib_decodes_top_down_bgrx32_and_discards_reserved_alpha() {
    let target = DrawingFixture::new();
    let info = header(1, -2, 32);
    let bits = [0x56, 0x34, 0x12, 0xff, 0xef, 0xcd, 0xab, 0x80];
    let args = arguments(target.handle, &info, &bits, [0, 0, 1, 2], [0, 0, 1, 2]);
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(0, 0), 0x123456);
    assert_eq!(target.pixel(0, 1), 0xabcdef);
}

#[test]
fn dib_rgb555_and_explicit_rgb565_use_channel_masks() {
    for (compression, masks, value, expected) in [
        (0u32, [0, 0, 0], 0x7c00u16, 0xff0000),
        (0, [0, 0, 0], 0x1084, 0x212121),
        (3, [0xf800u32, 0x07e0, 0x001f], 0x07e0, 0x00ff00),
        (3, [0x001f, 0x07e0, 0xf800], 0xf800, 0x0000ff),
    ] {
        let target = DrawingFixture::new();
        let mut info = header(1, 1, 16);
        info[16..20].copy_from_slice(&compression.to_le_bytes());
        if compression == 3 {
            for mask in masks {
                info.extend_from_slice(&mask.to_le_bytes());
            }
        }
        let bytes = value.to_le_bytes();
        let bits = [bytes[0], bytes[1], 0, 0];
        let args = arguments(target.handle, &info, &bits, [0, 0, 1, 1], [0, 0, 1, 1]);
        assert_eq!(call(transfer, &args), 1);
        assert_eq!(target.pixel(0, 0), expected);
    }
}

#[test]
fn dib_v4_and_v5_read_inline_bitfields_instead_of_palette_data() {
    for size in [108u32, 124] {
        let target = DrawingFixture::new();
        let mut info = header(1, 1, 32);
        info.resize(size as usize, 0);
        info[..4].copy_from_slice(&size.to_le_bytes());
        info[16..20].copy_from_slice(&3u32.to_le_bytes());
        for (index, mask) in [0x000000ffu32, 0x0000ff00, 0x00ff0000]
            .into_iter()
            .enumerate()
        {
            info[40 + index * 4..44 + index * 4].copy_from_slice(&mask.to_le_bytes());
        }
        let bits = [0x12, 0x34, 0x56, 0x78];
        let args = arguments(target.handle, &info, &bits, [0, 0, 1, 1], [0, 0, 1, 1]);
        assert_eq!(call(transfer, &args), 1);
        assert_eq!(target.pixel(0, 0), 0x123456);
    }
}

#[test]
fn dib_indexed_formats_use_most_significant_bit_and_high_nibble_first() {
    for (depth, bits) in [
        (1u16, [0x40, 0, 0, 0]),
        (4, [0x01, 0, 0, 0]),
        (8, [0, 1, 0, 0]),
    ] {
        let target = DrawingFixture::new();
        let mut info = header(2, 1, depth);
        info[32..36].copy_from_slice(&2u32.to_le_bytes());
        info.extend_from_slice(&[0, 0, 0xff, 0, 0xff, 0, 0, 0]);
        let args = arguments(target.handle, &info, &bits, [0, 0, 2, 1], [0, 0, 2, 1]);
        assert_eq!(call(transfer, &args), 1);
        assert_eq!(target.pixel(0, 0), 0xff0000);
        assert_eq!(target.pixel(1, 0), 0xff);
    }
}

#[test]
fn dib_core_header_uses_rgbtriples_and_implicit_palette_size() {
    let target = DrawingFixture::new();
    let info = [12, 0, 0, 0, 2, 0, 1, 0, 1, 0, 1, 0, 0, 0xff, 0, 0xff, 0, 0];
    let bits = [0x40, 0, 0, 0];
    let args = arguments(target.handle, &info, &bits, [0, 0, 2, 1], [0, 0, 2, 1]);
    assert_eq!(call(transfer, &args), 1);
    assert_eq!(target.pixel(0, 0), 0xff00);
    assert_eq!(target.pixel(1, 0), 0xff);
}

#[test]
fn dib_reuses_destination_mapping_clip_mirroring_and_rop() {
    let target = DrawingFixture::new();
    dc::get_dc(target.handle).unwrap().viewport_org = dc::PointL::new(2, 3);
    call(
        clipping::nt_gdi_intersect_clip_rect,
        &[target.handle, 0, 0, 3, 1],
    );
    let info = header(2, -1, 32);
    let bits = [0xff, 0, 0, 0, 0, 0, 0xff, 0];
    let mut args = arguments(target.handle, &info, &bits, [4, 0, -4, 1], [0, 0, 2, 1]);
    args[12] = 0x0066_0046;
    assert_eq!(call(transfer, &args), u64::from(u32::MAX));
    assert_eq!(target.pixel(2, 3), 0x123456 ^ 0xff0000);
    assert_eq!(target.pixel(3, 3), 0x123456 ^ 0xff0000);
    assert_eq!(target.pixel(4, 3), 0x123456 ^ 0xff);
    assert_eq!(target.pixel(5, 3), 0x123456);
}

#[test]
fn dib_invalid_headers_masks_and_palettes_do_not_draw() {
    let target = DrawingFixture::new();
    let bits = [0xff; 16];
    let mut invalid = Vec::new();
    for (width, height, depth) in [
        (0, 1, 32),
        (-1, 1, 32),
        (1, 0, 32),
        (1, i32::MIN, 32),
        (i32::MAX, 2, 32),
        (1, 1, 2),
    ] {
        invalid.push(header(width, height, depth));
    }
    for compression in [1u32, 2, 4, 5, 6, u32::MAX] {
        let mut info = header(1, 1, 32);
        info[16..20].copy_from_slice(&compression.to_le_bytes());
        invalid.push(info);
    }
    for masks in [
        [0u32, 0xff00, 0xff],
        [0xff, 0xff, 0xff0000],
        [0x55, 0xff00, 0xff0000],
        [0x10000, 0xff00, 0xff],
    ] {
        let mut info = header(1, 1, 16);
        info[16..20].copy_from_slice(&3u32.to_le_bytes());
        for mask in masks {
            info.extend_from_slice(&mask.to_le_bytes());
        }
        invalid.push(info);
    }
    let mut invalid_palette = header(1, 1, 8);
    invalid_palette[32..36].copy_from_slice(&1u32.to_le_bytes());
    invalid_palette.extend_from_slice(&[0; 4]);
    invalid.push(invalid_palette);
    for info in invalid {
        let args = arguments(target.handle, &info, &bits, [0, 0, 1, 1], [0, 0, 1, 1]);
        assert_eq!(call(transfer, &args), 0, "header: {info:?}");
        assert_eq!(target.pixel(0, 0), 0x123456);
    }
}

#[test]
fn dib_honors_native_buffer_bounds_invalid_pointers_and_unimplemented_modes() {
    let target = DrawingFixture::new();
    let info = header(1, 1, 24);
    let bits = [0xff, 0, 0, 0];
    let args = arguments(target.handle, &info, &bits, [0, 0, 1, 1], [0, 0, 1, 1]);
    for (index, value) in [
        (0, u64::MAX),
        (9, 0),
        (10, 0),
        (11, 1),
        (12, 0x40cc0020),
        (13, 39),
        (14, 3),
        (15, 1),
        (7, 0),
        (3, 0),
        (5, i32::MAX as u64),
    ] {
        let mut invalid = args;
        invalid[index] = value;
        assert_eq!(call(transfer, &invalid), 0, "argument {index}");
        assert_eq!(target.pixel(0, 0), 0x123456);
    }
    dc::get_dc(target.handle).unwrap().stretch_mode = 4;
    assert_eq!(call(transfer, &args), 0);
    assert_eq!(target.pixel(0, 0), 0x123456);
}

#[test]
fn dib_temporary_surface_is_released_on_success_and_allocation_failure() {
    let target = DrawingFixture::new();
    let info = header(1, 1, 24);
    let bits = [0xff, 0, 0, 0];
    let args = arguments(target.handle, &info, &bits, [0, 0, 1, 1], [0, 0, 1, 1]);
    let before = mm::pool::balance();
    mm::pool::fail_after(0);
    assert_eq!(call(transfer, &args), 0);
    assert_eq!(target.pixel(0, 0), 0x123456);
    assert_eq!(mm::pool::balance(), before);
    assert_eq!(call(transfer, &args), 1);
    assert_eq!(mm::pool::balance(), before);
}

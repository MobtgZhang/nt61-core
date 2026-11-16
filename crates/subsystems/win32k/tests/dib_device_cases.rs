use super::*;
use raster::dib::nt_gdi_set_dibits_to_device_internal as transfer;

fn header(height: i32) -> Vec<u8> {
    let mut bytes = vec![0; 40];
    bytes[..4].copy_from_slice(&40u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&1i32.to_le_bytes());
    bytes[8..12].copy_from_slice(&height.to_le_bytes());
    bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&32u16.to_le_bytes());
    bytes
}

fn arguments(dc: u64, info: &[u8], bits: &[u8], start: u32, scans: u32) -> [u64; 16] {
    mm::user_copy::allow(info.as_ptr() as u64, info.len(), false);
    mm::user_copy::allow(bits.as_ptr() as u64, bits.len(), false);
    [
        dc,
        0,
        0,
        1,
        2,
        0,
        0,
        start as u64,
        scans as u64,
        bits.as_ptr() as u64,
        info.as_ptr() as u64,
        0,
        bits.len() as u64,
        info.len() as u64,
        1,
        0,
    ]
}

#[test]
fn dib_device_orders_both_row_directions_without_scaling() {
    let pixels = [0xffu8, 0, 0, 0x80, 0, 0, 0xff, 0xff];
    for (height, expected) in [(2, [0xff0000, 0xff]), (-2, [0xff, 0xff0000])] {
        let target = DrawingFixture::new();
        let info = header(height);
        let args = arguments(target.handle, &info, &pixels, 0, 2);
        assert_eq!(call(transfer, &args), 2);
        assert_eq!(target.pixel(0, 0), expected[0]);
        assert_eq!(target.pixel(0, 1), expected[1]);
        assert_eq!(target.pixel(1, 0), 0x123456);
    }
}

#[test]
fn dib_device_partial_band_buffer_starts_at_first_supplied_scan() {
    let target = DrawingFixture::new();
    let info = header(5);
    let pixels = [0xffu8, 0, 0, 0, 0, 0, 0xff, 0];
    let mut args = arguments(target.handle, &info, &pixels, 2, 2);
    args[6] = 2;
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(0, 0), 0xff0000);
    assert_eq!(target.pixel(0, 1), 0xff);
    assert_eq!(target.pixel(0, 2), 0x123456);
}

#[test]
fn dib_device_clamps_bottom_up_scan_count_to_bitmap_height() {
    let target = DrawingFixture::new();
    let info = header(3);
    let pixels = [0xffu8, 0, 0, 0];
    let mut args = arguments(target.handle, &info, &pixels, 2, 100);
    args[4] = 1;
    args[6] = 2;
    assert_eq!(call(transfer, &args), 1);
    assert_eq!(target.pixel(0, 0), 0xff);
}

#[test]
fn dib_device_maps_only_origin_and_ignores_stretch_filter() {
    let target = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(target.handle).unwrap();
        dc.map_mode = 8;
        dc.viewport_org = dc::PointL::new(2, 3);
        dc.viewport_ext = dc::SizeL::new(2, 2);
        dc.window_ext = dc::SizeL::new(1, 1);
        dc.stretch_mode = 4;
    }
    let info = header(2);
    let pixels = [0xffu8, 0, 0, 0, 0, 0, 0xff, 0];
    let mut args = arguments(target.handle, &info, &pixels, 0, 2);
    args[1] = 1;
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(4, 3), 0xff0000);
    assert_eq!(target.pixel(4, 4), 0xff);
    assert_eq!(target.pixel(5, 3), 0x123456);
    assert_eq!(target.pixel(4, 5), 0x123456);
    args[14] = 0;
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(1, 0), 0xff0000);
    assert_eq!(target.pixel(1, 1), 0xff);
}

#[test]
fn dib_device_obeys_clip_and_returns_scan_count_for_fully_clipped_draw() {
    let target = DrawingFixture::new();
    call(
        clipping::nt_gdi_intersect_clip_rect,
        &[target.handle, 0, 1, 1, 2],
    );
    let info = header(2);
    let pixels = [0xffu8, 0, 0, 0, 0, 0, 0xff, 0];
    let mut args = arguments(target.handle, &info, &pixels, 0, 2);
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(0, 0), 0x123456);
    assert_eq!(target.pixel(0, 1), 0xff);
    args[1] = 4;
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(target.pixel(4, 0), 0x123456);
}

#[test]
fn dib_device_validates_native_limits_before_any_destination_write() {
    let target = DrawingFixture::new();
    let info = header(2);
    let pixels = [0xffu8, 0, 0, 0, 0, 0, 0xff, 0];
    let args = arguments(target.handle, &info, &pixels, 0, 2);
    for (index, value) in [
        (0, u64::MAX),
        (3, u32::MAX as u64),
        (4, 0),
        (7, 2),
        (8, 0),
        (9, 1),
        (10, 1),
        (11, 1),
        (12, 7),
        (13, 39),
        (15, 1),
    ] {
        let mut invalid = args;
        invalid[index] = value;
        assert_eq!(call(transfer, &invalid), 0, "argument {index}");
        assert_eq!(target.pixel(0, 0), 0x123456);
    }
    let before = mm::pool::balance();
    mm::pool::fail_after(0);
    assert_eq!(call(transfer, &args), 0);
    assert_eq!(mm::pool::balance(), before);
    assert_eq!(call(transfer, &args), 2);
    assert_eq!(mm::pool::balance(), before);
}

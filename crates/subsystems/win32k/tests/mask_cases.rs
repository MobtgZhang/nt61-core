use super::*;
use raster::mask::nt_gdi_mask_blt;

struct MaskFixture(u64);

impl MaskFixture {
    fn new(width: i32, height: i32, bytes: &[(usize, u8)]) -> Self {
        let handle = objects::gdi_create_bitmap(width, height, 1);
        assert_ne!(handle, 0);
        objects::with_handle_table(|table| {
            let entry = table.get(handle).unwrap();
            // SAFETY: the table guard pins the live bitmap and serializes writes.
            let bitmap = unsafe { &mut *(entry.obj_ptr as *mut objects::GdiBitmap) };
            for &(offset, byte) in bytes {
                assert!(offset < bitmap.size as usize);
                // SAFETY: the offset is inside the exclusively borrowed bitmap.
                unsafe {
                    bitmap.bits.add(offset).write(byte);
                }
            }
        });
        Self(handle)
    }
}

impl Drop for MaskFixture {
    fn drop(&mut self) {
        assert!(objects::gdi_delete_object(self.0));
    }
}

fn masked(
    destination: u64,
    source: u64,
    mask: u64,
    target: [i32; 4],
    input: [i32; 2],
    origin: [i32; 2],
    rop: u64,
) -> u64 {
    call(
        nt_gdi_mask_blt,
        &[
            destination,
            target[0] as u64,
            target[1] as u64,
            target[2] as u64,
            target[3] as u64,
            source,
            input[0] as u64,
            input[1] as u64,
            mask,
            origin[0] as u64,
            origin[1] as u64,
            rop,
            0,
        ],
    )
}

#[test]
fn mask_bits_choose_independent_foreground_and_background_rops() {
    let mut source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    source.surface.clear(0xffaabb);
    let mask = MaskFixture::new(8, 1, &[(0, 0xa0)]);
    assert_eq!(
        masked(
            destination.handle,
            source.handle,
            mask.0,
            [0, 0, 3, 1],
            [0, 0],
            [0, 0],
            0xaacc_0020
        ),
        1
    );
    assert_eq!(destination.pixel(0, 0), 0xffaabb);
    assert_eq!(destination.pixel(1, 0), 0x123456);
    assert_eq!(destination.pixel(2, 0), 0xffaabb);
    assert_eq!(
        masked(
            destination.handle,
            source.handle,
            mask.0,
            [0, 1, 2, 1],
            [0, 0],
            [0, 0],
            0xff66_0046
        ),
        1
    );
    assert_eq!(destination.pixel(0, 1), 0x123456 ^ 0xffaabb);
    assert_eq!(destination.pixel(1, 1), 0xffffff);
}

#[test]
fn mask_source_capture_precedes_overlapping_destination_writes() {
    let mut fixture = DrawingFixture::new();
    for (x, pixel) in [0xff0000, 0xff00, 0xff].into_iter().enumerate() {
        assert!(fixture.surface.set_pixel(x as i32, 0, pixel));
    }
    let mask = MaskFixture::new(8, 1, &[(0, 0xff)]);
    assert_eq!(
        masked(
            fixture.handle,
            fixture.handle,
            mask.0,
            [1, 0, 3, 1],
            [0, 0],
            [0, 0],
            0xaacc_0020
        ),
        1
    );
    assert_eq!(fixture.pixel(1, 0), 0xff0000);
    assert_eq!(fixture.pixel(2, 0), 0xff00);
    assert_eq!(fixture.pixel(3, 0), 0xff);
}

#[test]
fn mask_origin_crosses_byte_boundaries_and_uses_both_dc_origins() {
    let mut source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    assert!(source.surface.set_pixel(1, 0, 0xabcdef));
    assert!(source.surface.set_pixel(2, 0, 0xfedcba));
    assert!(source.surface.set_pixel(3, 0, 0x778899));
    let mask = MaskFixture::new(16, 2, &[(5, 0x50)]);
    dc::get_dc(source.handle).unwrap().viewport_org = dc::PointL::new(1, 0);
    dc::get_dc(source.handle).unwrap().application_clip = Some(Region::default());
    dc::get_dc(destination.handle).unwrap().viewport_org = dc::PointL::new(2, 3);
    call(
        clipping::nt_gdi_intersect_clip_rect,
        &[destination.handle, 1, 0, 3, 1],
    );
    assert_eq!(
        masked(
            destination.handle,
            source.handle,
            mask.0,
            [0, 0, 3, 1],
            [0, 0],
            [9, 1],
            0xaacc_0020
        ),
        1
    );
    assert_eq!(destination.pixel(2, 3), 0x123456);
    assert_eq!(destination.pixel(3, 3), 0x123456);
    // Offset 9 selects bit 6, then bit 5, then bit 4: 1, 0, 1.
    assert_eq!(destination.pixel(4, 3), 0x778899);
}

#[test]
fn mask_rops_that_do_not_use_source_accept_an_unused_source_handle() {
    let destination = DrawingFixture::new();
    let mask = MaskFixture::new(8, 1, &[(0, 0x80)]);
    let brush = objects::gdi_create_solid_brush(0xff00);
    call(selection::nt_gdi_select_brush, &[destination.handle, brush]);
    assert_eq!(
        masked(
            destination.handle,
            u64::MAX,
            mask.0,
            [0, 0, 2, 1],
            [i32::MAX; 2],
            [0, 0],
            0x00f0_0021
        ),
        1
    );
    assert_eq!(destination.pixel(0, 0), 0xff00);
    assert_eq!(destination.pixel(1, 0), 0);
    call(
        selection::nt_gdi_select_brush,
        &[destination.handle, objects::gdi_get_stock_object(5)],
    );
    assert_eq!(
        masked(
            destination.handle,
            0,
            mask.0,
            [0, 1, 2, 1],
            [0, 0],
            [0, 0],
            0xfff0_0021
        ),
        1
    );
    assert_eq!(destination.pixel(0, 1), 0x123456);
    assert_eq!(destination.pixel(1, 1), 0xffffff);
    assert!(objects::gdi_delete_object(brush));
}

#[test]
fn absent_mask_uses_only_the_foreground_rop_and_ignores_mask_coordinates() {
    let destination = DrawingFixture::new();
    assert_eq!(
        masked(
            destination.handle,
            u64::MAX,
            0,
            [0, 0, 2, 1],
            [0, 0],
            [i32::MIN; 2],
            0xcc00_0042
        ),
        1
    );
    assert_eq!(destination.pixel(0, 0), 0);
    assert_eq!(destination.pixel(1, 0), 0);
}

#[test]
fn invalid_mask_formats_handles_offsets_and_sizes_fail_without_drawing() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    let mask = MaskFixture::new(8, 2, &[(0, 0xff)]);
    let color = objects::gdi_create_bitmap(8, 2, 32);
    let stale = objects::gdi_create_bitmap(8, 2, 1);
    let brush = objects::gdi_create_solid_brush(0);
    assert!(objects::gdi_delete_object(stale));
    for handle in [color, stale, brush, u64::MAX] {
        assert_eq!(
            masked(
                destination.handle,
                source.handle,
                handle,
                [0, 0, 2, 1],
                [0, 0],
                [0, 0],
                0xaacc_0020
            ),
            0
        );
    }
    for (target, origin) in [
        ([0, 0, 2, 1], [-1, 0]),
        ([0, 0, 2, 1], [7, 0]),
        ([0, 0, 2, 1], [0, 2]),
        ([0, 0, 2, 1], [i32::MAX, 0]),
        ([0, 0, -1, 1], [0, 0]),
        ([i32::MAX, 0, 2, 1], [0, 0]),
    ] {
        assert_eq!(
            masked(
                destination.handle,
                source.handle,
                mask.0,
                target,
                [0, 0],
                origin,
                0xaacc_0020
            ),
            0
        );
    }
    assert_eq!(
        masked(
            destination.handle,
            0,
            mask.0,
            [0, 0, 2, 1],
            [0, 0],
            [0, 0],
            0xaacc_0020
        ),
        0
    );
    assert_eq!(destination.pixel(0, 0), 0x123456);
    assert!(objects::gdi_delete_object(color));
    assert!(objects::gdi_delete_object(brush));
}

#[test]
fn mask_capture_serializes_with_deletion() {
    let destination = DrawingFixture::new();
    let handle = objects::gdi_create_bitmap(16, 16, 1);
    let destination_handle = destination.handle;
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            for _ in 0..100 {
                assert!(
                    masked(
                        destination_handle,
                        0,
                        handle,
                        [0, 0, 16, 16],
                        [0, 0],
                        [0, 0],
                        0xff00_0042
                    ) <= 1
                );
            }
        });
        assert!(objects::gdi_delete_object(handle));
        worker.join().unwrap();
    });
    assert_eq!(
        masked(
            destination.handle,
            0,
            handle,
            [0, 0, 1, 1],
            [0, 0],
            [0, 0],
            0xff00_0042
        ),
        0
    );
}

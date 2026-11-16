use super::*;
use dc::PointL;
use raster::parallelogram::nt_gdi_plg_blt;

fn target() -> DrawingFixture {
    let fixture = DrawingFixture::new();
    dc::get_dc(fixture.handle).unwrap().stretch_mode = raster::stretch::COLORONCOLOR;
    fixture
}

fn copy(
    destination: u64,
    source: u64,
    points: [[i32; 2]; 3],
    input: [i32; 4],
    mask: u64,
    mask_origin: [i32; 2],
) -> u64 {
    let points = points.map(|point| PointL::new(point[0], point[1]));
    let pointer = points.as_ptr() as u64;
    mm::user_copy::allow(pointer, core::mem::size_of_val(&points), false);
    call(
        nt_gdi_plg_blt,
        &[
            destination,
            pointer,
            source,
            input[0] as u64,
            input[1] as u64,
            input[2] as u64,
            input[3] as u64,
            mask,
            mask_origin[0] as u64,
            mask_origin[1] as u64,
            0,
        ],
    )
}

#[test]
fn parallelogram_scales_axis_aligned_copies_and_discards_source_alpha() {
    let mut source = DrawingFixture::new();
    let destination = target();
    source.surface.set_pixel(0, 0, 0x80ff0000);
    source.surface.set_pixel(1, 0, 0x8000ff00);
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[1, 2], [5, 2], [1, 4]],
            [0, 0, 2, 1],
            0,
            [0, 0]
        ),
        1
    );
    for y in 2..4 {
        for x in 1..5 {
            assert_eq!(
                destination.pixel(x, y),
                if x < 3 { 0xff0000 } else { 0xff00 }
            );
        }
    }
    assert_eq!(destination.pixel(0, 2), 0x123456);
    assert_eq!(destination.pixel(5, 2), 0x123456);
}

#[test]
fn parallelogram_shear_covers_the_polygon_and_preserves_dc_state() {
    let mut source = DrawingFixture::new();
    source.surface.clear(0xabcdef);
    let destination = target();
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[1, 1], [5, 1], [3, 3]],
            [0, 0, 4, 2],
            0,
            [0, 0]
        ),
        1
    );
    for y in 0..5 {
        for x in 0..8 {
            let covered = y == 1 && (1..5).contains(&x) || y == 2 && (2..6).contains(&x);
            assert_eq!(
                destination.pixel(x, y),
                if covered { 0xabcdef } else { 0x123456 },
                "({x}, {y})"
            );
        }
    }
    let dc = dc::get_dc(destination.handle).unwrap();
    assert_eq!(dc.world_transform.m11, 1.0);
    assert_eq!(dc.world_transform.m12, 0.0);
    assert_eq!(dc.world_transform.m21, 0.0);
    assert_eq!(dc.world_transform.m22, 1.0);
    assert_eq!(dc.world_transform.dx, 0.0);
    assert_eq!(dc.world_transform.dy, 0.0);
    assert_eq!((dc.viewport_org.x, dc.viewport_org.y), (0, 0));
    assert_eq!(dc.stretch_mode, raster::stretch::COLORONCOLOR);
}

#[test]
fn parallelogram_rotates_a_rectangle_through_ninety_degrees() {
    let mut source = DrawingFixture::new();
    source.surface.set_pixel(0, 0, 0xff0000);
    source.surface.set_pixel(1, 0, 0xff00);
    source.surface.set_pixel(0, 1, 0xff);
    source.surface.set_pixel(1, 1, 0xffffff);
    let destination = target();
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[4, 1], [4, 3], [2, 1]],
            [0, 0, 2, 2],
            0,
            [0, 0]
        ),
        1
    );
    assert_eq!(destination.pixel(3, 1), 0xff0000);
    assert_eq!(destination.pixel(3, 2), 0xff00);
    assert_eq!(destination.pixel(2, 1), 0xff);
    assert_eq!(destination.pixel(2, 2), 0xffffff);
    assert_eq!(destination.pixel(4, 1), 0x123456);
}

#[test]
fn parallelogram_reflects_negative_source_and_destination_directions() {
    let mut source = DrawingFixture::new();
    for (x, color) in [0xff0000, 0xff00, 0xff].into_iter().enumerate() {
        source.surface.set_pixel(x as i32, 0, color);
    }
    let destination = target();
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[3, 0], [0, 0], [3, 1]],
            [0, 0, 3, 1],
            0,
            [0, 0]
        ),
        1
    );
    assert_eq!(destination.pixel(0, 0), 0xff);
    assert_eq!(destination.pixel(2, 0), 0xff0000);
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[0, 1], [3, 1], [0, 2]],
            [3, 1, -3, -1],
            0,
            [0, 0]
        ),
        1
    );
    assert_eq!(destination.pixel(0, 1), 0xff);
    assert_eq!(destination.pixel(2, 1), 0xff0000);
}

#[test]
fn parallelogram_applies_both_mappings_and_destination_clip_only() {
    let mut source = DrawingFixture::new();
    source.surface.set_pixel(1, 0, 0xff0000);
    source.surface.set_pixel(2, 0, 0xff00);
    let destination = target();
    {
        let mut dc = dc::get_dc(source.handle).unwrap();
        dc.viewport_org = PointL::new(1, 0);
        dc.application_clip = Some(Region::default());
    }
    dc::get_dc(destination.handle).unwrap().viewport_org = PointL::new(2, 3);
    call(
        clipping::nt_gdi_intersect_clip_rect,
        &[destination.handle, 1, 0, 3, 2],
    );
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[0, 0], [4, 0], [0, 2]],
            [0, 0, 2, 1],
            0,
            [0, 0]
        ),
        1
    );
    assert_eq!(destination.pixel(2, 3), 0x123456);
    assert_eq!(destination.pixel(3, 3), 0xff0000);
    assert_eq!(destination.pixel(4, 4), 0xff00);
    assert_eq!(destination.pixel(5, 4), 0x123456);
    let origin = dc::get_dc(destination.handle).unwrap().viewport_org;
    assert_eq!((origin.x, origin.y), (2, 3));
}

#[test]
fn parallelogram_captures_source_before_overlapping_writes() {
    let mut fixture = target();
    for (x, color) in [0xff0000, 0xff00, 0xff].into_iter().enumerate() {
        fixture.surface.set_pixel(x as i32, 0, color);
    }
    assert_eq!(
        copy(
            fixture.handle,
            fixture.handle,
            [[1, 0], [7, 0], [1, 1]],
            [0, 0, 3, 1],
            0,
            [0, 0]
        ),
        1
    );
    for (x, color) in [0xff0000, 0xff0000, 0xff00, 0xff00, 0xff, 0xff]
        .into_iter()
        .enumerate()
    {
        assert_eq!(fixture.pixel(x as i32 + 1, 0), color);
    }
}

#[test]
fn parallelogram_repeats_a_small_monochrome_mask() {
    let mut source = DrawingFixture::new();
    source.surface.clear(0xabcdef);
    let destination = target();
    let bits = [0x80u8, 0, 0x40, 0];
    mm::user_copy::allow(bits.as_ptr() as u64, bits.len(), false);
    let mask = call(
        bitmap::storage::nt_gdi_create_bitmap,
        &[2, 2, 1, 1, bits.as_ptr() as u64],
    );
    assert_ne!(mask, 0);
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[0, 0], [6, 0], [0, 4]],
            [0, 0, 6, 4],
            mask,
            [1, 0]
        ),
        1
    );
    for y in 0..4 {
        for x in 0..6 {
            assert_eq!(
                destination.pixel(x, y),
                if (x + y) % 2 == 1 { 0xabcdef } else { 0x123456 }
            );
        }
    }
    assert!(objects::gdi_delete_object(mask));
}

#[test]
fn parallelogram_rejects_bad_points_masks_and_handles_without_drawing() {
    let source = DrawingFixture::new();
    let destination = target();
    assert_eq!(
        call(
            nt_gdi_plg_blt,
            &[destination.handle, 0, source.handle, 0, 0, 2, 2]
        ),
        0
    );
    let color = objects::gdi_create_bitmap(2, 2, 32);
    let stale = objects::gdi_create_bitmap(2, 2, 1);
    assert!(objects::gdi_delete_object(stale));
    for mask in [color, stale, u64::MAX] {
        assert_eq!(
            copy(
                destination.handle,
                source.handle,
                [[0, 0], [2, 0], [0, 2]],
                [0, 0, 2, 2],
                mask,
                [0, 0]
            ),
            0
        );
    }
    for (target, input) in [
        ([[0, 0], [1, 1], [2, 2]], [0, 0, 2, 2]),
        ([[0, 0], [2, 0], [0, 2]], [i32::MAX, 0, 2, 2]),
        ([[0, 0], [2, 0], [0, 2]], [0, 0, 0, 2]),
        ([[0, 0], [2, 0], [0, 2]], [0, 0, i32::MIN, 2]),
        ([[0, 0], [i32::MAX, 0], [i32::MAX, 2]], [0, 0, 2, 2]),
    ] {
        assert_eq!(
            copy(destination.handle, source.handle, target, input, 0, [0, 0]),
            0
        );
    }
    assert_eq!(
        copy(
            destination.handle,
            u64::MAX,
            [[0, 0], [2, 0], [0, 2]],
            [0, 0, 2, 2],
            0,
            [0, 0]
        ),
        0
    );
    assert_eq!(
        copy(
            u64::MAX,
            source.handle,
            [[0, 0], [2, 0], [0, 2]],
            [0, 0, 2, 2],
            0,
            [0, 0]
        ),
        0
    );
    assert_eq!(destination.pixel(0, 0), 0x123456);
    assert!(objects::gdi_delete_object(color));
}

#[test]
fn parallelogram_rejects_source_shear_and_unimplemented_filters() {
    let source = DrawingFixture::new();
    let destination = target();
    dc::get_dc(source.handle).unwrap().world_transform.m12 = 1.0;
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[0, 0], [2, 0], [0, 2]],
            [0, 0, 2, 2],
            0,
            [0, 0]
        ),
        0
    );
    dc::get_dc(source.handle).unwrap().world_transform.m12 = 0.0;
    dc::get_dc(destination.handle).unwrap().stretch_mode = 4;
    assert_eq!(
        copy(
            destination.handle,
            source.handle,
            [[0, 0], [2, 0], [0, 2]],
            [0, 0, 2, 2],
            0,
            [0, 0]
        ),
        0
    );
    assert_eq!(destination.pixel(0, 0), 0x123456);
}

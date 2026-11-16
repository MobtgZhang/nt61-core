use super::*;
use raster::alpha::nt_gdi_alpha_blend;

fn blend(destination: u64, source: u64, target: [i32; 4], input: [i32; 4], function: u64) -> u64 {
    call(
        nt_gdi_alpha_blend,
        &[
            destination,
            target[0] as u64,
            target[1] as u64,
            target[2] as u64,
            target[3] as u64,
            source,
            input[0] as u64,
            input[1] as u64,
            input[2] as u64,
            input[3] as u64,
            function,
            0,
        ],
    )
}

#[test]
fn constant_alpha_blends_rgb_and_ignores_the_source_alpha_byte() {
    let mut source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    assert!(source.surface.set_pixel(0, 0, 0x0064_c832));
    for (x, alpha, expected) in [(0, 0, 0x123456), (1, 255, 0x64c832), (2, 128, 0x3b7e44)] {
        assert_eq!(
            blend(
                destination.handle,
                source.handle,
                [x, 0, 1, 1],
                [0, 0, 1, 1],
                alpha << 16
            ),
            1
        );
        assert_eq!(destination.pixel(x, 0), expected);
    }
}

#[test]
fn per_pixel_alpha_uses_premultiplied_channels_and_combines_constant_alpha() {
    let mut source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    for (x, pixel) in [0, 0xff64_c832, 0x8032_6419].into_iter().enumerate() {
        assert!(source.surface.set_pixel(x as i32, 0, pixel));
    }
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [0, 0, 3, 1],
            [0, 0, 3, 1],
            0x01ff_0000
        ),
        1
    );
    assert_eq!(destination.pixel(0, 0), 0x123456);
    assert_eq!(destination.pixel(1, 0), 0x64c832);
    assert_eq!(destination.pixel(2, 0), 0x3b7e44);
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [3, 0, 1, 1],
            [2, 0, 1, 1],
            0x0180_0000
        ),
        1
    );
    assert_eq!(destination.pixel(3, 0), 0x26594d);
}

#[test]
fn argb_destination_receives_source_over_alpha() {
    let mut source = DrawingFixture::new();
    let mut destination = DrawingFixture::new();
    destination.surface.format = surface::PIXEL_FORMAT_32BPP_ARGB;
    assert!(destination.surface.set_pixel(0, 0, 0x4012_3456));
    assert!(source.surface.set_pixel(0, 0, 0x8032_6419));
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [0, 0, 1, 1],
            [0, 0, 1, 1],
            0x01ff_0000
        ),
        1
    );
    assert_eq!(destination.pixel(0, 0), 0xa03b_7e44);
}

#[test]
fn alpha_scaling_uses_both_mappings_and_destination_clip_without_scan_reduction() {
    let mut source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    assert!(source.surface.set_pixel(1, 0, 0xff0000));
    assert!(source.surface.set_pixel(2, 0, 0x00ff00));
    {
        let mut dc = dc::get_dc(source.handle).unwrap();
        dc.viewport_org = dc::PointL::new(1, 0);
        dc.application_clip = Some(Region::default());
    }
    {
        let mut dc = dc::get_dc(destination.handle).unwrap();
        dc.viewport_org = dc::PointL::new(2, 3);
        dc.stretch_mode = raster::stretch::BLACKONWHITE;
    }
    assert_ne!(
        call(
            clipping::nt_gdi_intersect_clip_rect,
            &[destination.handle, 1, 0, 4, 2]
        ),
        0
    );
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [0, 0, 4, 2],
            [0, 0, 2, 1],
            0x00ff_0000
        ),
        1
    );
    assert_eq!(destination.pixel(2, 3), 0x123456);
    assert_eq!(destination.pixel(3, 3), 0xff0000);
    assert_eq!(destination.pixel(4, 4), 0x00ff00);
    assert_eq!(destination.pixel(5, 4), 0x00ff00);
    dc::get_dc(destination.handle).unwrap().application_clip = None;
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [0, 3, 1, 1],
            [0, 0, 2, 1],
            0x00ff_0000
        ),
        1
    );
    assert_eq!(destination.pixel(2, 6), 0xff0000);
}

#[test]
fn alpha_rejects_invalid_functions_and_geometry_before_changing_pixels() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    for function in [0x00ff_0001, 0x00ff_0100, 0x02ff_0000, 0x80ff_0000] {
        assert_eq!(
            blend(
                destination.handle,
                source.handle,
                [0, 0, 2, 2],
                [0, 0, 2, 2],
                function
            ),
            0
        );
    }
    for (target, input) in [
        ([0, 0, -1, 1], [0, 0, 1, 1]),
        ([0, 0, 1, 1], [0, 0, 1, -1]),
        ([i32::MAX, 0, 2, 2], [0, 0, 1, 1]),
        ([0, 0, 1, 1], [-1, 0, 1, 1]),
        ([0, 0, 1, 1], [15, 0, 2, 1]),
        ([0, 0, 1, 1], [0, 0, i32::MAX, 1]),
    ] {
        assert_eq!(
            blend(
                destination.handle,
                source.handle,
                target,
                input,
                0x00ff_0000
            ),
            0
        );
    }
    for (target, input) in [(0, source.handle), (destination.handle, 0)] {
        assert_eq!(
            blend(target, input, [0, 0, 1, 1], [0, 0, 1, 1], 0x00ff_0000),
            0
        );
    }
    assert_eq!(destination.pixel(0, 0), 0x123456);
}

#[test]
fn alpha_rejects_overlap_but_allows_disjoint_regions_of_one_surface() {
    let mut fixture = DrawingFixture::new();
    assert!(fixture.surface.set_pixel(0, 0, 0x876543));
    assert_eq!(
        blend(
            fixture.handle,
            fixture.handle,
            [1, 0, 2, 1],
            [0, 0, 2, 1],
            0x00ff_0000
        ),
        0
    );
    assert_eq!(fixture.pixel(1, 0), 0x123456);
    assert_eq!(
        blend(
            fixture.handle,
            fixture.handle,
            [2, 0, 2, 1],
            [0, 0, 2, 1],
            0x00ff_0000
        ),
        1
    );
    assert_eq!(fixture.pixel(2, 0), 0x876543);
    assert_eq!(fixture.pixel(3, 0), 0x123456);
}

#[test]
fn alpha_empty_copies_are_noops_and_dword_slots_ignore_high_bits() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [0, 0, 0, 2],
            [0, 0, 1, 1],
            0x00ff_0000
        ),
        1
    );
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [0, 0, 1, 1],
            [0, 0, 0, 1],
            0x00ff_0000
        ),
        1
    );
    assert_eq!(
        blend(
            destination.handle,
            source.handle,
            [0, 0, 1, 1],
            [0, 0, 1, 1],
            0xdead_beef_00ff_0000
        ),
        1
    );
    let result = call(
        nt_gdi_alpha_blend,
        &[
            destination.handle,
            0,
            0,
            1,
            1,
            source.handle,
            0,
            0,
            1,
            1,
            0x00ff_0000,
            1,
        ],
    );
    assert_eq!(result, 0);
}

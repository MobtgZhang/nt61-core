use super::*;
use raster::extended::*;

#[test]
fn transparent_copy_scales_skips_key_preserves_alpha_and_captures_overlap() {
    let source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    {
        let dc = dc::get_dc(source.handle).unwrap();
        assert!(raster::set_pixel(&dc, 0, 0, 0xff));
        assert!(raster::set_pixel(&dc, 1, 0, 0xff00));
        assert!(raster::set_pixel(&dc, 2, 0, 0xff0000));
    }
    assert_eq!(call(nt_gdi_transparent_blt,
        &[destination.handle, 0, 0, 6, 2, source.handle, 0, 0, 3, 1, 0xff00]), 1);
    for y in 0..2 {
        assert_eq!(destination.pixel(0, y), 0xff0000);
        assert_eq!(destination.pixel(1, y), 0xff0000);
        assert_eq!(destination.pixel(2, y), 0x123456);
        assert_eq!(destination.pixel(3, y), 0x123456);
        assert_eq!(destination.pixel(4, y), 0xff);
        assert_eq!(destination.pixel(5, y), 0xff);
    }
    assert_eq!(call(nt_gdi_transparent_blt,
        &[source.handle, 1, 0, 3, 1, source.handle, 0, 0, 3, 1, 0xffffff]), 1);
    assert_eq!(source.pixel(1, 0), 0xff0000);
    assert_eq!(source.pixel(2, 0), 0xff00);
    assert_eq!(source.pixel(3, 0), 0xff);
    assert_eq!(call(nt_gdi_transparent_blt,
        &[source.handle, 0, 0, (-1i64) as u64, 1, source.handle, 0, 0, 1, 1, 0]), 0);
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vertex { x: i32, y: i32, red: u16, green: u16, blue: u16, alpha: u16 }

fn gradient(fixture: &DrawingFixture, vertices: &[Vertex], mesh: &[u32], mode: u64) -> u64 {
    mm::user_copy::allow(vertices.as_ptr() as u64, core::mem::size_of_val(vertices), false);
    mm::user_copy::allow(mesh.as_ptr() as u64, core::mem::size_of_val(mesh), false);
    call(nt_gdi_gradient_fill, &[fixture.handle, vertices.as_ptr() as u64, vertices.len() as u64,
        mesh.as_ptr() as u64, (mesh.len() / if mode == 2 { 3 } else { 2 }) as u64, mode])
}

#[test]
fn horizontal_vertical_and_triangle_gradients_interpolate_16bit_channels() {
    let fixture = DrawingFixture::new();
    let vertices = [Vertex::default(), Vertex { x: 4, y: 4, red: 0xff00, ..Vertex::default() }];
    assert_eq!(gradient(&fixture, &vertices, &[0, 1], 0), 1);
    for y in 0..4 {
        for (x, red) in [0, 63, 127, 191].into_iter().enumerate() {
            assert_eq!(fixture.pixel(x as i32, y), red << 16);
        }
    }
    assert_eq!(gradient(&fixture, &vertices, &[0, 1], 1), 1);
    assert_eq!(fixture.pixel(0, 3), 191 << 16);
    let vertices = [
        Vertex { x: 0, y: 0, red: 0xff00, ..Vertex::default() },
        Vertex { x: 8, y: 0, green: 0xff00, ..Vertex::default() },
        Vertex { x: 0, y: 8, blue: 0xff00, ..Vertex::default() },
    ];
    assert_eq!(gradient(&fixture, &vertices, &[0, 1, 2], 2), 1);
    assert_eq!(fixture.pixel(2, 2), 0x7f3f3f);
    assert_eq!(fixture.pixel(7, 7), 0x123456);
    assert_eq!(gradient(&fixture, &vertices, &[2, 1, 0], 2), 1);
    assert_eq!(fixture.pixel(2, 2), 0x7f3f3f);
}

#[test]
fn gradients_validate_all_indices_before_drawing_and_obey_clip() {
    let fixture = DrawingFixture::new();
    let vertices = [Vertex::default(), Vertex { x: 8, y: 8, blue: 0xff00, ..Vertex::default() }];
    assert_eq!(gradient(&fixture, &vertices, &[0, 1, 0, 2], 0), 0);
    assert_eq!(fixture.pixel(0, 0), 0x123456);
    call(clipping::nt_gdi_intersect_clip_rect, &[fixture.handle, 2, 2, 4, 4]);
    assert_eq!(gradient(&fixture, &vertices, &[0, 1], 0), 1);
    assert_eq!(fixture.pixel(1, 2), 0x123456);
    assert_eq!(fixture.pixel(2, 2), 63);
    assert_eq!(fixture.pixel(4, 2), 0x123456);
    assert_eq!(call(nt_gdi_gradient_fill, &[fixture.handle, 1, 2, 1, 1, 0]), 0);
}

#[test]
fn flood_fill_respects_connected_components_boundary_mode_and_holes() {
    let fixture = DrawingFixture::new();
    let brush = objects::gdi_create_solid_brush(0xff);
    call(selection::nt_gdi_select_brush, &[fixture.handle, brush]);
    {
        let dc = dc::get_dc(fixture.handle).unwrap();
        for y in 0..16 { assert!(raster::set_pixel(&dc, 7, y, 0)); }
    }
    assert_eq!(call(nt_gdi_ext_flood_fill, &[fixture.handle, 2, 2, 0x563412, 1]), 1);
    assert_eq!(fixture.pixel(6, 2), 0xff0000);
    assert_eq!(fixture.pixel(7, 2), 0);
    assert_eq!(fixture.pixel(8, 2), 0x123456);
    assert_eq!(call(nt_gdi_ext_flood_fill, &[fixture.handle, 8, 2, 0, 0]), 1);
    assert_eq!(fixture.pixel(15, 2), 0xff0000);
    assert_eq!(call(nt_gdi_ext_flood_fill, &[fixture.handle, 7, 2, 0, 0]), 0);
    call(selection::nt_gdi_select_brush, &[fixture.handle, objects::gdi_get_stock_object(0)]);
    assert!(objects::gdi_delete_object(brush));
}

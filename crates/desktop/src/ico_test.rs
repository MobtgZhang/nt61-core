#[path = "bin/explorer/png.rs"]
mod png;

#[path = "bin/explorer/ico.rs"]
mod ico;

#[test]
fn decodes_win7_login_wallpaper() {
    let image = png::decode_png(include_bytes!(
        "../assets/windows7/wallpapers/login-1024.png"
    ))
    .expect("login wallpaper");
    assert_eq!((image.width, image.height), (1024, 768));
    assert!(image.pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[0] > 32));
}

#[test]
fn desktop_icons_use_clean_48_pixel_frames() {
    for bytes in [
        include_bytes!("../assets/windows7/icons/imageres/ICON109_1.ico") as &[u8],
        include_bytes!("../assets/windows7/icons/imageres/ICON1013_1.ico"),
        include_bytes!("../assets/windows7/icons/imageres/ICON55_1.ico"),
        include_bytes!("../assets/windows7/icons/shell32/ICON22_1.ico"),
    ] {
        let image = ico::decode_ico_preferred(bytes, 48).expect("desktop icon frame");
        assert_eq!((image.width, image.height), (48, 48));
    }
}

#[test]
fn start_orb_solid_disc_fits_the_taskbar_in_every_visual_state() {
    use crate::shell::layout::{ShellLayout, TASKBAR_HEIGHT};
    for (width, height) in [(1024, 768), (1366, 768), (1920, 1080), (640, 480)] {
        let layout = ShellLayout::new(width, height);
        let b = layout.start_orb();
        for bytes in [
            include_bytes!("../assets/windows7/start_button_normal.png") as &[u8],
            include_bytes!("../assets/windows7/start_button_hover.png"),
            include_bytes!("../assets/windows7/start_button_pressed.png"),
        ] {
            let image = png::decode_png(bytes).expect("start orb");
            let mut top = i32::MAX;
            let mut bottom = i32::MIN;
            for y in 0..b.height {
                for x in 0..b.width {
                    let i = ((y * image.height / b.height * image.width + x * image.width / b.width) * 4) as usize;
                    if image.pixels[i + 3] > 128 {
                        top = top.min(b.y + y as i32);
                        bottom = bottom.max(b.y + y as i32);
                    }
                }
            }
            assert!(bottom - top < TASKBAR_HEIGHT as i32, "orb disc is taller than the taskbar");
            assert!(top >= layout.taskbar_top(), "solid disc protrudes above the taskbar");
            assert!(bottom < height as i32, "solid disc is clipped at screen bottom");
        }
    }
}

#[test]
fn resize_cursors_decode_with_the_hotspot_of_the_selected_frame() {
    for (bytes, hotspot) in [
        (include_bytes!("../assets/windows7/cursors/aero_ew.cur") as &[u8], (11, 4)),
        (include_bytes!("../assets/windows7/cursors/aero_ns.cur"), (4, 11)),
        (include_bytes!("../assets/windows7/cursors/aero_nwse.cur"), (8, 8)),
        (include_bytes!("../assets/windows7/cursors/aero_nesw.cur"), (8, 8)),
    ] {
        let (image, actual) = ico::decode_cur_preferred(bytes, 32).expect("resize cursor");
        assert_eq!((image.width, image.height), (32, 32));
        assert_eq!(actual, hotspot);
        assert!(image.pixels.chunks_exact(4).any(|p| p[3] > 200));
        assert!(image.pixels.chunks_exact(4).any(|p| p[3] == 0));
    }
}

#[test]
fn decodes_win7_cursor() {
    let bytes = include_bytes!("../assets/windows7/cursors/aero_arrow_l.cur");
    match ico::decode_ico_preferred(bytes, 32) {
        Ok(image) => assert!(image.width > 0 && image.height > 0),
        Err(error) => panic!("cursor decode failed: {error:?}"),
    }
}

#[test]
fn decodes_win7_cursor_transparency() {
    let image = ico::decode_ico(include_bytes!(
        "../assets/windows7/cursors/aero_arrow_l.cur"
    ))
    .expect("cursor");
    let alpha = |x: u32, y: u32| image.pixels[((y * image.width + x) * 4 + 3) as usize];
    let mut max_alpha = 0u8;
    for pixel in image.pixels.as_chunks::<4>().0 {
        max_alpha = max_alpha.max(pixel[3]);
    }
    assert_eq!(alpha(0, 47), 0);
    assert_eq!(alpha(47, 47), 0);
    assert!(max_alpha > 200);
    assert_eq!(alpha(47, 47), 0);
}

#[test]
fn decodes_every_win7_cursor_entry() {
    let bytes = include_bytes!("../assets/windows7/cursors/aero_arrow_l.cur");
    for (index, entry) in ico::directory_entries(bytes).unwrap().iter().enumerate() {
        let result: Result<(), &'static str> =
            if entry.payload.get(0..4) == Some(&[0x89, 0x50, 0x4E, 0x47]) {
                png::decode_png(&entry.payload)
                    .map(|_| ())
                    .map_err(|_| "png")
            } else {
                ico::decode_bmp(&entry.payload)
                    .map(|_| ())
                    .map_err(|_| "bmp")
            };
        assert!(
            result.is_ok(),
            "entry {index} {}x{} bits={} size={} failed: {:?}",
            entry.width,
            entry.height,
            entry.bit_count,
            entry.payload.len(),
            result.err(),
        );
    }
}

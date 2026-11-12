#[path = "bin/explorer/png.rs"]
mod png;

#[path = "bin/explorer/ico.rs"]
mod ico;

#[test]
fn decodes_win7_flag_ico() {
    let bytes = include_bytes!("../assets/windows7/icons/shell32/ICON34_1.ico");
    match ico::decode_ico(bytes) {
        Ok(image) => assert!(image.width > 0 && image.height > 0),
        Err(error) => panic!("flag ICO decode failed: {error:?}"),
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

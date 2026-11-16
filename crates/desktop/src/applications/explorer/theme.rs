//! Original Windows 7 shell artwork. The extractor records provenance in sources.json.
//! Assets are decoded at build time, so painting never opens a DLL or allocates.
use crate::graphics::{Rect, Surface};

macro_rules! asset {
    ($name:literal) => {
        include_bytes!(concat!("resources/shell/", $name, ".rgba")).as_slice()
    };
}

pub fn navigation(
    frame: &mut Surface,
    at: Rect,
    forward: bool,
    enabled: bool,
    hot: bool,
    pressed: bool,
) {
    let pixels = match (forward, enabled, hot, pressed) {
        (false, false, _, _) => asset!("back-disabled"),
        (true, false, _, _) => asset!("forward-disabled"),
        (false, true, _, true) => asset!("back-pressed"),
        (true, true, _, true) => asset!("forward-pressed"),
        (false, true, true, false) => asset!("back-hot"),
        (true, true, true, false) => asset!("forward-hot"),
        (false, true, false, false) => asset!("back-normal"),
        (true, true, false, false) => asset!("forward-normal"),
    };
    draw(frame, pixels, (27, 27), at);
}

pub fn icon(frame: &mut Surface, name: &str, at: Rect) {
    let pixels = match name {
        "Favorites" => asset!("favorites-16"),
        "Desktop" => asset!("desktop-16"),
        "Downloads" => asset!("downloads-16"),
        "Recent Places" => asset!("recent-16"),
        "Libraries" => asset!("libraries-16"),
        "Documents" => asset!("documents-16"),
        "Music" => asset!("music-16"),
        "Pictures" => asset!("pictures-16"),
        "Videos" => asset!("videos-16"),
        "Computer" => asset!("computer-16"),
        "Local Disk (C:)" => asset!("drive-16"),
        "Network" => asset!("network-16"),
        "Shield" => asset!("shield-16"),
        "Help" => asset!("help-16"),
        "Search" => asset!("search"),
        "Refresh" => asset!("refresh"),
        "Document" => asset!("document-16"),
        "Printer" => asset!("printer-16"),
        _ => asset!("folder-16"),
    };
    draw(frame, pixels, (16, 16), at);
}

pub fn printer(frame: &mut Surface, at: Rect) {
    draw(frame, asset!("printer-32"), (32, 32), at);
}

pub fn large_icon(frame: &mut Surface, name: &str, at: Rect) {
    let pixels = match name {
        "Computer" => asset!("computer-48"),
        "Local Disk (C:)" => asset!("drive-48"),
        _ => asset!("folder-48"),
    };
    draw(frame, pixels, (48, 48), at);
}

pub fn brand(frame: &mut Surface, at: Rect) {
    draw(frame, asset!("windows-brand"), (275, 174), at);
}

fn draw(frame: &mut Surface, pixels: &[u8], size: (u32, u32), at: Rect) {
    if at.width == 0 || at.height == 0 {
        return;
    }
    let Some(clip) = Rect::new(0, 0, frame.width(), frame.height()).intersection(&at) else {
        return;
    };
    for y in clip.y..clip.y + clip.height as i32 {
        let sy = (y - at.y) as u32 * size.1 / at.height;
        for x in clip.x..clip.x + clip.width as i32 {
            let sx = (x - at.x) as u32 * size.0 / at.width;
            let offset = ((sy * size.0 + sx) * 4) as usize;
            let p = &pixels[offset..offset + 4];
            let color = u32::from_le_bytes([p[2], p[1], p[0], p[3]]);
            frame.blend_pixel(x as u32, y as u32, color);
        }
    }
}

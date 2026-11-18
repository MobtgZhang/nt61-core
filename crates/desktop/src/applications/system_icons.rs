//! Source-drawn small system glyphs. No Windows binary or artwork is embedded.
use crate::graphics::{Rect, Surface};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Photo,
    Computer,
    Device,
    Disk,
    Display,
    Network,
    Sound,
    Input,
    Mouse,
    Processor,
    Optical,
    Usb,
    Warning,
    Folder,
    Zoom,
    Fit,
    Previous,
    Next,
    Slideshow,
    RotateLeft,
    RotateRight,
    Delete,
    Scan,
    Help,
    Back,
    Forward,
}
pub fn draw(s: &mut Surface, b: Rect, glyph: Glyph) {
    if matches!(glyph, Glyph::Photo | Glyph::Device) {
        super::icons::draw(
            s,
            if glyph == Glyph::Photo {
                super::AppId::PhotoViewer
            } else {
                super::AppId::DeviceManager
            },
            b,
        );
        return;
    }
    let mut tile = Surface::new(32, 32);
    let blue = 0xff1276c5;
    let dark = 0xff455b65;
    match glyph {
        Glyph::Photo | Glyph::Device => unreachable!(),
        Glyph::Computer | Glyph::Display => {
            tile.fill_rect(3, 4, 26, 20, dark);
            tile.fill_rect(4, 5, 24, 18, 0xffeaf7ff);
            tile.fill_rect(6, 7, 20, 14, 0xff419eda);
            tile.fill_rect(14, 24, 4, 3, dark);
            tile.fill_rect(9, 28, 15, 2, dark);
        }
        Glyph::Slideshow => {
            tile.fill_rect(4, 7, 24, 19, 0xff476881);
            tile.fill_rect(5, 6, 23, 19, 0xfff0f8ff);
            tile.fill_rect(7, 8, 19, 14, 0xff2f93be);
            tile.fill_rect(8, 9, 17, 12, 0xffb3ddf6);
            for y in 0..9 {
                tile.fill_rect(8, 21 - y, 5 + y as u32, 1, 0xff558549);
            }
            for y in 0..6 {
                tile.fill_rect(19 - y, 21 - y, 6 + y as u32, 1, 0xff8ba16b);
            }
            tile.blend_rounded_rect(Rect::new(20, 10, 4, 4), 2, 0xffffefa4);
            tile.fill_rect(6, 25, 21, 1, 0xfff7ffff);
        }
        Glyph::Scan => {
            tile.fill_rect(3, 5, 13, 24, 0xff647b86);
            tile.fill_rect(4, 6, 11, 21, 0xffd7e0e3);
            for y in [9, 13, 17] {
                tile.fill_rect(6, y, 7, 2, dark);
            }
            tile.fill_rect(18, 15, 12, 11, 0xff5284a0);
            tile.fill_rect(20, 17, 8, 7, 0xffabd4ea);
            if glyph == Glyph::Scan {
                draw(&mut tile, Rect::new(15, 1, 15, 15), Glyph::Zoom);
            }
        }
        Glyph::Usb => {
            tile.fill_rect(14, 6, 3, 21, 0xff545960);
            tile.fill_rect(6, 13, 3, 7, 0xff545960);
            tile.fill_rect(8, 19, 8, 3, 0xff545960);
            tile.fill_rect(22, 10, 3, 7, 0xff545960);
            tile.fill_rect(15, 15, 9, 3, 0xff545960);
            for n in 0..5 {
                tile.fill_rect(11 + n, 7 - n, (9 - n * 2) as u32, 1, 0xff545960);
            }
            tile.blend_rounded_rect(Rect::new(4, 9, 7, 7), 3, 0xff71767c);
            tile.fill_rect(20, 5, 7, 7, 0xff71767c);
            tile.blend_rounded_rect(Rect::new(11, 24, 9, 7), 4, 0xff71767c);
            tile.fill_rect(15, 10, 1, 13, 0xffe5e9ed);
        }
        Glyph::Disk => {
            tile.fill_rect(2, 14, 28, 13, 0xff7d8f94);
            tile.fill_rect(3, 13, 26, 10, 0xffdce7e8);
            tile.fill_rect(4, 24, 24, 2, dark);
            tile.fill_rect(25, 24, 2, 2, 0xff36bf2d);
        }
        Glyph::Network => {
            for (x, y) in [(1, 3), (17, 17)] {
                tile.fill_rect(x, y, 14, 11, dark);
                tile.fill_rect(x + 1, y + 1, 12, 8, 0xff64bced);
            }
            tile.fill_rect(7, 16, 2, 10, dark);
            tile.fill_rect(7, 25, 10, 2, dark);
        }
        Glyph::Sound => {
            tile.fill_rect(4, 12, 7, 10, dark);
            for x in 0..8 {
                tile.fill_rect(11 + x, 11 - x, 1, 12 + x as u32 * 2, 0xff869eac);
            }
            for x in [22, 27] {
                tile.fill_rect(x, 8, 2, 19, blue);
            }
        }
        Glyph::Input => {
            tile.fill_rect(2, 12, 28, 16, 0xff6b7e86);
            tile.fill_rect(3, 13, 26, 14, 0xffe8eff3);
            for y in [15, 19] {
                for x in (5..28).step_by(4) {
                    tile.fill_rect(x, y, 2, 2, dark);
                }
            }
            tile.fill_rect(8, 23, 16, 2, dark);
        }
        Glyph::Mouse => {
            tile.fill_rect(15, 1, 2, 6, 0xff737e83);
            tile.blend_rounded_rect(Rect::new(7, 6, 18, 24), 8, 0xff65757e);
            tile.blend_rounded_rect(Rect::new(8, 7, 16, 21), 7, 0xffe7ebed);
            tile.fill_rect(15, 8, 1, 9, 0xff7f8b91);
            tile.fill_rect(10, 17, 12, 1, 0xff7f8b91);
            tile.fill_rect(14, 9, 3, 6, 0xff80939f);
        }
        Glyph::Processor => {
            for n in (5..29).step_by(4) {
                tile.fill_rect(n, 1, 2, 30, 0xffb4c2c6);
                tile.fill_rect(1, n, 30, 2, 0xffb4c2c6);
            }
            tile.fill_rect(5, 5, 23, 23, 0xff4f6263);
            tile.fill_rect(7, 7, 19, 19, 0xff789c8b);
            tile.fill_rect(10, 10, 13, 13, 0xff465a5c);
            tile.fill_rect(12, 12, 9, 9, 0xffbac7c5);
        }
        Glyph::Optical => {
            tile.fill_rect(2, 17, 28, 12, 0xff8a989b);
            tile.fill_rect(3, 18, 26, 9, 0xffe6e9e8);
            tile.fill_rect(5, 24, 18, 1, 0xff586568);
            tile.fill_rect(26, 24, 2, 2, 0xff43ad34);
            tile.blend_rounded_rect(Rect::new(8, 2, 22, 22), 11, 0xffa0aebd);
            tile.blend_rounded_rect(Rect::new(9, 3, 20, 20), 10, 0xffe5ecf2);
            tile.blend_rounded_rect(Rect::new(16, 10, 6, 6), 3, 0xff7a8d99);
            tile.blend_rounded_rect(Rect::new(18, 12, 2, 2), 1, 0xffe9edf0);
        }
        Glyph::Warning => {
            for y in 0..25 {
                tile.fill_rect(16 - y / 2, 3 + y, 1 + y as u32, 1, 0xffffd440);
            }
            tile.fill_rect(15, 11, 2, 9, dark);
            tile.fill_rect(15, 23, 2, 2, dark);
        }
        Glyph::Folder => {
            tile.fill_rect(3, 7, 12, 5, 0xffd4a43d);
            tile.fill_rect(2, 11, 28, 17, 0xffffdd75);
        }
        Glyph::Zoom => {
            tile.blend_rounded_rect(Rect::new(3, 3, 19, 19), 9, blue);
            tile.blend_rounded_rect(Rect::new(6, 6, 13, 13), 6, 0xffdcf7ff);
            for n in 0..11 {
                tile.fill_rect(19 + n, 19 + n, 3, 3, blue);
            }
            tile.fill_rect(8, 11, 9, 2, blue);
            tile.fill_rect(11, 8, 2, 9, blue);
        }
        Glyph::Previous | Glyph::Next => {
            let right = glyph == Glyph::Next;
            let x = if right { 9 } else { 14 };
            triangle(&mut tile, x - 1, 7, right, 0xfff0ffff);
            triangle(&mut tile, x, 8, right, 0xff1471bb);
            tile.fill_rect(if right { 24 } else { 5 }, 7, 4, 18, 0xffe7ffff);
            tile.fill_rect(if right { 25 } else { 6 }, 8, 2, 16, 0xff1663a5);
        }
        Glyph::Back | Glyph::Forward => {
            let right = glyph == Glyph::Forward;
            for n in 0..13 {
                let width = if n < 7 { n + 1 } else { 13 - n };
                tile.fill_rect(
                    if right { 18 } else { 14 - width },
                    9 + n,
                    width as u32,
                    1,
                    0xff57849e,
                );
            }
            tile.fill_rect(if right { 5 } else { 14 }, 13, 13, 5, 0xff799db2);
            tile.fill_rect(if right { 5 } else { 14 }, 13, 12, 1, 0xffcfebf8);
        }
        Glyph::Delete => {
            for n in 0..18 {
                tile.fill_rect(7 + n, 7 + n, 2, 2, 0xffd85542);
                tile.fill_rect(24 - n, 7 + n, 2, 2, 0xffd85542);
            }
        }
        Glyph::Fit => {
            tile.fill_rect(10, 10, 12, 12, 0xff145f9d);
            tile.fill_rect(12, 12, 8, 8, 0xfff0fbff);
            // Four diagonal arrows point outwards from the picture frame.
            for (cx, cy, sx, sy) in [
                (4, 4, 1, 1),
                (27, 4, -1, 1),
                (4, 27, 1, -1),
                (27, 27, -1, -1),
            ] {
                for n in 0..7 {
                    tile.fill_rect(cx + n * sx, cy, 1, 1, 0xfff0ffff);
                    tile.fill_rect(cx, cy + n * sy, 1, 1, 0xfff0ffff);
                }
                for n in 0..6 {
                    tile.fill_rect(cx + n * sx, cy + n * sy, 2, 2, blue);
                    tile.fill_rect(cx + n * sx, cy, 2, 2, blue);
                    tile.fill_rect(cx, cy + n * sy, 2, 2, blue);
                }
            }
        }
        Glyph::RotateLeft | Glyph::RotateRight => {
            let left = glyph == Glyph::RotateLeft;
            for y in 7..28 {
                for x in 5..28 {
                    let dx = x - 16;
                    let dy = y - 17;
                    let r = dx * dx + dy * dy;
                    if (35..=111).contains(&r) && !(y < 14 && if left { x < 14 } else { x > 18 }) {
                        tile.set_pixel(
                            x as u32,
                            y as u32,
                            if r > 90 || r < 49 {
                                0xff2365a9
                            } else {
                                0xff6bc7ff
                            },
                        );
                    }
                }
            }
            // Arrowhead joins the top of the loop and points horizontally.
            for y in 0..11 {
                let width = 6 - (y - 5i32).abs();
                let x = if left { 7 } else { 25 - width };
                tile.fill_rect(x, 3 + y, width as u32, 1, 0xffe3ffff);
                if width > 1 {
                    tile.fill_rect(
                        x + i32::from(left),
                        4 + y,
                        (width - 1) as u32,
                        1,
                        0xff2582cc,
                    );
                }
            }
        }
        Glyph::Help => {
            tile.blend_rounded_rect(Rect::new(2, 2, 28, 28), 14, blue);
            tile.fill_rect(11, 8, 10, 3, 0xffffffff);
            tile.fill_rect(18, 11, 3, 6, 0xffffffff);
            tile.fill_rect(14, 16, 6, 3, 0xffffffff);
            tile.fill_rect(14, 18, 3, 4, 0xffffffff);
            tile.fill_rect(14, 25, 3, 3, 0xffffffff);
        }
    }
    // Area sample in premultiplied color so small tree icons retain fine edges
    // without dark fringes around their transparent outline.
    for y in 0..b.height {
        for x in 0..b.width {
            let mut sums = [0u32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let p = tile
                        .pixel(
                            ((x * 4 + sx) * 32 / (b.width * 4)).min(31),
                            ((y * 4 + sy) * 32 / (b.height * 4)).min(31),
                        )
                        .unwrap_or(0);
                    let a = p >> 24;
                    sums[0] += a;
                    sums[1] += ((p >> 16) & 255) * a;
                    sums[2] += ((p >> 8) & 255) * a;
                    sums[3] += (p & 255) * a;
                }
            }
            if sums[0] > 0 {
                let p = (sums[0] / 16) << 24
                    | (sums[1] / sums[0]) << 16
                    | (sums[2] / sums[0]) << 8
                    | sums[3] / sums[0];
                s.blend_pixel((b.x + x as i32) as u32, (b.y + y as i32) as u32, p);
            }
        }
    }
}
pub fn draw_disabled(s: &mut Surface, b: Rect, glyph: Glyph) {
    let mut icon = Surface::new(b.width, b.height);
    draw(&mut icon, Rect::new(0, 0, b.width, b.height), glyph);
    for y in 0..b.height {
        for x in 0..b.width {
            let p = icon.pixel(x, y).unwrap_or(0);
            let gray = (((p >> 16) & 255) * 30 + ((p >> 8) & 255) * 59 + (p & 255) * 11) / 100;
            let gray = (gray + 180) / 2;
            s.blend_pixel(
                (b.x + x as i32) as u32,
                (b.y + y as i32) as u32,
                (p & 0xff000000) | gray << 16 | gray << 8 | gray,
            );
        }
    }
}
fn triangle(s: &mut Surface, x: i32, y: i32, right: bool, c: u32) {
    for n in 0..16 {
        let w = if n < 8 { n + 1 } else { 16 - n };
        s.fill_rect(if right { x } else { x + 8 - w }, y + n, w as u32, 1, c);
    }
}
// Caption artwork is rasterized at its target size from this source geometry.
// The Photo Viewer reference uses a white photograph with a blue glass rim;
// Device Manager uses the grey device cabinet with the upright status panel.
const fn shade(a: u32, b: u32, t: u32, span: u32) -> u32 {
    let mut p = 0xff000000;
    let mut shift = 0;
    while shift < 24 {
        p |= (((a >> shift & 255) * (span - t) + (b >> shift & 255) * t) / span) << shift;
        shift += 8;
    }
    p
}
const fn artwork(photo: bool, x: u32, y: u32) -> u32 {
    if photo {
        if x < 5 || x >= 59 || y < 8 || y >= 57 {
            return 0;
        }
        if x < 8 || x >= 56 || y < 11 || y >= 54 {
            return shade(0x3b8cb9, 0x155984, y - 8, 49);
        }
        if x < 11 || x >= 53 || y < 14 || y >= 51 {
            return shade(0xffffff, 0xc8d5dc, y - 11, 43);
        }
        let mut p = shade(0x80c3ee, 0xe4f2ef, y - 14, 37);
        if y > 30 && y < 38 && ((x > 18 && x < 31) || (x > 38 && x < 49)) {
            p = 0xffe9f5f4;
        }
        if y >= 34 + x / 7 && y < 44 {
            p = shade(0x3f784a, 0x234e39, y - 34, 10);
        }
        if y > 40 {
            p = shade(0x67a6bb, 0x315f83, y - 40, 11);
        }
        if y > 43 && x > 34 && x < 51 {
            p = shade(0x72a27b, 0x507d71, y - 43, 8);
        }
        return p;
    }
    let mut p = 0;
    // Upright panel behind the cabinet, with a green status indicator.
    if x >= 26 && x < 53 && y >= 7 && y < 39 {
        p = 0xffa5abad;
        if x >= 29 && x < 50 && y >= 10 {
            p = 0xfff4f7f7;
        }
        if x >= 35 && x < 45 && y >= 19 {
            p = shade(0x9da3a3, 0xc2c7c7, y - 19, 20);
        }
        if x >= 35 && x < 39 && y >= 15 && y < 23 {
            p = 0xff83bd38;
        }
    }
    if x >= 18 && x < 34 && y >= 31 && y < 36 {
        p = 0xff303c41;
    }
    if x >= 5 && x < 59 && y >= 36 && y < 58 {
        p = shade(0x81939b, 0x455c66, y - 36, 22);
        if x >= 8 && x < 56 && y >= 39 && y < 54 {
            p = shade(0xb6c6cd, 0x889ea8, y - 39, 15);
        }
        if x >= 10 && x < 54 && y >= 43 && y < 52 {
            p = 0xff40545f;
        }
        if x >= 15 && x < 49 && y >= 47 && y < 52 {
            p = 0xffc7d4d8;
        }
        if x >= 12 && x < 17 && y >= 40 && y < 45 {
            p = 0xffeef4f5;
        }
        if x >= 49 && x < 54 && y >= 40 && y < 45 {
            p = 0xffedf4f5;
        }
        if y == 37 {
            p = 0xffbdcbd2;
        }
    }
    return p;
}
const fn icon<const SIDE: usize, const BYTES: usize>(photo: bool) -> [u8; BYTES] {
    let mut b = [0; BYTES];
    let mut y = 0;
    while y < SIDE {
        let mut x = 0;
        while x < SIDE {
            let mut a = 0;
            let mut r = 0;
            let mut g = 0;
            let mut blue = 0;
            let mut sample = 0;
            while sample < 16 {
                let p = artwork(
                    photo,
                    ((x * 4 + sample % 4) * 64 / (SIDE * 4)) as u32,
                    ((y * 4 + sample / 4) * 64 / (SIDE * 4)) as u32,
                );
                let alpha = p >> 24;
                a += alpha;
                r += (p >> 16 & 255) * alpha;
                g += (p >> 8 & 255) * alpha;
                blue += (p & 255) * alpha;
                sample += 1;
            }
            let i = (y * SIDE + x) * 4;
            if a > 0 {
                b[i] = (r / a) as u8;
                b[i + 1] = (g / a) as u8;
                b[i + 2] = (blue / a) as u8;
                b[i + 3] = (a / 16) as u8;
            }
            x += 1;
        }
        y += 1;
    }
    b
}
pub static PHOTO_RGBA: [u8; 64 * 64 * 4] = icon::<64, { 64 * 64 * 4 }>(true);
pub static DEVICE_RGBA: [u8; 64 * 64 * 4] = icon::<64, { 64 * 64 * 4 }>(false);
pub static PHOTO_16: [u8; 16 * 16 * 4] = icon::<16, { 16 * 16 * 4 }>(true);
pub static DEVICE_16: [u8; 16 * 16 * 4] = icon::<16, { 16 * 16 * 4 }>(false);
pub static PHOTO_32: [u8; 32 * 32 * 4] = icon::<32, { 32 * 32 * 4 }>(true);
pub static DEVICE_32: [u8; 32 * 32 * 4] = icon::<32, { 32 * 32 * 4 }>(false);

//! Picture layout preview and bounded PostScript sheets for the installed file printer.
use super::{Command, Viewer};
use crate::applications::{
    view::{Button, Command as ViewCommand},
    ApplicationSession,
};
use crate::{
    compositor::aero,
    graphics::{Rect, Surface},
};
use alloc::{format, string::String, vec::Vec};
use core::fmt::Write;

pub const LAYOUTS: [&str; 7] = [
    "Full page photo",
    "8 x 10 in.",
    "5 x 7 in. (2)",
    "4 x 6 in. (2)",
    "3.5 x 5 in. (4)",
    "Wallet (9)",
    "Contact sheet (35)",
];
pub const PAPERS: [&str; 4] = ["Letter", "A4", "4 x 6 in.", "5 x 7 in."];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub layout: usize,
    pub paper: usize,
    pub copies: u16,
    pub fill: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            layout: 0,
            paper: 0,
            copies: 1,
            fill: true,
        }
    }
}
impl Options {
    fn grid(&self) -> (u32, u32) {
        match self.layout {
            2 | 3 => (1, 2),
            4 => (2, 2),
            5 => (3, 3),
            6 => (5, 7),
            _ => (1, 1),
        }
    }
    fn paper_points(&self) -> (u32, u32) {
        match self.paper {
            1 => (595, 842),
            2 => (288, 432),
            3 => (360, 504),
            _ => (612, 792),
        }
    }
    fn pages(&self) -> u32 {
        let (cols, rows) = self.grid();
        u32::from(self.copies).div_ceil(cols * rows)
    }
}

fn bounds(b: Rect) -> Rect {
    let scale = (b.width.saturating_sub(8) * 1000 / 650)
        .min(b.height.saturating_sub(8) * 1000 / 470)
        .min(1000);
    let width = (650 * scale / 1000).max(1);
    let height = (470 * scale / 1000).max(1);
    Rect::new(
        b.x + (b.width as i32 - width as i32) / 2,
        b.y + (b.height as i32 - height as i32) / 2,
        width,
        height,
    )
}
fn map_rect(r: Rect, p: Rect) -> Rect {
    let x = r.x as u32 * p.width / 650;
    let y = r.y as u32 * p.height / 470;
    let right = (r.x as u32 + r.width) * p.width / 650;
    let bottom = (r.y as u32 + r.height) * p.height / 470;
    Rect::new(p.x + x as i32, p.y + y as i32, right - x, bottom - y)
}
pub fn buttons(b: Rect) -> Vec<Button> {
    let p = bounds(b);
    let mut result = raw_buttons(Rect::new(0, 0, 650, 470));
    for button in &mut result {
        button.bounds = map_rect(button.bounds, p);
    }
    result
}
fn raw_buttons(p: Rect) -> Vec<Button> {
    let mut buttons = Vec::new();
    let mut add = |r, label: &str, c| {
        buttons.push(Button {
            bounds: r,
            label: label.into(),
            command: ViewCommand::Photo(c),
        })
    };
    add(
        Rect::new(p.x + 329, p.y + 70, 126, 25),
        "Paper size",
        Command::PrintPaper,
    );
    for (i, label) in LAYOUTS.iter().enumerate() {
        add(
            Rect::new(p.x + 475, p.y + 117 + i as i32 * 38, 159, 36),
            label,
            Command::PrintLayout(i),
        );
    }
    add(
        Rect::new(p.x + 188, p.y + 410, 26, 25),
        "-",
        Command::PrintCopies(-1),
    );
    add(
        Rect::new(p.x + 258, p.y + 410, 26, 25),
        "+",
        Command::PrintCopies(1),
    );
    add(
        Rect::new(p.x + 309, p.y + 410, 149, 25),
        "Fit picture to frame",
        Command::PrintFill,
    );
    add(
        Rect::new(p.x + 467, p.y + 435, 80, 25),
        "Print",
        Command::PrintSubmit,
    );
    add(
        Rect::new(p.x + 555, p.y + 435, 80, 25),
        "Cancel",
        Command::PrintCancel,
    );
    buttons
}

fn paint_picture(s: &mut Surface, image: &Surface, cell: Rect, fill: bool) {
    if cell.width == 0 || cell.height == 0 || image.width() == 0 || image.height() == 0 {
        return;
    }
    let sx = cell.width as u64 * 65536 / image.width() as u64;
    let sy = cell.height as u64 * 65536 / image.height() as u64;
    let scale = if fill { sx.max(sy) } else { sx.min(sy) }.max(1);
    let w = (image.width() as u64 * scale / 65536).max(1) as u32;
    let h = (image.height() as u64 * scale / 65536).max(1) as u32;
    let x = cell.x + (cell.width as i32 - w as i32) / 2;
    let y = cell.y + (cell.height as i32 - h as i32) / 2;
    let image_rect = Rect::new(x, y, w, h);
    if let Some(clip) = image_rect
        .intersection(&cell)
        .and_then(|c| c.intersection(&s.drawing_bounds()))
    {
        for py in clip.y..clip.y + clip.height as i32 {
            for px in clip.x..clip.x + clip.width as i32 {
                s.blend_pixel(
                    px as u32,
                    py as u32,
                    super::sample(image, px - x, py - y, w, h),
                );
            }
        }
    }
}

/// Render the same arrangement for screen preview and printer output.
fn sheet(s: &mut Surface, image: &Surface, o: &Options, page: u32) {
    s.fill_rect(0, 0, s.width(), s.height(), 0xffffffff);
    let (pw, ph) = o.paper_points();
    let (cols, rows) = o.grid();
    let margin = 12;
    let gap = 8;
    let maxw = pw.saturating_sub(margin * 2 + gap * (cols - 1)) / cols;
    let maxh = ph.saturating_sub(margin * 2 + gap * (rows - 1)) / rows;
    let (tw, th) = match o.layout {
        1 => (576.min(maxw), 720.min(maxh)),
        2 => (504.min(maxw), 360.min(maxh)),
        3 => (432.min(maxw), 288.min(maxh)),
        4 => (252.min(maxw), 360.min(maxh)),
        _ => (maxw, maxh),
    };
    let groupw = tw * cols + gap * (cols - 1);
    let grouph = th * rows + gap * (rows - 1);
    let start = page * cols * rows;
    for i in 0..cols * rows {
        if start + i >= u32::from(o.copies) {
            break;
        }
        let x = (pw - groupw) / 2 + (i % cols) * (tw + gap);
        let y = (ph - grouph) / 2 + (i / cols) * (th + gap);
        let r = Rect::new(
            (x * s.width() / pw) as i32,
            (y * s.height() / ph) as i32,
            (tw * s.width() / pw).max(1),
            (th * s.height() / ph).max(1),
        );
        paint_picture(s, image, r, o.fill);
    }
}

pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    v: &Viewer,
    s: &mut Surface,
    b: Rect,
    text: &F,
) {
    if v.print.is_none() || b.width == 0 || b.height == 0 {
        return;
    }
    let p = bounds(b);
    let canonical = Rect::new(0, 0, 650, 470);
    let hot = raw_buttons(canonical)
        .into_iter()
        .find(|button| app.hot == Some(map_rect(button.bounds, p)))
        .map(|button| button.bounds);
    let mut canvas = Surface::new(650, 470);
    draw_contents(v, &mut canvas, canonical, hot, text);
    if let Some(clip) = p.intersection(&s.drawing_bounds()) {
        for y in clip.y..clip.y + clip.height as i32 {
            for x in clip.x..clip.x + clip.width as i32 {
                s.set_pixel(
                    x as u32,
                    y as u32,
                    super::sample(&canvas, x - p.x, y - p.y, p.width, p.height),
                );
            }
        }
    }
}
fn draw_contents<F: Fn(&mut Surface, &str, Rect, u32)>(
    v: &Viewer,
    s: &mut Surface,
    p: Rect,
    hot: Option<Rect>,
    text: &F,
) {
    let Some(o) = &v.print else {
        return;
    };
    aero::shadow(s, p, 6);
    s.fill_rect(p.x, p.y, p.width, p.height, 0xfff0f0f0);
    s.stroke_rounded_rect(p, 0, 1, 0xff6c8499);
    s.fill_rect(p.x + 1, p.y + 1, p.width - 2, 41, 0xffffffff);
    text(
        s,
        "How do you want to print your pictures?",
        Rect::new(p.x + 17, p.y + 15, 610, 24),
        0xff003399,
    );
    for (x, width, title, value) in [
        (16, 300, "Printer:", "NT61 PostScript Document Writer"),
        (329, 126, "Paper size:", PAPERS[o.paper]),
        (475, 159, "Quality:", "Automatic"),
    ] {
        text(
            s,
            title,
            Rect::new(p.x + x, p.y + 49, width, 20),
            0xff222222,
        );
        let r = Rect::new(p.x + x, p.y + 70, width, 25);
        s.fill_rect(r.x, r.y, r.width, r.height, 0xffffffff);
        s.stroke_rounded_rect(r, 0, 1, 0xff9aabb9);
        text(
            s,
            value,
            Rect::new(r.x + 5, r.y + 5, width - 12, 19),
            0xff222222,
        );
    }
    let backdrop = Rect::new(p.x + 16, p.y + 110, 439, 290);
    s.fill_rect(
        backdrop.x,
        backdrop.y,
        backdrop.width,
        backdrop.height,
        0xff647b91,
    );
    let (pw, ph) = o.paper_points();
    let h = 272;
    let w = pw * h / ph;
    let mut preview = Surface::new(w, h);
    if let Some(image) = &v.image {
        sheet(&mut preview, image, o, 0);
    }
    let x = backdrop.x + (backdrop.width - w) as i32 / 2;
    let y = backdrop.y + 8;
    aero::shadow(s, Rect::new(x, y, w, h), 0);
    for py in 0..h {
        for px in 0..w {
            if x + px as i32 >= 0 && y + py as i32 >= 0 {
                s.set_pixel(
                    (x + px as i32) as u32,
                    (y + py as i32) as u32,
                    preview.pixel(px, py).unwrap_or(0xffffffff),
                );
            }
        }
    }
    for button in raw_buttons(p) {
        let ViewCommand::Photo(c) = &button.command else {
            continue;
        };
        if matches!(c, Command::PrintPaper) {
            continue;
        }
        if let Command::PrintLayout(index) = c {
            let r = button.bounds;
            s.fill_rect(
                r.x,
                r.y,
                r.width,
                r.height,
                if *index == o.layout {
                    0xffcce8ff
                } else {
                    0xffffffff
                },
            );
            s.stroke_rounded_rect(
                r,
                0,
                1,
                if *index == o.layout {
                    0xff3399ff
                } else {
                    0xffb9c4cc
                },
            );
            text(
                s,
                &button.label,
                Rect::new(r.x + 7, r.y + 11, r.width - 12, 20),
                0xff222222,
            );
        } else if matches!(c, Command::PrintFill) {
            let r = button.bounds;
            s.fill_rect(r.x, r.y + 5, 13, 13, 0xffffffff);
            s.stroke_rounded_rect(Rect::new(r.x, r.y + 5, 13, 13), 0, 1, 0xff708090);
            if o.fill {
                text(s, "x", Rect::new(r.x + 3, r.y + 3, 12, 20), 0xff111111);
            }
            text(
                s,
                &button.label,
                Rect::new(r.x + 19, r.y + 5, 130, 20),
                0xff222222,
            );
        } else {
            aero::glass_button(s, button.bounds, hot == Some(button.bounds));
            text(
                s,
                &button.label,
                Rect::new(
                    button.bounds.x + 9,
                    button.bounds.y + 5,
                    button.bounds.width - 12,
                    20,
                ),
                0xff222222,
            );
        }
    }
    text(
        s,
        "Copies of each picture:",
        Rect::new(p.x + 16, p.y + 415, 170, 22),
        0xff222222,
    );
    text(
        s,
        &format!("{}", o.copies),
        Rect::new(p.x + 224, p.y + 415, 35, 22),
        0xff222222,
    );
    text(
        s,
        &format!("{} page(s)", o.pages()),
        Rect::new(p.x + 16, p.y + 442, 200, 20),
        0xff444444,
    );
}

pub fn document(image: &Surface, options: &Options) -> Result<Vec<u8>, String> {
    if image.width() == 0
        || image.height() == 0
        || options.layout >= LAYOUTS.len()
        || options.paper >= PAPERS.len()
        || !(1..=999).contains(&options.copies)
    {
        return Err("Invalid picture print options.".into());
    }
    const LIMIT: usize = 1024 * 1024;
    let (pw, ph) = options.paper_points();
    let pages = options.pages();
    let budget = (LIMIT - 4096).saturating_sub(pages as usize * 256) / pages as usize / 7;
    let (mut rw, mut rh) = (pw, ph);
    while rw as usize * rh as usize > budget {
        rw = (rw * 9 / 10).max(1);
        rh = (rh * 9 / 10).max(1);
    }
    if rw < 16 || rh < 16 {
        return Err("Too many picture copies for one print job.".into());
    }
    let mut raster = Surface::new(rw, rh);
    let mut output = format!("%!PS-Adobe-3.0\n%%Creator: NT61 Print Pictures\n%%Pages: {pages}\n%%EndComments\n<< /PageSize [{pw} {ph}] >> setpagedevice\n");
    for page in 0..pages {
        sheet(&mut raster, image, options, page);
        let _ = writeln!(output, "%%Page: {} {}\ngsave\n{pw} {ph} scale\n{rw} {rh} 8 [{rw} 0 0 -{rh} 0 {rh}]\n{{currentfile /ASCIIHexDecode filter}} false 3 colorimage", page + 1, page + 1);
        for (i, p) in raster.pixels().iter().enumerate() {
            let _ = write!(output, "{:06x}", p & 0xffffff);
            if i % 24 == 23 {
                output.push('\n');
            }
        }
        output.push_str(">\ngrestore\nshowpage\n");
        if output.len() > LIMIT - 64 {
            return Err("Print job exceeds 1 MiB.".into());
        }
    }
    output.push_str("%%Trailer\n%%EOF\n");
    Ok(output.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_and_print_share_copy_layout_and_keep_alpha_on_paper() {
        let mut image = Surface::new(2, 1);
        image.set_pixel(0, 0, 0x00ff0000);
        image.set_pixel(1, 0, 0xff123456);
        let options = Options {
            layout: 3,
            copies: 3,
            fill: false,
            ..Options::default()
        };
        assert_eq!(options.pages(), 2);
        let ps = String::from_utf8(document(&image, &options).unwrap()).unwrap();
        assert!(ps.contains("%%Pages: 2\n"));
        assert!(ps.contains("%%Page: 2 2\n"));
        assert!(!ps.contains("ff0000"));
        let mut invalid = options;
        invalid.copies = 0;
        assert!(document(&image, &invalid).is_err());
    }
    #[test]
    fn fit_retains_edges_while_fill_crops_to_the_frame() {
        let mut image = Surface::new(4, 2);
        image.fill_rect(0, 0, 4, 2, 0xff112233);
        let cell = Rect::new(0, 0, 20, 20);
        let mut paper = Surface::new(20, 20);
        paper.fill_rect(0, 0, 20, 20, 0xffffffff);
        paint_picture(&mut paper, &image, cell, false);
        assert_eq!(paper.pixel(10, 0), Some(0xffffffff));
        paint_picture(&mut paper, &image, cell, true);
        assert_eq!(paper.pixel(10, 0), Some(0xff112233));
    }
}

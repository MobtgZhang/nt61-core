//! Paint ribbon, palette and canvas viewport. The document model owns raster operations.
use super::*;
use crate::applications::shared::toolbar::{self, Icon};
use crate::applications::{
    view::{paint_bounds, Button, Command},
    AppState, ApplicationSession,
};
use crate::graphics::Rect;
use alloc::format;
const SMALL_ICONS: &[u8] = include_bytes!("resources/toolbar-16.rgba");
const LARGE_ICONS: &[u8] = include_bytes!("resources/toolbar-32.rgba");
fn button(
    out: &mut Vec<Button>,
    b: Rect,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    label: &str,
    command: Command,
) {
    let bounds = Rect::new(b.x + x, b.y + y, w, h);
    if bounds.intersection(&b) == Some(bounds) {
        out.push(Button {
            bounds,
            label: label.into(),
            command,
        });
    }
}
pub fn buttons(app: &ApplicationSession, b: Rect) -> Vec<Button> {
    let AppState::Paint(p) = &app.state else {
        return Vec::new();
    };
    let mut out = Vec::new();

    for (i, (label, command)) in [
        ("File", Command::File),
        ("Home", Command::PaintTab(false)),
        ("View", Command::PaintTab(true)),
        ("Save", Command::Save),
        ("Undo", Command::Undo),
        ("Redo", Command::Redo),
    ]
    .into_iter()
    .enumerate()
    {
        button(&mut out, b, 6 + i as i32 * 65, 4, 61, 25, label, command);
    }
    if p.view_tab {
        for (i, (label, command)) in [
            ("Zoom in", Command::PaintZoom(50)),
            ("Zoom out", Command::PaintZoom(-50)),
            ("100%", Command::PaintZoom(0)),
            ("Gridlines", Command::Grid),
        ]
        .into_iter()
        .enumerate()
        {
            button(&mut out, b, 8 + i as i32 * 100, 38, 94, 62, label, command);
        }
    } else {
        for (i, (label, command)) in [
            ("Pencil", Command::Tool(Tool::Pencil)),
            ("Fill", Command::Tool(Tool::Fill)),
            ("Picker", Command::Tool(Tool::Picker)),
            ("Eraser", Command::Tool(Tool::Eraser)),
            ("Brush", Command::Tool(Tool::Brush)),
            ("Size", Command::BrushSize),
        ]
        .into_iter()
        .enumerate()
        {
            button(
                &mut out,
                b,
                8 + (i % 3) as i32 * 58,
                38 + (i / 3) as i32 * 30,
                54,
                27,
                label,
                command,
            );
        }
        for (i, (label, command)) in [
            ("Line", Command::Tool(Tool::Line)),
            ("Rect", Command::Tool(Tool::Rectangle)),
            ("Oval", Command::Tool(Tool::Ellipse)),
            ("Rotate", Command::Rotate),
            ("Flip H", Command::Flip(true)),
            ("Flip V", Command::Flip(false)),
        ]
        .into_iter()
        .enumerate()
        {
            button(
                &mut out,
                b,
                190 + (i % 3) as i32 * 55,
                38 + (i / 3) as i32 * 30,
                51,
                27,
                label,
                command,
            );
        }
        for i in 0..COLORS.len() {
            button(
                &mut out,
                b,
                365 + (i % 10) as i32 * 23,
                38 + (i / 10) as i32 * 29,
                21,
                26,
                "",
                Command::Color(i),
            );
        }
        button(&mut out, b, 8, 112, 80, 25, "Clear", Command::New);
        button(&mut out, b, 365, 99, 112, 27, "Edit colors", Command::EditColors);
        button(
            &mut out,
            b,
            94,
            112,
            110,
            25,
            "Resize 50%",
            Command::ResizeHalf,
        );
    }

    out
}
pub fn draw_canvas<F: Fn(&mut Surface, &str, Rect, u32)>(
    p: &Paint,
    frame: &mut Surface,
    b: Rect,
    text: &F,
) {
    frame.fill_rect(b.x, b.y + 30, b.width, 112, 0xFFDDE7F3);
    let dest = paint_bounds(b);
    frame.fill_rect(dest.x, dest.y, dest.width, dest.height, 0xFF8B9BAD);
    for y in 0..dest.height {
        for x in 0..dest.width {
            let sx = x * 100 / p.zoom + p.scroll.0;
            let sy = y * 100 / p.zoom + p.scroll.1;
            if let Some(pixel) = p.canvas.pixel(sx, sy) {
                let color = if p.grid
                    && p.zoom >= 200
                    && ((x * 100 % p.zoom == 0) || (y * 100 % p.zoom == 0))
                {
                    0xFFB0B0B0
                } else {
                    pixel
                };
                frame.set_pixel(
                    (dest.x + x as i32) as u32,
                    (dest.y + y as i32) as u32,
                    color,
                );
            }
        }
    }
    for (x, label) in [
        (12, "Tools / Brushes"),
        (194, "Shapes / Image"),
        (370, "Colors"),
    ] {
        text(
            frame,
            label,
            Rect::new(b.x + x, b.y + 98, 170, 20),
            0xFF4B6180,
        );
    }
    text(
        frame,
        &format!(
            "{} x {} px    {}%    Size {}    Color 1 / Color 2",
            p.canvas.width(),
            p.canvas.height(),
            p.zoom,
            p.size
        ),
        Rect::new(
            b.x + 8,
            b.y + b.height as i32 - 27,
            b.width.saturating_sub(16),
            22,
        ),
        0xFF526578,
    );
}

fn icon(command: &Command) -> Option<Icon> {
    Some(match command {
        Command::Save => Icon::Save,
        Command::Undo => Icon::Undo,
        Command::Redo => Icon::Redo,
        Command::Tool(tool) => match tool {
            Tool::Pencil => Icon::Pencil,
            Tool::Brush => Icon::Brush,
            Tool::Eraser => Icon::Eraser,
            Tool::Fill => Icon::Fill,
            Tool::Picker => Icon::Picker,
            Tool::Line => Icon::Line,
            Tool::Rectangle => Icon::Rectangle,
            Tool::Ellipse => Icon::Ellipse,
        },
        Command::BrushSize => Icon::BrushSize,
        Command::Rotate => Icon::Rotate,
        Command::Flip(true) => Icon::FlipH,
        Command::Flip(false) => Icon::FlipV,
        Command::New => Icon::Clear,
        Command::ResizeHalf => Icon::Resize,
        Command::Grid => Icon::Grid,
        Command::PaintZoom(n) => {
            if *n > 0 {
                Icon::ZoomIn
            } else if *n < 0 {
                Icon::ZoomOut
            } else {
                Icon::ActualSize
            }
        }
        _ => return None,
    })
}
pub fn draw_button<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    frame: &mut Surface,
    b: Rect,
    button: &Button,
    text: &F,
) -> bool {
    let AppState::Paint(p) = &app.state else {
        return false;
    };
    let r = button.bounds;
    // File-menu rows retain their normal labels and shared command handling.
    if app.menu && r.x == b.x + 6 && r.y >= b.y + 33 {
        return false;
    }
    let selected = match button.command {
        Command::Tool(tool) => p.tool == tool,
        Command::PaintTab(view) => p.view_tab == view,
        Command::Color(i) => p.color == COLORS[i],
        Command::Grid => p.grid,
        _ => false,
    };
    if selected || app.hot == Some(r) {
        crate::compositor::aero::menu_item(frame, r, true);
    }
    if let Command::Color(i) = button.command {
        frame.fill_rect(r.x + 3, r.y + 3, r.width - 6, r.height - 6, COLORS[i]);
        if p.background == COLORS[i] {
            frame.fill_rect(
                r.x + r.width as i32 - 7,
                r.y + r.height as i32 - 7,
                4,
                4,
                0xff347db5,
            );
        }
    } else if button.command == Command::File {
        frame.fill_rect(r.x, r.y, r.width, r.height, 0xff286aa7);
        text(
            frame,
            "File",
            Rect::new(r.x + 12, r.y + 3, r.width - 14, 20),
            0xffffffff,
        );
    } else if let Some(icon) = icon(&button.command) {
        let large = r.height > 40;
        let size = if large { 32 } else { 16 };
        let label = large || r.width >= 78;
        let x = if label && !large {
            r.x + 4
        } else {
            r.x + ((r.width - size) / 2) as i32
        };
        let y = if large {
            r.y + 2
        } else {
            r.y + ((r.height - size) / 2) as i32
        };
        toolbar::draw(
            frame,
            if large { LARGE_ICONS } else { SMALL_ICONS },
            icon,
            size,
            Rect::new(x, y, size, size),
        );
        if label {
            let bounds = if large {
                Rect::new(r.x + 4, r.y + 36, r.width - 6, 20)
            } else {
                Rect::new(r.x + 25, r.y + 3, r.width - 27, 20)
            };
            text(frame, &button.label, bounds, 0xff203d55);
        } else if app.hot == Some(r) {
            let width = (button.label.len() as u32 * 8 + 12).min(b.width);
            let x = r.x.min(b.x + b.width as i32 - width as i32);
            frame.fill_rect(x, r.y + r.height as i32 + 3, width, 25, 0xfffffff0);
            text(
                frame,
                &button.label,
                Rect::new(x + 5, r.y + r.height as i32 + 5, width - 8, 20),
                0xff253f54,
            );
        }
    } else {
        text(
            frame,
            &button.label,
            Rect::new(r.x + 6, r.y + 3, r.width - 8, r.height - 4),
            0xff203d55,
        );
    }
    true
}

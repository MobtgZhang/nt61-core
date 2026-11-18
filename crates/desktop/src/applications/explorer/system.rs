//! Win7 System page. Hardware fields describe the current NT61 session.
use super::theme;
use crate::{
    control_browser::strings,
    graphics::{Rect, Surface},
};

pub const SIDEBAR_WIDTH: u32 = 220;
const BLUE: u32 = 0xFF0066CC;
const TEXT: u32 = 0xFF161616;
pub fn home_link(b: Rect) -> Rect {
    Rect::new(b.x + 18, b.y + 14, SIDEBAR_WIDTH - 24, 24)
}
pub fn sidebar_link(b: Rect, i: usize) -> Rect {
    Rect::new(b.x + 30, b.y + 58 + i as i32 * 28, SIDEBAR_WIDTH - 36, 26)
}
pub fn device_manager_link(b: Rect) -> Rect {
    sidebar_link(b, 0)
}
pub fn change_link(b: Rect) -> Rect {
    Rect::new(b.x + b.width as i32 - 126, b.y + 380, 110, 24)
}
pub fn draw_scrolled(
    f: &mut Surface,
    viewport: Rect,
    b: Rect,
    cpu: Option<u32>,
    memory: Option<u64>,
    font: &crate::graphics::font::FontLib,
    text: impl Fn(&mut Surface, &str, Rect, u32),
) {
    f.fill_rect(
        viewport.x,
        viewport.y,
        viewport.width,
        viewport.height,
        0xFFFFFFFF,
    );
    // Navigation stays visible while only the information pane scrolls.
    f.fill_rect(
        viewport.x,
        viewport.y,
        SIDEBAR_WIDTH,
        viewport.height,
        0xFFEAF1FB,
    );
    text(f, strings::CONTROL_PANEL_HOME, home_link(viewport), BLUE);
    for (i, value) in [
        strings::DEVICE_MANAGER,
        strings::REMOTE_SETTINGS,
        strings::SYSTEM_PROTECTION,
        strings::ADVANCED_SYSTEM_SETTINGS,
    ]
    .iter()
    .enumerate()
    {
        let r = sidebar_link(viewport, i);
        theme::icon(f, "Shield", Rect::new(r.x - 18, r.y + 2, 14, 14));
        text(f, value, r, BLUE);
    }
    let related_y = (viewport.height as i32 - 110).max(200);
    for (i, value) in [
        strings::SEE_ALSO,
        strings::ACTION_CENTER,
        strings::WINDOWS_UPDATE,
        strings::PERFORMANCE_INFORMATION_AND_TOOLS,
    ]
    .iter()
    .enumerate()
    {
        {
            crate::control_browser::paint::wrapped_label(
                &font,
                f,
                value,
                Rect::new(
                    viewport.x + 18,
                    viewport.y + related_y + i as i32 * 23,
                    SIDEBAR_WIDTH - 24,
                    42,
                ),
                if i == 0 { 0xFF777777 } else { 0xFF666666 },
                12,
                17,
            );
        }
    }
    let x = b.x + SIDEBAR_WIDTH as i32 + 20;
    let w = b.width.saturating_sub(SIDEBAR_WIDTH + 38);
    let Some(clip) = f
        .drawing_bounds()
        .intersection(&Rect::new(x, viewport.y, w, viewport.height))
    else {
        return;
    };
    let previous = f.drawing_bounds();
    f.set_draw_clip(Some(clip));
    let line = |f: &mut Surface, value: &str, y: i32, color| {
        text(f, value, Rect::new(x, b.y + y, w, 28), color)
    };
    line(
        f,
        strings::VIEW_BASIC_INFORMATION_ABOUT_YOUR_COMPUTER,
        18,
        0xFF003399,
    );
    for (title, y) in [
        (strings::WINDOWS_EDITION, 55),
        (strings::SYSTEM, 211),
        (strings::COMPUTER_NAME_DOMAIN_AND_WORKGROUP_SETTINGS, 347),
        (strings::WINDOWS_ACTIVATION, 468),
    ] {
        line(f, title, y, TEXT);
        f.fill_rect(x, b.y + y + 25, w, 1, 0xFFD8D8D8);
    }
    line(f, "NT 6.1", 88, TEXT);
    line(f, strings::WINDOWS_7_SP1_COMPATIBLE_SYSTEM, 112, TEXT);
    line(f, strings::VERSION_6_1_7601_SERVICE_PACK_1, 136, TEXT);
    if w >= 300 {
        theme::system_logo(f, Rect::new(x + w as i32 - 128, b.y + 68, 120, 116));
    }
    let label_width = 166.min(w / 2);
    let value_x = x + label_width as i32 + 12;
    let value_w = w.saturating_sub(label_width + 12);
    for (i, (name, value)) in [
        (
            strings::RATING,
            strings::SYSTEM_RATING_IS_NOT_AVAILABLE.into(),
        ),
        (strings::PROCESSOR, strings::processor(architecture(), cpu)),
        (
            strings::INSTALLED_MEMORY_RAM,
            memory.map_or_else(|| strings::NOT_REPORTED.into(), strings::memory_mb),
        ),
        (
            strings::SYSTEM_TYPE,
            strings::LABEL_64_BIT_OPERATING_SYSTEM.into(),
        ),
        (
            strings::PEN_AND_TOUCH,
            strings::NO_PEN_OR_TOUCH_INPUT_IS_AVAILABLE_FOR_THIS_DISPLAY.into(),
        ),
    ]
    .iter()
    .enumerate()
    {
        let y = b.y + 244 + i as i32 * 20;
        text(f, name, Rect::new(x + 12, y, label_width - 12, 24), TEXT);
        text(
            f,
            value,
            Rect::new(value_x, y, value_w, 24),
            if i == 0 { BLUE } else { TEXT },
        );
    }
    for (i, (name, value)) in [
        (strings::COMPUTER_NAME_2, "NT61"),
        (strings::FULL_COMPUTER_NAME, "NT61"),
        (strings::WORKGROUP, strings::NOT_JOINED),
    ]
    .iter()
    .enumerate()
    {
        let y = b.y + 383 + i as i32 * 23;
        text(f, name, Rect::new(x + 12, y, label_width - 12, 22), TEXT);
        text(
            f,
            value,
            Rect::new(
                value_x,
                y,
                value_w.saturating_sub(if i == 0 { 100 } else { 0 }),
                22,
            ),
            TEXT,
        );
    }
    let change = change_link(b);
    theme::icon(f, "Shield", Rect::new(change.x - 18, change.y, 16, 16));
    text(f, strings::CHANGE_SETTINGS, change, BLUE);
    line(
        f,
        strings::MICROSOFT_WINDOWS_ACTIVATION_DOES_NOT_APPLY_TO_NT61,
        505,
        TEXT,
    );
    f.set_draw_clip(Some(previous));
}
pub fn draw_page(
    f: &mut Surface,
    b: Rect,
    cpu: Option<u32>,
    memory: Option<u64>,
    font: &crate::graphics::font::FontLib,
    text: impl Fn(&mut Surface, &str, Rect, u32),
) {
    draw_scrolled(f, b, b, cpu, memory, font, text);
}
fn architecture() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "ARM64"
    } else if cfg!(target_arch = "riscv64") {
        "RISC-V 64"
    } else {
        "LoongArch64"
    }
}
pub fn draw(
    f: &mut Surface,
    b: Rect,
    cpu: Option<u32>,
    font: &crate::graphics::font::FontLib,
    text: impl Fn(&mut Surface, &str, Rect, u32),
) {
    draw_page(f, b, cpu, None, font, text);
}

//! Host rendering oracle for the production Control Panel browser.
use nt61_desktop::{
    control_browser::{self, Page},
    graphics::{Rect, Surface},
    shell::{ShellModel, WindowKind},
    theme::Theme,
};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "control-preview <page> <client-width> <client-height> <output.ppm>"
    );
    let kind = match args[1].as_str() {
        "category" | "view-menu" => WindowKind::ControlPanel,
        "items" => WindowKind::ControlPage(Page::AllItems),
        "system" => WindowKind::System,
        "resolution" | "resolution-list" => WindowKind::DisplaySettings,
        "advanced" => WindowKind::ControlPage(Page::DisplayAdvanced),
        "text" => WindowKind::ControlPage(Page::TextSize),
        value if value.starts_with("properties-") => {
            WindowKind::ControlPage(Page::Properties(value[11..].parse().unwrap()))
        }
        value => WindowKind::ControlCategory(value.parse().unwrap()),
    };
    let (width, height) = (
        args[2].parse::<u32>().unwrap(),
        args[3].parse::<u32>().unwrap(),
    );
    let mut model = ShellModel::new(Theme::aero());
    let id = model.open_window_with_kind(
        control_browser::title(kind),
        -6,
        -30,
        width as i32 + 12,
        height as i32 + 36,
        kind,
    );
    model.display.modes = vec![
        (800, 600),
        (1024, 768),
        (1280, 720),
        (1280, 800),
        (1280, 960),
        (1280, 1024),
        (1440, 900),
        (1600, 900),
        (1920, 1080),
    ];
    model.display.selected = 1;
    model.display.active = (1024, 768);
    if args[1] == "resolution-list" {
        model.display.open = true;
    }
    if args[1] == "view-menu" {
        model.control_browser_mut(id).unwrap().view_open = true;
    }
    let mut frame = Surface::new(width, height);
    let font = model.fonts.get(control_browser::paint::FONT).unwrap();
    control_browser::paint::draw(
        &model,
        id,
        &mut frame,
        Rect::new(0, 0, width, height),
        |f, t, r, c| control_browser::paint::label(&font, f, t, r, c, 12),
    );
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    for p in frame.pixels() {
        bytes.extend([(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(&args[4], bytes).unwrap();
}

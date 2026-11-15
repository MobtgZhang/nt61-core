//! Control Panel's System page. Hardware and activation fields describe this
//! running NT61 session; Windows artwork does not imply Microsoft activation.
use super::theme;
use crate::graphics::{Rect, Surface};
use alloc::format;

pub const SIDEBAR_WIDTH: u32 = 180;
const BLUE: u32 = 0xFF005A9E;
const TEXT: u32 = 0xFF202020;

pub fn home_link(body: Rect) -> Rect {
    Rect::new(body.x + 12, body.y + 48, SIDEBAR_WIDTH.saturating_sub(20), 28)
}

pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(frame: &mut Surface, body: Rect, cpu_count: Option<u32>, text: F) {
    frame.fill_rect(body.x, body.y, body.width, body.height, 0xFFFFFFFF);
    let sidebar = SIDEBAR_WIDTH.min(body.width / 3);
    frame.fill_rect(body.x, body.y + 35, sidebar, body.height.saturating_sub(35), 0xFFF0F5FB);
    frame.fill_rect(body.x, body.y, body.width, 35.min(body.height), 0xFFEAF0F8);
    theme::navigation(frame, Rect::new(body.x + 4, body.y + 4, 27, 27), false, false, false, false);
    theme::navigation(frame, Rect::new(body.x + 34, body.y + 4, 27, 27), true, false, false, false);
    text(frame, "Control Panel  >  System and Security  >  System", Rect::new(body.x + 76, body.y + 9, body.width.saturating_sub(88), 20), TEXT);
    let home = home_link(body);
    text(frame, "Control Panel Home", home, BLUE);
    // The four Win7 administrative tasks require services not yet exposed by
    // this shell. Keep their familiar positions, with explicit disabled styling.
    for (i, title) in ["Device Manager", "Remote settings", "System protection", "Advanced system settings"].iter().enumerate() {
        let y = body.y + 98 + i as i32 * 30;
        if y + 20 > body.y + body.height as i32 { break; }
        theme::icon(frame, "Shield", Rect::new(body.x + 12, y, 16, 16));
        text(frame, title, Rect::new(body.x + 34, y, sidebar.saturating_sub(38), 20), 0xFF7F8790);
    }
    let x = body.x + sidebar as i32 + 22;
    let width = body.width.saturating_sub(sidebar + 44);
    let label = |frame: &mut Surface, value: &str, y: i32, color| {
        if y + 20 <= body.y + body.height as i32 {
            text(frame, value, Rect::new(x, y, width, 20), color);
        }
    };
    label(frame, "View basic information about your computer", body.y + 52, BLUE);
    label(frame, "Windows edition", body.y + 93, BLUE);
    label(frame, "NT 6.1", body.y + 121, TEXT);
    label(frame, "Version 6.1.7601", body.y + 145, TEXT);
    label(frame, "Windows 7 SP1 compatibility project", body.y + 169, TEXT);
    if width >= 400 {
        theme::brand(frame, Rect::new(x + width as i32 - 150, body.y + 91, 145, 92));
    }
    for (title, y) in [("System", 209), ("Computer name, domain, and workgroup settings", 329), ("Windows activation", 439)] {
        label(frame, title, body.y + y, BLUE);
        frame.fill_rect(x, body.y + y + 24, width, 1, 0xFFE2E8EF);
    }
    let architecture = if cfg!(target_arch = "x86_64") { "x64" }
        else if cfg!(target_arch = "aarch64") { "ARM64" }
        else if cfg!(target_arch = "riscv64") { "RISC-V 64" } else { "LoongArch64" };
    label(frame, &format!("Processor:                 {architecture}"), body.y + 245, TEXT);
    label(frame, &cpu_count.map_or_else(|| "Logical processors:    Not reported".into(), |count| format!("Logical processors:    {count}")), body.y + 269, TEXT);
    label(frame, "System type:              64-bit Operating System", body.y + 293, TEXT);
    label(frame, "Computer name:       NT61", body.y + 365, TEXT);
    label(frame, "Workgroup:                 Not configured", body.y + 389, TEXT);
    label(frame, "Microsoft Windows activation is not applicable to NT61.", body.y + 475, TEXT);
}

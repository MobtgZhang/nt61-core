//! Application-owned icons; attribution lives beside each SVG source.
use super::AppId;
use crate::graphics::{Rect, Surface};
pub fn draw(frame: &mut Surface, id: AppId, b: Rect) {
    let (source, size) = if id == AppId::Notepad {
        notepad(b.width.max(b.height))
    } else if id == AppId::WordPad {
        wordpad(b.width.max(b.height))
    } else if id == AppId::TaskManager {
        task_manager(b.width.max(b.height))
    } else if id == AppId::Paint {
        paint(b.width.max(b.height))
    } else {
        (rgba(id), 64)
    };
    for y in 0..b.height {
        for x in 0..b.width {
            let i = (((y * size / b.height) * size + x * size / b.width) * 4) as usize;
            let p = &source[i..i + 4];
            frame.blend_pixel(
                (b.x + x as i32) as u32,
                (b.y + y as i32) as u32,
                u32::from_le_bytes([p[2], p[1], p[0], p[3]]),
            );
        }
    }
}
pub fn rgba(id: AppId) -> &'static [u8] {
    match id {
        AppId::Calculator => include_bytes!("calculator/resources/icon.rgba"),
        AppId::Paint => include_bytes!("paint/resources/icon.rgba"),
        AppId::StickyNotes => include_bytes!("sticky_notes/resources/icon.rgba"),
        AppId::WordPad => include_bytes!("wordpad/resources/icon-64.rgba"),
        AppId::CharacterMap => include_bytes!("character_map/resources/icon.rgba"),
        AppId::SystemInformation => include_bytes!("system_information/resources/icon.rgba"),
        AppId::TaskManager => include_bytes!("task_manager/resources/icon.rgba"),
        AppId::Run => include_bytes!("run/resources/icon.rgba"),
        AppId::OnScreenKeyboard => include_bytes!("on_screen_keyboard/resources/icon.rgba"),
        AppId::Magnifier => include_bytes!("magnifier/resources/icon.rgba"),
        AppId::SnippingTool => include_bytes!("snipping_tool/resources/icon.rgba"),
        AppId::Minesweeper => include_bytes!("minesweeper/resources/icon.rgba"),
        AppId::GettingStarted => include_bytes!("getting_started/resources/icon.rgba"),
        AppId::Help => include_bytes!("help/resources/icon.rgba"),
        AppId::DefaultPrograms => include_bytes!("default_programs/resources/icon.rgba"),
        AppId::GamesExplorer => include_bytes!("games_explorer/resources/icon.rgba"),
        AppId::Notepad => include_bytes!("notepad/resources/icon-64.rgba"),
        AppId::Explorer => include_bytes!("explorer/resources/icon.rgba"),
        AppId::CommandPrompt => include_bytes!("command_prompt/resources/icon.rgba"),
        AppId::ResourceMonitor => include_bytes!("resource_monitor/resources/icon.rgba"),
        AppId::EaseOfAccessCenter => include_bytes!("ease_of_access_center/resources/icon.rgba"),
        AppId::DiskPart => include_bytes!("diskpart/resources/icon.rgba"),
        AppId::BcdEdit => include_bytes!("bcdedit/resources/icon.rgba"),
    }
}
fn notepad(size: u32) -> (&'static [u8], u32) {
    match size {
        0..=16 => (include_bytes!("notepad/resources/icon-16.rgba"), 16),
        17..=24 => (include_bytes!("notepad/resources/icon-24.rgba"), 24),
        25..=32 => (include_bytes!("notepad/resources/icon-32.rgba"), 32),
        33..=48 => (include_bytes!("notepad/resources/icon-48.rgba"), 48),
        49..=64 => (include_bytes!("notepad/resources/icon-64.rgba"), 64),
        _ => (include_bytes!("notepad/resources/icon-256.rgba"), 256),
    }
}

fn wordpad(size: u32) -> (&'static [u8], u32) {
    match size {
        0..=16 => (include_bytes!("wordpad/resources/icon-16.rgba"), 16),
        17..=24 => (include_bytes!("wordpad/resources/icon-24.rgba"), 24),
        25..=32 => (include_bytes!("wordpad/resources/icon-32.rgba"), 32),
        33..=48 => (include_bytes!("wordpad/resources/icon-48.rgba"), 48),
        49..=64 => (include_bytes!("wordpad/resources/icon-64.rgba"), 64),
        _ => (include_bytes!("wordpad/resources/icon-256.rgba"), 256),
    }
}

fn paint(size: u32) -> (&'static [u8], u32) {
    match size {
        0..=16 => (include_bytes!("paint/resources/icon-16.rgba"), 16),
        17..=24 => (include_bytes!("paint/resources/icon-24.rgba"), 24),
        25..=32 => (include_bytes!("paint/resources/icon-32.rgba"), 32),
        33..=48 => (include_bytes!("paint/resources/icon-48.rgba"), 48),
        49..=64 => (include_bytes!("paint/resources/icon-64.rgba"), 64),
        _ => (include_bytes!("paint/resources/icon-256.rgba"), 256),
    }
}
fn task_manager(size: u32) -> (&'static [u8], u32) {
    match size {
        0..=16 => (include_bytes!("task_manager/resources/icon-16.rgba"), 16),
        17..=24 => (include_bytes!("task_manager/resources/icon-24.rgba"), 24),
        25..=32 => (include_bytes!("task_manager/resources/icon-32.rgba"), 32),
        33..=48 => (include_bytes!("task_manager/resources/icon-48.rgba"), 48),
        49..=64 => (include_bytes!("task_manager/resources/icon-64.rgba"), 64),
        _ => (include_bytes!("task_manager/resources/icon-256.rgba"), 256),
    }
}

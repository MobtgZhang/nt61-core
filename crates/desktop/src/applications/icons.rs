//! Retained application icons; source records live beside each application.
use super::AppId;
use crate::graphics::{Rect, Surface};
pub fn draw(frame: &mut Surface, id: AppId, b: Rect) {
    let (source, size) = sized(id, b.width.max(b.height));
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
        AppId::PhotoViewer => &super::system_icons::PHOTO_RGBA,
        AppId::DeviceManager => &super::system_icons::DEVICE_RGBA,
        AppId::MediaPlayer => &super::media_player::icon::RGBA,
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
// Select the retained raster for the requested size before any final scaling.
// In particular, caption icons must use their 16 px artwork, not a 64 px resize.
macro_rules! native_icon {
    ($directory:literal, $size:expr) => {
        match $size {
            0..=16 => (
                &include_bytes!(concat!($directory, "/resources/icon-16.rgba"))[..],
                16,
            ),
            17..=24 => (
                &include_bytes!(concat!($directory, "/resources/icon-24.rgba"))[..],
                24,
            ),
            25..=32 => (
                &include_bytes!(concat!($directory, "/resources/icon-32.rgba"))[..],
                32,
            ),
            33..=48 => (
                &include_bytes!(concat!($directory, "/resources/icon-48.rgba"))[..],
                48,
            ),
            49..=64 => (
                &include_bytes!(concat!($directory, "/resources/icon-64.rgba"))[..],
                64,
            ),
            _ => (
                &include_bytes!(concat!($directory, "/resources/icon-256.rgba"))[..],
                256,
            ),
        }
    };
}

fn sized(id: AppId, size: u32) -> (&'static [u8], u32) {
    match id {
        AppId::PhotoViewer => match size {0..=16 => (&super::system_icons::PHOTO_16,16),17..=32 => (&super::system_icons::PHOTO_32,32),_ => (&super::system_icons::PHOTO_RGBA,64)},
        AppId::DeviceManager => match size {0..=16 => (&super::system_icons::DEVICE_16,16),17..=32 => (&super::system_icons::DEVICE_32,32),_ => (&super::system_icons::DEVICE_RGBA,64)},
        AppId::MediaPlayer => (&super::media_player::icon::RGBA,64),
        AppId::Calculator => native_icon!("calculator", size),
        AppId::Paint => native_icon!("paint", size),
        AppId::StickyNotes => native_icon!("sticky_notes", size),
        AppId::WordPad => native_icon!("wordpad", size),
        AppId::CharacterMap => native_icon!("character_map", size),
        AppId::SystemInformation => native_icon!("system_information", size),
        AppId::TaskManager => native_icon!("task_manager", size),
        AppId::Run => native_icon!("run", size),
        AppId::OnScreenKeyboard => native_icon!("on_screen_keyboard", size),
        AppId::Magnifier => native_icon!("magnifier", size),
        AppId::SnippingTool => native_icon!("snipping_tool", size),
        AppId::Minesweeper => native_icon!("minesweeper", size),
        AppId::GettingStarted => native_icon!("getting_started", size),
        AppId::Help => native_icon!("help", size),
        AppId::DefaultPrograms => native_icon!("default_programs", size),
        AppId::GamesExplorer => native_icon!("games_explorer", size),
        AppId::Notepad => native_icon!("notepad", size),
        AppId::Explorer => native_icon!("explorer", size),
        AppId::CommandPrompt => native_icon!("command_prompt", size),
        AppId::ResourceMonitor => native_icon!("resource_monitor", size),
        AppId::EaseOfAccessCenter => native_icon!("ease_of_access_center", size),
        AppId::DiskPart => native_icon!("diskpart", size),
        AppId::BcdEdit => native_icon!("bcdedit", size),
    }
}

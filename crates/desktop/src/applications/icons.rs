//! Licensed GitHub icon assets. Provenance: assets/applications/sources.json.
use crate::graphics::{Rect,Surface};
use super::AppId;
pub fn draw(frame:&mut Surface,id:AppId,b:Rect){let source=rgba(id);for y in 0..b.height{for x in 0..b.width{let i=(((y*64/b.height)*64+x*64/b.width)*4)as usize;let p=&source[i..i+4];frame.blend_pixel((b.x+x as i32)as u32,(b.y+y as i32)as u32,u32::from_le_bytes([p[2],p[1],p[0],p[3]]));}}}
pub fn rgba(id:AppId)->&'static [u8]{match id{
    AppId::Calculator=>include_bytes!("../../assets/applications/calculator.rgba"),
    AppId::Paint=>include_bytes!("../../assets/applications/paint.rgba"),
    AppId::StickyNotes=>include_bytes!("../../assets/applications/notes.rgba"),
    AppId::WordPad|AppId::Notepad=>include_bytes!("../../assets/applications/editor.rgba"),
    AppId::CharacterMap=>include_bytes!("../../assets/applications/characters.rgba"),
    AppId::SystemInformation=>include_bytes!("../../assets/applications/information.rgba"),
    AppId::TaskManager|AppId::ResourceMonitor=>include_bytes!("../../assets/applications/tasks.rgba"),
    AppId::Run|AppId::CommandPrompt|AppId::BcdEdit|AppId::DiskPart=>include_bytes!("../../assets/applications/run.rgba"),
    AppId::OnScreenKeyboard=>include_bytes!("../../assets/applications/keyboard.rgba"),
    AppId::Magnifier=>include_bytes!("../../assets/applications/magnifier.rgba"),
    AppId::SnippingTool=>include_bytes!("../../assets/applications/screenshot.rgba"),
    AppId::Minesweeper=>include_bytes!("../../assets/applications/mines.rgba"),
    AppId::DefaultPrograms|AppId::EaseOfAccessCenter=>include_bytes!("../../assets/applications/settings.rgba"),
    AppId::GamesExplorer=>include_bytes!("../../assets/applications/games.rgba"),
    AppId::Explorer=>include_bytes!("../../assets/applications/folder.rgba"),
    _=>include_bytes!("../../assets/applications/help.rgba"),
}}

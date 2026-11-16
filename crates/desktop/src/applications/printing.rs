//! File printer: bounded PostScript output in the kernel's system-volume mirror.
use super::{common_dialogs::PrintDialog, AppState, ApplicationSession};
use alloc::{format, string::String, vec::Vec};
use core::fmt::Write;
const LIMIT: usize = 1024 * 1024;
fn literal(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(char::from(byte));
            }
            32..=126 => out.push(char::from(byte)),
            _ => {
                let _ = write!(out, "\\{byte:03o}");
            }
        }
    }
    out
}
pub fn document(app: &ApplicationSession, options: &PrintDialog) -> Result<Vec<u8>, String> {
    if !options.valid_options() {
        return Err("Invalid print options.".into());
    }
    let page = &app.notepad.page;
    let (mut width, mut height) = if page.letter {
        (612.0, 792.0)
    } else {
        (595.28, 841.89)
    };
    if page.landscape {
        core::mem::swap(&mut width, &mut height);
    }
    let margins: Vec<f32> = page
        .margins
        .iter()
        .map(|s| s.parse::<f32>().unwrap_or(20.0) * 72.0 / 25.4)
        .collect();
    let copies = options.copies.parse::<usize>().unwrap_or(1);
    let mut pages: Vec<String> = Vec::new();
    match &app.state {
        AppState::Document(d) => {
            let text = if options.selection {
                let Some((a, b)) = d.selection() else {
                    return Err("There is no selected text.".into());
                };
                &d.editor.text[a..b]
            } else {
                d.editor.text.as_str()
            };
            let size = app.notepad.font.points.clamp(1, 144) as f32;
            let lines = ((height - margins[2] - margins[3]) / (size * 1.2)) as usize;
            let columns = ((width - margins[0] - margins[1]) / (size * 0.6)) as usize;
            if lines == 0 || columns == 0 {
                return Err("Margins leave no printable area.".into());
            }
            let mut rows: Vec<String> = Vec::new();
            for line in text.split('\n') {
                let chars: Vec<char> = line.trim_end_matches('\r').chars().collect();
                if chars.is_empty() {
                    rows.push(String::new());
                } else {
                    for part in chars.chunks(columns) {
                        rows.push(part.iter().collect());
                    }
                }
            }
            for chunk in rows.chunks(lines) {
                let mut content = format!("/Courier findfont {size:.2} scalefont setfont\n");
                for (i, line) in chunk.iter().enumerate() {
                    let _ = writeln!(
                        content,
                        "{:.2} {:.2} moveto ({}) show",
                        margins[0],
                        height - margins[2] - size - i as f32 * size * 1.2,
                        literal(line)
                    );
                }
                pages.push(content);
            }
        }
        AppState::Paint(p) => {
            if p.canvas.pixels().len() * 6 > LIMIT - 4096 {
                return Err("Image exceeds the file printer's 1 MiB job limit.".into());
            }
            let scale = ((width - margins[0] - margins[1]) / p.canvas.width() as f32)
                .min((height - margins[2] - margins[3]) / p.canvas.height() as f32);
            let mut content=format!("gsave {:.2} {:.2} translate {:.2} {:.2} scale\n{} {} 8 [{} 0 0 -{} 0 {}]\n{{currentfile /ASCIIHexDecode filter}} false 3 colorimage\n",margins[0],height-margins[2]-p.canvas.height()as f32*scale,p.canvas.width()as f32*scale,p.canvas.height()as f32*scale,p.canvas.width(),p.canvas.height(),p.canvas.width(),p.canvas.height(),p.canvas.height());
            for (i, pixel) in p.canvas.pixels().iter().enumerate() {
                let _ = write!(content, "{:06x}", pixel & 0xffffff);
                if i % 24 == 23 {
                    content.push('\n');
                }
            }
            content.push_str(">\ngrestore\n");
            pages.push(content);
        }
        _ => return Err("This application has no printable document.".into()),
    }
    let selected: Vec<usize> = (0..pages.len())
        .filter(|i| {
            !options.range
                || (*i + 1 >= options.from.parse().unwrap_or(1)
                    && *i < options.to.parse().unwrap_or(1))
        })
        .collect();
    if selected.is_empty() {
        return Err("The page range contains no document pages.".into());
    }
    let mut output=format!("%!PS-Adobe-3.0\n%%Creator: NT61 PostScript Document Writer\n%%Pages: {}\n%%EndComments\n<< /PageSize [{width:.2} {height:.2}] >> setpagedevice\n",selected.len()*copies);
    let mut count = 0;
    for outer in 0..if options.collate {
        copies
    } else {
        selected.len()
    } {
        for inner in 0..if options.collate {
            selected.len()
        } else {
            copies
        } {
            let index = selected[if options.collate { inner } else { outer }];
            count += 1;
            let _ = writeln!(
                output,
                "%%Page: {count} {count}\ngsave\n{}grestore\nshowpage",
                pages[index]
            );
            if output.len() > LIMIT - 64 {
                return Err("Print job exceeds 1 MiB.".into());
            }
        }
    }
    output.push_str("%%Trailer\n%%EOF\n");
    Ok(output.into_bytes())
}
pub fn print(app: &ApplicationSession, options: &PrintDialog) -> Result<u32, String> {
    let data = document(app, options)?;
    let bytes =
        crate::platform::services::transfer(nt61_abi::dialog_bridge::PRINT_WRITE, "", &data)
            .map_err(|s| format!("Printing failed (0x{s:08X})."))?;
    bytes
        .as_slice()
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(|_| "Invalid print backend response.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::applications::{
        common_dialogs::{self, Dialog},
        AppId,
    };
    #[test]
    fn real_postscript_pages_ranges_and_copies() {
        let mut app = ApplicationSession::new(1, AppId::Notepad, 0);
        app.paste("Hello (printer)\\\nsecond line");
        common_dialogs::open_print(&mut app);
        let Some(Dialog::Print(mut d)) = app.common_dialog.clone() else {
            panic!()
        };
        d.copies = "2".into();
        d.collate = true;
        let ps = String::from_utf8(document(&app, &d).unwrap()).unwrap();
        assert!(ps.starts_with("%!PS-Adobe-3.0"));
        assert!(ps.contains("%%Pages: 2"));
        assert!(ps.contains("Hello \\(printer\\)\\\\"));
        assert!(ps.ends_with("%%EOF\n"));
        d.range = true;
        d.from = "8".into();
        d.to = "9".into();
        assert!(document(&app, &d).is_err());
    }
}

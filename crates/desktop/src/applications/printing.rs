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
        AppState::Paint(_) | AppState::PhotoViewer(_) => {
            let image = match &app.state {
                AppState::Paint(p) => &p.canvas,
                AppState::PhotoViewer(v) => v.image.as_ref().ok_or("No picture is loaded.")?,
                _ => unreachable!(),
            };
            let available = (width - margins[0] - margins[1], height - margins[2] - margins[3]);
            if available.0 <= 0.0 || available.1 <= 0.0 {return Err("Margins leave no printable area.".into());}
            // Resample the print raster to fit the bounded spool protocol. Keep the
            // original aspect ratio and retain the full in-memory source picture.
            let mut rw=image.width();let mut rh=image.height();
            while rw as usize * rh as usize * 6 > (LIMIT-8192)/copies.max(1) {
                rw=(rw*9/10).max(1);rh=(rh*9/10).max(1);
            }
            let scale=(available.0 / image.width() as f32).min(available.1 / image.height() as f32);
            let dw=image.width() as f32*scale;let dh=image.height()as f32*scale;
            let x=margins[0]+(available.0-dw)/2.0;let y=margins[3]+(available.1-dh)/2.0;
            let mut content=format!("gsave {x:.2} {y:.2} translate {dw:.2} {dh:.2} scale\n{rw} {rh} 8 [{rw} 0 0 -{rh} 0 {rh}]\n{{currentfile /ASCIIHexDecode filter}} false 3 colorimage\n");
            for y in 0..rh { for x in 0..rw {
                let p=image.pixel((x as u64*image.width() as u64/rw as u64)as u32,(y as u64*image.height()as u64/rh as u64)as u32).unwrap_or(0);
                let alpha=p>>24;
                let channel=|shift:u32| (((p>>shift)&255u32)*alpha+255*(255-alpha)+127)/255;
                let _=write!(content,"{:02x}{:02x}{:02x}",channel(16),channel(8),channel(0));
                if x%24==23 {content.push('\n');}
            }}
            content.push_str(">\ngrestore\n");pages.push(content);
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
    fn photo_print_raster_flattens_alpha_on_paper_and_rejects_empty_images() {
        let mut app=ApplicationSession::new(1,AppId::PhotoViewer,0);
        common_dialogs::open_print(&mut app);
        let Some(Dialog::Print(d))=app.common_dialog.clone()else{panic!()};
        assert!(document(&app,&d).is_err());
        let AppState::PhotoViewer(v)=&mut app.state else{panic!()};
        let mut image=crate::graphics::Surface::new(2,1);
        image.set_pixel(0,0,0x00ff0000);image.set_pixel(1,0,0xff123456);v.image=Some(image);
        let ps=String::from_utf8(document(&app,&d).unwrap()).unwrap();
        assert!(ps.contains("2 1 8 [2 0 0 -1 0 1]"));
        assert!(ps.contains("ffffff123456>"));
        assert!(ps.contains("%%Pages: 1"));
    }
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

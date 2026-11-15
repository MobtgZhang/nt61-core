use super::{Format, Imported, RichText, Style, MAX_DOCUMENT};
use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
#[derive(Clone, Copy)]
struct Group {
    style: Style,
    skip: bool,
    uc: usize,
    colors: bool,
    numbering: bool,
}
pub(super) fn read(bytes: &[u8]) -> Result<Imported, &'static str> {
    let mut g = Group {
        style: Style::default(),
        skip: false,
        uc: 1,
        colors: false,
        numbering: false,
    };
    let mut stack = Vec::new();
    let (mut i, mut fallback) = (0, 0usize);
    let mut units = Vec::<(u16, Style)>::new();
    let mut colors = Vec::new();
    let mut rgb = [0u32; 3];
    let mut color_set = false;
    while i < bytes.len() {
        let mut emit = None;
        match bytes[i] {
            b'{' => {
                if stack.len() >= 128 {
                    return Err("RTF nesting is too deep.");
                }
                stack.push(g);
                i += 1;
            }
            b'}' => {
                let numbering = g.numbering;
                let bullet = g.style.bullet;
                g = stack.pop().ok_or("Unbalanced RTF groups.")?;
                if numbering {
                    g.style.bullet = bullet;
                }
                i += 1;
                if stack.is_empty() && bytes[i..].iter().any(|b| !b.is_ascii_whitespace()) {
                    return Err("Unexpected data after RTF document.");
                }
            }
            b'\\' => {
                i += 1;
                let symbol = *bytes.get(i).ok_or("Truncated RTF control.")?;
                if matches!(symbol, b'\\' | b'{' | b'}' | b'~' | b'-' | b'_') {
                    emit = Some(match symbol {
                        b'~' => 0xa0,
                        b'-' => 0xad,
                        b'_' => 0x2011,
                        _ => symbol as u16,
                    });
                    i += 1;
                } else if symbol == b'*' {
                    g.skip = true;
                    i += 1;
                } else if symbol == b'\'' {
                    let hex =
                        core::str::from_utf8(bytes.get(i + 1..i + 3).ok_or("Truncated RTF byte.")?)
                            .map_err(|_| "Invalid RTF byte.")?;
                    emit = Some(super::super::super::document::cp1252(
                        u8::from_str_radix(hex, 16).map_err(|_| "Invalid RTF byte.")?,
                    ));
                    i += 3;
                } else {
                    let start = i;
                    while bytes.get(i).is_some_and(u8::is_ascii_alphabetic) {
                        i += 1;
                    }
                    if start == i {
                        i += 1;
                        continue;
                    }
                    let word = &bytes[start..i];
                    let start = i;
                    if bytes.get(i) == Some(&b'-') {
                        i += 1;
                    }
                    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    let n = if start < i {
                        Some(
                            core::str::from_utf8(&bytes[start..i])
                                .ok()
                                .and_then(|s| s.parse::<i32>().ok())
                                .ok_or("Invalid RTF argument.")?,
                        )
                    } else {
                        None
                    };
                    if bytes.get(i) == Some(&b' ') {
                        i += 1;
                    }
                    if word == b"bin" {
                        let len = usize::try_from(n.ok_or("Missing binary length.")?)
                            .map_err(|_| "Invalid binary length.")?;
                        i = i
                            .checked_add(len)
                            .filter(|n| *n <= bytes.len())
                            .ok_or("Truncated RTF binary data.")?;
                        continue;
                    }
                    if word == b"colortbl" {
                        g.colors = true;
                        g.skip = true;
                    }
                    if g.colors {
                        match word {
                            b"red" => {
                                rgb[0] = n.unwrap_or(0).clamp(0, 255) as u32;
                                color_set = true;
                            }
                            b"green" => {
                                rgb[1] = n.unwrap_or(0).clamp(0, 255) as u32;
                                color_set = true;
                            }
                            b"blue" => {
                                rgb[2] = n.unwrap_or(0).clamp(0, 255) as u32;
                                color_set = true;
                            }
                            _ => {}
                        }
                    }
                    if matches!(
                        word,
                        b"fonttbl"
                            | b"stylesheet"
                            | b"info"
                            | b"pict"
                            | b"object"
                            | b"header"
                            | b"footer"
                            | b"listtable"
                            | b"listoverridetable"
                            | b"fldinst"
                            | b"pntext"
                            | b"listtext"
                    ) {
                        g.skip = true;
                    }
                    // \pn is an ignorable destination, but its paragraph property is retained.
                    if word == b"pn" {
                        g.numbering = true;
                    }
                    if g.numbering && word == b"pnlvlblt" {
                        g.style.bullet = true;
                    }
                    if g.skip {
                        continue;
                    }
                    match word {
                        b"uc" => {
                            g.uc = usize::try_from(n.unwrap_or(1))
                                .ok()
                                .filter(|n| *n <= 16)
                                .ok_or("Invalid Unicode fallback length.")?
                        }
                        b"u" => {
                            let n = n.ok_or("Missing Unicode value.")?;
                            if !(-32768..=65535).contains(&n) {
                                return Err("Invalid Unicode value.");
                            }
                            units.push((n as u16, g.style));
                            fallback = g.uc;
                        }
                        b"par" | b"line" => emit = Some(10),
                        b"tab" => emit = Some(9),
                        b"b" => g.style.bold = n != Some(0),
                        b"i" => g.style.italic = n != Some(0),
                        b"ul" => g.style.underline = n != Some(0),
                        b"ulnone" => g.style.underline = false,
                        b"strike" => g.style.strike = n != Some(0),
                        b"fs" => g.style.size = n.unwrap_or(22).clamp(8, 288) as u16,
                        b"f" => g.style.mono = n == Some(1),
                        b"cf" => {
                            g.style.color = colors
                                .get(n.unwrap_or(0).max(0) as usize)
                                .copied()
                                .unwrap_or(0xff000000)
                        }
                        b"highlight" => {
                            g.style.highlight = if n == Some(0) {
                                0
                            } else {
                                colors
                                    .get(n.unwrap_or(0).max(0) as usize)
                                    .copied()
                                    .unwrap_or(0)
                            }
                        }
                        b"ql" => g.style.align = 0,
                        b"qc" => g.style.align = 1,
                        b"qr" => g.style.align = 2,
                        b"qj" => g.style.align = 3,
                        b"li" => g.style.indent = n.unwrap_or(0).clamp(0, 7200) as u16,
                        b"plain" => {
                            let s = g.style;
                            g.style = Style {
                                align: s.align,
                                bullet: s.bullet,
                                indent: s.indent,
                                ..Style::default()
                            };
                        }
                        b"pard" => {
                            g.style.align = 0;
                            g.style.bullet = false;
                            g.style.indent = 0;
                        }
                        b"bullet" => emit = Some(0x2022),
                        b"emdash" => emit = Some(0x2014),
                        b"endash" => emit = Some(0x2013),
                        _ => {}
                    }
                }
            }
            b'\r' | b'\n' => i += 1,
            b';' if g.colors => {
                colors.push(if color_set {
                    0xff000000 | rgb[0] << 16 | rgb[1] << 8 | rgb[2]
                } else {
                    0xff000000
                });
                rgb = [0; 3];
                color_set = false;
                i += 1;
            }
            b => {
                emit = Some(super::super::super::document::cp1252(b));
                i += 1;
            }
        }
        if let Some(u) = emit {
            if fallback > 0 {
                fallback -= 1;
            } else if !g.skip {
                units.push((u, g.style));
            }
        }
        if units.len() > MAX_DOCUMENT {
            return Err("RTF text is too large.");
        }
    }
    if !stack.is_empty() {
        return Err("Unbalanced RTF groups.");
    }
    let mut out = Imported::new(Format::Rtf);
    let mut at = 0;
    while at < units.len() {
        let (u, s) = units[at];
        let ch = if (0xd800..=0xdbff).contains(&u) {
            let low = units.get(at + 1).ok_or("Invalid RTF Unicode.")?.0;
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err("Invalid RTF Unicode.");
            }
            at += 1;
            char::from_u32(0x10000 + ((u as u32 - 0xd800) << 10) + (low as u32 - 0xdc00))
        } else {
            char::from_u32(u as u32)
        }
        .ok_or("Invalid RTF Unicode.")?;
        out.push(ch, s)?;
        at += 1;
    }
    out.rich.typing = out.rich.styles.last().copied().unwrap_or_default();
    if bytes.windows(5).any(|w| w == b"\\pict") || bytes.windows(7).any(|w| w == b"\\object") {
        out.rich.warning = "Embedded pictures and objects were omitted on import.".into();
    }
    Ok(out)
}
pub(super) fn write(text: &str, rich: &RichText) -> Vec<u8> {
    let mut colors = alloc::vec![0xff000000];
    let mut indices = BTreeMap::new();
    indices.insert(0xff000000, 1usize);
    for s in &rich.styles {
        for c in [s.color, s.highlight] {
            if c != 0 && !indices.contains_key(&c) {
                colors.push(c);
                indices.insert(c, colors.len());
            }
        }
    }
    let mut out=String::from("{\\rtf1\\ansi\\ansicpg1252\\uc1\\deff0{\\fonttbl{\\f0 DejaVu Sans;}{\\f1 DejaVu Sans Mono;}}{\\colortbl;");
    for c in &colors {
        out.push_str(&format!(
            "\\red{}\\green{}\\blue{};",
            (c >> 16) & 255,
            (c >> 8) & 255,
            c & 255
        ));
    }
    out.push('}');
    let mut previous = None;
    let mut paragraph_start = true;
    for (index, ch) in text.chars().enumerate() {
        let s = rich.style(index);
        if paragraph_start {
            out.push_str("\\pard");
            if s.bullet {
                out.push_str("{\\*\\pn\\pnlvlblt\\pnf0\\pnindent360{\\pntxtb\\u8226?}}{\\pntext\\u8226?\\tab}");
            }
            paragraph_start = false;
            previous = None;
        }
        if previous != Some(s) {
            out.push_str(&format!(
                "\\b{}\\i{}\\ul{}\\strike{}\\fs{}\\f{}\\cf{}\\highlight{}\\{}\\li{} ",
                u8::from(s.bold),
                u8::from(s.italic),
                u8::from(s.underline),
                u8::from(s.strike),
                s.size,
                u8::from(s.mono),
                indices.get(&s.color).copied().unwrap_or(1),
                if s.highlight == 0 {
                    0
                } else {
                    indices.get(&s.highlight).copied().unwrap_or(1)
                },
                ["ql", "qc", "qr", "qj"][s.align.min(3) as usize],
                s.indent
            ));
            previous = Some(s);
        }
        match ch {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => {
                out.push_str("\\par\n");
                paragraph_start = true;
            }
            '\t' => out.push_str("\\tab "),
            c if c.is_ascii() => out.push(c),
            c => {
                let mut u = [0; 2];
                for unit in c.encode_utf16(&mut u) {
                    out.push_str(&format!("\\u{}?", *unit as i16));
                }
            }
        }
    }
    out.push('}');
    out.into_bytes()
}

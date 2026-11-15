use super::{package, Format, Imported, RichText, Style};
use alloc::{format, string::String, vec::Vec};
use roxmltree::{Document, Node};
fn parse(xml: &str) -> Result<Document<'_>, roxmltree::Error> {
    Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            nodes_limit: 100_000,
            ..roxmltree::ParsingOptions::default()
        },
    )
}
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
fn is(n: Node, name: &str) -> bool {
    n.is_element()
        && n.tag_name().name() == name
        && matches!(n.tag_name().namespace(), Some(W) | Some(STRICT))
}
fn attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attribute((W, name))
        .or_else(|| n.attribute((STRICT, name)))
}
fn child<'a, 'b>(n: Node<'a, 'b>, name: &str) -> Option<Node<'a, 'b>> {
    n.children().find(|n| is(*n, name))
}
fn flag(n: Node) -> bool {
    !matches!(attr(n, "val"), Some("0" | "false" | "off" | "none"))
}
fn color(v: &str) -> Option<u32> {
    (v.len() == 6)
        .then(|| u32::from_str_radix(v, 16).ok())
        .flatten()
        .map(|c| c | 0xff000000)
}
fn props(n: Node, s: &mut Style) {
    for c in n
        .children()
        .filter(|c| c.is_element() && matches!(c.tag_name().namespace(), Some(W) | Some(STRICT)))
    {
        let v = attr(c, "val").unwrap_or("");
        match c.tag_name().name() {
            "b" => s.bold = flag(c),
            "i" => s.italic = flag(c),
            "u" => s.underline = flag(c),
            "strike" | "dstrike" => s.strike = flag(c),
            "sz" => {
                if let Ok(n) = v.parse::<u16>() {
                    s.size = n.clamp(8, 288);
                }
            }
            "rFonts" => {
                s.mono = attr(c, "ascii").is_some_and(|n| {
                    n.contains("Mono") || n.contains("Courier") || n.contains("Consolas")
                })
            }
            "color" => s.color = color(v).unwrap_or(0xff000000),
            "highlight" => {
                s.highlight = match v {
                    "yellow" => 0xffffff00,
                    "green" => 0xff00ff00,
                    "cyan" => 0xff00ffff,
                    "magenta" => 0xffff00ff,
                    "blue" => 0xff0000ff,
                    "red" => 0xffff0000,
                    _ => 0,
                }
            }
            "shd" => s.highlight = attr(c, "fill").and_then(color).unwrap_or(0),
            "jc" => {
                s.align = match v {
                    "center" => 1,
                    "right" | "end" => 2,
                    "both" => 3,
                    _ => 0,
                }
            }
            "ind" => {
                s.indent = attr(c, "left")
                    .or_else(|| attr(c, "start"))
                    .and_then(|v| v.parse::<u16>().ok())
                    .unwrap_or(0)
                    .min(7200)
            }
            "numPr" => s.bullet = true,
            _ => {}
        }
    }
}
fn style_by_id(styles: Option<&Document>, id: &str, s: &mut Style, depth: u8) {
    if depth >= 16 {
        return;
    }
    let Some(doc) = styles else {
        return;
    };
    if let Some(n) = doc
        .descendants()
        .find(|n| is(*n, "style") && attr(*n, "styleId") == Some(id))
    {
        if let Some(base) = child(n, "basedOn").and_then(|n| attr(n, "val")) {
            style_by_id(styles, base, s, depth + 1);
        }
        if let Some(p) = child(n, "pPr") {
            props(p, s);
        }
        if let Some(r) = child(n, "rPr") {
            props(r, s);
        }
    }
}
pub(super) fn read(bytes: &[u8]) -> Result<Imported, &'static str> {
    let relationships = package::part(bytes, "_rels/.rels")?;
    let rel_xml =
        core::str::from_utf8(&relationships).map_err(|_| "Invalid package relationships.")?;
    let rel = parse(rel_xml).map_err(|_| "Invalid package relationships.")?;
    let target = rel
        .descendants()
        .find(|n| {
            n.tag_name().name() == "Relationship"
                && n.attribute("Type")
                    .is_some_and(|t| t.ends_with("/officeDocument"))
                && n.attribute("TargetMode") != Some("External")
        })
        .and_then(|n| n.attribute("Target"))
        .ok_or("Missing main document relationship.")?;
    let target = target.trim_start_matches('/');
    if target.split('/').any(|v| v == "..") {
        return Err("Invalid main document path.");
    }
    let data = package::part(bytes, target)?;
    let xml = core::str::from_utf8(&data).map_err(|_| "Invalid DOCX XML encoding.")?;
    let doc = parse(xml).map_err(|_| "Invalid DOCX XML.")?;
    let body = doc
        .descendants()
        .find(|n| is(*n, "body"))
        .ok_or("DOCX body is missing.")?;
    let style_path = format!(
        "{}styles.xml",
        target
            .rsplit_once('/')
            .map_or("", |(a, _)| &target[..a.len() + 1])
    );
    let style_data = package::part(bytes, &style_path).ok();
    let styles = style_data
        .as_ref()
        .and_then(|b| core::str::from_utf8(b).ok())
        .and_then(|s| parse(s).ok());
    let mut default = Style::default();
    if let Some(d) = styles.as_ref() {
        if let Some(n) = d.descendants().find(|n| is(*n, "docDefaults")) {
            for p in n.descendants().filter(|n| is(*n, "rPr") || is(*n, "pPr")) {
                props(p, &mut default);
            }
        }
        style_by_id(Some(d), "Normal", &mut default, 0);
    }
    let mut out = Imported::new(Format::Docx);
    let mut first = true;
    let mut terminator = default;
    for p in body.descendants().filter(|n| is(*n, "p")) {
        let mut para = default;
        if let Some(pr) = child(p, "pPr") {
            if let Some(id) = child(pr, "pStyle").and_then(|n| attr(n, "val")) {
                style_by_id(styles.as_ref(), id, &mut para, 0);
            }
            props(pr, &mut para);
        }
        if !first {
            out.push('\n', terminator)?;
        }
        first = false;
        for run in p
            .descendants()
            .filter(|n| is(*n, "r") && !n.ancestors().any(|a| is(a, "del")))
        {
            let mut s = para;
            if let Some(pr) = child(run, "rPr") {
                if let Some(id) = child(pr, "rStyle").and_then(|n| attr(n, "val")) {
                    style_by_id(styles.as_ref(), id, &mut s, 0);
                }
                props(pr, &mut s);
            }
            for n in run.children() {
                if is(n, "t") {
                    for ch in n.text().unwrap_or("").chars() {
                        out.push(ch, s)?;
                    }
                } else if is(n, "tab") {
                    out.push('\t', s)?;
                } else if is(n, "br") || is(n, "cr") {
                    out.push('\n', s)?;
                }
            }
        }
        terminator = out.rich.styles.last().copied().unwrap_or(para);
        terminator.align = para.align;
        terminator.bullet = para.bullet;
        terminator.indent = para.indent;
    }
    out.rich.typing = out.rich.styles.last().copied().unwrap_or(default);
    if body.descendants().any(|n| {
        ["drawing", "object", "pict", "tbl"]
            .iter()
            .any(|t| is(n, t))
    }) {
        out.rich.warning = "Pictures/objects omitted; table text imported as paragraphs.".into();
    }
    Ok(out)
}
fn rpr(s: Style) -> String {
    let family = if s.mono {
        "DejaVu Sans Mono"
    } else {
        "DejaVu Sans"
    };
    let mut out = format!(
        concat!(
            "<w:rPr><w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/>",
            "<w:sz w:val=\"{}\"/><w:color w:val=\"{:06X}\"/>"
        ),
        family,
        family,
        s.size,
        s.color & 0xffffff,
    );
    for (on, tag) in [
        (s.bold, "b"),
        (s.italic, "i"),
        (s.underline, "u"),
        (s.strike, "strike"),
    ] {
        if on {
            out.push_str(&format!(
                "<w:{tag} w:val=\"{}\"/>",
                if tag == "u" { "single" } else { "1" }
            ));
        }
    }
    if s.highlight != 0 {
        out.push_str(&format!(
            "<w:shd w:fill=\"{:06X}\"/>",
            s.highlight & 0xffffff
        ));
    }
    out.push_str("</w:rPr>");
    out
}
pub(super) fn write(text: &str, rich: &RichText) -> Vec<u8> {
    let mut xml =
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{W}\"><w:body>");
    let mut index = 0;
    for p in text.split('\n') {
        let s = rich.style(index);
        xml.push_str(&format!(
            "<w:p><w:pPr><w:jc w:val=\"{}\"/><w:ind w:left=\"{}\"/>{}</w:pPr>",
            ["left", "center", "right", "both"][s.align.min(3) as usize],
            s.indent,
            if s.bullet {
                "<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>"
            } else {
                ""
            }
        ));
        let mut run = String::new();
        let mut style = None;
        for ch in p.chars() {
            let s = rich.style(index);
            index += 1;
            if style != Some(s) {
                if let Some(old) = style {
                    xml.push_str(&format!(
                        "<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                        rpr(old),
                        package::xml_text(&run)
                    ));
                    run.clear();
                }
                style = Some(s);
            }
            if ch == '\t' {
                if !run.is_empty() {
                    xml.push_str(&format!(
                        "<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                        rpr(s),
                        package::xml_text(&run)
                    ));
                    run.clear();
                }
                xml.push_str(&format!("<w:r>{}<w:tab/></w:r>", rpr(s)));
            } else {
                run.push(ch);
            }
        }
        if let Some(s) = style {
            if !run.is_empty() {
                xml.push_str(&format!(
                    "<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                    rpr(s),
                    package::xml_text(&run)
                ));
            }
        }
        xml.push_str("</w:p>");
        index += 1;
    }
    xml.push_str("<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr></w:body></w:document>");
    let content_types = br#"<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
</Types>"#;
    let root_rels = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;
    let document_rels = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdNumbering" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
</Relationships>"#;
    let numbering = format!(
        r#"<?xml version="1.0"?>
<w:numbering xmlns:w="{W}">
<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="singleLevel"/>
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/>
<w:lvlJc w:val="left"/><w:pPr><w:tabs><w:tab w:val="num" w:pos="360"/></w:tabs><w:ind w:left="360" w:hanging="360"/></w:pPr>
<w:rPr><w:rFonts w:ascii="DejaVu Sans" w:hAnsi="DejaVu Sans"/></w:rPr></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    package::write(&[
        ("[Content_Types].xml", content_types.to_vec()),
        ("_rels/.rels", root_rels.to_vec()),
        ("word/document.xml", xml.into_bytes()),
        ("word/_rels/document.xml.rels", document_rels.to_vec()),
        ("word/numbering.xml", numbering.into_bytes()),
    ])
}

//! Rich document interchange. No filesystem, platform calls or executable objects.
//! ZIP inflation and XML parsing use miniz_oxide and roxmltree, respectively.
use alloc::{string::String, vec::Vec};
mod docx;
mod legacy;
pub mod package;
mod rtf;
pub const MAX_DOCUMENT: usize = 128 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rtf,
    Docx,
    Text,
    LegacyDoc,
}
impl Format {
    pub fn for_name(name: &str) -> Result<Self, &'static str> {
        match name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "rtf" => Ok(Self::Rtf),
            "docx" => Ok(Self::Docx),
            "doc" => Ok(Self::LegacyDoc),
            "txt" => Ok(Self::Text),
            _ => Err("Choose an RTF, DOCX or text filename."),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub size: u16,
    pub color: u32,
    pub highlight: u32,
    pub mono: bool,
    /// 0 left, 1 center, 2 right, 3 justify.
    pub align: u8,
    pub bullet: bool,
    pub indent: u16,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            size: 22,
            color: 0xff000000,
            highlight: 0,
            mono: false,
            align: 0,
            bullet: false,
            indent: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichText {
    /// One style per Unicode scalar, including paragraph separators.
    pub styles: Vec<Style>,
    pub typing: Style,
    pub format: Format,
    pub warning: String,
}
impl Default for RichText {
    fn default() -> Self {
        Self {
            styles: Vec::new(),
            typing: Style::default(),
            format: Format::Rtf,
            warning: String::new(),
        }
    }
}
impl RichText {
    pub fn style(&self, index: usize) -> Style {
        self.styles.get(index).copied().unwrap_or(self.typing)
    }
    pub fn splice(&mut self, start: usize, end: usize, count: usize) {
        let start = start.min(self.styles.len());
        let end = end.min(self.styles.len()).max(start);
        self.styles
            .splice(start..end, core::iter::repeat(self.typing).take(count));
    }
    pub fn encode(&self, text: &str, format: Format) -> Result<Vec<u8>, &'static str> {
        match format {
            Format::Rtf => Ok(rtf::write(text, self)),
            Format::Docx => {
                let mut estimated = 1000usize;
                let mut previous = None;
                for (index, ch) in text.chars().enumerate() {
                    let s = self.style(index);
                    estimated += if previous != Some(s) || ch == '\n' {
                        500
                    } else {
                        6
                    };
                    previous = Some(s);
                    if estimated > 1024 * 1024 {
                        return Err("DOCX has too many formatting runs. Save as RTF instead.");
                    }
                }
                Ok(docx::write(text, self))
            }
            Format::Text => Ok(text.replace('\n', "\r\n").into_bytes()),
            Format::LegacyDoc => {
                Err("Binary DOC saving is unavailable. Use Save As with RTF or DOCX.")
            }
        }
    }
}
pub struct Imported {
    pub text: String,
    pub rich: RichText,
}
impl Imported {
    fn new(format: Format) -> Self {
        Self {
            text: String::new(),
            rich: RichText {
                format,
                ..RichText::default()
            },
        }
    }
    fn push(&mut self, ch: char, style: Style) -> Result<(), &'static str> {
        if self.text.len().saturating_add(ch.len_utf8()) > MAX_DOCUMENT {
            return Err("Document exceeds the supported text size.");
        }
        self.text.push(ch);
        self.rich.styles.push(style);
        Ok(())
    }
}
pub fn decode(bytes: &[u8]) -> Result<Imported, &'static str> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Document exceeds the supported file size (16 MiB).");
    }
    if bytes.starts_with(b"{\\rtf") {
        rtf::read(bytes)
    } else if bytes.starts_with(b"PK\x03\x04") {
        docx::read(bytes)
    } else if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        legacy::read(bytes)
    } else {
        let text = crate::applications::document::decode_text(bytes)?;
        if text.len() > MAX_DOCUMENT {
            return Err("Document is too large.");
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let styles = alloc::vec![Style::default();text.chars().count()];
        Ok(Imported {
            text,
            rich: RichText {
                styles,
                format: Format::Text,
                ..RichText::default()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_word97_piece_table_import_and_encryption_rejection() {
        let bytes = include_bytes!("../../../../tests/fixtures/wordpad/word97.doc");
        let d = decode(bytes).unwrap();
        assert_eq!(d.text, "This is a simple file created with Word 97-SR2.");
        assert_eq!(d.rich.format, Format::LegacyDoc);
        assert!(!d.rich.warning.is_empty());
        assert!(d.rich.encode(&d.text, Format::LegacyDoc).is_err());
        for length in [0, 20, 512, 1024, bytes.len() / 2] {
            assert!(legacy::read(&bytes[..length]).is_err());
        }
    }
    #[test]
    fn independent_deflated_docx_styles_unicode_and_xml_escaping() {
        let d = decode(include_bytes!(
            "../../../../tests/fixtures/wordpad/deflated.docx"
        ))
        .unwrap();
        assert_eq!(d.text, "WordPad & <XML> café 中😀\nSecond\tparagraph");
        assert!(d.rich.style(0).bold);
        assert_eq!(d.rich.style(0).size, 36);
        assert_eq!(d.rich.style(0).color, 0xff2058b0);
        assert_eq!(d.rich.style(0).align, 1);
        let index = d.text.chars().position(|c| c == 'é').unwrap();
        assert!(d.rich.style(index).italic && d.rich.style(index).underline);
        for format in [Format::Docx, Format::Rtf] {
            let saved = d.rich.encode(&d.text, format).unwrap();
            let reopened = decode(&saved).unwrap();
            assert_eq!(reopened.text, d.text);
            assert_eq!(reopened.rich.styles, d.rich.styles);
        }
    }
    #[test]
    fn scoped_rtf_formatting_and_surrogate_unicode() {
        let d =
            decode(br"{\rtf1\ansi\uc1 plain {\b bold {\i both}} plain \u20013?\u-10179?\u-8704?}")
                .unwrap();
        assert_eq!(d.text, "plain bold both plain 中😀");
        assert!(!d.rich.style(0).bold);
        assert!(d.rich.style(6).bold);
        assert!(d.rich.style(11).italic);
        assert!(!d.rich.style(17).bold);
        let saved = d.rich.encode(&d.text, Format::Rtf).unwrap();
        assert_eq!(decode(&saved).unwrap().rich.styles, d.rich.styles);
    }
    #[test]
    fn invalid_zip_crc_offsets_and_oversized_parts_fail() {
        let mut bytes = package::write(&[("test", b"valid".to_vec())]);
        assert_eq!(package::part(&bytes, "test").unwrap(), b"valid");
        bytes[34] ^= 1;
        assert!(package::part(&bytes, "test").is_err());
        let bytes = include_bytes!("../../../../tests/fixtures/wordpad/deflated.docx");
        for length in [4, 40, 200, bytes.len() - 10] {
            assert!(decode(&bytes[..length]).is_err());
        }
        let mut random = 0x21933u32;
        for len in 0..200 {
            let mut b = alloc::vec![0;len];
            for byte in &mut b {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                *byte = (random >> 24) as u8;
            }
            let _ = decode(&b);
        }
    }
}

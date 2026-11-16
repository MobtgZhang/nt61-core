//! Borrowed access to the same TrueType files used by the rasterizer.
//! Values in this interface remain in font design units; consumers choose
//! their logical size without conflating device pixels with GDI coordinates.

use ttf_parser::{Face, GlyphId, RawFace, Tag};

pub use ttf_parser::{OutlineBuilder, Rect as OutlineBounds};

#[derive(Clone, Copy, Debug)]
pub struct OutlineMetrics {
    pub advance: u16,
    pub left: i16,
    pub right: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct FontDescription {
    pub units_per_em: u16,
    pub ascent: u16,
    pub descent: u16,
    pub line_gap: i16,
    pub average_width: i16,
    pub maximum_width: u16,
    pub weight: u16,
    pub italic: bool,
    pub fixed_pitch: bool,
    pub first_char: u16,
    pub last_char: u16,
    pub default_char: u16,
    pub unicode_ranges: [u32; 4],
    pub code_page_ranges: [u32; 2],
}

pub struct SfntFont<'a> {
    data: &'a [u8],
    face: Face<'a>,
}

fn word(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(offset..offset + 2)?.try_into().ok()?))
}

fn dword(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(offset..offset + 4)?.try_into().ok()?))
}

impl<'a> SfntFont<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        Some(Self { data, face: Face::parse(data, 0).ok()? })
    }

    pub fn table(&self, tag: [u8; 4]) -> Option<&'a [u8]> {
        RawFace::parse(self.data, 0).ok()?.table(Tag::from_bytes(&tag))
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Family names in Unicode SFNT name records, without a terminating NUL.
    pub fn family_name(&self) -> Option<alloc::vec::Vec<u16>> {
        let names = self.face.names();
        let name = names.into_iter().find(|name| name.name_id == 1
            && name.is_unicode() && name.language_id == 0x0409)
            .or_else(|| names.into_iter().find(|name| name.name_id == 1 && name.is_unicode()))?;
        if name.name.is_empty() || name.name.len() % 2 != 0 { return None; }
        let mut units = alloc::vec::Vec::new();
        units.try_reserve_exact(name.name.len() / 2).ok()?;
        for pair in name.name.chunks_exact(2) {
            let unit = u16::from_be_bytes([pair[0], pair[1]]);
            if unit == 0 { return None; }
            units.push(unit);
        }
        // Validate paired surrogates before accepting the name for matching.
        if char::decode_utf16(units.iter().copied()).any(|unit| unit.is_err()) { return None; }
        Some(units)
    }

    pub fn glyph_count(&self) -> u16 {
        self.face.number_of_glyphs()
    }

    /// Visit the actual contour, including composite glyph transformations.
    pub fn outline(&self, index: u16, builder: &mut impl OutlineBuilder) -> Option<OutlineBounds> {
        self.face.outline_glyph(GlyphId(index), builder)
    }

    /// UTF-16 code units are intentional: GetGlyphIndicesW does not combine
    /// surrogate pairs. Supplementary shaping belongs to a higher layer.
    pub fn glyph_index(&self, code_unit: u16) -> Option<u16> {
        self.face
            .glyph_index(char::from_u32(u32::from(code_unit))?)
            .map(|id| id.0)
            .filter(|&id| id != 0)
    }

    pub fn glyph_metrics(&self, index: u16) -> Option<OutlineMetrics> {
        if index >= self.glyph_count() { return None; }
        let id = GlyphId(index);
        let bounds = self.face.glyph_bounding_box(id);
        Some(OutlineMetrics {
            advance: self.face.glyph_hor_advance(id)?,
            left: bounds.map_or(0, |bounds| bounds.x_min),
            right: bounds.map_or(0, |bounds| bounds.x_max),
        })
    }

    /// Minimum side bearings from hhea, in design units.
    pub fn minimum_bearings(&self) -> Option<[i16; 2]> {
        let table = self.table(*b"hhea")?;
        Some([word(table, 12)? as i16, word(table, 14)? as i16])
    }

    /// Enumerate horizontal, non-contextual kerning in the installed face.
    /// GDI's legacy KERNINGPAIR interface does not expose GPOS shaping rules.
    pub fn for_each_kerning_pair(&self, mut visit: impl FnMut(u16, u16, i16)) {
        let Some(table) = self.face.tables().kern else { return };
        for subtable in table.subtables {
            if !subtable.horizontal || subtable.variable || subtable.has_cross_stream {
                continue;
            }
            if let ttf_parser::kern::Format::Format0(data) = subtable.format {
                for pair in data.pairs {
                    if pair.value != 0 {
                        visit(pair.left().0, pair.right().0, pair.value);
                    }
                }
            }
        }
    }

    pub fn description(&self) -> Option<FontDescription> {
        let os2 = self.table(*b"OS/2")?;
        let hhea = self.table(*b"hhea")?;
        let version = word(os2, 0)?;
        let mut unicode_ranges = [0; 4];
        for (index, value) in unicode_ranges.iter_mut().enumerate() {
            *value = dword(os2, 42 + index * 4)?;
        }
        let code_page_ranges = if version >= 1 {
            [dword(os2, 78)?, dword(os2, 82)?]
        } else { [0; 2] };
        Some(FontDescription {
            units_per_em: self.face.units_per_em(),
            ascent: word(os2, 74)?,
            descent: word(os2, 76)?,
            line_gap: self.face.line_gap(),
            average_width: word(os2, 2)? as i16,
            maximum_width: word(hhea, 10)?,
            weight: self.face.weight().to_number(),
            italic: self.face.is_italic(),
            fixed_pitch: self.face.is_monospaced(),
            first_char: word(os2, 64)?,
            last_char: word(os2, 66)?,
            default_char: if version >= 2 { word(os2, 90)? } else { 0 },
            unicode_ranges,
            code_page_ranges,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_and_metrics_come_from_the_embedded_font() {
        let font = SfntFont::parse(crate::UI_FONT).unwrap();
        let description = font.description().unwrap();
        assert_eq!(description.units_per_em, 2048);
        assert!(font.glyph_count() > 1000);
        assert!(font.table(*b"head").is_some());
        assert!(font.table(*b"xxxx").is_none());
        assert!(font.glyph_index(0xd800).is_none());
        let narrow = font.glyph_metrics(font.glyph_index(b'i' as u16).unwrap()).unwrap();
        let wide = font.glyph_metrics(font.glyph_index(b'W' as u16).unwrap()).unwrap();
        assert!(wide.advance > narrow.advance);
        assert!(font.glyph_metrics(font.glyph_count()).is_none());
    }
}

//! Word 97–2003 text import via CFB streams and the CLX piece table (MS-DOC).
//! Binary formatting, fields and embedded OLE objects are not interpreted.
use super::{
    package::{u16_at, u32_at},
    Format, Imported, Style,
};
use alloc::{string::String, vec::Vec};
const END: u32 = 0xfffffffe;
struct Compound<'a> {
    bytes: &'a [u8],
    sector: usize,
    fat: Vec<u32>,
}
impl<'a> Compound<'a> {
    fn sector(&self, id: u32) -> Result<&'a [u8], &'static str> {
        let at = (id as usize)
            .checked_add(1)
            .and_then(|i| i.checked_mul(self.sector))
            .ok_or("Invalid DOC sector.")?;
        self.bytes
            .get(at..at + self.sector)
            .ok_or("Truncated DOC sector.")
    }
    fn chain(&self, start: u32, limit: usize) -> Result<Vec<u8>, &'static str> {
        let mut out = Vec::new();
        let mut id = start;
        let mut seen = alloc::vec![false;self.bytes.len()/self.sector];
        while id != END {
            let slot = seen
                .get_mut(id as usize)
                .ok_or("Invalid DOC sector chain.")?;
            if *slot {
                return Err("Cyclic DOC sector chain.");
            }
            *slot = true;
            if out.len().saturating_add(self.sector) > limit {
                return Err("DOC stream is too large.");
            }
            out.extend_from_slice(self.sector(id)?);
            id = *self
                .fat
                .get(id as usize)
                .ok_or("Invalid DOC allocation table.")?;
        }
        Ok(out)
    }
}
pub(super) fn read(bytes: &[u8]) -> Result<Imported, &'static str> {
    if u16_at(bytes, 28)? != 0xfffe {
        return Err("Unsupported DOC byte order.");
    }
    let major = u16_at(bytes, 26)?;
    let shift = u16_at(bytes, 30)?;
    if !matches!((major, shift), (3, 9) | (4, 12)) || u16_at(bytes, 32)? != 6 {
        return Err("Unsupported DOC compound format.");
    }
    let mut c = Compound {
        bytes,
        sector: 1usize << shift,
        fat: Vec::new(),
    };
    let sectors = bytes.len() / c.sector;
    let count = u32_at(bytes, 44)? as usize;
    if count > sectors {
        return Err("Invalid DOC FAT count.");
    }
    let mut ids = Vec::new();
    for i in 0..109 {
        let id = u32_at(bytes, 76 + i * 4)?;
        if id < 0xfffffffa {
            ids.push(id);
        }
    }
    let mut dif = u32_at(bytes, 68)?;
    let dif_count = u32_at(bytes, 72)? as usize;
    if dif_count > sectors {
        return Err("Invalid DOC DIFAT count.");
    }
    let mut seen = Vec::new();
    for _ in 0..dif_count {
        if seen.contains(&dif) {
            return Err("Cyclic DOC DIFAT.");
        }
        seen.push(dif);
        let b = c.sector(dif)?;
        for p in b[..b.len() - 4].chunks_exact(4) {
            let id = u32::from_le_bytes(p.try_into().map_err(|_| "Invalid DOC FAT.")?);
            if id < 0xfffffffa {
                ids.push(id);
            }
        }
        dif = u32_at(b, b.len() - 4)?;
    }
    if ids.len() < count {
        return Err("Truncated DOC FAT.");
    }
    for id in ids.into_iter().take(count) {
        let b = c.sector(id)?;
        for p in b.chunks_exact(4) {
            c.fat.push(u32::from_le_bytes(
                p.try_into().map_err(|_| "Invalid DOC FAT.")?,
            ));
        }
    }
    let directory = c.chain(u32_at(bytes, 48)?, 16 * 1024 * 1024)?;
    let mut entries = Vec::new();
    let mut root = None;
    for e in directory.chunks_exact(128) {
        let n = u16_at(e, 64)? as usize;
        if n < 2 || n > 64 || n % 2 != 0 {
            continue;
        }
        let name = String::from_utf16(
            &e[..n - 2]
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "Invalid DOC stream name.")?;
        let size = if major == 3 {
            u32_at(e, 120)? as u64
        } else {
            u32_at(e, 120)? as u64 | ((u32_at(e, 124)? as u64) << 32)
        };
        if size > 16 * 1024 * 1024 {
            return Err("DOC stream is too large.");
        }
        let entry = (name, u32_at(e, 116)?, size as usize);
        if e[66] == 5 {
            root = Some(entry);
        } else if e[66] == 2 {
            entries.push(entry);
        }
    }
    let mini_fat = c.chain(u32_at(bytes, 60)?, 16 * 1024 * 1024)?;
    let mini = root
        .map(|(_, id, _)| c.chain(id, 16 * 1024 * 1024))
        .transpose()?
        .unwrap_or_default();
    let stream = |name: &str| -> Result<Vec<u8>, &'static str> {
        let (_, start, len) = entries
            .iter()
            .find(|e| e.0 == name)
            .ok_or("Required DOC stream is missing.")?;
        let mut out = if *len < 4096 {
            let mut id = *start;
            let mut out = Vec::new();
            let mut seen = alloc::vec![false;mini.len()/64];
            while id != END {
                let slot = seen
                    .get_mut(id as usize)
                    .ok_or("Invalid DOC mini-sector.")?;
                if *slot {
                    return Err("Cyclic DOC mini-stream.");
                }
                *slot = true;
                let at = id as usize * 64;
                out.extend_from_slice(mini.get(at..at + 64).ok_or("Truncated DOC mini-stream.")?);
                id = u32_at(&mini_fat, id as usize * 4)?;
            }
            out
        } else {
            c.chain(*start, 16 * 1024 * 1024)?
        };
        if out.len() < *len {
            return Err("Truncated DOC stream.");
        }
        out.truncate(*len);
        Ok(out)
    };
    let word = stream("WordDocument")?;
    if u16_at(&word, 0)? != 0xa5ec || u16_at(&word, 2)? < 0xc1 {
        return Err("Only Word 97–2003 DOC text import is supported.");
    }
    let flags = u16_at(&word, 10)?;
    if flags & 0x8100 != 0 {
        return Err("Encrypted DOC documents are unsupported.");
    }
    let table = stream(if flags & 0x200 != 0 {
        "1Table"
    } else {
        "0Table"
    })?;
    let csw = u16_at(&word, 32)? as usize;
    let at = 34 + csw * 2;
    let cslw = u16_at(&word, at)? as usize;
    let ccp = u32_at(&word, at + 2 + 3 * 4)? as usize;
    let pairs = at + 2 + cslw * 4;
    let count = u16_at(&word, pairs)? as usize;
    if count <= 33 {
        return Err("DOC piece table is missing.");
    }
    let offset = u32_at(&word, pairs + 2 + 33 * 8)? as usize;
    let len = u32_at(&word, pairs + 6 + 33 * 8)? as usize;
    let clx = table
        .get(offset..offset.checked_add(len).ok_or("DOC size overflow.")?)
        .ok_or("Invalid DOC piece table.")?;
    let mut at = 0;
    while clx.get(at) == Some(&1) {
        at = at
            .checked_add(3 + u16_at(clx, at + 1)? as usize)
            .ok_or("DOC size overflow.")?;
    }
    if clx.get(at) != Some(&2) {
        return Err("Invalid DOC piece table.");
    }
    let len = u32_at(clx, at + 1)? as usize;
    let plc = clx
        .get(at + 5..(at + 5).checked_add(len).ok_or("DOC size overflow.")?)
        .ok_or("Truncated DOC piece table.")?;
    if len < 4 || (len - 4) % 12 != 0 {
        return Err("Invalid DOC piece table size.");
    }
    let n = (len - 4) / 12;
    let mut out = Imported::new(Format::LegacyDoc);
    for index in 0..n {
        let a = u32_at(plc, index * 4)? as usize;
        let b = u32_at(plc, index * 4 + 4)? as usize;
        if b < a {
            return Err("Invalid DOC character range.");
        }
        if a >= ccp {
            break;
        }
        let count = b.min(ccp) - a;
        let fc = u32_at(plc, (n + 1) * 4 + index * 8 + 2)?;
        let compressed = fc & 0x40000000 != 0;
        let at = (fc & 0x3fffffff) as usize / if compressed { 2 } else { 1 };
        let size = count
            .checked_mul(if compressed { 1 } else { 2 })
            .ok_or("DOC text overflow.")?;
        let data = word
            .get(at..at.checked_add(size).ok_or("DOC text overflow.")?)
            .ok_or("Truncated DOC text.")?;
        let text = if compressed {
            data.iter()
                .filter_map(|b| char::from_u32(super::super::super::document::cp1252(*b) as u32))
                .collect::<String>()
        } else {
            String::from_utf16(
                &data
                    .chunks_exact(2)
                    .map(|p| u16::from_le_bytes([p[0], p[1]]))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| "Invalid Unicode in DOC.")?
        };
        for ch in text.chars() {
            match ch {
                '\r' | '\u{b}' | '\u{c}' => out.push('\n', Style::default())?,
                '\u{7}' => out.push('\t', Style::default())?,
                c if c < ' ' && c != '\t' && c != '\n' => {}
                c => out.push(c, Style::default())?,
            }
        }
    }
    if out.text.ends_with('\n') {
        out.text.pop();
        out.rich.styles.pop();
    }
    out.rich.warning =
        "DOC text imported; binary formatting and objects omitted. Save As RTF or DOCX.".into();
    Ok(out)
}

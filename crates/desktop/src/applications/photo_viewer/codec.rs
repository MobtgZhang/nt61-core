//! Bounded image decoding and encoding for the source-built photo viewer.
use crate::graphics::Surface;
use alloc::vec::Vec;
#[cfg(test)]
use alloc::vec;
const LIMIT: usize = 16 * 1024 * 1024;
fn dimensions(w: u32, h: u32) -> Result<(), &'static str> {
    if w == 0
        || h == 0
        || w > 16384
        || h > 16384
        || (w as usize)
            .checked_mul(h as usize)
            .is_none_or(|n| n > LIMIT)
    {
        Err("This picture is too large to display.")
    } else {
        Ok(())
    }
}
pub fn decode(bytes: &[u8], page: usize) -> Result<(Surface, usize), &'static str> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("This picture file is too large.");
    }
    if bytes.starts_with(b"BM")
        || matches!(
            bytes.get(..4),
            Some([40, 0, 0, 0] | [52, 0, 0, 0] | [56, 0, 0, 0] | [108, 0, 0, 0] | [124, 0, 0, 0])
        )
    {
        return bitmap::decode(bytes).map(|s| (s, 1));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png(bytes).map(|s| (s, 1));
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        use zune_jpeg::{
            zune_core::{colorspace::ColorSpace, options::DecoderOptions},
            JpegDecoder,
        };
        let options = DecoderOptions::default()
            .set_max_width(16384)
            .set_max_height(16384)
            .jpeg_set_max_scans(64)
            .set_use_unsafe(false)
            .jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder = JpegDecoder::new_with_options(bytes, options);
        decoder
            .decode_headers()
            .map_err(|_| "Invalid JPEG header.")?;
        let (w, h) = decoder.dimensions().ok_or("Invalid JPEG dimensions.")?;
        dimensions(w as u32, h as u32)?;
        if decoder.output_buffer_size() != Some(w * h * 4) {
            return Err("Unsupported JPEG color space.");
        }
        let mut image = Surface::try_new(w as u32, h as u32)
            .ok_or("Not enough memory to display this picture.")?;
        decoder
            .decode_into(image.pixel_storage_bytes_mut())
            .map_err(|_| "Invalid JPEG picture data.")?;
        drop(decoder);
        image.finish_rgba_decode();
        return Ok((orient(image, jpeg_orientation(bytes))?, 1));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return gif(bytes).map(|s| (s, 1));
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return tiff(bytes, page);
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return ico(bytes).map(|s| (s, 1));
    }
    Err("Windows Photo Viewer can't open this picture. The file may be damaged or use an unsupported format.")
}
mod bitmap;
fn png(bytes: &[u8]) -> Result<Surface, &'static str> {
    use zune_png::{
        zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
        PngDecoder,
    };
    let options = DecoderOptions::default()
        .set_max_width(16384)
        .set_max_height(16384)
        .set_use_unsafe(false)
        .png_set_add_alpha_channel(true)
        .png_set_strip_to_8bit(true);
    let mut decoder = PngDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|_| "Invalid PNG header.")?;
    let (w, h) = decoder.dimensions().ok_or("Invalid PNG dimensions.")?;
    dimensions(w as u32, h as u32)?;
    let size = decoder
        .output_buffer_size()
        .filter(|n| *n <= LIMIT * 4)
        .ok_or("Invalid PNG pixel size.")?;
    let color = decoder.colorspace().ok_or("Invalid PNG color space.")?;
    let mut image =
        Surface::try_new(w as u32, h as u32).ok_or("Not enough memory to display this picture.")?;
    if !matches!(color, ColorSpace::RGBA | ColorSpace::LumaA) {
        return Err("Unsupported PNG color space.");
    }
    decoder
        .decode_into(
            image
                .pixel_storage_bytes_mut()
                .get_mut(..size)
                .ok_or("Invalid PNG pixel size.")?,
        )
        .map_err(|_| "Invalid or incomplete PNG picture data.")?;
    drop(decoder);
    match color {
        ColorSpace::RGBA if size == w * h * 4 => image.finish_rgba_decode(),
        ColorSpace::LumaA if size == w * h * 2 => {
            // Expand backwards so unread luminance/alpha pairs are preserved.
            let storage = image.pixel_storage_bytes_mut();
            for i in (0..w * h).rev() {
                let (luma, alpha) = (storage[i * 2], storage[i * 2 + 1]);
                storage[i * 4..i * 4 + 4]
                    .copy_from_slice(&u32::from_be_bytes([alpha, luma, luma, luma]).to_ne_bytes());
            }
        }
        _ => return Err("Invalid PNG pixel size."),
    }
    Ok(image)
}
fn le16(b: &[u8], at: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(
        b.get(at..at + 2)
            .ok_or("Incomplete picture header.")?
            .try_into()
            .map_err(|_| "Invalid picture header.")?,
    ))
}
fn ico(b: &[u8]) -> Result<Surface, &'static str> {
    let count = le16(b, 4)? as usize;
    if count == 0 || count > 256 {
        return Err("Invalid icon.");
    }
    let mut best = None;
    for n in 0..count {
        let at = 6 + n * 16;
        let e = b.get(at..at + 16).ok_or("Incomplete icon.")?;
        let w = if e[0] == 0 { 256 } else { e[0] as usize };
        let h = if e[1] == 0 { 256 } else { e[1] as usize };
        let size = u32::from_le_bytes(e[8..12].try_into().map_err(|_| "Invalid icon.")?) as usize;
        let offset =
            u32::from_le_bytes(e[12..16].try_into().map_err(|_| "Invalid icon.")?) as usize;
        if best.is_none_or(|(area, _, _)| w * h > area) {
            best = Some((w * h, offset, size));
        }
    }
    let (_, offset, size) = best.ok_or("Invalid icon.")?;
    let data = b
        .get(offset..offset.checked_add(size).ok_or("Invalid icon.")?)
        .ok_or("Incomplete icon.")?;
    if data.starts_with(b"\x89PNG") {
        return decode(data, 0).map(|p| p.0);
    }
    let header = data
        .get(..4)
        .and_then(|v| v.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or("Incomplete icon bitmap.")? as usize;
    if !matches!(header, 40 | 108 | 124) || data.len() < header {
        return Err("Unsupported icon bitmap header.");
    }
    let width = i32::from_le_bytes(data[4..8].try_into().map_err(|_| "Invalid icon.")?);
    let height = i32::from_le_bytes(data[8..12].try_into().map_err(|_| "Invalid icon.")?);
    if width <= 0 || width > 256 || height <= 0 || height > 512 || height % 2 != 0 {
        return Err("Invalid icon dimensions.");
    }
    let (w, h) = (width as usize, height as usize / 2);
    let depth = le16(data, 14)? as usize;
    if !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32) {
        return Err("Unsupported icon depth.");
    }
    let compression = u32::from_le_bytes(data[16..20].try_into().map_err(|_| "Invalid icon.")?);
    let used = u32::from_le_bytes(data[32..36].try_into().map_err(|_| "Invalid icon.")?) as usize;
    let palette = if depth <= 8 {
        if used > 1 << depth {
            return Err("Invalid icon palette.");
        }
        if used == 0 {
            1 << depth
        } else {
            used
        }
    } else {
        0
    };
    let pixels = header
        + if header == 40 && compression == 3 {
            12
        } else {
            0
        }
        + palette * 4;
    let row = (w * depth).div_ceil(32) * 4;
    let mask = pixels + row * h;
    if mask > data.len() {
        return Err("Incomplete icon pixels.");
    }
    let mut dib = data.to_vec();
    dib[8..12].copy_from_slice(&(h as u32).to_le_bytes());
    let mut image = bitmap::decode(&dib)?;
    // Unlike a standalone BI_RGB bitmap, a 32-bit icon may carry meaningful
    // alpha. Legacy icons with an entirely zero alpha channel use the AND mask.
    let has_alpha = depth == 32
        && compression == 0
        && (0..h).any(|y| (0..w).any(|x| data[pixels + y * row + x * 4 + 3] != 0));
    let stride = w.div_ceil(32) * 4;
    if !has_alpha && data.len() < mask + stride * h {
        return Err("Incomplete icon transparency mask.");
    }
    for y in 0..h {
        for x in 0..w {
            let src_y = h - y - 1;
            let mut color = image.pixel(x as u32, y as u32).unwrap_or(0);
            if has_alpha {
                color =
                    (color & 0x00ffffff) | (data[pixels + src_y * row + x * 4 + 3] as u32) << 24;
            }
            if data
                .get(mask + src_y * stride + x / 8)
                .is_some_and(|v| v & (0x80 >> (x % 8)) != 0)
            {
                color = 0;
            }
            image.set_pixel(x as u32, y as u32, color);
        }
    }
    Ok(image)
}
// GIFs are displayed as still pictures, as in the Windows 7 Photo Viewer.
fn gif(b: &[u8]) -> Result<Surface, &'static str> {
    let w = le16(b, 6)? as u32;
    let h = le16(b, 8)? as u32;
    dimensions(w, h)?;
    let flags = *b.get(10).ok_or("Invalid GIF.")?;
    let mut at = 13;
    let mut global = &[][..];
    if flags & 128 != 0 {
        let len = 3usize << (usize::from(flags & 7) + 1);
        global = b.get(at..at + len).ok_or("Invalid GIF palette.")?;
        at += len;
    }
    let mut transparent = None;
    loop {
        match *b.get(at).ok_or("Incomplete GIF.")? {
            0x21 => {
                let label = *b.get(at + 1).ok_or("Incomplete GIF extension.")?;
                at += 2;
                if label == 0xf9 {
                    if b.get(at) == Some(&4) && b.get(at + 1).is_some_and(|p| p & 1 != 0) {
                        transparent = b.get(at + 4).copied();
                    }
                }
                loop {
                    let n = *b.get(at).ok_or("Incomplete GIF block.")? as usize;
                    at += 1;
                    if n == 0 {
                        break;
                    }
                    at = at
                        .checked_add(n)
                        .filter(|n| *n <= b.len())
                        .ok_or("Invalid GIF block.")?;
                }
            }
            0x2c => {
                let x = le16(b, at + 1)? as u32;
                let y = le16(b, at + 3)? as u32;
                let fw = le16(b, at + 5)? as u32;
                let fh = le16(b, at + 7)? as u32;
                dimensions(fw, fh)?;
                if x + fw > w || y + fh > h {
                    return Err("Invalid GIF frame bounds.");
                }
                let f = *b.get(at + 9).ok_or("Incomplete GIF frame.")?;
                at += 10;
                let palette = if f & 128 != 0 {
                    let n = 3usize << (usize::from(f & 7) + 1);
                    let p = b.get(at..at + n).ok_or("Invalid GIF palette.")?;
                    at += n;
                    p
                } else {
                    global
                };
                let code_size = *b.get(at).ok_or("Incomplete GIF data.")?;
                at += 1;
                if !(2..=8).contains(&code_size) {
                    return Err("Invalid GIF code size.");
                }
                let mut compressed = Vec::new();
                loop {
                    let n = *b.get(at).ok_or("Incomplete GIF data.")? as usize;
                    at += 1;
                    if n == 0 {
                        break;
                    }
                    compressed.extend_from_slice(b.get(at..at + n).ok_or("Incomplete GIF data.")?);
                    at += n;
                }
                let indices = lzw(&compressed, code_size, false, fw as usize * fh as usize)?;
                let mut image = Surface::new(w, h);
                let mut rows = Vec::new();
                if f & 64 != 0 {
                    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                        rows.extend((start..fh).step_by(step));
                    }
                } else {
                    rows.extend(0..fh);
                }
                for (row, dy) in rows.iter().enumerate() {
                    for dx in 0..fw {
                        let p = indices[row * fw as usize + dx as usize];
                        if Some(p) == transparent {
                            continue;
                        }
                        let rgb = palette
                            .get(p as usize * 3..p as usize * 3 + 3)
                            .ok_or("Invalid GIF color index.")?;
                        image.set_pixel(
                            x + dx,
                            y + dy,
                            0xff000000
                                | (rgb[0] as u32) << 16
                                | (rgb[1] as u32) << 8
                                | rgb[2] as u32,
                        );
                    }
                }
                return Ok(image);
            }
            _ => return Err("This GIF contains no readable picture."),
        }
    }
}
fn lzw(data: &[u8], min: u8, msb: bool, limit: usize) -> Result<Vec<u8>, &'static str> {
    let clear = 1usize << min;
    let end = clear + 1;
    let mut size = min + 1;
    let mut next = end + 1;
    let mut bit = 0usize;
    let mut prefix = [0u16; 4096];
    let mut suffix = [0u8; 4096];
    let mut stack = [0u8; 4097];
    let mut previous = None;
    let mut first = 0u8;
    let mut out = Vec::with_capacity(limit.min(65536));
    loop {
        if bit + size as usize > data.len() * 8 {
            return Err("Incomplete compressed picture.");
        }
        let mut code = 0usize;
        for n in 0..size as usize {
            let p = bit + n;
            let v = if msb {
                (data[p / 8] >> (7 - p % 8)) & 1
            } else {
                (data[p / 8] >> (p % 8)) & 1
            };
            if msb {
                code = (code << 1) | v as usize;
            } else {
                code |= (v as usize) << n;
            }
        }
        bit += size as usize;
        if code == clear {
            size = min + 1;
            next = end + 1;
            previous = None;
            continue;
        }
        if code == end {
            break;
        }
        let original = code;
        let mut count = 0;
        if code >= next {
            if code != next || previous.is_none() {
                return Err("Invalid LZW dictionary.");
            }
            stack[count] = first;
            count += 1;
            code = previous.unwrap_or(0);
        }
        while code >= clear {
            if code >= next || count >= 4096 {
                return Err("Invalid LZW chain.");
            }
            stack[count] = suffix[code];
            count += 1;
            code = prefix[code] as usize;
        }
        first = code as u8;
        stack[count] = first;
        count += 1;
        if out.len() + count > limit {
            return Err("Compressed picture exceeds its dimensions.");
        }
        out.extend(stack[..count].iter().rev());
        if let Some(old) = previous {
            if next < 4096 {
                prefix[next] = old as u16;
                suffix[next] = first;
                next += 1;
                if next == ((1usize << size) - usize::from(msb)) && size < 12 {
                    size += 1;
                }
            }
        }
        previous = Some(original);
    }
    if out.len() != limit {
        return Err("Incomplete picture pixels.");
    }
    Ok(out)
}
struct Tiff<'a> {
    b: &'a [u8],
    little: bool,
}
impl Tiff<'_> {
    fn word(&self, at: usize) -> Result<u16, &'static str> {
        let b = self.b.get(at..at + 2).ok_or("Invalid TIFF offset.")?;
        Ok(if self.little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    }
    fn long(&self, at: usize) -> Result<u32, &'static str> {
        let b = self.b.get(at..at + 4).ok_or("Invalid TIFF offset.")?;
        Ok(if self.little {
            u32::from_le_bytes(b.try_into().map_err(|_| "Invalid TIFF.")?)
        } else {
            u32::from_be_bytes(b.try_into().map_err(|_| "Invalid TIFF.")?)
        })
    }
    fn values(&self, ifd: usize, tag: u16) -> Result<Vec<u32>, &'static str> {
        let count = self.word(ifd)? as usize;
        if count > 4096 {
            return Err("Too many TIFF tags.");
        }
        for n in 0..count {
            let at = ifd + 2 + n * 12;
            if self.word(at)? != tag {
                continue;
            }
            let kind = self.word(at + 2)?;
            let count = self.long(at + 4)? as usize;
            let stride = match kind {
                3 => 2,
                4 => 4,
                _ => return Err("Unsupported TIFF tag type."),
            };
            if count > 65536 {
                return Err("TIFF tag is too large.");
            }
            let start = if count * stride <= 4 {
                at + 8
            } else {
                self.long(at + 8)? as usize
            };
            let mut values = Vec::with_capacity(count);
            for i in 0..count {
                values.push(if stride == 2 {
                    self.word(start + i * 2)? as u32
                } else {
                    self.long(start + i * 4)?
                });
            }
            return Ok(values);
        }
        Ok(Vec::new())
    }
    fn value(&self, ifd: usize, tag: u16, default: u32) -> Result<u32, &'static str> {
        Ok(self.values(ifd, tag)?.first().copied().unwrap_or(default))
    }
}
fn tiff(b: &[u8], page: usize) -> Result<(Surface, usize), &'static str> {
    let t = Tiff {
        b,
        little: b.starts_with(b"II"),
    };
    let mut at = t.long(4)? as usize;
    let mut pages = Vec::new();
    while at != 0 {
        if pages.len() >= 256 || pages.contains(&at) {
            return Err("Invalid TIFF page chain.");
        }
        pages.push(at);
        let n = t.word(at)? as usize;
        if n > 4096 {
            return Err("Too many TIFF tags.");
        }
        at = t.long(at + 2 + n * 12)? as usize;
    }
    let at = *pages
        .get(page)
        .ok_or("The requested picture page does not exist.")?;
    let w = t.value(at, 256, 0)?;
    let h = t.value(at, 257, 0)?;
    dimensions(w, h)?;
    let samples = t.value(at, 277, 1)? as usize;
    let bits = t.values(at, 258)?;
    let compression = t.value(at, 259, 1)?;
    let photo = t.value(at, 262, 1)?;
    let rows = t.value(at, 278, h)?.max(1) as usize;
    let offsets = t.values(at, 273)?;
    let lengths = t.values(at, 279)?;
    if t.value(at, 284, 1)? != 1
        || !(1..=4).contains(&samples)
        || bits.len() != samples
        || bits.iter().any(|n| *n != 8)
        || !matches!(photo, 0..=2)
    {
        return Err("This TIFF color or pixel format is not supported.");
    }
    if offsets.len() != lengths.len() || offsets.len() != (h as usize).div_ceil(rows) {
        return Err("Invalid TIFF strips.");
    }
    let mut pixels = Vec::new();
    let row = w as usize * samples;
    for (n, (&offset, &length)) in offsets.iter().zip(&lengths).enumerate() {
        let slice = b
            .get(
                offset as usize
                    ..(offset as usize)
                        .checked_add(length as usize)
                        .ok_or("Invalid TIFF strip.")?,
            )
            .ok_or("Incomplete TIFF strip.")?;
        let expected = rows.min(h as usize - n * rows) * row;
        let mut strip = match compression {
            1 if slice.len() == expected => slice.to_vec(),
            1 => return Err("Invalid TIFF strip length."),
            5 => lzw(slice, 8, true, expected)?,
            8 | 32946 => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(slice, expected)
                .map_err(|_| "Invalid TIFF deflate data.")?,
            32773 => packbits(slice, expected)?,
            _ => return Err("This TIFF compression is not supported."),
        };
        if strip.len() != expected {
            return Err("Invalid TIFF strip length.");
        }
        match t.value(at, 317, 1)? {
            1 => {}
            2 => {
                for r in strip.chunks_exact_mut(row) {
                    for x in samples..r.len() {
                        r[x] = r[x].wrapping_add(r[x - samples]);
                    }
                }
            }
            _ => return Err("Unsupported TIFF predictor."),
        }
        pixels.extend(strip);
    }
    let extras = t.values(at, 338)?;
    let extra = extras.first().copied().unwrap_or(0);
    if !matches!((photo, samples), (0 | 1, 1 | 2) | (2, 3 | 4)) || extra > 2 || extras.len() > 1 {
        return Err("Unsupported TIFF extra samples.");
    }
    let mut s = Surface::try_new(w, h).ok_or("Not enough memory to display this picture.")?;
    for (i, p) in pixels.chunks_exact(samples).enumerate() {
        let (r, g, b) = if photo == 2 {
            if samples < 3 {
                return Err("Invalid TIFF RGB samples.");
            }
            (p[0], p[1], p[2])
        } else {
            let g = if photo == 0 { 255 - p[0] } else { p[0] };
            (g, g, g)
        };
        let a = if extra != 0 && (samples == 4 || samples == 2) {
            *p.last().unwrap_or(&255)
        } else {
            255
        };
        let unassociate = |c: u8| {
            if extra == 1 && a != 0 {
                (c as u32 * 255 / a as u32).min(255) as u8
            } else {
                c
            }
        };
        s.set_pixel(
            i as u32 % w,
            i as u32 / w,
            u32::from_be_bytes([a, unassociate(r), unassociate(g), unassociate(b)]),
        );
    }
    Ok((orient(s, t.value(at, 274, 1)? as u16)?, pages.len()))
}
fn packbits(b: &[u8], limit: usize) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let n = b[at] as i8;
        at += 1;
        if n >= 0 {
            let count = n as usize + 1;
            if out.len() + count > limit {
                return Err("TIFF strip exceeds its dimensions.");
            }
            out.extend_from_slice(b.get(at..at + count).ok_or("Invalid PackBits literal.")?);
            at += count;
        } else if n != -128 {
            let v = *b.get(at).ok_or("Invalid PackBits run.")?;
            at += 1;
            let end = out.len() + usize::from(n.unsigned_abs()) + 1;
            if end > limit {
                return Err("TIFF strip exceeds its dimensions.");
            }
            out.resize(end, v);
        }
        if out.len() > limit {
            return Err("TIFF strip exceeds its dimensions.");
        }
    }
    Ok(out)
}
fn jpeg_orientation(b: &[u8]) -> u16 {
    let mut at = 2;
    while at + 4 <= b.len() {
        if b[at] != 0xff {
            break;
        }
        let marker = b[at + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let len = u16::from_be_bytes([b[at + 2], b[at + 3]]) as usize;
        if len < 2 || at + 2 + len > b.len() {
            break;
        }
        if marker == 0xe1 && b.get(at + 4..at + 10) == Some(b"Exif\0\0") {
            let bytes = &b[at + 10..at + 2 + len];
            let t = Tiff {
                b: bytes,
                little: bytes.starts_with(b"II"),
            };
            return t
                .long(4)
                .and_then(|ifd| t.value(ifd as usize, 274, 1))
                .unwrap_or(1) as u16;
        }
        at += 2 + len;
    }
    1
}
fn orient(s: Surface, orientation: u16) -> Result<Surface, &'static str> {
    if orientation == 1 {
        return Ok(s);
    }
    transform(&s, orientation)
}
fn transform(s: &Surface, orientation: u16) -> Result<Surface, &'static str> {
    let (w, h) = (s.width(), s.height());
    let swap = matches!(orientation, 5..=8);
    let mut out = Surface::try_new(if swap { h } else { w }, if swap { w } else { h })
        .ok_or("Not enough memory.")?;
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => (x, y),
            };
            out.set_pixel(dx, dy, s.pixel(x, y).unwrap_or(0));
        }
    }
    Ok(out)
}
pub fn rotate(s: &Surface, right: bool) -> Result<Surface, &'static str> {
    transform(s, if right { 6 } else { 8 })
}

/// Replace only the selected TIFF directory. Other pages retain their original
/// encoded strips and directories, including pages this decoder cannot render.
pub fn encode_rotated(
    name: &str,
    original: &[u8],
    page: usize,
    image: &Surface,
) -> Result<Vec<u8>, &'static str> {
    if !same_format(name, "picture.tif") {
        return encode(name, image);
    }
    if !original.starts_with(b"II*\0") && !original.starts_with(b"MM\0*") {
        return Err("Invalid TIFF header.");
    }
    let t = Tiff {
        b: original,
        little: original.starts_with(b"II"),
    };
    let mut link = 4;
    let mut seen = Vec::new();
    loop {
        let at = t.long(link)? as usize;
        if at == 0 || seen.len() >= 256 || seen.contains(&at) {
            return Err("Invalid TIFF page chain.");
        }
        let count = t.word(at)? as usize;
        if count > 4096 {
            return Err("Too many TIFF tags.");
        }
        let next_link = at + 2 + count * 12;
        let next = t.long(next_link)?;
        if seen.len() == page {
            let mut out = original.to_vec();
            let offset = append_tiff_page(&mut out, image, t.little, next)?;
            let bytes = if t.little {
                offset.to_le_bytes()
            } else {
                offset.to_be_bytes()
            };
            out[link..link + 4].copy_from_slice(&bytes);
            return Ok(out);
        }
        seen.push(at);
        link = next_link;
    }
}

fn append_tiff_page(
    out: &mut Vec<u8>,
    image: &Surface,
    little: bool,
    next: u32,
) -> Result<u32, &'static str> {
    dimensions(image.width(), image.height())?;
    let start = (out.len() + 3) & !3;
    let bits = start + 2 + 12 * 12 + 4;
    let pixels = bits + 8;
    let length = image.width() as usize * image.height() as usize * 4;
    let end = pixels
        .checked_add(length)
        .filter(|end| *end <= 32 * 1024 * 1024)
        .ok_or("The rotated TIFF exceeds the file size limit.")?;
    out.try_reserve(end - out.len())
        .map_err(|_| "Not enough memory to save the picture.")?;
    out.resize(start, 0);
    let word = |out: &mut Vec<u8>, v: u16| {
        out.extend(if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        })
    };
    let long = |out: &mut Vec<u8>, v: u32| {
        out.extend(if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        })
    };
    word(out, 12);
    for (tag, kind, count, value) in [
        (256, 4, 1, image.width()),
        (257, 4, 1, image.height()),
        (258, 3, 4, bits as u32),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, pixels as u32),
        (274, 3, 1, 1),
        (277, 3, 1, 4),
        (278, 4, 1, image.height()),
        (279, 4, 1, length as u32),
        (284, 3, 1, 1),
        (338, 3, 1, 2),
    ] {
        word(out, tag);
        word(out, kind);
        long(out, count);
        if kind == 3 && count == 1 {
            word(out, value as u16);
            word(out, 0);
        } else {
            long(out, value);
        }
    }
    long(out, next);
    for _ in 0..4 {
        word(out, 8);
    }
    for p in image.pixels() {
        out.extend([(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
    }
    Ok(start as u32)
}
/// Extensions that name the same container can be copied byte for byte.
pub fn same_format(a: &str, b: &str) -> bool {
    fn format(name: &str) -> u8 {
        match name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "bmp" => 1,
            "dib" => 2,
            "png" => 3,
            "jpg" | "jpeg" | "jpe" | "jfif" => 4,
            "gif" => 5,
            "tif" | "tiff" => 6,
            "ico" => 7,
            _ => 0,
        }
    }
    let f = format(a);
    f != 0 && f == format(b)
}
pub fn encode(name: &str, s: &Surface) -> Result<Vec<u8>, &'static str> {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str(){
        "tif"|"tiff"=>{let mut out=b"II*\0\x08\0\0\0".to_vec();append_tiff_page(&mut out,s,true,0)?;Ok(out)},
        "bmp"=>Ok(super::super::paint::encode_bmp(s)),
        "dib"=>Ok(super::super::paint::encode_bmp(s)[14..].to_vec()),
        "png"=>{let mut out=b"\x89PNG\r\n\x1a\n".to_vec();let mut header=s.width().to_be_bytes().to_vec();header.extend(s.height().to_be_bytes());header.extend([8,6,0,0,0]);chunk(&mut out,b"IHDR",&header);let mut raw=Vec::new();for y in 0..s.height(){raw.push(0);for x in 0..s.width(){let p=s.pixel(x,y).unwrap_or(0);raw.extend([(p>>16)as u8,(p>>8)as u8,p as u8,(p>>24)as u8]);}}chunk(&mut out,b"IDAT",&miniz_oxide::deflate::compress_to_vec_zlib(&raw,6));chunk(&mut out,b"IEND",&[]);Ok(out)},
        "jpg"|"jpeg"|"jpe"|"jfif"=>{if s.width()>u16::MAX as u32||s.height()>u16::MAX as u32{return Err("Picture exceeds JPEG dimensions.");}let mut rgb=Vec::with_capacity(s.pixels().len()*3);for p in s.pixels(){rgb.extend([(p>>16)as u8,(p>>8)as u8,*p as u8]);}let mut out=Vec::new();jpeg_encoder::Encoder::new(&mut out,95).encode(&rgb,s.width()as u16,s.height()as u16,jpeg_encoder::ColorType::Rgb).map_err(|_|"Could not encode the rotated JPEG.")?;Ok(out)},
        _=>Err("This picture format cannot be saved by the viewer. Make a BMP or PNG copy before rotating."),
    }
}
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend(crc.finalize().to_be_bytes());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotating_one_tiff_page_preserves_other_directories_in_both_byte_orders() {
        let mut first = Surface::new(2, 3);
        first.set_pixel(0, 0, 0x80123456);
        let mut second = Surface::new(1, 2);
        second.set_pixel(0, 1, 0xffaabbcc);
        for little in [true, false] {
            let mut original = if little {
                b"II*\0\x08\0\0\0".to_vec()
            } else {
                b"MM\0*\0\0\0\x08".to_vec()
            };
            let first_offset = append_tiff_page(&mut original, &first, little, 0).unwrap() as usize;
            let second_offset = append_tiff_page(&mut original, &second, little, 0).unwrap();
            let first_link = first_offset + 2 + 12 * 12;
            original[first_link..first_link + 4].copy_from_slice(&if little {
                second_offset.to_le_bytes()
            } else {
                second_offset.to_be_bytes()
            });
            assert_eq!(decode(&original, 0).unwrap(), (first.clone(), 2));
            assert_eq!(decode(&original, 1).unwrap(), (second.clone(), 2));
            let rotated = rotate(&second, true).unwrap();
            let saved = encode_rotated("test.tiff", &original, 1, &rotated).unwrap();
            assert_eq!(decode(&saved, 0).unwrap(), (first.clone(), 2));
            assert_eq!(decode(&saved, 1).unwrap(), (rotated, 2));
            assert_eq!(
                &saved[second_offset as usize..original.len()],
                &original[second_offset as usize..]
            );
        }
        assert!(encode_rotated("bad.tif", b"II*\0", 0, &first).is_err());
    }
    #[test]
    fn copy_extensions_preserve_container_identity() {
        assert!(same_format("a.JPEG", "b.jfif"));
        assert!(same_format("a.tiff", "b.TIF"));
        assert!(!same_format("a.bmp", "b.dib"));
        assert!(!same_format("unknown", "unknown"));
    }
    #[test]
    fn dib_is_saved_without_a_bitmap_file_header_and_top_down_rows_decode() {
        let mut image = Surface::new(2, 2);
        for (i, p) in [0xffff0000, 0xff00ff00, 0xff0000ff, 0xffffffff]
            .into_iter()
            .enumerate()
        {
            image.set_pixel(i as u32 % 2, i as u32 / 2, p);
        }
        let mut dib = encode("copy.dib", &image).unwrap();
        assert_eq!(&dib[..4], &40u32.to_le_bytes());
        assert_eq!(decode(&dib, 0).unwrap().0, image);
        dib[8..12].copy_from_slice(&(-2i32).to_le_bytes());
        let top_down = decode(&dib, 0).unwrap().0;
        assert_eq!(top_down.pixel(0, 0), image.pixel(0, 1));
        assert_eq!(top_down.pixel(1, 1), image.pixel(1, 0));
    }
    #[test]
    fn icon_alpha_and_legacy_and_masks_are_preserved() {
        let mut b = vec![0u8; 22];
        b[2] = 1;
        b[4] = 1;
        b[6] = 2;
        b[7] = 1;
        b[10..12].copy_from_slice(&1u16.to_le_bytes());
        b[12..14].copy_from_slice(&32u16.to_le_bytes());
        b[14..18].copy_from_slice(&52u32.to_le_bytes());
        b[18..22].copy_from_slice(&22u32.to_le_bytes());
        let mut dib = vec![0; 40];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&2u32.to_le_bytes());
        dib[8..12].copy_from_slice(&2u32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        dib.extend([0x33, 0x22, 0x11, 128, 0x66, 0x55, 0x44, 255, 0x40, 0, 0, 0]);
        b.extend(dib);
        let icon = decode(&b, 0).unwrap().0;
        assert_eq!(icon.pixel(0, 0), Some(0x80112233));
        assert_eq!(icon.pixel(1, 0), Some(0));
        b[65] = 0;
        b[69] = 0;
        assert_eq!(decode(&b, 0).unwrap().0.pixel(0, 0), Some(0xff112233));
        assert!(decode(&b[..70], 0).is_err());
    }
    #[test]
    fn grayscale_and_indexed_png_expand_color_and_alpha() {
        for (color, depth, extra, raw, expected) in [
            (0, 8, vec![], vec![0, 0x7f], 0xff7f7f7f),
            (4, 8, vec![], vec![0, 0x42, 0x80], 0x80424242),
            (
                3,
                1,
                vec![(b"PLTE", vec![0x12, 0x34, 0x56]), (b"tRNS", vec![0x80])],
                vec![0, 0],
                0x80123456,
            ),
        ] {
            let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
            let mut header = 1u32.to_be_bytes().to_vec();
            header.extend(1u32.to_be_bytes());
            header.extend([depth, color, 0, 0, 0]);
            chunk(&mut b, b"IHDR", &header);
            for (tag, value) in extra {
                chunk(&mut b, tag, &value);
            }
            chunk(
                &mut b,
                b"IDAT",
                &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
            );
            chunk(&mut b, b"IEND", &[]);
            assert_eq!(decode(&b, 0).unwrap().0.pixel(0, 0), Some(expected));
        }
    }
    #[test]
    fn png_and_bitmap_rotation_preserve_pixels() {
        let mut s = Surface::new(2, 3);
        for (i, c) in [
            0xff123456, 0xffabcdef, 0xffee7722, 0xff00ff00, 0xff000000, 0xffffffff,
        ]
        .into_iter()
        .enumerate()
        {
            s.set_pixel(i as u32 % 2, i as u32 / 2, c);
        }
        for ext in ["test.png", "test.bmp"] {
            let b = encode(ext, &s).unwrap();
            assert!(decode(&b, 0).unwrap().0 == s);
        }
        let r = rotate(&s, true).unwrap();
        assert_eq!((r.width(), r.height()), (3, 2));
        assert_eq!(r.pixel(0, 0), s.pixel(0, 2));
        assert!(rotate(&r, false).unwrap() == s);
    }
    #[test]
    fn invalid_images_do_not_allocate_from_untrusted_dimensions() {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.resize(24, 255);
        assert!(decode(&b, 0).is_err());
        assert!(decode(b"GIF89a", 0).is_err());
        assert!(decode(b"II*\0\xff\xff\xff\xff", 0).is_err());
    }
    #[test]
    fn static_gif_and_jpeg_roundtrip() {
        let gif=b"GIF89a\x01\0\x01\0\x80\0\0\xff\0\0\0\0\0\x2c\0\0\0\0\x01\0\x01\0\0\x02\x02\x44\x01\0\x3b";
        let s = decode(gif, 0).unwrap().0;
        assert_eq!(s.pixel(0, 0), Some(0xffff0000));
        let jpeg = encode("test.jpg", &s).unwrap();
        let restored = decode(&jpeg, 0).unwrap().0;
        assert_eq!(restored.width(), 1);
        let pixel = restored.pixel(0, 0).unwrap();
        assert_eq!(pixel >> 24, 255);
        assert!(
            (pixel >> 16 & 255) > 245 && (pixel & 255) < 10,
            "JPEG channels must stay ARGB: {pixel:08x}"
        );
    }
    #[test]
    fn grayscale_png_expansion_keeps_all_unread_pairs() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = 4u32.to_be_bytes().to_vec();
        header.extend(1u32.to_be_bytes());
        header.extend([8, 4, 0, 0, 0]);
        chunk(&mut bytes, b"IHDR", &header);
        chunk(
            &mut bytes,
            b"IDAT",
            &miniz_oxide::deflate::compress_to_vec_zlib(
                &[0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0],
                6,
            ),
        );
        chunk(&mut bytes, b"IEND", &[]);
        assert_eq!(
            decode(&bytes, 0).unwrap().0.pixels(),
            &[0x34121212, 0x78565656, 0xbc9a9a9a, 0xf0dedede]
        );
    }
}

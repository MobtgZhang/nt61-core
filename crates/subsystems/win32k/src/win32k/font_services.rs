//! Native font queries over installed local TrueType faces. These handlers
//! capture caller memory and report the actual substituted font, including
//! its SFNT tables and cmap, rather than inventing fixed character metrics.

use super::{dc, objects};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_from_user, copy_to_user, probe_user_write, write_user_object};
use alloc::vec::Vec;
use nt61_fonts::{FontDescription, SfntFont};

#[path = "font_services/resources.rs"]
pub(crate) mod resources;
pub use resources::{nt_gdi_add_font_mem_resource_ex, nt_gdi_remove_font_mem_resource_ex};
use resources::FontSource;

#[path = "font_services/files.rs"]
mod files;
pub use files::{nt_gdi_get_font_file_data, nt_gdi_get_font_file_info, nt_gdi_get_realization_info};

#[path = "font_services/outline.rs"]
mod outline;
pub use outline::nt_gdi_get_glyph_outline;

#[path = "font_services/string_bitmap.rs"]
mod string_bitmap;

#[path = "font_services/drawing.rs"]
mod drawing;
pub use drawing::{draw_text_layout, nt_gdi_ext_text_out, nt_gdi_poly_text_out, nt_gdi_set_text_justification};

const GDI_ERROR: u64 = u32::MAX as u64;
const MAX_CHARACTERS: usize = 65_536;

/// Create an ordinary logical font from the native ENUMLOGFONTEXDVW layout.
/// Win7 accepts captures up to420 bytes, then clears the design-axis count
/// before hfontCreate stores the resulting356-byte logical description. Short
/// captures leave unspecified stack bytes in Win7; initialize those bytes here.
/// Client-side font cache/device-font modes remain unsupported.
pub extern "C" fn nt_gdi_hfont_create(frame: &SyscallFrame) -> u64 {
    const ENUMLOGFONTEXDVW_SIZE: usize = 420;
    let supplied = frame.args[1] as u32 as usize;
    if !(1..=ENUMLOGFONTEXDVW_SIZE).contains(&supplied)
        || frame.args[0] == 0 || frame.args[2] as u32 != 0 || frame.args[3] as u32 != 0
        || frame.args[4] != 0
    {
        return 0;
    }
    let mut bytes = [0u8; ENUMLOGFONTEXDVW_SIZE];
    if copy_from_user(&mut bytes[..supplied], frame.args[0]).is_err() {
        return 0;
    }
    // ENUMLOGFONTEXW occupies 348 bytes, followed by DESIGNVECTOR's
    // reserved DWORD, axis count, and sixteen LONG coordinates.
    bytes[352..356].fill(0);
    create_captured_font(&bytes)
}

pub fn create_logfont_w(logfont: &[u8; 92]) -> u64 {
    let mut bytes = [0; 420];
    bytes[..logfont.len()].copy_from_slice(logfont);
    create_captured_font(&bytes)
}

fn create_captured_font(bytes: &[u8; 420]) -> u64 {
    let integer = |offset| i32::from_le_bytes([
        bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3],
    ]);
    let mut font = objects::GdiFont::new();
    font.height = integer(0);
    font.width = integer(4);
    font.escapement = integer(8);
    font.orientation = integer(12);
    font.weight = integer(16);
    font.italic = bytes[20];
    font.underline = bytes[21];
    font.strike_out = bytes[22];
    font.charset = bytes[23];
    font.out_precision = bytes[24];
    font.clip_precision = bytes[25];
    font.quality = bytes[26];
    font.pitch_and_family = bytes[27];
    for (index, unit) in font.face_name.iter_mut().enumerate() {
        *unit = u16::from_le_bytes([bytes[28 + index * 2], bytes[29 + index * 2]]);
    }
    let mut description = [0; 356];
    description.copy_from_slice(&bytes[..356]);
    font.captured_description = Some(description);
    objects::gdi_create_logical_font(font)
}

/// Process termination revokes its private font handles and file identities.
pub fn release_process(process: u64) {
    resources::release_process(process);
    files::release_process(process);
}

struct FontSelection {
    requested: objects::GdiFont,
    source: FontSource,
}

struct RealizedFont<'a> {
    font: SfntFont<'a>,
    source: &'a FontSource,
    requested: objects::GdiFont,
    description: FontDescription,
    em_height: i64,
    horizontal_numerator: i64,
    horizontal_denominator: i64,
}

fn round_ratio(value: i64, numerator: i64, denominator: i64) -> Option<i32> {
    let product = value.checked_mul(numerator)?;
    let adjustment = if product < 0 { -(denominator / 2) } else { denominator / 2 };
    i32::try_from(product.checked_add(adjustment)? / denominator).ok()
}

pub(super) fn bind_logical_font(handle: u64) -> bool {
    objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        if entry.get_type()? != objects::GdiObjectType::Font || entry.obj_ptr == 0 { return None; }
        // SAFETY: the object table guard protects the logical font allocation.
        let requested = unsafe { &*(entry.obj_ptr as *const objects::GdiFont) };
        resources::select(requested, handle)
    }).is_some()
}

impl FontSelection {
    fn from_dc(dc: &dc::DcObject) -> Option<Self> {
        if dc.font == 0 {
            let requested = objects::GdiFont::new();
            Some(Self { source: resources::select(&requested, 0)?, requested })
        } else {
            objects::with_handle_table(|table| {
                let entry = table.get(dc.font)?;
                if entry.get_type()? != objects::GdiObjectType::Font || entry.obj_ptr == 0 {
                    return None;
                }
                // SAFETY: selected objects are pinned by the DC and the object
                // table guard serializes access while we copy the LOGFONT.
                let requested = unsafe { *(entry.obj_ptr as *const objects::GdiFont) };
                Some(Self { source: resources::select(&requested, dc.font)?, requested })
            })
        }
    }

    fn realize(&self) -> Option<RealizedFont<'_>> {
        if !self.source.accessible() { return None; }
        let requested = self.requested;
        let font = SfntFont::parse(self.source.data())?;
        let description = font.description()?;
        let requested_height = i64::from(requested.height).abs();
        if requested_height > 32768 || i64::from(requested.width).abs() > 32768 {
            return None;
        }
        let units = i64::from(description.units_per_em);
        let em_height = if requested.height > 0 {
            let cell = i64::from(description.ascent) + i64::from(description.descent);
            (requested_height * units + cell / 2) / cell
        } else if requested.height < 0 { requested_height } else { 16 };
        let (horizontal_numerator, horizontal_denominator) = if requested.width == 0 {
            (em_height.max(1), units)
        } else {
            (i64::from(requested.width).abs(), i64::from(description.average_width).max(1))
        };
        Some(RealizedFont { font, source: &self.source, requested, description,
            em_height: em_height.max(1), horizontal_numerator, horizontal_denominator })
    }
}

impl RealizedFont<'_> {
    fn horizontal(&self, value: i64) -> Option<i32> {
        round_ratio(value, self.horizontal_numerator, self.horizontal_denominator)
    }

    fn vertical(&self, value: i64) -> Option<i32> {
        round_ratio(value, self.em_height, i64::from(self.description.units_per_em))
    }

    fn glyph(&self, unit: u16, indices: bool) -> u16 {
        if indices {
            if unit < self.font.glyph_count() { unit } else { 0 }
        } else { self.font.glyph_index(unit).unwrap_or(0) }
    }

    fn abc(&self, glyph: u16) -> Option<[i32; 3]> {
        let metrics = self.font.glyph_metrics(glyph)?;
        let a = self.horizontal(i64::from(metrics.left))?;
        let right = self.horizontal(i64::from(metrics.right))?;
        let advance = self.horizontal(i64::from(metrics.advance))?;
        Some([a, right.checked_sub(a)?, advance.checked_sub(right)?])
    }

    fn cell_height(&self) -> Option<i32> {
        self.vertical(i64::from(self.description.ascent) + i64::from(self.description.descent))
    }
}

fn with_font<T>(handle: u64, operation: impl FnOnce(&RealizedFont) -> Option<T>) -> Option<T> {
    let dc = dc::get_dc(handle)?;
    let selection = FontSelection::from_dc(&dc)?;
    let font = selection.realize()?;
    drop(dc);
    operation(&font)
}

fn capture_units(address: u64, count: usize) -> Option<Vec<u16>> {
    if count > MAX_CHARACTERS { return None; }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(count.checked_mul(2)?).ok()?;
    bytes.resize(count * 2, 0);
    if count != 0 { copy_from_user(&mut bytes, address).ok()?; }
    Some(bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect())
}

fn output_bytes(size: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).ok()?;
    Some(bytes)
}

pub extern "C" fn nt_gdi_get_font_data(frame: &SyscallFrame) -> u64 {
    with_font(frame.args[0], |font| {
        let table = frame.args[1] as u32;
        let data = if table == 0 { font.font.data() } else { font.font.table(table.to_le_bytes())? };
        let remaining = data.get(frame.args[2] as u32 as usize..)?;
        if frame.args[3] == 0 { return Some(remaining.len() as u64); }
        let count = remaining.len().min(frame.args[4] as u32 as usize);
        copy_to_user(frame.args[3], &remaining[..count]).ok()?;
        Some(count as u64)
    }).unwrap_or(GDI_ERROR)
}

fn glyph_indices(frame: &SyscallFrame, subset: bool) -> u64 {
    if subset || frame.args[4] as u32 & !1 != 0 { return GDI_ERROR; }
    let count = frame.args[2] as i32;
    if count < 0 { return GDI_ERROR; }
    with_font(frame.args[0], |font| {
        if count == 0 {
            return (frame.args[1] == 0 && frame.args[3] == 0 && frame.args[4] == 0)
                .then_some(u64::from(font.font.glyph_count()));
        }
        let units = capture_units(frame.args[1], count as usize)?;
        let mut bytes = output_bytes(units.len() * 2)?;
        let default = if frame.args[4] & 1 != 0 { 0xffff }
            else { font.font.glyph_index(font.description.default_char).unwrap_or(0) };
        for unit in units {
            bytes.extend_from_slice(&font.font.glyph_index(unit).unwrap_or(default).to_le_bytes());
        }
        copy_to_user(frame.args[3], &bytes).ok()?;
        Some(count as u64)
    }).unwrap_or(GDI_ERROR)
}

pub extern "C" fn nt_gdi_get_glyph_indices_w(frame: &SyscallFrame) -> u64 {
    glyph_indices(frame, false)
}

pub extern "C" fn nt_gdi_get_glyph_indices_w_internal(frame: &SyscallFrame) -> u64 {
    glyph_indices(frame, frame.args[5] != 0)
}

fn character_widths(frame: &SyscallFrame, abc: bool) -> Option<()> {
    let count = frame.args[2] as u32 as usize;
    if count == 0 || count > MAX_CHARACTERS { return None; }
    let flags = frame.args[4] as u32;
    if flags & !(if abc { 3 } else { 0xb }) != 0 { return None; }
    let units = if frame.args[3] != 0 {
        capture_units(frame.args[3], count)?
    } else {
        let first = frame.args[1] as u32;
        if first.checked_add(count as u32)? > 0x10000 { return None; }
        (first..first + count as u32).map(|unit| unit as u16).collect()
    };
    with_font(frame.args[0], |font| {
        let mut bytes = output_bytes(count * if abc { 12 } else { 4 })?;
        let indices = flags & if abc { 2 } else { 8 } != 0;
        for unit in units {
            let glyph = font.glyph(unit, indices);
            let widths = font.abc(glyph)?;
            let integer = flags != 0;
            if abc {
                for value in widths {
                    bytes.extend_from_slice(&if integer { value.to_le_bytes() } else { (value as f32).to_le_bytes() });
                }
            } else {
                let advance = font.horizontal(i64::from(font.font.glyph_metrics(glyph)?.advance))?;
                bytes.extend_from_slice(&if integer { advance.to_le_bytes() } else { (advance as f32).to_le_bytes() });
            }
        }
        copy_to_user(frame.args[5], &bytes).ok()
    })
}

pub extern "C" fn nt_gdi_get_char_width_w(frame: &SyscallFrame) -> u64 {
    character_widths(frame, false).is_some() as u64
}

pub extern "C" fn nt_gdi_get_char_abc_widths_w(frame: &SyscallFrame) -> u64 {
    character_widths(frame, true).is_some() as u64
}

pub extern "C" fn nt_gdi_get_font_unicode_ranges(frame: &SyscallFrame) -> u64 {
    with_font(frame.args[0], |font| {
        let mut ranges: Vec<(u16, u16)> = Vec::new();
        let mut supported = 0u32;
        for code in 0..=u16::MAX {
            if font.font.glyph_index(code).is_none() { continue; }
            supported += 1;
            if let Some((start, count)) = ranges.last_mut() {
                if u32::from(*start) + u32::from(*count) == u32::from(code) && *count != u16::MAX {
                    *count += 1;
                    continue;
                }
            }
            ranges.try_reserve(1).ok()?;
            ranges.push((code, 1));
        }
        let size = 16 + ranges.len() * 4;
        if frame.args[1] == 0 { return Some(size as u64); }
        let mut bytes = output_bytes(size)?;
        for value in [size as u32, 0, supported, ranges.len() as u32] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for (first, count) in ranges {
            bytes.extend_from_slice(&first.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        copy_to_user(frame.args[1], &bytes).ok()?;
        Some(size as u64)
    }).unwrap_or(0)
}

fn measure_text(dc: u64, units: &[u16], glyph_indices: bool, mut distance: impl FnMut(i32)) -> Option<[i32; 2]> {
    if units.len() > MAX_CHARACTERS { return None; }
    with_font(dc, |font| {
        let mut advance = 0i64;
        let mut width = 0;
        for &unit in units {
            let glyph = font.glyph(unit, glyph_indices);
            advance = advance.checked_add(i64::from(font.font.glyph_metrics(glyph)?.advance))?;
            width = font.horizontal(advance)?;
            distance(width);
        }
        Some([width, if units.is_empty() { 0 } else { font.cell_height()? }])
    })
}

pub fn text_extent_w(dc: u64, units: &[u16]) -> Option<[i32; 2]> {
    measure_text(dc, units, false, |_| {})
}

fn extent(frame: &SyscallFrame, extended: bool) -> Option<()> {
    let count = frame.args[2] as i32;
    if count < 0 { return None; }
    let flags = frame.args[if extended { 7 } else { 4 }] as u32;
    if flags & !1 != 0 { return None; }
    let units = capture_units(frame.args[1], count as usize)?;
    let size_output = frame.args[if extended { 6 } else { 3 }];
    probe_user_write(size_output, 8).ok()?;
    if extended {
        if frame.args[4] != 0 { probe_user_write(frame.args[4], 4).ok()?; }
        if frame.args[5] != 0 && count != 0 { probe_user_write(frame.args[5], count as usize * 4).ok()?; }
    }
    let mut fit = 0i32;
    let mut distances = output_bytes(if extended { units.len() * 4 } else { 0 })?;
    let mut fits = true;
    let size = measure_text(frame.args[0], &units, flags & 1 != 0, |width| {
        if extended {
            distances.extend_from_slice(&width.to_le_bytes());
            fits &= width <= frame.args[3] as i32;
            if fits { fit += 1; }
        }
    })?;
    if extended {
        if frame.args[4] != 0 { write_user_object(frame.args[4], &fit).ok()?; }
        if frame.args[5] != 0 && count != 0 { copy_to_user(frame.args[5], &distances).ok()?; }
    }
    write_user_object(size_output, &size).ok()
}

pub extern "C" fn nt_gdi_get_text_extent(frame: &SyscallFrame) -> u64 {
    extent(frame, false).is_some() as u64
}

pub extern "C" fn nt_gdi_get_text_extent_ex_w(frame: &SyscallFrame) -> u64 {
    extent(frame, true).is_some() as u64
}

pub fn text_face_w(dc_handle: u64) -> Option<Vec<u16>> {
    with_font(dc_handle, |font| Some(font.source.family()))
}

pub extern "C" fn nt_gdi_get_text_face_w(frame: &SyscallFrame) -> u64 {
    let count = frame.args[1] as i32;
    if count < 0 { return 0; }
    let copy_face = || {
        let units = text_face_w(frame.args[0])?;
        if frame.args[2] == 0 { return Some(units.len() as u64 + 1); }
        if count == 0 { return Some(0); }
        let copied = (count as usize).min(units.len() + 1);
        let mut bytes = output_bytes(copied * 2)?;
        for unit in &units[..copied - 1] { bytes.extend_from_slice(&unit.to_le_bytes()); }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        copy_to_user(frame.args[2], &bytes).ok()?;
        Some(copied as u64)
    };
    copy_face().unwrap_or(0)
}

pub extern "C" fn nt_gdi_get_text_metrics_w(frame: &SyscallFrame) -> u64 {
    // Native slot two is the caller's buffer size, not Win32 flags.
    if frame.args[2] < 60 { return 0; }
    text_metrics_w(frame.args[0])
        .is_some_and(|bytes| copy_to_user(frame.args[1], &bytes).is_ok()) as u64
}

pub fn text_metrics_w(dc_handle: u64) -> Option<[u8; 60]> {
    with_font(dc_handle, metrics_w)
}

fn metrics_w(font: &RealizedFont<'_>) -> Option<[u8; 60]> {
        let info = font.description;
        let ascent = font.vertical(i64::from(info.ascent))?;
        let descent = font.vertical(i64::from(info.descent))?;
        let height = ascent.checked_add(descent)?;
        let mut bytes = [0u8; 60];
        let values = [height, ascent, descent, (height - font.em_height as i32).max(0),
            font.vertical(i64::from(info.line_gap))?.max(0),
            font.horizontal(i64::from(info.average_width))?, font.horizontal(i64::from(info.maximum_width))?,
            i32::from(info.weight), 0, 96, 96];
        for (destination, value) in bytes[..44].chunks_exact_mut(4).zip(values) {
            destination.copy_from_slice(&value.to_le_bytes());
        }
        for (destination, value) in bytes[44..52].chunks_exact_mut(2)
            .zip([info.first_char, info.last_char, info.default_char, 32]) {
            destination.copy_from_slice(&value.to_le_bytes());
        }
        bytes[52] = u8::from(info.italic);
        bytes[53] = font.requested.underline;
        bytes[54] = font.requested.strike_out;
        bytes[55] = 4 | u8::from(!info.fixed_pitch); // TMPF_TRUETYPE, variable pitch
        bytes[56] = font.requested.charset;
        Some(bytes)
}

pub fn enumerate_fonts_w(dc_handle: u64, family: Option<&str>) -> Option<Vec<([u8; 92], [u8; 60])>> {
    dc::get_dc(dc_handle)?;
    let filter = family.map(str::to_lowercase);
    let mut fonts = Vec::new();
    for mono in [false, true] {
        let source = FontSource::BuiltIn(mono);
        let name = source.family();
        if filter.as_ref().is_some_and(|filter| {
            alloc::string::String::from_utf16_lossy(&name).to_lowercase() != *filter
        }) {
            continue;
        }
        let selection = FontSelection { requested: objects::GdiFont::new(), source };
        let font = selection.realize()?;
        let metrics = metrics_w(&font)?;
        let mut logical = [0; 92];
        logical[..4].copy_from_slice(&metrics[..4]);
        logical[16..20].copy_from_slice(&metrics[28..32]);
        logical[20] = metrics[52];
        logical[23] = metrics[56];
        logical[27] = if mono { 1 } else { 2 };
        for (slot, unit) in logical[28..].chunks_exact_mut(2).take(31).zip(name) {
            slot.copy_from_slice(&unit.to_le_bytes());
        }
        fonts.push((logical, metrics));
    }
    Some(fonts)
}

pub extern "C" fn nt_gdi_get_char_width_info(frame: &SyscallFrame) -> u64 {
    with_font(frame.args[0], |font| {
        let [left, right] = font.font.minimum_bearings()?;
        let values = [font.horizontal(i64::from(left))?, font.horizontal(i64::from(right))?, 0];
        write_user_object(frame.args[1], &values).ok()
    }).is_some() as u64
}

pub extern "C" fn nt_gdi_get_rasterizer_caps(frame: &SyscallFrame) -> u64 {
    let count = (frame.args[1] as u32 as usize).min(6);
    if count == 0 { return 0; }
    // RASTERIZER_STATUS contains three WORDs. The local SFNT rasterizer
    // supports scalable TrueType; language-specific selection is not enabled.
    copy_to_user(frame.args[0], &[6, 0, 3, 0, 0, 0][..count]).is_ok() as u64
}

pub extern "C" fn nt_gdi_get_kerning_pairs(frame: &SyscallFrame) -> u64 {
    let capacity = frame.args[1] as u32 as usize;
    if capacity == 0 && frame.args[2] != 0 { return 0; }
    with_font(frame.args[0], |font| {
        let mut characters = Vec::new();
        characters.try_reserve_exact(usize::from(font.font.glyph_count())).ok()?;
        characters.resize(usize::from(font.font.glyph_count()), None);
        for unit in 0..=u16::MAX {
            if let Some(glyph) = font.font.glyph_index(unit) {
                characters[usize::from(glyph)].get_or_insert(unit);
            }
        }
        let mut pairs = Vec::new();
        let mut valid = true;
        font.font.for_each_kerning_pair(|left, right, amount| {
            let first = characters.get(usize::from(left)).copied().flatten();
            let second = characters.get(usize::from(right)).copied().flatten();
            if let (Some(first), Some(second), Some(amount)) =
                (first, second, font.horizontal(i64::from(amount)))
            {
                if pairs.try_reserve(1).is_err() { valid = false; return; }
                pairs.push((first, second, amount));
            }
        });
        if !valid { return None; }
        if frame.args[2] == 0 { return Some(pairs.len() as u64); }
        let copied = capacity.min(pairs.len());
        if copied != 0 {
            let mut bytes = output_bytes(copied.checked_mul(8)?)?;
            for &(first, second, amount) in &pairs[..copied] {
                bytes.extend_from_slice(&first.to_le_bytes());
                bytes.extend_from_slice(&second.to_le_bytes());
                bytes.extend_from_slice(&amount.to_le_bytes());
            }
            copy_to_user(frame.args[2], &bytes).ok()?;
        }
        Some(copied as u64)
    }).unwrap_or(0)
}

fn charset(font: &RealizedFont) -> (u8, u16) {
    // Select only code pages advertised by the actual face's OS/2 table.
    let requested = font.requested.charset;
    let mapping = match requested {
        0 | 1 => Some((0, 1252, 0)),
        238 => Some((238, 1250, 1)),
        204 => Some((204, 1251, 2)),
        161 => Some((161, 1253, 3)),
        162 => Some((162, 1254, 4)),
        177 => Some((177, 1255, 5)),
        178 => Some((178, 1256, 6)),
        186 => Some((186, 1257, 7)),
        163 => Some((163, 1258, 8)),
        _ => None,
    };
    if let Some((charset, page, bit)) = mapping {
        if font.description.code_page_ranges[0] & (1 << bit) != 0 {
            return (charset, page);
        }
    }
    (0, 1252)
}

pub extern "C" fn nt_gdi_get_char_set(frame: &SyscallFrame) -> u64 {
    with_font(frame.args[0], |font| {
        let (charset, page) = charset(font);
        Some((u64::from(charset) << 16) | u64::from(page))
    }).unwrap_or(0)
}

pub extern "C" fn nt_gdi_get_text_charset_info(frame: &SyscallFrame) -> u64 {
    with_font(frame.args[0], |font| {
        if frame.args[1] != 0 {
            let mut bytes = [0u8; 24];
            for (output, value) in bytes.chunks_exact_mut(4).zip(
                font.description.unicode_ranges.into_iter().chain(font.description.code_page_ranges)
            ) {
                output.copy_from_slice(&value.to_le_bytes());
            }
            copy_to_user(frame.args[1], &bytes).ok()?;
        }
        Some(u64::from(charset(font).0))
    }).unwrap_or(1)
}

#[path = "font_services/linking.rs"]
pub mod linking;

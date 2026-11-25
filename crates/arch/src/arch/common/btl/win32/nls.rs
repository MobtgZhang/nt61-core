//! Installed code pages for the isolated runtime: Western ANSI (1252) and UTF-8.
//! This is a fixed runtime profile, independent of the host locale. Other
//! locales/code pages and Windows best-fit tables remain unsupported. CTYPE1
//! classification covers Latin-1 and the additional CP1252 characters only.
use super::*;

#[path = "nls/fold_digits.rs"]
mod fold_digits;

const ANSI_CODE_PAGE: u32 = 1252;
const UTF8_CODE_PAGE: u32 = 65001;
const DEFAULT_FORMAT_LOCALE: u32 = 0x0409;
const LOCALE_SLIST: u32 = 0x000d;
const LOCALE_USE_CP_ACP: u32 = 0x4000_0000;
const LOCALE_NOUSEROVERRIDE: u32 = 0x8000_0000;
pub(super) const DEFAULT_UI_LANGUAGE: u16 = 0x0409;
pub(super) const UI_LANGUAGES: [(u16, &str, &str); 2] = [
    (DEFAULT_UI_LANGUAGE, "en-US", "0409"),
    (0x0804, "zh-CN", "0804"),
];
const INVALID_FLAGS: u32 = 1004;
const NO_UNICODE_TRANSLATION: u32 = 1113;
const FIND_STARTSWITH: u32 = 0x0010_0000;
const FIND_ENDSWITH: u32 = 0x0020_0000;
const FIND_FROMSTART: u32 = 0x0040_0000;
const FIND_FROMEND: u32 = 0x0080_0000;
const FIND_DIRECTION: u32 = FIND_STARTSWITH | FIND_ENDSWITH | FIND_FROMSTART | FIND_FROMEND;
const NORM_IGNORECASE: u32 = 1;
const FIND_FILTERS: u32 = 0x0803_0077;
const CP1252_HIGH: [u16; 32] = [
    0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021,
    0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d, 0x8f,
    0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
    0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
];

pub(super) fn handles(name: &str) -> bool {
    matches!(name, "GetUserDefaultUILanguage" | "GetACP" | "GetCPInfo" | "IsValidCodePage"
        | "AreFileApisANSI" | "GetLocaleInfoW" | "GetUserDefaultLCID"
        | "MultiByteToWideChar" | "WideCharToMultiByte" | "GetStringTypeW" | "LCMapStringW" | "FindNLSString"
        | "FoldStringW" | "LCMapStringEx" | "SetThreadPreferredUILanguages" | "GetThreadUILanguage")
}

fn set_thread_ui_languages(image: &mut GuestImage, args: &[u64]) -> Result<u64, ApiError> {
    let flags = args[0] as u32;
    if !matches!(flags, 4 | 8) || args[1] == 0 || args[2] == 0 {
        return Err(INVALID_PARAMETER.into());
    }
    let mut languages = Vec::new();
    let mut cursor = 0u64;
    loop {
        let mut tag = String::new();
        loop {
            if cursor >= 512 || tag.len() > 85 {
                return Err(INVALID_PARAMETER.into());
            }
            let address = args[1].checked_add(cursor).ok_or(NOACCESS)?;
            let unit = word(&mut image.memory, address, 2)? as u16;
            cursor += 2;
            if unit == 0 {
                break;
            }
            if unit > 0x7f {
                return Err(INVALID_PARAMETER.into());
            }
            tag.push(char::from(unit as u8));
        }
        if tag.is_empty() {
            if cursor == 2 && word(&mut image.memory, args[1].checked_add(2).ok_or(NOACCESS)?, 2)? != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            break;
        }
        let language = UI_LANGUAGES.iter().find(|(_, name, langid)|
            tag.eq_ignore_ascii_case(if flags == 8 { name } else { langid }))
            .map(|(language, _, _)| *language).ok_or(INVALID_PARAMETER)?;
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    put(&mut image.memory, args[2], languages.len() as u64, 4)?;
    image.win32.local.ui_languages = languages;
    Ok(1)
}

// Windows CTYPE1 flags for the installed Western repertoire. Keep the table
// independent of Rust's evolving Unicode version. In particular Windows marks
// superscript digits as DIGIT and symbols such as '+' and the euro as PUNCT.
// Other UTF-16 units need the Windows Unicode tables; do not guess their type.
pub(super) fn ctype1(unit: u16) -> Result<u16, ApiError> {
    let kind = match unit {
        9 => 0x68,                           // space, control, blank
        0xa..=0xd | 0x85 => 0x28,            // space, control
        0..=8 | 0xe..=0x1f | 0x7f..=0x9f => 0x20,
        0x20 | 0xa0 => 0x48,                // space, blank
        0x30..=0x39 => 0x84,                // digit, hex digit
        0x41..=0x46 => 0x181,               // upper, alpha, hex digit
        0x61..=0x66 => 0x182,               // lower, alpha, hex digit
        0x47..=0x5a | 0xc0..=0xd6 | 0xd8..=0xde
            | 0x152 | 0x160 | 0x178 | 0x17d => 0x101,
        0x67..=0x7a | 0xb5 | 0xdf..=0xf6 | 0xf8..=0xff
            | 0x153 | 0x161 | 0x17e | 0x192 => 0x102,
        0xaa | 0xba => 0x100,
        0xb2 | 0xb3 | 0xb9 => 4,
        0xad => 0x30,                       // punctuation, format control
        0x2c6 | 0x2dc => 0,                 // defined modifier symbols
        0x21..=0x7e | 0xa1..=0xff
            | 0x2013..=0x2014 | 0x2018..=0x201a | 0x201c..=0x201e
            | 0x2020..=0x2022 | 0x2026 | 0x2030 | 0x2039 | 0x203a
            | 0x20ac | 0x2122 => 0x10,
        _ => return Err(ApiError::Unsupported),
    };
    Ok(0x200 | kind) // C1_DEFINED
}

pub(super) fn map_case(unit: u16, upper: bool) -> Result<u16, ApiError> {
    // Validate the complete installed repertoire, even characters that map to
    // themselves. Locale-sensitive and context-dependent Unicode mappings
    // require additional tables. Legacy Win7 uppercasing leaves sharp-s alone.
    ctype1(unit)?;
    Ok(if upper {
        match unit {
            0x61..=0x7a | 0xe0..=0xf6 | 0xf8..=0xfe => unit - 0x20,
            0xff => 0x178,
            0x153 | 0x161 | 0x17e | 0x192 => unit - 1,
            0xb5 => 0x39c,
            _ => unit,
        }
    } else {
        match unit {
            0x41..=0x5a | 0xc0..=0xd6 | 0xd8..=0xde => unit + 0x20,
            0x178 => 0xff,
            0x152 | 0x160 | 0x17d => unit + 1,
            _ => unit,
        }
    })
}

fn page(number: u64) -> Result<u32, u32> {
    match number as u32 {
        0 | 3 | ANSI_CODE_PAGE => Ok(ANSI_CODE_PAGE), // ACP/current thread ACP
        UTF8_CODE_PAGE => Ok(UTF8_CODE_PAGE),
        _ => Err(INVALID_PARAMETER),
    }
}

pub(super) fn decode_ansi(byte: u8) -> u16 {
    if (0x80..0xa0).contains(&byte) { CP1252_HIGH[(byte - 0x80) as usize] }
    else { u16::from(byte) }
}

pub(super) fn encode_ansi(unit: u16) -> Option<u8> {
    if unit < 0x80 || (0xa0..=0xff).contains(&unit) { Some(unit as u8) }
    else { CP1252_HIGH.iter().position(|v| *v == unit).map(|i| 0x80 + i as u8) }
}

fn source(image: &mut GuestImage, address: u64, count: u64, width: usize) -> Result<Vec<u8>, u32> {
    let count = count as u32 as i32;
    if address == 0 || count == 0 || count < -1 { return Err(INVALID_PARAMETER); }
    if count > 0 {
        let length = (count as usize).checked_mul(width).ok_or(INVALID_PARAMETER)?;
        return read(&mut image.memory, address, length);
    }
    let mut bytes = Vec::new();
    while bytes.len() < MAX_BUFFER {
        let at = address.checked_add(bytes.len() as u64).ok_or(NOACCESS)?;
        let unit = word(&mut image.memory, at, width)?;
        bytes.extend_from_slice(&unit.to_le_bytes()[..width]);
        if unit == 0 { return Ok(bytes); }
    }
    Err(INVALID_PARAMETER)
}

pub(super) fn uppercase_in_place(image: &mut GuestImage, address: u64) -> Result<u64, ApiError> {
    if let Ok(unit) = u16::try_from(address) {
        return Ok(u64::from(map_case(unit, true)?));
    }
    let mut bytes = source(image, address, u32::MAX.into(), 2)?;
    for unit in bytes.chunks_exact_mut(2) {
        let uppercase = map_case(u16::from_le_bytes([unit[0], unit[1]]), true)?;
        unit.copy_from_slice(&uppercase.to_le_bytes());
    }
    if !image.memory.write_virt(address, &bytes) {
        return Err(NOACCESS.into());
    }
    Ok(address)
}

fn destination(image: &mut GuestImage, address: u64, capacity: u64, data: &[u8], width: usize) -> Result<u64, u32> {
    let capacity = capacity as u32 as i32;
    if capacity < 0 || (capacity != 0 && address == 0) { return Err(INVALID_PARAMETER); }
    let units = data.len() / width;
    if capacity == 0 { return Ok(units as u64); }
    if (capacity as usize) < units { return Err(122); }
    if !image.memory.write_virt(address, data) { return Err(NOACCESS); }
    Ok(units as u64)
}

fn locale_info_w(image: &mut GuestImage, args: &[u64]) -> Result<u64, ApiError> {
    if !matches!(args[0] as u32, 0x400 | 0x800 | DEFAULT_FORMAT_LOCALE) {
        return Err(INVALID_PARAMETER.into());
    }
    let value = match args[1] as u32 & !(LOCALE_USE_CP_ACP | LOCALE_NOUSEROVERRIDE) {
        LOCALE_SLIST => [b',', 0, 0, 0],
        _ => return Err(INVALID_FLAGS.into()),
    };
    destination(image, args[2], args[3], &value, 2).map_err(Into::into)
}

fn search_text(image: &mut GuestImage, address: u64, count: u64, ignore_case: bool) -> Result<String, ApiError> {
    let bytes = source(image, address, count, 2)?;
    let length = bytes.len() - if count as u32 == u32::MAX { 2 } else { 0 };
    let mut text = String::with_capacity(length / 2);
    for unit in bytes[..length].chunks_exact(2) {
        let character = unit[0];
        if unit[1] != 0 || !(character.is_ascii_alphanumeric() || character == b' ') {
            return Err(ApiError::Unsupported);
        }
        text.push(char::from(if ignore_case { character.to_ascii_lowercase() } else { character }));
    }
    Ok(text)
}

fn find_nls_string(image: &mut GuestImage, a: &[u64]) -> Result<u64, ApiError> {
    let flags = a[1] as u32;
    let direction = flags & FIND_DIRECTION;
    if direction.count_ones() > 1 || flags & !(FIND_DIRECTION | FIND_FILTERS) != 0 {
        return Err(INVALID_FLAGS.into());
    }
    if !matches!(a[0] as u32, 0x7f | 0x400 | 0x800 | 0x409)
        || flags & !(FIND_DIRECTION | NORM_IGNORECASE) != 0 {
        return Err(ApiError::Unsupported);
    }
    let ignore_case = flags & NORM_IGNORECASE != 0;
    let text = search_text(image, a[2], a[3], ignore_case)?;
    let value = search_text(image, a[4], a[5], ignore_case)?;
    if value.is_empty() {
        return Err(INVALID_PARAMETER.into());
    }
    let found = match direction {
        FIND_FROMEND => text.rfind(value.as_str()),
        FIND_STARTSWITH => text.starts_with(value.as_str()).then_some(0),
        FIND_ENDSWITH => text.strip_suffix(value.as_str()).map(str::len),
        _ => text.find(value.as_str()),
    };
    let Some(offset) = found else {
        error(image, 0);
        return Ok(u64::from(u32::MAX));
    };
    if a[6] != 0 {
        put(&mut image.memory, a[6], value.len() as u64, 4)?;
    }
    Ok(offset as u64)
}

fn fold_string(image: &mut GuestImage, args: &[u64]) -> Result<u64, ApiError> {
    let [flags, input, count, output, capacity, ..] = *args else {
        return Err(INVALID_PARAMETER.into());
    };
    let count = count as i32;
    let capacity = capacity as i32;
    if input == 0 || count == 0 || capacity < 0 || (capacity > 0 && output == 0) || input == output {
        return Err(INVALID_PARAMETER.into());
    }
    match flags as u32 {
        0x80 => (),
        0x10 | 0x20 | 0x30 | 0x40 | 0x50 | 0x90 | 0xa0 | 0xb0 | 0xc0 | 0xd0
        | 0x2000 | 0x2010 | 0x2080 | 0x2090 => return Err(ApiError::Unsupported),
        _ => return Err(INVALID_FLAGS.into()),
    }
    if capacity == 0 && count > 0 {
        return Ok(count as u64);
    }
    if capacity > 0 && count > capacity {
        return Err(122.into());
    }
    let bytes = source(image, input, if count < 0 { u32::MAX as u64 } else { count as u64 }, 2)?;
    let units = bytes.len() / 2;
    if capacity == 0 {
        return Ok(units as u64);
    }
    if (capacity as usize) < units {
        return Err(122.into());
    }
    let folded = fold_digits::fold(&bytes);
    destination(image, output, capacity as u64, &folded, 2).map_err(Into::into)
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64, ApiError> {
    Ok(match name {
        "LCMapStringW" | "LCMapStringEx" => {
            let flags = a[1] as u32;
            if flags == 0 || flags & 0x300 == 0x300 { return Err(INVALID_FLAGS.into()); }
            let installed_locale = if name == "LCMapStringEx" {
                if a[7] != 0 { return Err(INVALID_PARAMETER.into()); }
                if a[6] != 0 || a[8] != 0 { return Err(ApiError::Unsupported); }
                a[0] == 0 || {
                    let locale = string(&mut image.memory, a[0], true)?;
                    locale.is_empty() || locale.eq_ignore_ascii_case("en-US")
                }
            } else {
                matches!(a[0] as u32, 0 | 0x7f | 0x400 | 0x800 | 0x409)
            };
            // The runtime's user/system default locale is Western English.
            // Other locales, sort keys, kana/width and linguistic casing need
            // locale data which is not installed in this runtime profile.
            if !installed_locale || !matches!(flags, 0x100 | 0x200) {
                return Err(ApiError::Unsupported);
            }
            let bytes = source(image, a[2], a[3], 2)?;
            let mut output = Vec::with_capacity(bytes.len());
            for unit in bytes.chunks_exact(2) {
                output.extend_from_slice(&map_case(u16::from_le_bytes([unit[0], unit[1]]), flags == 0x200)?.to_le_bytes());
            }
            destination(image, a[4], a[5], &output, 2)?
        }
        "GetStringTypeW" => {
            let info = a[0] as u32;
            if !matches!(info, 1 | 2 | 4) || a[1] == 0 || a[3] == 0 || a[1] == a[3] {
                return Err(INVALID_PARAMETER.into());
            }
            let bytes = source(image, a[1], a[2], 2)?;
            if info != 1 { return Err(ApiError::Unsupported); }
            let mut output = Vec::with_capacity(bytes.len());
            for unit in bytes.chunks_exact(2) {
                output.extend_from_slice(&ctype1(u16::from_le_bytes([unit[0], unit[1]]))?.to_le_bytes());
            }
            // Classification/validation completes before any guest write.
            if !image.memory.write_virt(a[3], &output) { return Err(NOACCESS.into()); }
            1
        }
        "FindNLSString" => find_nls_string(image, a)?,
        "FoldStringW" => fold_string(image, a)?,
        "GetUserDefaultUILanguage" => u64::from(image.win32.ui_language),
        "GetThreadUILanguage" => u64::from(image.win32.local.ui_languages.first()
            .copied().unwrap_or(image.win32.ui_language)),
        "SetThreadPreferredUILanguages" => set_thread_ui_languages(image, a)?,
        "GetLocaleInfoW" => locale_info_w(image, a)?,
        "GetUserDefaultLCID" => u64::from(DEFAULT_FORMAT_LOCALE),
        "AreFileApisANSI" => 1,
        "GetACP" => u64::from(ANSI_CODE_PAGE),
        "IsValidCodePage" => u64::from(matches!(a[0] as u32, ANSI_CODE_PAGE | UTF8_CODE_PAGE)),
        "GetCPInfo" => {
            let code_page = page(a[0])?;
            if a[1] == 0 { return Err(INVALID_PARAMETER.into()); }
            // CPINFO is 20 bytes on both Windows ABIs, including tail padding.
            let mut info = [0; 20];
            info[..4].copy_from_slice(&(if code_page == UTF8_CODE_PAGE {4u32} else {1}).to_le_bytes());
            info[4] = b'?';
            if !image.memory.write_virt(a[1], &info) { return Err(NOACCESS.into()); }
            1
        }
        "MultiByteToWideChar" => {
            if a[2] == a[4] { return Err(INVALID_PARAMETER.into()); }
            let code_page = page(a[0])?;
            let flags = a[1] as u32;
            let allowed = if code_page == ANSI_CODE_PAGE { 9 } else { 8 };
            if flags & !allowed != 0 {
                // The complete CP1252 mapping above is already precomposed.
                // COMPOSITE/USEGLYPHCHARS need additional mapping tables.
                if code_page == ANSI_CODE_PAGE && flags & !0xf == 0 {
                    return Err(120.into());
                }
                return Err(INVALID_FLAGS.into());
            }
            let bytes = source(image, a[2], a[3], 1)?;
            let units: Vec<u16> = if code_page == ANSI_CODE_PAGE {
                bytes.into_iter().map(decode_ansi).collect()
            } else if flags & 8 != 0 {
                core::str::from_utf8(&bytes).map_err(|_| NO_UNICODE_TRANSLATION)?.encode_utf16().collect()
            } else {
                String::from_utf8_lossy(&bytes).encode_utf16().collect()
            };
            let output: Vec<u8> = units.into_iter().flat_map(u16::to_le_bytes).collect();
            destination(image, a[4], a[5], &output, 2)?
        }
        "WideCharToMultiByte" => {
            if a[2] == a[4] { return Err(INVALID_PARAMETER.into()); }
            let code_page = page(a[0])?;
            let flags = a[1] as u32;
            let allowed = if code_page == UTF8_CODE_PAGE {0x80} else {0x400};
            if flags & !allowed != 0 { return Err(INVALID_FLAGS.into()); }
            if code_page == UTF8_CODE_PAGE && (a[6] != 0 || a[7] != 0) {
                return Err(INVALID_PARAMETER.into());
            }
            let bytes = source(image, a[2], a[3], 2)?;
            let units: Vec<u16> = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0],b[1]])).collect();
            let mut used_default = false;
            let output = if code_page == UTF8_CODE_PAGE {
                let text = if flags & 0x80 != 0 {
                    String::from_utf16(&units).map_err(|_| NO_UNICODE_TRANSLATION)?
                } else { String::from_utf16_lossy(&units) };
                text.into_bytes()
            } else {
                let fallback = if a[6] == 0 {b'?'} else {word(&mut image.memory, a[6], 1)? as u8};
                let mut output = Vec::with_capacity(units.len());
                for unit in units {
                    // Supplementary/default-character behavior for legacy
                    // code pages is outside this profile. Never split a UTF-16
                    // pair into two falsely successful substitutions.
                    if (0xd800..=0xdfff).contains(&unit) { return Err(120.into()); }
                    if let Some(byte) = encode_ansi(unit) { output.push(byte); }
                    else {
                        // Exact mappings work with flags=0. Best-fit tables
                        // are absent; substitution is provided only when the
                        // caller explicitly requests WC_NO_BEST_FIT_CHARS.
                        if flags & 0x400 == 0 { return Err(120.into()); }
                        used_default = true;
                        output.push(fallback);
                    }
                }
                output
            };
            // Validate both destinations before either is changed. Query mode
            // still reports whether a default character would be required.
            if a[7] != 0 && !image.memory.probe_write(a[7], 4) { return Err(NOACCESS.into()); }
            let count = destination(image, a[4], a[5], &output, 1)?;
            if a[7] != 0 { put(&mut image.memory, a[7], u64::from(used_default), 4)?; }
            count
        }
        _ => return Err(ApiError::Unsupported),
    })
}

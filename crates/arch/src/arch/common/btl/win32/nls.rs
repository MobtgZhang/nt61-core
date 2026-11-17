//! Installed code pages for the isolated runtime: Western ANSI (1252) and UTF-8.
//! This is a fixed runtime profile, independent of the host locale. Other
//! locales/code pages and Windows best-fit tables remain unsupported. CTYPE1
//! classification covers Latin-1 and the additional CP1252 characters only.
use super::*;

const ANSI_CODE_PAGE: u32 = 1252;
const UTF8_CODE_PAGE: u32 = 65001;
const INVALID_FLAGS: u32 = 1004;
const NO_UNICODE_TRANSLATION: u32 = 1113;
const CP1252_HIGH: [u16; 32] = [
    0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021,
    0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d, 0x8f,
    0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
    0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
];

pub(super) fn handles(name: &str) -> bool {
    matches!(name, "GetACP" | "GetCPInfo" | "IsValidCodePage"
        | "MultiByteToWideChar" | "WideCharToMultiByte" | "GetStringTypeW" | "LCMapStringW")
}

// Windows CTYPE1 flags for the installed Western repertoire. Keep the table
// independent of Rust's evolving Unicode version. In particular Windows marks
// superscript digits as DIGIT and symbols such as '+' and the euro as PUNCT.
// Other UTF-16 units need the Windows Unicode tables; do not guess their type.
fn ctype1(unit: u16) -> Result<u16, ApiError> {
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

fn destination(image: &mut GuestImage, address: u64, capacity: u64, data: &[u8], width: usize) -> Result<u64, u32> {
    let capacity = capacity as u32 as i32;
    if capacity < 0 || (capacity != 0 && address == 0) { return Err(INVALID_PARAMETER); }
    let units = data.len() / width;
    if capacity == 0 { return Ok(units as u64); }
    if (capacity as usize) < units { return Err(122); }
    if !image.memory.write_virt(address, data) { return Err(NOACCESS); }
    Ok(units as u64)
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64, ApiError> {
    Ok(match name {
        "LCMapStringW" => {
            let flags = a[1] as u32;
            if flags == 0 || flags & 0x300 == 0x300 { return Err(INVALID_FLAGS.into()); }
            // The runtime's user/system default locale is Western English.
            // Other locales, sort keys, kana/width and linguistic casing need
            // locale data which is not installed in this runtime profile.
            if !matches!(a[0] as u32, 0 | 0x7f | 0x400 | 0x800 | 0x409)
                || !matches!(flags, 0x100 | 0x200) {
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

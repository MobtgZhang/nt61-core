//! Bounded in-memory OPC ZIP subset; stored and deflated entries, CRC verified.
use alloc::{string::String, vec::Vec};
const LIMIT: usize = 1024 * 1024;
pub(super) fn u16_at(b: &[u8], i: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(
        b.get(i..i + 2)
            .ok_or("Truncated document.")?
            .try_into()
            .map_err(|_| "Truncated document.")?,
    ))
}
pub(super) fn u32_at(b: &[u8], i: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(
        b.get(i..i + 4)
            .ok_or("Truncated document.")?
            .try_into()
            .map_err(|_| "Truncated document.")?,
    ))
}
pub fn part(bytes: &[u8], name: &str) -> Result<Vec<u8>, &'static str> {
    let eocd = (bytes.len().saturating_sub(65557)..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            bytes.get(i..i + 4) == Some(b"PK\x05\x06")
                && u16_at(bytes, i + 20).is_ok_and(|n| i + 22 + n as usize == bytes.len())
        })
        .ok_or("Missing ZIP directory.")?;
    if u16_at(bytes, eocd + 4)? != 0 || u16_at(bytes, eocd + 6)? != 0 {
        return Err("Split ZIP archives are unsupported.");
    }
    let count = u16_at(bytes, eocd + 10)? as usize;
    if count > 4096 {
        return Err("Too many ZIP parts.");
    }
    let mut at = u32_at(bytes, eocd + 16)? as usize;
    for _ in 0..count {
        if bytes.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err("Invalid ZIP directory.");
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        let crc = u32_at(bytes, at + 16)?;
        let compressed = u32_at(bytes, at + 20)? as usize;
        let length = u32_at(bytes, at + 24)? as usize;
        let n = u16_at(bytes, at + 28)? as usize;
        let extra = u16_at(bytes, at + 30)? as usize;
        let comment = u16_at(bytes, at + 32)? as usize;
        let local = u32_at(bytes, at + 42)? as usize;
        let path = bytes.get(at + 46..at + 46 + n).ok_or("Invalid ZIP name.")?;
        if path == name.as_bytes() {
            if flags & 1 != 0 {
                return Err("Password-protected documents are unsupported.");
            }
            if length > LIMIT || compressed > 16 * 1024 * 1024 {
                return Err("Document part is too large.");
            }
            if bytes.get(local..local + 4) != Some(b"PK\x03\x04")
                || u16_at(bytes, local + 8)? != method
            {
                return Err("Invalid ZIP entry.");
            }
            let start = local
                .checked_add(
                    30 + u16_at(bytes, local + 26)? as usize + u16_at(bytes, local + 28)? as usize,
                )
                .ok_or("ZIP offset overflow.")?;
            let data = bytes
                .get(start..start.checked_add(compressed).ok_or("ZIP size overflow.")?)
                .ok_or("Truncated ZIP entry.")?;
            let out = match method {
                0 => data.to_vec(),
                8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, LIMIT)
                    .map_err(|_| "Invalid compressed document.")?,
                _ => return Err("Unsupported ZIP compression."),
            };
            if out.len() != length || crc32fast::hash(&out) != crc {
                return Err("Document ZIP checksum mismatch.");
            }
            return Ok(out);
        }
        at = at
            .checked_add(46 + n + extra + comment)
            .ok_or("ZIP directory overflow.")?;
    }
    Err("Required document part is missing.")
}
/// OPC parts are emitted as stored ZIP entries for interoperable, bounded saves.
pub fn write(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in parts {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let len = data.len() as u32;
        out.extend(b"PK\x03\x04");
        for n in [20u16, 0, 0, 0, 0] {
            out.extend(n.to_le_bytes());
        }
        for n in [crc, len, len] {
            out.extend(n.to_le_bytes());
        }
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(data);
        directory.extend(b"PK\x01\x02");
        for n in [20u16, 20, 0, 0, 0, 0] {
            directory.extend(n.to_le_bytes());
        }
        for n in [crc, len, len] {
            directory.extend(n.to_le_bytes());
        }
        directory.extend((name.len() as u16).to_le_bytes());
        for _ in 0..4 {
            directory.extend(0u16.to_le_bytes());
        }
        directory.extend(0u32.to_le_bytes());
        directory.extend(offset.to_le_bytes());
        directory.extend(name.as_bytes());
    }
    let start = out.len() as u32;
    let size = directory.len() as u32;
    out.extend(directory);
    out.extend(b"PK\x05\x06");
    out.extend([0u8; 4]);
    out.extend((parts.len() as u16).to_le_bytes());
    out.extend((parts.len() as u16).to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}
pub(super) fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

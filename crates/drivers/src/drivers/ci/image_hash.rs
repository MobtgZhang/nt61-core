//! Checked Authenticode SHA-256 content hashing for contiguous PE file layouts.
//! Hashing content is separate from establishing a certificate's trust.
use super::Sha256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageHashError {
    Header,
    Section,
    Certificate,
    UnsupportedLayout,
}

fn u16_at(bytes: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(off..off.checked_add(2)?)?.try_into().ok()?))
}
fn u32_at(bytes: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(off..off.checked_add(4)?)?.try_into().ok()?))
}

/// Compute the PE/COFF Authenticode content digest, excluding CheckSum,
/// the security-directory entry, and the terminal WIN_CERTIFICATE table.
/// Sections are ordered by raw file position. Sparse/overlapping raw layouts
/// are rejected rather than giving a potentially ambiguous digest. Header
/// bytes, section contents, raw padding and non-certificate overlay are covered.
/// Certificate bytes are opaque here: this is not signature validation.
pub fn compute_pe_hash(image: &[u8]) -> Result<[u8; 32], ImageHashError> {
    use ImageHashError::*;
    if image.get(..2) != Some(b"MZ") { return Err(Header); }
    let pe = u32_at(image, 0x3c).ok_or(Header)? as usize;
    if pe < 0x40 || image.get(pe..pe.checked_add(4).ok_or(Header)?) != Some(b"PE\0\0") {
        return Err(Header);
    }
    let coff = pe.checked_add(4).ok_or(Header)?;
    let sections = u16_at(image, coff + 2).ok_or(Header)? as usize;
    if sections == 0 || sections > 96 { return Err(Section); }
    let opt_size = u16_at(image, coff + 16).ok_or(Header)? as usize;
    let opt = coff.checked_add(20).ok_or(Header)?;
    let opt_end = opt.checked_add(opt_size).filter(|&end| end <= image.len()).ok_or(Header)?;
    let directory = match u16_at(image, opt).ok_or(Header)? {
        0x10b => opt + 96,
        0x20b => opt + 112,
        _ => return Err(Header),
    };
    if directory > opt_end { return Err(Header); }
    let count = u32_at(image, directory - 4).ok_or(Header)? as usize;
    if count > (opt_end - directory) / 8 { return Err(Header); }
    let headers = u32_at(image, opt + 60).ok_or(Header)? as usize;
    let checksum = opt + 64;
    let table_end = opt_end.checked_add(sections * 40).ok_or(Header)?;
    if table_end > headers || headers > image.len() || checksum + 4 > opt_end {
        return Err(Header);
    }
    let security = if count > 4 { Some(directory + 32) } else { None };
    let (certificate, cert_size) = if let Some(off) = security {
        (u32_at(image, off).ok_or(Header)? as usize, u32_at(image, off + 4).ok_or(Header)? as usize)
    } else { (0, 0) };
    let content_end = if certificate == 0 && cert_size == 0 {
        image.len()
    } else {
        if certificate == 0 || cert_size < 8 || certificate % 8 != 0
            || cert_size % 8 != 0 || certificate < headers
            || certificate.checked_add(cert_size) != Some(image.len()) {
            return Err(Certificate);
        }
        certificate
    };
    // At most 96 sections; no allocator is required in the integrity path.
    let mut ranges = [(0usize, 0usize); 96];
    let mut n = 0;
    for index in 0..sections {
        let section = opt_end + index * 40;
        let size = u32_at(image, section + 16).ok_or(Section)? as usize;
        let start = u32_at(image, section + 20).ok_or(Section)? as usize;
        if size == 0 { continue; }
        let end = start.checked_add(size).filter(|&end| end <= content_end).ok_or(Section)?;
        if start < headers { return Err(Section); }
        ranges[n] = (start, end);
        n += 1;
    }
    ranges[..n].sort_unstable_by_key(|range| range.0);
    let mut end = headers;
    for &(start, next) in &ranges[..n] {
        if start != end { return Err(UnsupportedLayout); }
        end = next;
    }
    if content_end < end { return Err(Certificate); }
    let mut hash = Sha256::new();
    hash.update(&image[..checksum]);
    let after_checksum = checksum + 4;
    if let Some(off) = security {
        hash.update(&image[after_checksum..off]);
        hash.update(&image[off + 8..content_end]);
    } else {
        hash.update(&image[after_checksum..content_end]);
    }
    Ok(hash.finalize())
}

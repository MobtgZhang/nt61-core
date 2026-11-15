//! Pure checksum and sidecar decoding primitives, independent of firmware I/O.
extern crate alloc;

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn hex_decode(text: &str) -> Option<alloc::vec::Vec<u8>> {
    let bytes = text.as_bytes();
    let mut digits = bytes
        .iter()
        .copied()
        .filter(|&b| b != b' ' && b != b'\r' && b != b'\n');
    let mut out = alloc::vec::Vec::new();
    while let Some(hi) = digits.next() {
        // An unmatched final digit is invalid, including after whitespace.
        let lo = digits.next()?;
        out.push(hex_val(hi)? * 16 + hex_val(lo)?);
    }
    Some(out)
}

/// Standard PE CheckSum algorithm (CheckSumMappedFile): sum every u16
/// word with the entire stored 4-byte checksum field excluded, fold
/// the carry, then add the file length.
pub fn pe_file_checksum(image: &[u8], checksum_field_off: usize) -> u32 {
    let mut sum: u64 = 0;
    let mut i = 0;
    while i + 2 <= image.len() {
        let word = if i == checksum_field_off || i == checksum_field_off + 2 {
            0
        } else {
            u32::from(image[i]) | (u32::from(image[i + 1]) << 8)
        };
        sum = sum + u64::from(word);
        sum = (sum & 0xFFFF) + (sum >> 16);
        i += 2;
    }
    if i < image.len() {
        sum = sum + u64::from(image[i]);
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    sum = (sum & 0xFFFF) + (sum >> 16);
    sum.wrapping_add(image.len() as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn odd_or_invalid_hex_is_never_silently_truncated() {
        assert_eq!(hex_decode("ab cd\r\n"), Some(alloc::vec![0xab, 0xcd]));
        for text in ["a", "abc", "ab c \n", "gg", "001z"] {
            assert!(hex_decode(text).is_none());
        }
    }
    #[test]
    fn checksum_excludes_both_words_and_handles_odd_file_lengths() {
        let bytes = [1, 0, 0xaa, 0xbb, 0xcc, 0xdd, 2];
        assert_eq!(pe_file_checksum(&bytes, 2), 10);
        let mut changed = bytes;
        changed[2..6].fill(0);
        assert_eq!(pe_file_checksum(&changed, 2), 10);
    }
}

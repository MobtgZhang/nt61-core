#![allow(dead_code, unused_imports)]
extern crate alloc;
// The same cryptography source used by ntoskrnl, without kernel services.
#[path = "../../crypto/src/crypto/hash.rs"]
pub mod hash;
mod crypto { pub use crate::hash; }
#[path = "../src/drivers/ci/mod.rs"]
mod ci;

fn hex(text: &str) -> [u8; 32] {
    let mut out = [0; 32];
    for (i, part) in text.as_bytes().chunks_exact(2).enumerate() {
        out[i] = u8::from_str_radix(core::str::from_utf8(part).unwrap(), 16).unwrap();
    }
    out
}
fn pe(plus: bool) -> Vec<u8> {
    let mut image = vec![0; 0x428];
    image[..2].copy_from_slice(b"MZ");
    image[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    image[0x80..0x84].copy_from_slice(b"PE\0\0");
    image[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
    let size = if plus { 0xf0u16 } else { 0xe0 };
    image[0x94..0x96].copy_from_slice(&size.to_le_bytes());
    image[0x98..0x9a].copy_from_slice(&(if plus {0x20bu16} else {0x10b}).to_le_bytes());
    image[0xd4..0xd8].copy_from_slice(&0x200u32.to_le_bytes());
    let dir = 0x98 + if plus {112} else {96};
    image[dir-4..dir].copy_from_slice(&16u32.to_le_bytes());
    image[dir+32..dir+36].copy_from_slice(&0x420u32.to_le_bytes());
    image[dir+36..dir+40].copy_from_slice(&8u32.to_le_bytes());
    let section = 0x98+usize::from(size);
    image[section+16..section+20].copy_from_slice(&0x200u32.to_le_bytes());
    image[section+20..section+24].copy_from_slice(&0x200u32.to_le_bytes());
    image[0x200..0x400].fill(0x42);
    image[0x400..0x420].fill(0x13); // non-certificate overlay is authenticated
    image[0x420..].copy_from_slice(&[8,0,0,0,0,2,2,0]);
    image
}
#[test]
fn standard_sha256_vectors_and_incremental_block_boundaries() {
    for (input, expected) in [
        (b"".as_slice(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        (b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        (b"Hello, World!", "dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f"),
    ] { assert_eq!(ci::compute_sha256(input),hex(expected)); }
    let data = vec![b'a'; 1_000_000];
    let expected = hex("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    assert_eq!(ci::compute_sha256(&data), expected);
    for size in [1, 55, 56, 63, 64, 65, 127] {
        let mut state = ci::Sha256::new();
        for part in data.chunks(size) { state.update(part); }
        assert_eq!(state.finalize(), expected);
    }
    assert!(ci::smoke_test());
}
#[test]
fn authenticode_exclusions_cover_headers_sections_and_overlay() {
    for plus in [false, true] {
        let mut image=pe(plus);
        let digest=ci::compute_pe_hash(&image).unwrap();
        let dir=0x98+if plus{112}else{96};
        // Independently construct the exact authenticated byte stream.
        let mut bytes=image[..0xd8].to_vec();
        bytes.extend_from_slice(&image[0xdc..dir+32]);
        bytes.extend_from_slice(&image[dir+40..0x420]);
        assert_eq!(digest, ci::compute_sha256(&bytes));
        image[0xd8..0xdc].fill(0x77);
        image[0x420..].fill(0x99);
        assert_eq!(ci::compute_pe_hash(&image).unwrap(),digest);
        for offset in [2, 0x40, 0x1ff, 0x210, 0x400] {
            let mut tampered=image.clone(); tampered[offset]^=1;
            assert_ne!(ci::compute_pe_hash(&tampered).unwrap(),digest);
            assert_ne!(ci::validate_pe_image_with_hashes(&tampered,&[digest]),0);
        }
        assert_eq!(ci::validate_pe_image_with_hashes(&image,&[digest]),0);
        assert_eq!(ci::validate_pe_image(&image),0xc0000428);
        assert_eq!(ci::se_ci_check_image(&image),0xc0000428);
    }
}
#[test]
fn malformed_or_ambiguous_files_cannot_acquire_a_digest() {
    let image=pe(true);
    for len in 0..image.len() { assert!(ci::compute_pe_hash(&image[..len]).is_err()); }
    for (offset, value) in [(0x3c,u32::MAX),(0xd4,0x80),(0x128,0x210), (0x12c,0), (0x19c,0x210),(0x198,0x400)] {
        let mut bad=image.clone();bad[offset..offset+4].copy_from_slice(&value.to_le_bytes());
        assert!(ci::compute_pe_hash(&bad).is_err(),"offset {offset:x}");
    }
    assert_eq!(ci::validate_pe_image(b"bad"),0xc000007b);
}

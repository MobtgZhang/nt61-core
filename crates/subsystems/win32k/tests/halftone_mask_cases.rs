use super::*;
use palette::halftone::nt_gdi_ht_get_8bpp_mask_palette as get_mask;

fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}

#[test]
fn halftone_mask_all_byte_masks_match_original_win7_machine_code_outputs() {
    let mut bytes = [0xa5u8; 1024];
    let pointer = bytes.as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, bytes.len(), true);
    // Golden digests cover return DWORD + all 1024 output bytes for each of
    // the 256 CMYMask values. Obtained by executing the original Win7 helper
    // (RVA 0x16f7d0, source SHA256 20ada396709e694b47976a269fe0c1c640192eed9e39ec288157aeffa8e9ad4b)
    // in Unicorn, independently of this Rust implementation.
    for (reverse, expected) in [(false, 0x30c592f3aa393d39u64), (true, 0x32f7d3750616896d)] {
        let mut hash = 0xcbf29ce484222325;
        for mask in 0..=255 {
            bytes.fill(0xa5);
            if reverse {
                bytes[..4].copy_from_slice(b"RGB0");
            }
            let result = call(get_mask, &[pointer, 1, mask, 10_000, 22_000, 65_535]);
            std::hint::black_box(&mut bytes);
            assert_eq!(call(get_mask, &[0, 1, mask, 0, 1, 2]), result);
            fnv1a(&mut hash, &(result as u32).to_le_bytes());
            fnv1a(&mut hash, &bytes);
        }
        assert_eq!(hash, expected, "RGB0 marker present: {reverse}");
    }
}

#[test]
fn halftone_mask_centers_and_reverses_rgb0_and_preserves_format_tail() {
    let mut guarded = [0xa5u8; 1026];
    let pointer = guarded[1..].as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, 1024, true);
    guarded[1..5].copy_from_slice(b"RGB0");
    assert_eq!(call(get_mask, &[pointer, 1, 1, 0, 0, 0]), 256);
    std::hint::black_box(&mut guarded);
    let bytes = &guarded[1..1025];
    assert_eq!(
        &bytes[126 * 4..130 * 4],
        &[128, 128, 64, 0, 128, 128, 128, 0, 128, 128, 128, 0, 128, 128, 191, 0]
    );
    assert_eq!(&bytes[..65 * 4], &[0; 65 * 4]);
    assert_eq!(guarded[0], 0xa5);
    assert_eq!(guarded[1025], 0xa5);

    guarded[1..1025].fill(0xa5);
    // BOOL is 32-bit; a set high DWORD alone is false, and mask is ignored.
    assert_eq!(call(get_mask, &[pointer, 1u64 << 32, 3, 65_535, 0, 1]), 216);
    std::hint::black_box(&mut guarded);
    assert_eq!(&guarded[1..9], &[0, 0, 0, 0, 51, 0, 0, 0]);
    assert_eq!(&guarded[861..865], &[255, 255, 255, 0]);
    assert_eq!(&guarded[865..1025], &[0xa5; 160]);
    // CMYMask is a BYTE, including when the register's upper bits are set.
    assert_eq!(call(get_mask, &[0, u32::MAX as u64, 0x101, 0, 0, 0]), 256);
    assert_eq!(call(get_mask, &[0, 1, 0x103, 0, 0, 0]), 0);
}

#[test]
fn halftone_mask_captures_full_input_and_rejects_bad_output() {
    assert_eq!(call(get_mask, &[1, 1, 3, 0, 0, 0]), 0);
    assert_eq!(call(get_mask, &[1, 1, 1, 0, 0, 0]), 0);
    let read_only = [0xacu8; 1024];
    let read_only_pointer = read_only.as_ptr() as u64;
    mm::user_copy::allow(read_only_pointer, read_only.len(), false);
    assert_eq!(call(get_mask, &[read_only_pointer, 1, 0, 0, 0, 0]), 0);
    assert_eq!(read_only, [0xac; 1024]);
    let mut short = [0xbdu8; 1024];
    let pointer = short.as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, 1023, true);
    assert_eq!(call(get_mask, &[pointer, 1, 2, 0, 0, 0]), 0);
    std::hint::black_box(&mut short);
    assert_eq!(short, [0xbd; 1024]);
    let mut format = [0xbeu8; 864];
    let format_pointer = format.as_mut_ptr() as u64;
    mm::user_copy::allow(format_pointer, 863, true);
    assert_eq!(call(get_mask, &[format_pointer, 0, 0, 0, 0, 0]), 0);
    std::hint::black_box(&mut format);
    assert_eq!(format, [0xbe; 864]);
}

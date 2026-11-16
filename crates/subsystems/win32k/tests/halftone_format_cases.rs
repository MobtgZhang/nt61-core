use super::*;
use palette::halftone::nt_gdi_ht_get_8bpp_format_palette as get_format;

#[test]
fn halftone_format_queries_count_and_ignores_gamma_parameters() {
    assert_eq!(call(get_format, &[0, 0, 0, 0]), 216);
    assert_eq!(call(get_format, &[0, u64::MAX, 1, 22_000]), 216);

    let mut first = [0xabu8; 864];
    let mut second = [0xcdu8; 864];
    let first_pointer = first.as_mut_ptr() as u64;
    let second_pointer = second.as_mut_ptr() as u64;
    mm::user_copy::allow(first_pointer, first.len(), true);
    mm::user_copy::allow(second_pointer, second.len(), true);
    assert_eq!(call(get_format, &[first_pointer, 0, 0, 0]), 216);
    assert_eq!(
        call(get_format, &[second_pointer, 10_000, 22_000, 65_535]),
        216
    );
    std::hint::black_box(&mut first);
    std::hint::black_box(&mut second);
    assert_eq!(first, second);
}

#[test]
fn halftone_format_writes_exact_rgb_cube_with_red_varying_fastest() {
    // PALETTEENTRY is byte-aligned. Guard bytes also detect an oversized copy.
    let mut guarded = [0xa5u8; 866];
    let pointer = guarded[1..].as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, 864, true);
    assert_eq!(call(get_format, &[pointer, 0, 0, 0]), 216);
    std::hint::black_box(&mut guarded);
    assert_eq!(guarded[0], 0xa5);
    assert_eq!(guarded[865], 0xa5);
    let levels = [0u8, 51, 102, 153, 204, 255];
    let mut entries = guarded[1..865].chunks_exact(4);
    for blue in levels {
        for green in levels {
            for red in levels {
                assert_eq!(entries.next(), Some([red, green, blue, 0].as_slice()));
            }
        }
    }
    assert_eq!(entries.next(), None);
}

#[test]
fn halftone_format_rejects_unwritable_or_short_output() {
    assert_eq!(call(get_format, &[1, 0, 0, 0]), 0);
    let read_only = [0xacu8; 864];
    let read_only_pointer = read_only.as_ptr() as u64;
    mm::user_copy::allow(read_only_pointer, read_only.len(), false);
    assert_eq!(call(get_format, &[read_only_pointer, 0, 0, 0]), 0);
    assert_eq!(read_only, [0xac; 864]);
    let mut short = [0xbdu8; 864];
    let short_pointer = short.as_mut_ptr() as u64;
    mm::user_copy::allow(short_pointer, 863, true);
    assert_eq!(call(get_format, &[short_pointer, 0, 0, 0]), 0);
    std::hint::black_box(&mut short);
    assert_eq!(short, [0xbd; 864]);
}

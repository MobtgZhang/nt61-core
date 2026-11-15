//! Host harness for the production bitmap code and its real native wrappers.
//! rustc --edition=2021 --test crates/ntoskrnl/tests/native_bitmap.rs -o /tmp/nt61-bitmap-tests
#![allow(dead_code)]
#[path = "../src/native_exports.rs"]
mod api;
#[path = "../../libs/ntdll/src/ntdll/rtl_bitmap.rs"]
mod internal;
use api::RtlBitmap;

type Search = unsafe extern "efiapi" fn(*mut RtlBitmap, u32, u32) -> u32;
const CLEAR: Search = api::nt61_export_RtlFindClearBits;
const SET: Search = api::nt61_export_RtlFindSetBits;

fn search(function: Search, size: u32, mask: u32, count: u32, hint: u32) -> u32 {
    let mut storage = [mask, 0x8765_4321];
    let original = storage;
    let mut bitmap = RtlBitmap::new();
    // SAFETY: The live backing words cover every bit used by this helper.
    unsafe {
        api::nt61_export_RtlInitializeBitMap(&mut bitmap, storage.as_mut_ptr(), size);
        let result = function(&mut bitmap, count, hint);
        assert_eq!(storage, original, "search must not mutate the bitmap");
        result
    }
}

#[test]
fn layout_and_initialize_follow_native_64_bit_contract() {
    assert_eq!(core::mem::size_of::<RtlBitmap>(), 16);
    assert_eq!(core::mem::align_of::<RtlBitmap>(), 8);
    assert_eq!(core::mem::offset_of!(RtlBitmap, buffer), 8);
    let mut words = [0x1357_2468, 0xdead_beef];
    let mut bitmap = RtlBitmap::new();
    unsafe {
        api::nt61_export_RtlInitializeBitMap(&mut bitmap, words.as_mut_ptr(), 35);
    }
    assert_eq!(bitmap.size_of_bit_map, 35);
    assert_eq!(bitmap.buffer, words.as_mut_ptr());
    assert_eq!(words, [0x1357_2468, 0xdead_beef]);
    unsafe {
        api::nt61_export_RtlInitializeBitMap(&mut bitmap, core::ptr::null_mut(), 0);
        assert_eq!(CLEAR(&mut bitmap, 0, u32::MAX), 0);
        assert_eq!(SET(&mut bitmap, 1, 0), u32::MAX);
    }
    assert_eq!(bitmap.size_of_bit_map, 0);
    assert!(bitmap.buffer.is_null());
}

#[test]
fn searches_wrap_and_can_cross_hint_but_never_bitmap_end() {
    // Only run is [2,6), with hint inside it. Restarting at zero must find it.
    assert_eq!(search(CLEAR, 8, 0b1100_0011, 4, 5), 2);
    assert_eq!(search(SET, 8, 0b0011_1100, 4, 5), 2);
    // Tail and head have two matching bits each, not a contiguous run of four.
    assert_eq!(search(CLEAR, 8, 0b0011_1100, 4, 5), u32::MAX);
    assert_eq!(search(SET, 8, 0b1100_0011, 4, 5), u32::MAX);
    assert_eq!(search(SET, 8, 0b0000_0011, 2, 7), 0);
}

#[test]
fn zero_count_aligns_normalized_hint_without_reading_buffer() {
    let mut bitmap = RtlBitmap {
        size_of_bit_map: 100,
        buffer: core::ptr::null_mut(),
    };
    for function in [CLEAR, SET] {
        for hint in [0, 7, 8, 15, 99, 100, u32::MAX] {
            let normalized = if hint < 100 { hint } else { 0 };
            assert_eq!(unsafe { function(&mut bitmap, 0, hint) }, normalized & !7);
        }
    }
}

#[test]
fn out_of_range_hints_restart_and_impossible_counts_do_not_read_buffer() {
    for hint in [8, 9, u32::MAX] {
        assert_eq!(search(CLEAR, 8, 0b0011_1100, 2, hint), 0);
        assert_eq!(search(SET, 8, 0b1100_0011, 2, hint), 0);
    }
    let mut bitmap = RtlBitmap {
        size_of_bit_map: 8,
        buffer: core::ptr::null_mut(),
    };
    for function in [CLEAR, SET] {
        assert_eq!(unsafe { function(&mut bitmap, 9, 7) }, u32::MAX);
        assert_eq!(
            unsafe { function(&mut bitmap, u32::MAX, u32::MAX) },
            u32::MAX
        );
    }
}

#[test]
fn word_boundaries_and_unused_storage_are_respected() {
    let mut words = [0x7fff_ffff, 0xffff_fffe, 0];
    let mut bitmap = RtlBitmap {
        size_of_bit_map: 64,
        buffer: words.as_mut_ptr(),
    };
    assert_eq!(unsafe { CLEAR(&mut bitmap, 2, 31) }, 31);
    assert_eq!(unsafe { CLEAR(&mut bitmap, 3, 31) }, u32::MAX);
    bitmap.size_of_bit_map = 33;
    assert_eq!(unsafe { CLEAR(&mut bitmap, 2, 32) }, 31);
    assert_eq!(search(CLEAR, 5, 0b0001_1111, 1, 0), u32::MAX);
    assert_eq!(search(SET, 5, 0xffff_ffe0, 1, 0), u32::MAX);
}

#[test]
fn exhaustive_small_maps_match_ordered_run_contract() {
    for size in 0..=8u32 {
        for mask in 0..(1u32 << size) {
            for count in 0..=size + 1 {
                for hint in 0..=size + 1 {
                    let normalized = if hint < size { hint } else { 0 };
                    for (function, set) in [(CLEAR, false), (SET, true)] {
                        // Independent oracle enumerates candidate starts and
                        // checks the complete range; it carries no run state.
                        let expected = if count == 0 {
                            normalized & !7
                        } else if count > size {
                            u32::MAX
                        } else {
                            (normalized..size)
                                .chain(0..normalized)
                                .find(|&start| {
                                    count <= size - start
                                        && (start..start + count)
                                            .all(|bit| ((mask >> bit) & 1 != 0) == set)
                                })
                                .unwrap_or(u32::MAX)
                        };
                        assert_eq!(
                            search(function, size, mask, count, hint),
                            expected,
                            "size={size}, mask={mask:x}, count={count}, hint={hint}, set={set}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn internal_production_entry_points_share_the_fixed_semantics() {
    let mut word = 0b1100_0011;
    let mut bitmap = internal::RtlBitmap {
        size_of_bit_map: 8,
        buffer: &mut word,
    };
    unsafe {
        assert_eq!(internal::rtl_find_clear_bits(&mut bitmap, 4, 5), 2);
        assert_eq!(internal::rtl_find_set_bits(&mut bitmap, 4, 5), u32::MAX);
    }
}

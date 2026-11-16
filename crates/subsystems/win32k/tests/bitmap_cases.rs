//! Logical bitmap dimensions are metadata, independent of storage dimensions.
use super::*;
use bitmap::storage::{
    nt_gdi_create_bitmap as create_bitmap, nt_gdi_get_bitmap_bits as get_bits,
    nt_gdi_set_bitmap_bits as set_bits,
};
use bitmap::{nt_gdi_get_bitmap_dimension as get_size, nt_gdi_set_bitmap_dimension as set_size};
use dc::services::nt_gdi_set_brush_org as set_origin;

fn input(bytes: &[u8]) -> u64 {
    let address = bytes.as_ptr() as u64;
    mm::user_copy::allow(address, bytes.len(), false);
    address
}

fn output(bytes: &mut [u8]) -> u64 {
    let address = bytes.as_mut_ptr() as u64;
    mm::user_copy::allow(address, bytes.len(), true);
    address
}

#[test]
fn bitmap_storage_rejects_overflows_and_invalid_formats_without_allocating() {
    for (width, height, depth) in [
        (0, 1, 1),
        (-1, 1, 1),
        (1, 0, 1),
        (1, i32::MIN, 1),
        (i32::MAX, 1, 32),
        (1, 1, 3),
        (1, 1, u16::MAX),
    ] {
        assert!(objects::GdiBitmap::new(width, height, depth).is_none());
    }
    for depth in [1, 4, 8, 16, 24, 32] {
        let bitmap = objects::GdiBitmap::new(5, 3, depth).unwrap();
        assert_eq!(bitmap.width_bytes % 4, 0);
        // SAFETY: the bitmap owns size initialized bytes until it is dropped.
        let bytes = unsafe { std::slice::from_raw_parts(bitmap.bits, bitmap.size as usize) };
        assert!(bytes.iter().all(|&byte| byte == 0));
    }
}

#[test]
fn bitmap_creation_allocation_failures_release_every_owned_allocation() {
    for failures_after in [0, 1] {
        let before = mm::pool::balance();
        mm::pool::fail_after(failures_after);
        assert_eq!(objects::gdi_create_bitmap(8, 8, 1), 0);
        assert_eq!(mm::pool::balance(), before);
    }
}

#[test]
fn bitmap_bits_use_three_arguments_and_word_aligned_scanlines() {
    for (width, depth, bytes) in [
        (9, 1, vec![0xa5, 0x80, 0x5a, 0x00]),
        (3, 8, vec![1, 2, 3, 0, 4, 5, 6, 0]),
        (1, 24, vec![1, 2, 3, 0, 4, 5, 6, 0]),
        (1, 32, vec![1, 2, 3, 4, 5, 6, 7, 8]),
    ] {
        let handle = call(create_bitmap, &[width, 2, 1, depth, input(&bytes)]);
        assert_ne!(handle, 0);
        assert_eq!(call(get_bits, &[handle, 0, 0]), bytes.len() as u64);
        let mut copied = vec![0xab; bytes.len() + 2];
        let address = output(&mut copied[1..bytes.len() + 1]);
        assert_eq!(
            call(get_bits, &[handle, u32::MAX as u64, address]),
            bytes.len() as u64
        );
        std::hint::black_box(&mut copied);
        assert_eq!(&copied[1..bytes.len() + 1], bytes);
        assert_eq!(copied[0], 0xab);
        assert_eq!(copied[bytes.len() + 1], 0xab);
        assert!(objects::gdi_delete_object(handle));
    }
}

#[test]
fn bitmap_partial_transfers_preserve_remaining_bytes_and_skip_internal_padding() {
    let handle = objects::gdi_create_bitmap(9, 3, 1);
    let bytes = [0xaa, 0x80, 0x55];
    assert_eq!(
        call(set_bits, &[handle, 0xdead_beef_0000_0003, input(&bytes)]),
        3
    );
    let mut copied = [0xab; 8];
    let address = output(&mut copied);
    assert_eq!(call(get_bits, &[handle, 0xface_feed_0000_0006, address]), 6);
    std::hint::black_box(&mut copied);
    assert_eq!(copied, [0xaa, 0x80, 0x55, 0, 0, 0, 0xab, 0xab]);
    assert_eq!(call(get_bits, &[handle, 1, address + 7]), 1);
    std::hint::black_box(&mut copied);
    assert_eq!(copied[7], 0xaa);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn bitmap_transfer_faults_and_wrong_types_do_not_change_stored_pixels() {
    let handle = objects::gdi_create_bitmap(1, 1, 32);
    let brush = objects::gdi_create_solid_brush(0);
    let bytes = [1, 2, 3, 4];
    assert_eq!(call(set_bits, &[handle, 4, input(&bytes)]), 4);
    assert_eq!(call(set_bits, &[handle, 4, 1]), 0);
    let short = [9, 9, 9];
    assert_eq!(call(set_bits, &[handle, 4, input(&short)]), 0);
    assert_eq!(call(get_bits, &[handle, 4, 1]), 0);
    assert_eq!(call(get_bits, &[handle, 4, input(&bytes)]), 0);
    assert_eq!(call(set_bits, &[handle, 4, 0]), 0);
    let mut copied = [0; 4];
    assert_eq!(call(get_bits, &[handle, 4, output(&mut copied)]), 4);
    std::hint::black_box(&mut copied);
    assert_eq!(copied, bytes);
    assert!(objects::gdi_delete_object(handle));
    for invalid in [handle, brush, 0, u64::MAX] {
        assert_eq!(call(get_bits, &[invalid, 4, 0]), 0);
        assert_eq!(call(set_bits, &[invalid, 4, input(&bytes)]), 0);
    }
    assert!(objects::gdi_delete_object(brush));
}

#[test]
fn native_bitmap_creation_validates_format_and_cleans_up_on_input_faults() {
    let before = mm::pool::balance();
    assert_eq!(call(create_bitmap, &[2, 2, 1, 32, 1]), 0);
    assert_eq!(mm::pool::balance(), before);
    for arguments in [
        [2, 2, 2, 32, 0],
        [2, 2, 1, 33, 0],
        [0, 2, 1, 32, 0],
        [2, 2, 1, 0x1_0000_0020, 0],
    ] {
        // ULONG arguments use their low DWORD; the last case is valid 32 bpp.
        let result = call(create_bitmap, &arguments);
        if arguments[3] > u32::MAX as u64 {
            assert_ne!(result, 0);
            assert!(objects::gdi_delete_object(result));
        } else {
            assert_eq!(result, 0);
        }
    }
}

#[test]
fn monochrome_query_checks_the_live_bitmap_format() {
    let mono = objects::gdi_create_bitmap(8, 8, 1);
    let color = objects::gdi_create_bitmap(8, 8, 32);
    let brush = objects::gdi_create_solid_brush(0);
    assert_ne!(mono, 0);
    assert_eq!(call(bitmap::nt_gdi_mono_bitmap, &[mono]), 1);
    for handle in [0, u64::MAX, color, brush] {
        assert_eq!(call(bitmap::nt_gdi_mono_bitmap, &[handle]), 0);
    }
    assert!(objects::gdi_delete_object(mono));
    assert_eq!(call(bitmap::nt_gdi_mono_bitmap, &[mono]), 0);
    assert!(objects::gdi_delete_object(color));
    assert!(objects::gdi_delete_object(brush));
}

fn writable<T>(value: &mut T) -> u64 {
    let address = value as *mut T as u64;
    mm::user_copy::allow(address, core::mem::size_of::<T>(), true);
    address
}

fn dimensions(handle: u64) -> (u64, (i32, i32)) {
    let mut output = dc::SizeL::new(-99, -99);
    let pointer = writable(&mut output);
    let result = call(get_size, &[handle, pointer]);
    std::hint::black_box(&mut output);
    (result, (output.cx, output.cy))
}

#[test]
fn logical_dimensions_default_to_zero_and_do_not_resize_pixels() {
    let handle = objects::gdi_create_bitmap(5, 3, 32);
    assert_ne!(handle, 0);
    assert_eq!(dimensions(handle), (1, (0, 0)));
    let storage = || {
        objects::with_handle_table(|table| {
            let entry = table.get(handle).unwrap();
            // SAFETY: the table guard keeps this live bitmap allocation valid.
            let bitmap = unsafe { &*(entry.obj_ptr as *const objects::GdiBitmap) };
            (
                bitmap.width,
                bitmap.height,
                bitmap.size,
                bitmap.bits as usize,
            )
        })
    };
    let before = storage();
    let mut old = dc::SizeL::new(-99, -99);
    let pointer = writable(&mut old);
    assert_eq!(call(set_size, &[handle, 123, 456, pointer]), 1);
    assert_eq!((old.cx, old.cy), (0, 0));
    assert_eq!(dimensions(handle), (1, (123, 456)));
    assert_eq!(storage(), before);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn logical_dimensions_accept_signed_values_and_truncate_int_slots() {
    let handle = objects::gdi_create_bitmap(1, 1, 32);
    assert_eq!(
        call(
            set_size,
            &[handle, 0x1122_3344_8000_0000, 0x5566_7788_7fff_ffff, 0]
        ),
        1
    );
    assert_eq!(dimensions(handle), (1, (i32::MIN, i32::MAX)));
    assert_eq!(call(set_size, &[handle, (-12i32) as u64, 0, 0]), 1);
    assert_eq!(dimensions(handle), (1, (-12, 0)));
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn bitmap_output_failure_does_not_undo_the_new_dimensions() {
    let handle = objects::gdi_create_bitmap(2, 2, 32);
    assert_eq!(call(set_size, &[handle, 10, 20, 1]), 0);
    assert_eq!(dimensions(handle), (1, (10, 20)));
    let mut old = dc::SizeL::new(-99, -99);
    let pointer = &mut old as *mut _ as u64;
    mm::user_copy::allow(pointer, 8, false);
    assert_eq!(call(set_size, &[handle, 30, 40, pointer]), 0);
    assert_eq!((old.cx, old.cy), (-99, -99));
    assert_eq!(dimensions(handle), (1, (30, 40)));
    assert_eq!(call(get_size, &[handle, 0]), 0);
    assert_eq!(call(get_size, &[handle, pointer]), 0);
    assert_eq!(dimensions(handle), (1, (30, 40)));
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn x64_bitmap_dimension_outputs_allow_unaligned_addresses() {
    let handle = objects::gdi_create_bitmap(2, 2, 32);
    let mut output = [0xabu8; 10];
    let pointer = output.as_mut_ptr() as u64 + 1;
    mm::user_copy::allow(pointer, 8, true);
    assert_eq!(call(set_size, &[handle, 7, 9, pointer]), 1);
    assert_eq!(output, [0xab, 0, 0, 0, 0, 0, 0, 0, 0, 0xab]);
    assert_eq!(call(get_size, &[handle, pointer]), 1);
    assert_eq!(output, [0xab, 7, 0, 0, 0, 9, 0, 0, 0, 0xab]);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn invalid_bitmap_types_and_deleted_handles_leave_output_untouched() {
    let bitmap = objects::gdi_create_bitmap(2, 2, 32);
    let brush = objects::gdi_create_solid_brush(0);
    let dc = dc::gre_create_compatible_dc(0);
    assert!(objects::gdi_delete_object(bitmap));
    for handle in [0, u64::MAX, bitmap, brush, dc] {
        assert_eq!(dimensions(handle), (0, (-99, -99)));
        let mut old = dc::SizeL::new(-99, -99);
        let pointer = writable(&mut old);
        assert_eq!(call(set_size, &[handle, 1, 2, pointer]), 0);
        assert_eq!((old.cx, old.cy), (-99, -99));
    }
    assert!(objects::gdi_delete_object(brush));
    assert!(dc::gre_delete_dc(dc));
}

#[test]
fn different_bitmaps_keep_independent_dimensions() {
    let first = objects::gdi_create_bitmap(1, 1, 1);
    let second = objects::gdi_create_bitmap(1, 1, 32);
    assert_eq!(call(set_size, &[first, 7, 8, 0]), 1);
    assert_eq!(dimensions(second), (1, (0, 0)));
    assert_eq!(call(set_size, &[second, 9, 10, 0]), 1);
    assert_eq!(dimensions(first), (1, (7, 8)));
    assert_eq!(dimensions(second), (1, (9, 10)));
    assert!(objects::gdi_delete_object(first));
    assert!(objects::gdi_delete_object(second));
}

#[test]
fn bitmap_dimension_pairs_are_not_torn_by_concurrent_updates() {
    let handle = objects::gdi_create_bitmap(1, 1, 32);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for value in 1..=1000i32 {
                assert_eq!(
                    call(set_size, &[handle, value as u64, (-value) as u64, 0]),
                    1
                );
            }
        });
        for _ in 0..1000 {
            let (success, (x, y)) = dimensions(handle);
            assert_eq!(success, 1);
            assert_eq!(x, -y);
        }
    });
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn brush_origin_updates_old_value_and_survives_saved_dc_state() {
    let handle = dc::gre_create_compatible_dc(0);
    let mut old = dc::PointL::new(-99, -99);
    let pointer = writable(&mut old);
    assert_eq!(call(set_origin, &[handle, 7, 9, pointer]), 1);
    assert_eq!((old.x, old.y), (0, 0));
    assert_eq!(dc::gre_save_dc(handle), 1);
    assert_eq!(
        call(
            set_origin,
            &[
                handle,
                0x1122_3344_8000_0000,
                0x5566_7788_7fff_ffff,
                pointer
            ]
        ),
        1
    );
    assert_eq!((old.x, old.y), (7, 9));
    assert_eq!(call(set_origin, &[handle, 2, 3, pointer]), 1);
    assert_eq!((old.x, old.y), (i32::MIN, i32::MAX));
    assert!(dc::gre_restore_dc(handle, -1));
    assert_eq!(call(set_origin, &[handle, 0, 0, pointer]), 1);
    assert_eq!((old.x, old.y), (7, 9));
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn brush_origin_null_output_and_bad_output_preserve_commit_order() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(set_origin, &[handle, 3, 4, 0]), 1);
    assert_eq!(call(set_origin, &[handle, 5, 6, 1]), 0);
    let mut old = dc::PointL::new(-99, -99);
    let pointer = writable(&mut old);
    assert_eq!(call(set_origin, &[handle, 0, 0, pointer]), 1);
    assert_eq!((old.x, old.y), (5, 6));
    assert!(dc::gre_delete_dc(handle));
    for invalid in [0, handle, u64::MAX] {
        old = dc::PointL::new(-99, -99);
        assert_eq!(call(set_origin, &[invalid, 7, 8, pointer]), 0);
        assert_eq!((old.x, old.y), (-99, -99));
    }
}

#[test]
fn brush_origin_outputs_allow_unaligned_addresses() {
    let handle = dc::gre_create_compatible_dc(0);
    assert_eq!(call(set_origin, &[handle, 7, 9, 0]), 1);
    let mut output = [0xabu8; 10];
    let pointer = output.as_mut_ptr() as u64 + 1;
    mm::user_copy::allow(pointer, 8, true);
    assert_eq!(call(set_origin, &[handle, 0, 0, pointer]), 1);
    assert_eq!(output, [0xab, 7, 0, 0, 0, 9, 0, 0, 0, 0xab]);
    assert!(dc::gre_delete_dc(handle));
}

#[test]
fn bitmap_dimension_calls_serialize_with_deletion() {
    let handle = objects::gdi_create_bitmap(1, 1, 32);
    let ready = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            ready.wait();
            for _ in 0..1000 {
                let result = call(set_size, &[handle, 3, 4, 0]);
                assert!(result == 0 || result == 1);
                let (result, size) = dimensions(handle);
                if result == 1 {
                    assert_eq!(size, (3, 4));
                } else {
                    assert_eq!(size, (-99, -99));
                }
            }
        });
        ready.wait();
        assert!(objects::gdi_delete_object(handle));
    });
    assert_eq!(dimensions(handle), (0, (-99, -99)));
}

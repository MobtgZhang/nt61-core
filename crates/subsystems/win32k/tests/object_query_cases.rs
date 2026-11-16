use super::*;
use object_queries::{nt_gdi_ext_get_simple_object_w as get_object, Description};

#[test]
fn object_queries_match_672_original_win7_machine_code_cases() {
    let corpus = include_bytes!("data/win7-object-query.bin");
    assert_eq!(corpus.len(), 672 * 120);
    for (index, record) in corpus.chunks_exact(120).enumerate() {
        let word = |n: usize| u32::from_le_bytes(record[n * 4..n * 4 + 4].try_into().unwrap());
        let object = match word(0) {
            0 => Description::Brush {
                style: word(1),
                color: if word(1) == 1 { 0 } else { 0x123456 },
                hatch: if word(1) == 2 { 4 } else { 0 },
            },
            1 => Description::Palette {
                entries: word(2) as u16,
            },
            2 => Description::Bitmap {
                width: word(2) as i32,
                height: word(3) as i32,
                depth: word(4) as u16,
            },
            _ => unreachable!(),
        };
        let count = word(5) as i32;
        let supplied = if (0..=0x250).contains(&count) {
            count as usize
        } else {
            0x250
        };
        let result = object_queries::serialize(object, 8, supplied, word(6) == 0);
        let mut output = [0xa5u8; 40];
        let returned = if let Some((bytes, returned, copied)) = result {
            output[..copied].copy_from_slice(&bytes[..copied]);
            returned
        } else {
            0
        };
        assert_eq!(
            returned as u32,
            word(8),
            "return case {index}: {object:?}, count={count}"
        );
        for byte in 0..40 {
            assert_eq!(
                output[byte] & record[80 + byte],
                record[40 + byte] & record[80 + byte],
                "buffer case {index}, byte {byte}: {object:?}, count={count}"
            );
        }
    }
}

#[test]
fn query_abis_preserve_word_stride_null_bits_and_brush_copy_lengths() {
    for pointer_bytes in [4, 8] {
        let bitmap_size = if pointer_bytes == 8 { 32 } else { 24 };
        let brush_size = 8 + pointer_bytes;
        for (width, depth, stride) in [
            (1, 1, 2i32),
            (3, 8, 4),
            (3, 24, 10),
            (17, 1, 4),
            (17, 32, 68),
        ] {
            let bitmap = Description::Bitmap {
                width,
                height: 3,
                depth,
            };
            assert!(
                object_queries::serialize(bitmap, pointer_bytes, bitmap_size - 1, false).is_none()
            );
            let (bytes, returned, copied) =
                object_queries::serialize(bitmap, pointer_bytes, 592, false).unwrap();
            assert_eq!((returned, copied), (bitmap_size, bitmap_size));
            assert_eq!(&bytes[12..16], &stride.to_le_bytes());
            assert!(bytes[20..bitmap_size].iter().all(|&v| v == 0));
        }
        let brush = Description::Brush {
            style: 2,
            color: 7,
            hatch: 0x1234567887654321,
        };
        let (bytes, returned, copied) =
            object_queries::serialize(brush, pointer_bytes, 1, false).unwrap();
        assert_eq!((returned, copied), (brush_size, 1));
        assert_eq!(
            &bytes[8..brush_size],
            &0x1234567887654321u64.to_le_bytes()[..pointer_bytes]
        );
        assert_eq!(
            object_queries::serialize(brush, pointer_bytes, 0, true)
                .unwrap()
                .1,
            brush_size
        );
    }
    assert!(object_queries::serialize(Description::Palette { entries: 2 }, 3, 2, false).is_none());
    assert!(object_queries::serialize(
        Description::Bitmap {
            width: i32::MAX,
            height: 1,
            depth: 32
        },
        8,
        32,
        false
    )
    .is_none());
}

#[test]
fn query_real_brushes_prefix_copy_alignment_faults_and_ownership() {
    let brushes = [
        (
            objects::gdi_create_solid_brush(0x123456),
            0u32,
            0x123456u32,
            0u64,
        ),
        (objects::gdi_get_stock_object(5), 1, 0, 0),
        (
            call(
                objects::nt_gdi_create_hatch_brush_internal,
                &[4, 0x123456, 0],
            ),
            2,
            0x123456,
            4,
        ),
    ];
    for &(brush, style, color, hatch) in &brushes {
        assert_ne!(brush, 0);
        assert_eq!(call(get_object, &[brush, 0, 0]), 16);
        for count in [0, 1, 2, 12, 15, 16, 32, u32::MAX as u64] {
            let mut output = [0xa5u8; 40];
            let pointer = output.as_mut_ptr() as u64;
            mm::user_copy::allow(pointer, 40, true);
            assert_eq!(
                call(get_object, &[brush, count, pointer]),
                if count == 0 { 0 } else { 16 }
            );
            std::hint::black_box(&mut output);
            let mut expected = [0xa5u8; 40];
            if count != 0 {
                let mut bytes = [0u8; 16];
                bytes[..4].copy_from_slice(&style.to_le_bytes());
                bytes[4..8].copy_from_slice(&color.to_le_bytes());
                bytes[8..16].copy_from_slice(&hatch.to_le_bytes());
                let copied = (count as usize).min(16);
                expected[..copied].copy_from_slice(&bytes[..copied]);
            }
            assert_eq!(output, expected);
            assert_eq!(call(get_object, &[brush, 16, pointer | 1]), 0);
        }
        let read_only = [0xa5u8; 16];
        let pointer = read_only.as_ptr() as u64;
        mm::user_copy::allow(pointer, 16, false);
        assert_eq!(call(get_object, &[brush, 16, pointer]), 0);
        assert_eq!(read_only, [0xa5; 16]);
    }
    let brush = brushes[0].0;
    let mut output = [0xa5u8; 32];
    let pointer = output.as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, 32, true);
    objects::with_handle_table(|table| table.get_mut(brush).unwrap().process_id = 999);
    assert_eq!(call(get_object, &[brush, 16, pointer]), 0);
    assert_eq!(call(get_object, &[brush, 0, 0]), 0);
    objects::with_handle_table(|table| table.get_mut(brush).unwrap().process_id = 1);
    assert!(objects::gdi_delete_object(brush));
    assert_eq!(call(get_object, &[brush, 16, pointer]), 0);
    assert!(objects::gdi_delete_object(brushes[2].0));
}

#[test]
fn query_real_bitmap_and_resized_palette_never_leak_storage_or_overwrite_tail() {
    let bitmap = objects::gdi_create_bitmap(3, 3, 24);
    let palette = call(palette::nt_gdi_create_halftone_palette, &[0]);
    assert_ne!(bitmap, 0);
    assert_ne!(palette, 0);
    assert_eq!(call(palette::nt_gdi_resize_palette, &[palette, 3]), 1);
    let mut output = [0xa5u8; 112];
    let pointer = output.as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, output.len(), true);
    for (handle, size) in [(bitmap, 32), (palette, 2)] {
        assert_eq!(call(get_object, &[handle, 0, 0]), size);
        assert_eq!(call(get_object, &[handle, size - 1, pointer]), 0);
        assert_eq!(output, [0xa5; 112]);
        assert_eq!(call(get_object, &[handle, 104, pointer]), size);
        std::hint::black_box(&mut output);
        assert_eq!(&output[size as usize..], &vec![0xa5; 112 - size as usize]);
        if handle == bitmap {
            assert_eq!(&output[12..16], &10i32.to_le_bytes());
            assert_eq!(&output[24..32], &[0; 8]);
        } else {
            assert_eq!(&output[..2], &3u16.to_le_bytes());
        }
        output.fill(0xa5);
        assert!(objects::gdi_delete_object(handle));
        assert_eq!(call(get_object, &[handle, 104, pointer]), 0);
        // This size-only query intentionally precedes handle validation in Win7.
        assert_eq!(call(get_object, &[handle, 0, 0]), size);
    }
}

#[test]
fn selected_and_saved_brush_query_remains_live_until_all_dc_references_release() {
    let fixture = DrawingFixture::new();
    let brush = objects::gdi_create_solid_brush(7);
    let previous = call(selection::nt_gdi_select_brush, &[fixture.handle, brush]);
    assert_ne!(previous, 0);
    assert_eq!(dc::gre_save_dc(fixture.handle), 1);
    assert_eq!(
        call(selection::nt_gdi_select_brush, &[fixture.handle, previous]),
        brush
    );
    assert!(!objects::gdi_delete_object(brush));
    assert_eq!(call(get_object, &[brush, 0, 0]), 16);
    assert!(dc::gre_restore_dc(fixture.handle, -1));
    assert_eq!(call(get_object, &[brush, 0, 0]), 16);
    assert_eq!(
        call(selection::nt_gdi_select_brush, &[fixture.handle, previous]),
        brush
    );
    assert!(objects::gdi_delete_object(brush));
    assert_eq!(call(get_object, &[brush, 0, 0]), 0);
}

#[test]
fn aggregate_object_query_routes_pen_brush_bitmap_palette_and_rejects_other_classes() {
    let get = object_queries::nt_gdi_ext_get_object_w;
    let legacy = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 7);
    let extended = call(pen_services::nt_gdi_ext_create_pen, &[0x10000, 2, 0, 7]);
    let brush = objects::gdi_create_solid_brush(7);
    let bitmap = objects::gdi_create_bitmap(3, 3, 24);
    let palette = call(palette::nt_gdi_create_halftone_palette, &[0]);
    for (handle, size) in [
        (legacy, 16),
        (extended, 28),
        (brush, 16),
        (bitmap, 32),
        (palette, 2),
    ] {
        assert_ne!(handle, 0);
        assert_eq!(call(get, &[handle, 0, 0]), size);
        let mut output = [0xa5u8; 32];
        let pointer = output.as_mut_ptr() as u64;
        mm::user_copy::allow(pointer, 32, true);
        assert_eq!(call(get, &[handle, size, pointer]), size);
        assert!(objects::gdi_delete_object(handle));
    }
    let dc = dc::gre_create_compatible_dc(0);
    let font = objects::gdi_create_font(12, 0, 400, false, &[65]);
    assert_eq!(call(get, &[font, 0, 0]), 356);
    for handle in [dc, 0x400c0001, 0x12340001, 0] {
        assert_eq!(call(get, &[handle, 592, 0]), 0);
    }
    assert!(dc::gre_delete_dc(dc));
    assert!(objects::gdi_delete_object(font));
}

#[test]
fn aggregate_query_preserves_native_last_error_and_stock_alias_rules() {
    struct RestoreTeb(*mut u8);
    impl Drop for RestoreTeb {
        fn drop(&mut self) {
            // SAFETY: this fixture owns its calling thread's mock ETHREAD.
            unsafe {
                (*ps::thread::get_current_ethread()).kthread.teb = self.0;
            }
        }
    }
    let get = object_queries::nt_gdi_ext_get_object_w;
    let mut teb = [0u32; 32];
    let teb_pointer = teb.as_mut_ptr().cast::<u8>();
    // SAFETY: this fixture owns its calling thread's mock ETHREAD. The guard
    // restores the pointer before the stack-backed TEB leaves scope.
    let restore = unsafe {
        let thread = &mut *ps::thread::get_current_ethread();
        let previous = core::mem::replace(&mut thread.kthread.teb, teb_pointer);
        RestoreTeb(previous)
    };
    mm::user_copy::allow(teb_pointer as u64 + 0x68, 4, true);
    let mut output = [0xa5u8; 32];
    let pointer = output.as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, 32, true);
    for (prefix, expected) in [
        (0, 0x5678),
        (0x4001, 0x5678),
        (0x4004, 0x5678),
        (0x400c, 0x5678),
        (0x4002, 0x5678),
        (0x4003, 0x5678),
        (0x4005, 87),
        (0x4006, 87),
        (0x4007, 87),
        (0x4087, 87),
        (0x401f, 0x5678),
    ] {
        teb[26] = 0x5678;
        std::hint::black_box(&mut teb);
        assert_eq!(call(get, &[(prefix << 16) | 0xffff, 32, pointer]), 0);
        std::hint::black_box(&mut teb);
        assert_eq!(teb[26], expected, "type prefix {prefix:x}");
    }
    let brush = objects::gdi_create_solid_brush(7);
    let stock = call(
        objects::attributes::nt_gdi_set_brush_attributes,
        &[brush, 1],
    );
    assert_eq!(stock, brush | 0x00800000);
    assert_eq!(call(get, &[brush, 0, 0]), 0);
    assert_eq!(call(get, &[stock, 0, 0]), 16);
    assert_eq!(
        call(
            objects::attributes::nt_gdi_clear_brush_attributes,
            &[stock, 1]
        ),
        brush
    );
    assert_eq!(call(get, &[stock, 0, 0]), 0);
    assert_eq!(call(get, &[brush, 0, 0]), 16);
    assert!(objects::gdi_delete_object(brush));
    drop(restore);
}

#[test]
fn font_and_colorspace_queries_match_60_original_win7_machine_code_cases() {
    let corpus = include_bytes!("data/win7-font-colorspace-query.bin");
    assert_eq!(corpus.len(), 60 * 1892);
    for (index, record) in corpus.chunks_exact(1892).enumerate() {
        let word = |n: usize| u32::from_le_bytes(record[n * 4..n * 4 + 4].try_into().unwrap());
        let count = word(2) as i32;
        let supplied = if (0..=592).contains(&count) {
            count as usize
        } else {
            592
        };
        let source: &[u8; 592] = record[20..612].try_into().unwrap();
        let result = if word(0) == 0 {
            object_queries::serialize_font(&source[..word(1) as usize], supplied, word(3) == 0).map(
                |(font, size)| {
                    let mut bytes = [0; 592];
                    bytes[..420].copy_from_slice(&font);
                    (bytes, size)
                },
            )
        } else {
            object_queries::serialize_color_space(source, supplied, word(3) == 0)
        };
        let mut output = [0xa5u8; 640];
        let returned = if let Some((bytes, size)) = result {
            if word(3) != 0 {
                output[..size].copy_from_slice(&bytes[..size]);
            }
            size
        } else {
            0
        };
        assert_eq!(returned as u32, word(4), "case {index}");
        for byte in 0..640 {
            let mask = record[1252 + byte];
            assert_eq!(
                output[byte] & mask,
                record[612 + byte] & mask,
                "case {index}, byte {byte}"
            );
        }
        if word(0) == 1 && returned != 0 {
            assert!(output[328..588].iter().all(|&byte| byte == 0));
        }
    }
}

#[test]
fn logical_font_capture_matches_65_original_win7_axis_and_length_cases() {
    let corpus = include_bytes!("data/win7-hfont-capture.bin");
    assert_eq!(corpus.len(), 65 * 856);
    let get = object_queries::nt_gdi_ext_get_object_w;
    for (index, record) in corpus.chunks_exact(856).enumerate() {
        let word = |n: usize| u32::from_le_bytes(record[n * 4..n * 4 + 4].try_into().unwrap());
        let mut source: [u8; 420] =
            core::array::from_fn(|n| (n as u8).wrapping_mul(11).wrapping_add(7));
        source[352..356].copy_from_slice(&word(1).to_le_bytes());
        let pointer = source.as_ptr() as u64;
        mm::user_copy::allow(pointer, source.len(), false);
        let font = call(
            font_services::nt_gdi_hfont_create,
            &[pointer, word(0) as u64, 0, 0, 0],
        );
        assert_eq!(font != 0, word(2) != 0, "case {index}");
        if font == 0 {
            continue;
        }
        let mut expected = [0u8; 356];
        expected[..(word(0) as usize).min(356)]
            .copy_from_slice(&source[..(word(0) as usize).min(356)]);
        expected[352..356].fill(0);
        source.fill(0); // Queries must be independent of the input allocation.
        let mut output = [0xa5u8; 420];
        let address = output.as_mut_ptr() as u64;
        mm::user_copy::allow(address, output.len(), true);
        assert_eq!(call(get, &[font, 0, 0]), word(3) as u64);
        assert_eq!(call(get, &[font, 420, address]), 356);
        std::hint::black_box(&mut output);
        for byte in 0..356 {
            assert_eq!(
                output[byte] & record[436 + byte],
                record[16 + byte] & record[436 + byte],
                "case {index}, byte {byte}"
            );
        }
        assert_eq!(&output[..356], &expected);
        assert_eq!(&output[356..], &[0xa5; 64]);
        assert!(objects::gdi_delete_object(font));
    }
}

#[test]
fn font_query_preserves_full_description_prefixes_and_failed_outputs() {
    let get = object_queries::nt_gdi_ext_get_object_w;
    let mut input: [u8; 420] = core::array::from_fn(|n| (n as u8).wrapping_mul(7).wrapping_add(1));
    let mut expected = input[..356].to_vec();
    expected[352..356].fill(0);
    mm::user_copy::allow(input.as_ptr() as u64, input.len(), false);
    let font = call(
        font_services::nt_gdi_hfont_create,
        &[input.as_ptr() as u64, 420, 0, 0, 0],
    );
    assert_ne!(font, 0);
    input.fill(0);
    for count in [
        0,
        1,
        91,
        92,
        93,
        347,
        348,
        351,
        352,
        355,
        356,
        420,
        592,
        u32::MAX as u64,
    ] {
        let mut output = [0xa5u8; 608];
        let pointer = output.as_mut_ptr() as u64;
        mm::user_copy::allow(pointer, output.len(), true);
        let copied = (count as usize).min(356);
        assert_eq!(call(get, &[font, count, pointer]), copied as u64);
        std::hint::black_box(&mut output);
        assert_eq!(&output[..copied], &expected[..copied]);
        assert!(output[copied..].iter().all(|&byte| byte == 0xa5));
        assert_eq!(call(get, &[font, 356, pointer | 1]), 0);
    }
    let output = [0xa5u8; 356];
    mm::user_copy::allow(output.as_ptr() as u64, output.len(), false);
    assert_eq!(call(get, &[font, 356, output.as_ptr() as u64]), 0);
    assert_eq!(output, [0xa5; 356]);
    objects::with_handle_table(|table| table.get_mut(font).unwrap().process_id = 999);
    assert_eq!(call(get, &[font, 0, 0]), 0);
    objects::with_handle_table(|table| table.get_mut(font).unwrap().process_id = 1);
    assert!(objects::gdi_delete_object(font));
    assert_eq!(call(get, &[font, 0, 0]), 0);
}

#[test]
fn colorspace_queries_capture_defined_bytes_and_zero_original_unwritten_range() {
    let get = object_queries::nt_gdi_ext_get_object_w;
    let mut input: [u32; 148] =
        core::array::from_fn(|n| (n as u32).wrapping_mul(0x1020304).wrapping_add(7));
    input[..5].copy_from_slice(&[0x50534f43, 0x400, 588, 0x73524742, 1]);
    input[147] = 0;
    let mut expected = [0u8; 592];
    for (slot, value) in expected.chunks_exact_mut(4).zip(input) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    expected[328..588].fill(0);
    mm::user_copy::allow(input.as_ptr() as u64, 592, false);
    let color = call(
        color_space::nt_gdi_create_color_space,
        &[input.as_ptr() as u64],
    );
    assert_ne!(color, 0);
    input.fill(0);
    assert_eq!(call(get, &[color, 0, 0]), 0);
    for count in [0, 1, 587, 588, 589, 590, 591, 592, 593, u32::MAX as u64] {
        let mut output = [0xa5u8; 608];
        let pointer = output.as_mut_ptr() as u64;
        mm::user_copy::allow(pointer, output.len(), true);
        let returned = if count < 588 {
            0
        } else if count < 592 {
            588
        } else {
            592
        };
        assert_eq!(call(get, &[color, count, pointer]), returned as u64);
        std::hint::black_box(&mut output);
        assert_eq!(&output[..returned], &expected[..returned]);
        assert!(output[returned..].iter().all(|&byte| byte == 0xa5));
        assert_eq!(call(get, &[color, 592, pointer | 1]), 0);
    }
    let read_only = [0xa5u8; 592];
    mm::user_copy::allow(read_only.as_ptr() as u64, read_only.len(), false);
    assert_eq!(call(get, &[color, 592, read_only.as_ptr() as u64]), 0);
    assert_eq!(read_only, [0xa5; 592]);
    objects::with_handle_table(|table| table.get_mut(color).unwrap().process_id = 999);
    assert_eq!(call(get, &[color, 592, read_only.as_ptr() as u64]), 0);
    objects::with_handle_table(|table| table.get_mut(color).unwrap().process_id = 1);
    assert!(objects::gdi_delete_object(color));
    assert_eq!(call(get, &[color, 592, read_only.as_ptr() as u64]), 0);
}

#[test]
fn saved_dc_pins_complete_font_and_colorspace_descriptions_until_deselection() {
    let get = object_queries::nt_gdi_ext_get_object_w;
    let hdc = dc::gre_create_compatible_dc(0);
    let font = objects::gdi_create_font(12, 0, 400, false, &[65]);
    let mut input = [0u32; 148];
    input[..5].copy_from_slice(&[0x50534f43, 0x400, 588, 0x73524742, 1]);
    mm::user_copy::allow(input.as_ptr() as u64, 592, false);
    let color = call(
        color_space::nt_gdi_create_color_space,
        &[input.as_ptr() as u64],
    );
    let second = call(
        color_space::nt_gdi_create_color_space,
        &[input.as_ptr() as u64],
    );
    assert_ne!(color, 0);
    assert_ne!(second, 0);
    let previous_font = call(selection::nt_gdi_select_font, &[hdc, font]);
    assert_ne!(previous_font, 0);
    assert_eq!(call(color_space::nt_gdi_set_color_space, &[hdc, color]), 1);
    assert_eq!(dc::gre_save_dc(hdc), 1);
    assert_eq!(
        call(selection::nt_gdi_select_font, &[hdc, previous_font]),
        font
    );
    assert_eq!(call(color_space::nt_gdi_set_color_space, &[hdc, second]), 1);
    assert!(!objects::gdi_delete_object(font));
    assert!(!objects::gdi_delete_object(color));
    let mut output = [0xa5u8; 592];
    let address = output.as_mut_ptr() as u64;
    mm::user_copy::allow(address, 592, true);
    assert_eq!(call(get, &[font, 592, address]), 356);
    assert_eq!(call(get, &[color, 592, address]), 592);
    assert!(dc::gre_restore_dc(hdc, -1));
    assert_eq!(dc::get_dc(hdc).unwrap().font, font);
    assert_eq!(dc::get_dc(hdc).unwrap().attributes.color_space, color);
    assert_eq!(
        call(selection::nt_gdi_select_font, &[hdc, previous_font]),
        font
    );
    assert_eq!(call(color_space::nt_gdi_set_color_space, &[hdc, second]), 1);
    assert!(objects::gdi_delete_object(font));
    assert!(objects::gdi_delete_object(color));
    assert_eq!(call(get, &[font, 592, address]), 0);
    assert_eq!(call(get, &[color, 592, address]), 0);
    assert!(dc::gre_delete_dc(hdc));
    assert!(objects::gdi_delete_object(second));
}

#[test]
fn font_and_colorspace_capture_allocation_failures_publish_no_object() {
    let mut input = [0u32; 148];
    input[..5].copy_from_slice(&[0x50534f43, 0x400, 588, 0x73524742, 1]);
    mm::user_copy::allow(input.as_ptr() as u64, 592, false);
    let baseline = mm::pool::balance();
    mm::pool::fail_after(0);
    assert_eq!(
        call(
            color_space::nt_gdi_create_color_space,
            &[input.as_ptr() as u64]
        ),
        0
    );
    mm::pool::fail_after(0);
    assert_eq!(
        call(
            font_services::nt_gdi_hfont_create,
            &[input.as_ptr() as u64, 420, 0, 0, 0]
        ),
        0
    );
    assert_eq!(mm::pool::balance(), baseline);
}

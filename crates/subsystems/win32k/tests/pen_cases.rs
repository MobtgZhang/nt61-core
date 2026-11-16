use super::*;
use pen_services::{
    nt_gdi_ext_create_pen as create_pen, nt_gdi_ext_get_pen_object_w as get_pen, PenError,
    PenParameters,
};

fn parameters(style: u32, width: u32) -> PenParameters {
    PenParameters {
        style,
        width,
        brush_style: 0,
        color: 0x123456,
        client_hatch: 0,
        hatch: 0,
        style_count: 0,
        old_style: false,
    }
}

#[test]
fn pen_validation_matches_442_original_win7_machine_code_cases() {
    // Golden bytes come from original Win7 machine code, not the Rust model.
    // Source identity, record format and emulation limits accompany the corpus.
    let corpus = include_bytes!("data/win7-pen-validation.bin");
    assert_eq!(corpus.len(), 442 * 164);
    for (index, record) in corpus.chunks_exact(164).enumerate() {
        let word = |n: usize| u32::from_le_bytes(record[n * 4..n * 4 + 4].try_into().unwrap());
        let input = PenParameters {
            style: word(0),
            width: word(1),
            brush_style: word(2),
            color: 0x123456,
            client_hatch: u64::from(word(3)),
            hatch: u64::from(word(3)),
            style_count: word(4),
            old_style: word(5) != 0,
        };
        let entries: [u32; 16] = core::array::from_fn(|n| word(9 + n));
        let result =
            pen_services::validate_pen(input, (word(6) != 0).then_some(entries.as_slice()));
        let class = match &result {
            Err(PenError::InvalidParameter) => 0,
            Ok(None) => 1,
            Ok(Some(_)) => 2,
            Err(PenError::BrushFailure) => 3,
        };
        assert_eq!(class, word(7), "original case {index}: {input:?}");
        if let Ok(Some(pen)) = result {
            let len = word(8) as usize;
            if len != 0 {
                let mut styles = [0; 64];
                for (n, slot) in styles[..len].chunks_exact_mut(4).enumerate() {
                    let value = if input.style & 0xf0000 != 0 && !input.old_style {
                        pen.realized_styles[n].to_bits()
                    } else {
                        pen.realized_styles[n] as u32
                    };
                    slot.copy_from_slice(&value.to_le_bytes());
                }
                assert_eq!(
                    &styles[..len],
                    &record[100..100 + len],
                    "style array, case {index}"
                );
            }
        }
    }
}

#[test]
fn pen_extended_query_matches_original_layout_and_short_buffers_are_untouched() {
    let entries = [3u32, 2];
    let pointer = entries.as_ptr() as u64;
    mm::user_copy::allow(pointer, 8, false);
    let pen = call(
        create_pen,
        &[
            0x12207,
            7,
            2,
            0x123456,
            0x1234567887654321,
            4,
            2,
            pointer,
            0,
            0,
            0,
        ],
    );
    assert_ne!(pen, 0);
    assert_eq!(
        objects::gdi_get_object_type(pen),
        Some(objects::GdiObjectType::ExtPen)
    );
    assert_eq!(call(get_pen, &[pen, 0, 0]), 36);
    let mut output = [0xa5; 48];
    let target = output.as_mut_ptr() as u64;
    mm::user_copy::allow(target, 48, true);
    for count in [0, 15, 16, 27, 28, 31, 32, 35] {
        assert_eq!(call(get_pen, &[pen, count, target]), 0);
        std::hint::black_box(&mut output);
        assert_eq!(output, [0xa5; 48]);
    }
    assert_eq!(call(get_pen, &[pen, 36, target]), 36);
    std::hint::black_box(&mut output);
    // Exact original cjGetBrushOrPen output, from isolated Win7 execution.
    let golden: [u8; 36] = [
        7, 34, 1, 0, 7, 0, 0, 0, 2, 0, 0, 0, 86, 52, 18, 0, 33, 67, 101, 135, 120, 86, 52, 18, 2,
        0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0,
    ];
    assert_eq!(&output[..36], &golden);
    assert_eq!(&output[36..], &[0xa5; 12]);
    assert_eq!(objects::gdi_get_pen_width(pen), 7);
    assert_eq!(objects::gdi_get_pen_color(pen), 0x123456);
    assert!(objects::gdi_delete_object(pen));
    assert_eq!(call(get_pen, &[pen, 0, 0]), 0);
}

#[test]
fn pen_serialization_respects_pointer_width_and_realized_float_rounding() {
    let mut input = parameters(0x10007, (-7i32) as u32);
    input.style_count = 2;
    input.client_hatch = 0x1234567887654321;
    let pen = pen_services::validate_pen(input, Some(&[16_777_217, 1]))
        .unwrap()
        .unwrap();
    assert_eq!(pen.width, 7);
    let (native, size) = pen_services::serialize_pen(&pen, 8, 96, false).unwrap();
    let (compat, compat_size) = pen_services::serialize_pen(&pen, 4, 96, false).unwrap();
    assert_eq!((size, compat_size), (36, 32));
    assert_eq!(&native[16..24], &input.client_hatch.to_le_bytes());
    assert_eq!(&compat[16..20], &(input.client_hatch as u32).to_le_bytes());
    assert_eq!(&compat[20..24], &2u32.to_le_bytes());
    assert_eq!(&native[28..32], &16_777_216u32.to_le_bytes());
    assert_eq!(&compat[24..28], &16_777_216u32.to_le_bytes());
    assert!(pen_services::serialize_pen(&pen, 3, 96, false).is_none());
}

#[test]
fn pen_capture_order_and_query_failures_do_not_publish_or_write() {
    let stock = objects::gdi_get_stock_object(8);
    let baseline = mm::pool::balance();
    assert_eq!(call(create_pen, &[5, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0]), 0);
    assert_eq!(call(create_pen, &[7, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0]), 0);
    assert_eq!(
        call(create_pen, &[0x10000, 1, 6, 0, 0, 1, 0, 0, 8, 0, 0]),
        0
    );
    assert_eq!(call(create_pen, &[0x80000005, 0x80000000, 99]), stock);
    let pen = call(create_pen, &[0x10000, 1, 0, 0x123456]);
    assert_ne!(pen, 0);
    assert_eq!(call(get_pen, &[pen, 0, 0]), 28);
    assert_eq!(call(get_pen, &[pen, 28, 1]), 0);
    let read_only = [0xa5u8; 32];
    let pointer = read_only.as_ptr() as u64;
    mm::user_copy::allow(pointer, 32, false);
    assert_eq!(call(get_pen, &[pen, 32, pointer]), 0);
    assert_eq!(read_only, [0xa5; 32]);
    assert!(objects::gdi_delete_object(pen));
    assert_eq!(mm::pool::balance(), baseline);
}

#[test]
fn pen_selection_saved_dc_and_deletion_keep_real_objects_alive() {
    let fixture = DrawingFixture::new();
    let pen = call(create_pen, &[0x10000, 1, 0, 0x000000ff]);
    assert_ne!(pen, 0);
    let previous = call(selection::nt_gdi_select_pen, &[fixture.handle, pen]);
    assert_ne!(previous, 0);
    assert!(!objects::gdi_delete_object(pen));
    assert_eq!(dc::gre_save_dc(fixture.handle), 1);
    assert_eq!(
        call(selection::nt_gdi_select_pen, &[fixture.handle, previous]),
        pen
    );
    assert!(!objects::gdi_delete_object(pen));
    assert!(dc::gre_restore_dc(fixture.handle, -1));
    assert_eq!(dc::get_dc(fixture.handle).unwrap().pen, pen);
    assert_eq!(raster::pen_color(pen), Some(Some(0xff0000)));
    assert!(raster::line_to(
        &mut dc::get_dc(fixture.handle).unwrap(),
        4,
        0
    ));
    assert_eq!(fixture.pixel(0, 0), 0xff0000);
    assert_eq!(fixture.pixel(3, 0), 0xff0000);
    assert_eq!(fixture.pixel(4, 0), 0x00123456);
    let brush = objects::gdi_create_solid_brush(7);
    assert_eq!(
        call(selection::nt_gdi_select_pen, &[fixture.handle, brush]),
        0
    );
    assert_eq!(dc::get_dc(fixture.handle).unwrap().pen, pen);
    assert!(objects::gdi_delete_object(brush));
    drop(fixture);
    assert!(objects::gdi_delete_object(pen));
}

#[test]
fn pen_legacy_validation_and_pool_failure_have_no_leak() {
    let legacy = pen_services::nt_gdi_create_pen;
    assert_eq!(call(legacy, &[7, 1, 0]), 0);
    assert_eq!(call(legacy, &[u32::MAX as u64, 1, 0]), 0);
    let pen = call(legacy, &[2, (-3i32) as u64, 0x123456]);
    assert_ne!(pen, 0);
    assert_eq!(call(get_pen, &[pen, 0, 0]), 16);
    let mut bytes = [0xa5u8; 32];
    let output = bytes.as_mut_ptr() as u64;
    mm::user_copy::allow(output, 32, true);
    assert_eq!(call(get_pen, &[pen, 32, output]), 16);
    std::hint::black_box(&mut bytes);
    assert_eq!(
        &bytes[..16],
        &[2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 86, 52, 18, 0]
    );
    assert_eq!(&bytes[16..], &[0xa5; 16]);
    assert!(objects::gdi_delete_object(pen));
    let baseline = mm::pool::balance();
    mm::pool::fail_after(0);
    assert_eq!(call(create_pen, &[0x10000, 1, 0, 0]), 0);
    assert_eq!(mm::pool::balance(), baseline);
    // The frontend accepts these descriptors, but unavailable pattern/DIB
    // realization must not publish a pretend object.
    assert_eq!(call(create_pen, &[0x10000, 1, 3, 0, 0, 123]), 0);
    assert_eq!(mm::pool::balance(), baseline);
}

#[test]
fn pen_getobject_matches_66_original_win7_query_and_buffer_cases() {
    let corpus = include_bytes!("data/win7-pen-getobject.bin");
    assert_eq!(corpus.len(), 66 * 64);
    for (index, record) in corpus.chunks_exact(64).enumerate() {
        let word = |n: usize| u32::from_le_bytes(record[n * 4..n * 4 + 4].try_into().unwrap());
        let pen = match word(0) {
            0 => objects::GdiPen::new(objects::PenStyle::Dot, 7, 0x123456),
            1 => objects::GdiPen::new(objects::PenStyle::Null, 7, 0x123456),
            2 => {
                let mut input = parameters(0x12207, 7);
                input.brush_style = 2;
                input.style_count = 2;
                input.client_hatch = 0x1234567887654321;
                input.hatch = 4;
                pen_services::validate_pen(input, Some(&[3, 2]))
                    .unwrap()
                    .unwrap()
            }
            _ => unreachable!(),
        };
        let query = word(2) == 0;
        let result = pen_services::serialize_pen(&pen, 8, word(1) as usize, query);
        assert_eq!(
            result.as_ref().map_or(0, |(_, len)| *len as u32),
            word(3),
            "case {index}"
        );
        let mut output = [0xa5u8; 48];
        if !query {
            if let Some((bytes, len)) = result {
                // Win7 does not define the last four bytes of this special
                // response. The implementation deterministically zeros them.
                let defined = if word(0) == 1 && word(1) == 32 {
                    28
                } else {
                    len
                };
                output[..defined].copy_from_slice(&bytes[..defined]);
            }
        }
        assert_eq!(&output, &record[16..], "defined output bytes, case {index}");
    }
}

#[test]
fn pen_query_rejects_foreign_owner_and_reused_handle_generation() {
    let pen = call(create_pen, &[0x10000, 1, 0, 0]);
    let owner = objects::with_handle_table(|table| {
        let entry = table.get_mut(pen).unwrap();
        let owner = entry.process_id;
        entry.process_id = owner.wrapping_add(1).max(1);
        owner
    });
    assert_eq!(call(get_pen, &[pen, 0, 0]), 0);
    objects::with_handle_table(|table| table.get_mut(pen).unwrap().process_id = owner);
    assert!(objects::gdi_delete_object(pen));
    let replacement = call(create_pen, &[0x10000, 1, 0, 0]);
    assert_ne!(replacement, pen);
    assert_eq!(call(get_pen, &[pen, 0, 0]), 0);
    assert_ne!(call(get_pen, &[replacement, 0, 0]), 0);
    assert!(objects::gdi_delete_object(replacement));
}

#[test]
fn pen_geometric_query_conversion_matches_original_bftol_boundary_cases() {
    // Original bFToL RVA0x1c8d0, mode6, executed without a conversion hook.
    // Captured source identity and all15 return/output pairs are preserved in
    // build/win7-pen-bftol-emulation.json and its reproduction script.
    let cases = [
        (0u32, 0u32),
        (1, 1),
        (2, 2),
        (3, 3),
        (16383, 16383),
        (16384, 16384),
        (16777215, 16777215),
        (16777216, 16777216),
        (16777217, 16777216),
        (16777218, 16777218),
        (0x3fffffff, 0x40000000),
        (0x40000000, 0x40000000),
        (0x7fffff00, 0x7fffff00),
        (0x7fffff80, 0x7fffff80),
        (0x7fffffff, 0x80000000),
    ];
    for (input_value, expected) in cases {
        let mut input = parameters(0x10007, 1);
        input.style_count = if input_value == 0 { 2 } else { 1 };
        let entries = [input_value, 1];
        let pen = pen_services::validate_pen(input, Some(&entries))
            .unwrap()
            .unwrap();
        let (bytes, _) = pen_services::serialize_pen(&pen, 8, 96, false).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            expected
        );
    }
}

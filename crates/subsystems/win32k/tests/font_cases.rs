use crate::{dc, font_services::*, mm::user_copy::allow, objects};

fn call(handler: extern "C" fn(&crate::arch::common::syscall_frame::SyscallFrame) -> u64, args: &[u64]) -> u64 {
    let mut frame = crate::arch::common::syscall_frame::SyscallFrame::default();
    frame.args[..args.len()].copy_from_slice(args);
    handler(&frame)
}

#[test]
fn font_data_query_reads_real_sfnt_tables_and_rejects_bad_offsets() {
    let hdc = dc::gre_create_compatible_dc(0);
    let tag = u32::from_le_bytes(*b"head") as u64;
    let length = call(nt_gdi_get_font_data, &[hdc, tag, 0, 0, 0]);
    assert_eq!(length, 54);
    let mut header = [0u8; 54];
    allow(header.as_mut_ptr() as u64, header.len(), true);
    assert_eq!(call(nt_gdi_get_font_data, &[hdc, tag, 0, header.as_mut_ptr() as u64, 54]), 54);
    assert_eq!(&header[12..16], &[0x5f, 0x0f, 0x3c, 0xf5]);
    assert_eq!(call(nt_gdi_get_font_data, &[hdc, tag, 55, 0, 0]), u32::MAX as u64);
    assert_eq!(call(nt_gdi_get_font_data, &[hdc, 1, 0, 0, 0]), u32::MAX as u64);
    assert_eq!(call(nt_gdi_get_font_data, &[0, tag, 0, 0, 0]), u32::MAX as u64);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn character_widths_use_native_count_and_match_glyph_abc_sums() {
    let hdc = dc::gre_create_compatible_dc(0);
    let chars = [b'i' as u16, b'W' as u16, b'j' as u16];
    let mut widths = [0i32; 3];
    let mut abc = [0i32; 9];
    let mut glyphs = [0u16; 3];
    allow(chars.as_ptr() as u64, 6, false);
    allow(widths.as_mut_ptr() as u64, 12, true);
    allow(abc.as_mut_ptr() as u64, 36, true);
    allow(glyphs.as_mut_ptr() as u64, 6, true);
    assert_eq!(call(nt_gdi_get_char_width_w, &[hdc, 0, 3, chars.as_ptr() as u64, 3, widths.as_mut_ptr() as u64]), 1);
    assert!(widths[1] > widths[0]);
    assert_eq!(call(nt_gdi_get_char_abc_widths_w, &[hdc, 0, 3, chars.as_ptr() as u64, 1, abc.as_mut_ptr() as u64]), 1);
    for (width, abc) in widths.iter().zip(abc.chunks_exact(3)) {
        assert_eq!(*width, abc.iter().sum::<i32>());
    }
    assert_eq!(call(nt_gdi_get_glyph_indices_w, &[hdc, chars.as_ptr() as u64, 3, glyphs.as_mut_ptr() as u64, 0]), 3);
    let expected = widths;
    assert_eq!(call(nt_gdi_get_char_width_w, &[hdc, 0, 3, glyphs.as_ptr() as u64, 0xb, widths.as_mut_ptr() as u64]), 1);
    assert_eq!(widths, expected);
    assert_eq!(call(nt_gdi_get_char_width_w, &[hdc, 0xffff, 2, 0, 3, widths.as_mut_ptr() as u64]), 0);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn glyph_queries_mark_surrogates_and_unicode_ranges_match_cmap() {
    let hdc = dc::gre_create_compatible_dc(0);
    let chars = [b'A' as u16, 0xd800, 0xffff];
    let mut glyphs = [0u16; 3];
    allow(chars.as_ptr() as u64, 6, false);
    allow(glyphs.as_mut_ptr() as u64, 6, true);
    assert!(call(nt_gdi_get_glyph_indices_w, &[hdc, 0, 0, 0, 0]) > 1000);
    assert_eq!(call(nt_gdi_get_glyph_indices_w, &[hdc, chars.as_ptr() as u64, 3, glyphs.as_mut_ptr() as u64, 1]), 3);
    assert_ne!(glyphs[0], 0xffff);
    assert_eq!(&glyphs[1..], &[0xffff, 0xffff]);
    let size = call(nt_gdi_get_font_unicode_ranges, &[hdc, 0]) as usize;
    assert!(size > 16);
    let mut bytes = vec![0; size];
    allow(bytes.as_mut_ptr() as u64, size, true);
    assert_eq!(call(nt_gdi_get_font_unicode_ranges, &[hdc, bytes.as_mut_ptr() as u64]), size as u64);
    let mut sum = 0u32;
    for pair in bytes[16..].chunks_exact(4) {
        let start = u16::from_le_bytes([pair[0], pair[1]]);
        let count = u16::from_le_bytes([pair[2], pair[3]]);
        assert!(count > 0);
        assert!(!(start..=start.saturating_add(count - 1)).contains(&0xd800));
        sum += u32::from(count);
    }
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), sum);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn extent_writes_size_cumulative_distances_fit_count_and_realized_face() {
    let hdc = dc::gre_create_compatible_dc(0);
    let text = [b'i' as u16, b'W' as u16, b'i' as u16];
    let mut size = [0i32; 2];
    let mut distances = [0i32; 3];
    let mut fit = -1i32;
    allow(text.as_ptr() as u64, 6, false);
    allow(size.as_mut_ptr() as u64, 8, true);
    allow(distances.as_mut_ptr() as u64, 12, true);
    allow(&mut fit as *mut _ as u64, 4, true);
    assert_eq!(call(nt_gdi_get_text_extent_ex_w, &[hdc, text.as_ptr() as u64, 3, 5, &mut fit as *mut _ as u64, distances.as_mut_ptr() as u64, size.as_mut_ptr() as u64, 0]), 1);
    assert_eq!(fit, 1);
    assert_eq!(size[0], distances[2]);
    assert!(distances[1] - distances[0] > distances[0]);
    let mut basic = [0i32; 2];
    allow(basic.as_mut_ptr() as u64, 8, true);
    assert_eq!(call(nt_gdi_get_text_extent, &[hdc, text.as_ptr() as u64, 3, basic.as_mut_ptr() as u64, 0]), 1);
    assert_eq!(basic, size);
    let mut metrics = [0xffu8; 60];
    allow(metrics.as_mut_ptr() as u64, 60, true);
    assert_eq!(call(nt_gdi_get_text_metrics_w, &[hdc, metrics.as_mut_ptr() as u64, 59]), 0);
    assert_eq!(call(nt_gdi_get_text_metrics_w, &[hdc, metrics.as_mut_ptr() as u64, 60]), 1);
    assert_eq!(i32::from_le_bytes(metrics[..4].try_into().unwrap()), size[1]);
    assert_eq!(&metrics[57..], &[0, 0, 0]);
    assert_eq!(call(nt_gdi_get_text_face_w, &[hdc, 0, 0, 0]), 12);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn selected_font_size_and_monospace_substitution_change_queries() {
    let hdc = dc::gre_create_compatible_dc(0);
    let mut request = objects::GdiFont::new();
    request.height = -32;
    request.set_face_name(&"Consolas".encode_utf16().collect::<Vec<_>>());
    let font = objects::gdi_create_font(request.height, request.width, request.weight, request.italic != 0, &request.face_name);
    assert_ne!(font, 0);
    assert_ne!(dc::gre_select_object(hdc, font), u64::MAX);
    let mut widths = [0i32; 2];
    let chars = [b'i' as u16, b'W' as u16];
    allow(chars.as_ptr() as u64, 4, false);
    allow(widths.as_mut_ptr() as u64, 8, true);
    assert_eq!(call(nt_gdi_get_char_width_w, &[hdc, 0, 2, chars.as_ptr() as u64, 3, widths.as_mut_ptr() as u64]), 1);
    assert_eq!(widths[0], widths[1]);
    assert!(widths[0] > 10);
    assert_eq!(call(nt_gdi_get_text_face_w, &[hdc, 0, 0, 0]), 17);
    assert!(dc::gre_delete_dc(hdc));
    assert!(objects::gdi_delete_object(font));
}

#[test]
fn empty_extent_and_charset_signature_use_native_layouts() {
    let hdc = dc::gre_create_compatible_dc(0);
    let mut size = [-1i32; 2];
    let mut signature = [0u32; 6];
    allow(size.as_mut_ptr() as u64, 8, true);
    allow(signature.as_mut_ptr() as u64, 24, true);
    assert_eq!(call(nt_gdi_get_text_extent, &[hdc, 0, 0, size.as_mut_ptr() as u64, 0]), 1);
    assert_eq!(size, [0, 0]);
    assert_eq!(call(nt_gdi_get_char_set, &[hdc]), 1252);
    assert_eq!(call(nt_gdi_get_text_charset_info, &[hdc, signature.as_mut_ptr() as u64, 0]), 0);
    assert_ne!(signature[0], 0);
    assert_ne!(signature[4] & 1, 0);
    assert_eq!(call(nt_gdi_get_text_charset_info, &[hdc, 1, 0]), 1);
    assert_eq!(call(nt_gdi_get_char_set, &[0]), 0);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn kerning_pairs_have_real_characters_and_scale_with_selected_font() {
    let hdc = dc::gre_create_compatible_dc(0);
    let count = call(nt_gdi_get_kerning_pairs, &[hdc, 0, 0]);
    assert!(count > 100);
    let mut pairs = vec![0u8; count as usize * 8];
    allow(pairs.as_mut_ptr() as u64, pairs.len(), true);
    assert_eq!(call(nt_gdi_get_kerning_pairs, &[hdc, count, pairs.as_mut_ptr() as u64]), count);
    let av = pairs.chunks_exact(8).find(|pair| pair[..4] == [b'A', 0, b'V', 0]).unwrap();
    assert!(i32::from_le_bytes(av[4..8].try_into().unwrap()) < 0);
    assert_eq!(call(nt_gdi_get_kerning_pairs, &[hdc, 1, pairs.as_mut_ptr() as u64]), 1);
    assert_eq!(call(nt_gdi_get_kerning_pairs, &[hdc, 0, pairs.as_mut_ptr() as u64]), 0);
    assert_eq!(call(nt_gdi_get_kerning_pairs, &[hdc, count, 1]), 0);
    let mut bearings = [0i32; 3];
    allow(bearings.as_mut_ptr() as u64, 12, true);
    assert_eq!(call(nt_gdi_get_char_width_info, &[hdc, bearings.as_mut_ptr() as u64]), 1);
    assert!(bearings[0] < 0 && bearings[1] < 0);
    assert_eq!(call(nt_gdi_get_char_width_info, &[0, bearings.as_mut_ptr() as u64]), 0);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn rasterizer_caps_are_three_words_and_respect_short_buffers() {
    let mut output = [0xffu8; 8];
    allow(output.as_mut_ptr() as u64, 8, true);
    assert_eq!(call(nt_gdi_get_rasterizer_caps, &[output.as_mut_ptr() as u64, 8]), 1);
    assert_eq!(output, [6, 0, 3, 0, 0, 0, 0xff, 0xff]);
    output.fill(0xff);
    assert_eq!(call(nt_gdi_get_rasterizer_caps, &[output.as_mut_ptr() as u64, 1]), 1);
    assert_eq!(output, [6, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    assert_eq!(call(nt_gdi_get_rasterizer_caps, &[output.as_mut_ptr() as u64, 0]), 0);
    assert_eq!(call(nt_gdi_get_rasterizer_caps, &[1, 6]), 0);
}

#[test]
fn realized_file_identity_reads_the_selected_sfnt_and_checks_ranges() {
    let hdc = dc::gre_create_compatible_dc(0);
    let mut info = [24u32, 0, 0, 0, 0, 0, 0xdeadbeef];
    let address = info.as_mut_ptr() as u64;
    allow(address, 28, true);
    assert_eq!(call(nt_gdi_get_realization_info, &[hdc, address]), 1);
    assert_eq!(info[1], 3);
    assert_ne!(info[3], 0);
    assert_eq!(&info[4..], &[1, 0, 0xdeadbeef]);
    let id = info[3] as u64;
    assert_eq!(call(nt_gdi_get_realization_info, &[hdc, address]), 1);
    assert_eq!(info[3] as u64, id);
    let offset = 0u64;
    let offset_address = &offset as *const u64 as u64;
    allow(offset_address, 8, false);
    let mut data = [0xffu8; 12];
    allow(data.as_mut_ptr() as u64, 12, true);
    assert_eq!(call(nt_gdi_get_font_file_data, &[id, 0, offset_address, data.as_mut_ptr() as u64, 12]), 1);
    assert_eq!(&data, &nt61_fonts::UI_FONT[..12]);
    info[0] = 20;
    let short_header = info.as_mut_ptr() as u64;
    assert_eq!(call(nt_gdi_get_realization_info, &[hdc, short_header]), 0);
    // Identity remains backed by retained bytes after the DC is destroyed.
    assert!(dc::gre_delete_dc(hdc));
    assert_eq!(call(nt_gdi_get_font_file_data, &[id, 0, offset_address, data.as_mut_ptr() as u64, 12]), 1);
    assert_eq!(call(nt_gdi_get_font_file_data, &[id, 1, offset_address, 0, 0]), 0);
    assert_eq!(call(nt_gdi_get_font_file_data, &[id, 0, 0, 0, 0]), 0);
    assert_eq!(call(nt_gdi_get_font_file_data, &[0, 0, offset_address, 0, 0]), 0);
    let overflow = u64::MAX;
    allow(&overflow as *const u64 as u64, 8, false);
    assert_eq!(call(nt_gdi_get_font_file_data, &[id, 0, &overflow as *const u64 as u64, data.as_mut_ptr() as u64, 12]), 0);

}

#[test]
fn legacy_realization_layout_does_not_write_extended_fields() {
    let hdc = dc::gre_create_compatible_dc(0);
    let mut info = [16u32, 0, 0, 0, 0xdeadbeef];
    let address = info.as_mut_ptr() as u64;
    allow(address, 16, true);
    assert_eq!(call(nt_gdi_get_realization_info, &[hdc, address]), 1);
    assert_eq!(info[4], 0xdeadbeef);
    assert_eq!(call(nt_gdi_get_realization_info, &[0, address]), 0);
    assert!(dc::gre_delete_dc(hdc));
}

fn outline_call(hdc: u64, ch: u16, format: u32, matrix: &[i32; 4], metrics: &mut [u8; 20], output: Option<&mut [u8]>) -> u64 {
    allow(matrix.as_ptr() as u64, 16, false);
    allow(metrics.as_mut_ptr() as u64, 20, true);
    let (address, size) = if let Some(output) = output {
        allow(output.as_mut_ptr() as u64, output.len(), true);
        (output.as_mut_ptr() as u64, output.len() as u64)
    } else { (0, 0) };
    call(nt_gdi_get_glyph_outline, &[hdc, ch as u64, format as u64,
        metrics.as_mut_ptr() as u64, size, address, matrix.as_ptr() as u64, 0])
}

#[test]
fn glyph_outline_native_and_bezier_streams_have_valid_closed_contours() {
    let hdc = dc::gre_create_compatible_dc(0);
    let matrix = [65536, 0, 0, 65536];
    let mut metrics = [0u8; 20];
    for format in [2, 3] {
        let size = outline_call(hdc, b'S' as u16, format, &matrix, &mut metrics, None);
        assert!(size > 16 && size < 65536);
        let mut output = vec![0; size as usize];
        assert_eq!(outline_call(hdc, b'S' as u16, format, &matrix, &mut metrics, Some(&mut output)), size);
        let mut polygon = 0;
        let mut curved = false;
        while polygon < output.len() {
            let length = u32::from_le_bytes(output[polygon..polygon + 4].try_into().unwrap()) as usize;
            assert!(length >= 16);
            assert_eq!(&output[polygon + 4..polygon + 8], &24u32.to_le_bytes());
            let mut curve = polygon + 16;
            while curve < polygon + length {
                let kind = u16::from_le_bytes(output[curve..curve + 2].try_into().unwrap());
                let count = u16::from_le_bytes(output[curve + 2..curve + 4].try_into().unwrap()) as usize;
                assert!(count > 0);
                if kind != 1 {
                    assert_eq!(kind, if format == 2 { 2 } else { 3 });
                    curved = true;
                }
                curve += 4 + count * 8;
            }
            assert_eq!(curve, polygon + length);
            assert_eq!(&output[curve - 8..curve], &output[polygon + 8..polygon + 16]);
            polygon += length;
        }
        assert!(curved);
    }
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn glyph_bitmaps_are_aligned_top_down_and_use_requested_gray_range() {
    let hdc = dc::gre_create_compatible_dc(0);
    let matrix = [65536, 0, 0, 65536];
    let mut metrics = [0u8; 20];
    for (format, maximum) in [(1, 1), (4, 4), (5, 16), (6, 64)] {
        let size = outline_call(hdc, b'F' as u16, format, &matrix, &mut metrics, None);
        assert!(size > 0 && size < 65536);
        let width = u32::from_le_bytes(metrics[..4].try_into().unwrap()) as usize;
        let height = u32::from_le_bytes(metrics[4..8].try_into().unwrap()) as usize;
        let stride = if format == 1 { (width + 31) / 32 * 4 } else { (width + 3) / 4 * 4 };
        assert_eq!(size as usize, stride * height);
        let mut output = vec![0xff; size as usize + 4];
        assert_eq!(outline_call(hdc, b'F' as u16, format, &matrix, &mut metrics, Some(&mut output)), size);
        assert_eq!(&output[size as usize..], &[0xff; 4]);
        assert!(output[..size as usize].iter().any(|&pixel| pixel != 0));
        if format != 1 {
            assert!(output[..size as usize].iter().all(|&pixel| pixel <= maximum));
            for row in output[..size as usize].chunks_exact(stride) {
                assert!(row[width..].iter().all(|&pixel| pixel == 0));
            }
            let top: u32 = output[..stride * (height / 3)].iter().map(|&v| v as u32).sum();
            let bottom: u32 = output[stride * (height - height / 3)..size as usize].iter().map(|&v| v as u32).sum();
            assert!(top > bottom, "F must have its horizontal bars at the top: format={format}, top={top}, bottom={bottom}, rows={:?}", output[..size as usize].chunks_exact(stride).map(|row| row.iter().map(|&v| v as u32).sum::<u32>()).collect::<Vec<_>>());
        }
    }
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn glyph_outline_applies_fractional_mat2_and_glyph_index_flag() {
    let hdc = dc::gre_create_compatible_dc(0);
    let mut normal = [0u8; 20];
    let mut stretched = [0u8; 20];
    assert_eq!(outline_call(hdc, b'W' as u16, 0, &[65536, 0, 0, 65536], &mut normal, None), 0);
    assert_eq!(outline_call(hdc, b'W' as u16, 0, &[131072, 0, 0, 98304], &mut stretched, None), 0);
    let word = |bytes: &[u8], offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert!(word(&stretched, 0) >= word(&normal, 0) * 2 - 1);
    assert!(word(&stretched, 4) > word(&normal, 4));
    let font = nt61_fonts::SfntFont::parse(nt61_fonts::UI_FONT).unwrap();
    let index = font.glyph_index(b'W' as u16).unwrap();
    let mut indexed = [0u8; 20];
    assert_eq!(outline_call(hdc, index, 0x80, &[65536, 0, 0, 65536], &mut indexed, None), 0);
    assert_eq!(indexed, normal);
    assert_eq!(outline_call(hdc, b'W' as u16, 6, &[0, 65536, -65536, 0], &mut indexed, None) as usize,
        ((word(&indexed, 0) as usize + 3) / 4 * 4) * word(&indexed, 4) as usize);
    assert_eq!(word(&indexed, 0), word(&normal, 4));
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn glyph_outline_rejects_short_buffers_and_invalid_memory_without_writes() {
    let hdc = dc::gre_create_compatible_dc(0);
    let matrix = [65536, 0, 0, 65536];
    let mut metrics = [0x55u8; 20];
    let mut short = [0xaau8; 4];
    assert_eq!(outline_call(hdc, b'A' as u16, 2, &matrix, &mut metrics, Some(&mut short)), u32::MAX as u64);
    assert_eq!(metrics, [0x55; 20]);
    assert_eq!(short, [0xaa; 4]);
    assert_eq!(outline_call(hdc, b'A' as u16, 7, &matrix, &mut metrics, None), u32::MAX as u64);
    assert_eq!(outline_call(0, b'A' as u16, 0, &matrix, &mut metrics, None), u32::MAX as u64);
    assert_eq!(call(nt_gdi_get_glyph_outline, &[hdc, b'A' as u64, 0, metrics.as_mut_ptr() as u64, 0, 0, 1, 0]), u32::MAX as u64);
    assert_eq!(call(nt_gdi_get_glyph_outline, &[hdc, b'A' as u64, 0, 1, 0, 0, matrix.as_ptr() as u64, 0]), u32::MAX as u64);
    assert_eq!(outline_call(hdc, b' ' as u16, 2, &matrix, &mut metrics, None), 0);
    assert_eq!(&metrics[..16], &[0; 16]);
    assert!(i16::from_le_bytes(metrics[16..18].try_into().unwrap()) > 0);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn font_file_info_reports_retained_memory_font_length_and_native_size() {
    let hdc = dc::gre_create_compatible_dc(0);
    let mut realization = [24u32, 0, 0, 0, 0, 0];
    allow(realization.as_mut_ptr() as u64, 24, true);
    assert_eq!(call(nt_gdi_get_realization_info, &[hdc, realization.as_mut_ptr() as u64]), 1);
    let id = realization[2] as u64;
    let mut needed = 0u64;
    allow(&mut needed as *mut _ as u64, 8, true);
    let mut info = [0xaau8; 32];
    allow(info.as_mut_ptr() as u64, info.len(), true);
    assert_eq!(call(nt_gdi_get_font_file_info, &[id, 0, info.as_mut_ptr() as u64, 23,
        &mut needed as *mut _ as u64]), 0);
    assert_eq!(needed, 24);
    assert_eq!(info, [0xaa; 32]);
    assert_eq!(call(nt_gdi_get_font_file_info, &[id, 0, info.as_mut_ptr() as u64, 32,
        &mut needed as *mut _ as u64]), 1);
    assert_eq!(&info[..8], &[0; 8]);
    assert_eq!(u64::from_le_bytes(info[8..16].try_into().unwrap()), nt61_fonts::UI_FONT.len() as u64);
    assert_eq!(&info[16..24], &[0; 8]);
    assert_eq!(&info[24..], &[0xaa; 8]);
    assert_eq!(call(nt_gdi_get_font_file_info, &[id, 1, info.as_mut_ptr() as u64, 32, 0]), 0);
    assert!(dc::gre_delete_dc(hdc));
}

#[test]
fn extreme_and_singular_glyph_matrices_do_not_overflow_or_publish_partial_output() {
    let hdc = dc::gre_create_compatible_dc(0);
    for matrix in [[1, 1, 1, 1], [65536, 65535, 65535, 65534],
        [i32::MAX, i32::MIN, i32::MAX, i32::MIN]] {
        let mut metrics = [0u8; 20];
        let size = outline_call(hdc, b'W' as u16, 6, &matrix, &mut metrics, None);
        if size != u32::MAX as u64 {
            assert!(size <= 8 * 1024 * 1024);
            let mut bitmap = vec![0xaau8; size as usize];
            let result = outline_call(hdc, b'W' as u16, 6, &matrix, &mut metrics, Some(&mut bitmap));
            if result == u32::MAX as u64 { assert!(bitmap.iter().all(|v| *v == 0xaa)); }
            else { assert_eq!(result, size); }
        }
    }
    assert!(dc::gre_delete_dc(hdc));
}

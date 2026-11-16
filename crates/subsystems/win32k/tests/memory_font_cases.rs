use crate::{call, dc, font_services::*, mm::user_copy::allow, objects, ps};

fn table(data: &[u8], tag: &[u8; 4]) -> (usize, usize) {
    let count = u16::from_be_bytes(data[4..6].try_into().unwrap()) as usize;
    let entry = data[12..12 + count * 16]
        .chunks_exact(16)
        .find(|entry| &entry[..4] == tag)
        .unwrap();
    (
        u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize,
        u32::from_be_bytes(entry[12..16].try_into().unwrap()) as usize,
    )
}

// Each test installs a distinct family so concurrent production-code tests
// continue selecting their own faces. Also alter A's advance to detect fallback.
fn private_face(name: &str) -> Vec<u8> {
    assert_eq!(name.len(), 11);
    let mut data = nt61_fonts::UI_FONT.to_vec();
    let (offset, _) = table(&data, b"name");
    let count = u16::from_be_bytes(data[offset + 2..offset + 4].try_into().unwrap()) as usize;
    let strings =
        offset + u16::from_be_bytes(data[offset + 4..offset + 6].try_into().unwrap()) as usize;
    let mut changed = 0;
    for index in 0..count {
        let record = offset + 6 + index * 12;
        let platform = u16::from_be_bytes(data[record..record + 2].try_into().unwrap());
        let id = u16::from_be_bytes(data[record + 6..record + 8].try_into().unwrap());
        let size = u16::from_be_bytes(data[record + 8..record + 10].try_into().unwrap()) as usize;
        let start = strings
            + u16::from_be_bytes(data[record + 10..record + 12].try_into().unwrap()) as usize;
        if id == 1 && (platform == 0 || platform == 3) && size == 22 {
            for (slot, unit) in data[start..start + size]
                .chunks_exact_mut(2)
                .zip(name.encode_utf16())
            {
                slot.copy_from_slice(&unit.to_be_bytes());
            }
            changed += 1;
        }
    }
    assert!(changed > 0);
    let glyph = nt61_fonts::SfntFont::parse(&data)
        .unwrap()
        .glyph_index(b'A' as u16)
        .unwrap();
    let (hmtx, _) = table(&data, b"hmtx");
    data[hmtx + glyph as usize * 4..hmtx + glyph as usize * 4 + 2]
        .copy_from_slice(&600u16.to_be_bytes());
    data
}

fn install(data: &[u8]) -> u64 {
    let mut count = 0xdead_beefu32;
    allow(data.as_ptr() as u64, data.len(), false);
    allow(&mut count as *mut _ as u64, 4, true);
    let handle = call(
        nt_gdi_add_font_mem_resource_ex,
        &[
            data.as_ptr() as u64,
            data.len() as u64,
            0,
            0,
            &mut count as *mut _ as u64,
        ],
    );
    assert_ne!(handle, 0);
    assert_eq!(count, 1);
    handle
}

fn select(hdc: u64, family: &str) -> u64 {
    let mut description = [0u8; 420];
    description[..4].copy_from_slice(&(-32i32).to_le_bytes());
    description[16..20].copy_from_slice(&400i32.to_le_bytes());
    for (index, unit) in family.encode_utf16().enumerate() {
        description[28 + index * 2..30 + index * 2].copy_from_slice(&unit.to_le_bytes());
    }
    allow(description.as_ptr() as u64, description.len(), false);
    let font = call(nt_gdi_hfont_create, &[description.as_ptr() as u64, 420, 0, 0, 0]);
    description.fill(0); // The logical font owns a captured description.
    assert_ne!(font, 0);
    dc::gre_select_object(hdc, font);
    assert_eq!(dc::get_dc(hdc).unwrap().font, font);
    font
}

#[test]
fn native_logical_font_capture_rejects_inaccessible_and_unsupported_cache_modes() {
    let mut description = [0u8; 420];
    let address = description.as_ptr() as u64;
    assert_eq!(call(nt_gdi_hfont_create, &[1, 420, 0, 0, 0]), 0);
    allow(address, 92, false); // A public LOGFONT alone is not the native input.
    assert_eq!(call(nt_gdi_hfont_create, &[address, 420, 0, 0, 0]), 0);
    allow(address, description.len(), false);
    for arguments in [
        [address, 0, 0, 0, 0], [address, 421, 0, 0, 0], [address, 420, 1, 0, 0],
        [address, 420, 0, 1, 0], [address, 420, 0, 0, 1],
    ] { assert_eq!(call(nt_gdi_hfont_create, &arguments), 0); }
    description[352] = 1; // Native Nt wrapper clears the user design-axis count.
    core::hint::black_box(&description);
    let font = call(nt_gdi_hfont_create, &[address, 420, 0, 0, 0]);
    assert_ne!(font, 0);
    assert!(objects::gdi_delete_object(font));
}

#[test]
fn native_logical_font_preserves_logfont_fields_and_has_independent_lifetime() {
    let mut description = [0u8; 420];
    for (index, value) in [-25i32, 12, 900, 450, 700].iter().enumerate() {
        description[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    description[20..28].copy_from_slice(&[1, 1, 1, 2, 3, 4, 5, 6]);
    for (index, unit) in "Captured Face".encode_utf16().enumerate() {
        description[28 + index * 2..30 + index * 2].copy_from_slice(&unit.to_le_bytes());
    }
    allow(description.as_ptr() as u64, description.len(), false);
    let args = [description.as_ptr() as u64, 420, 0, 0, 0];
    let first = call(nt_gdi_hfont_create, &args);
    let second = call(nt_gdi_hfont_create, &args);
    assert_ne!(first, 0);
    assert_ne!(second, 0);
    assert_ne!(first, second);
    description.fill(0);
    objects::with_handle_table(|table| {
        let entry = table.get(first).unwrap();
        assert_eq!(entry.get_type(), Some(objects::GdiObjectType::Font));
        assert_eq!(entry.process_id, ps::thread::IDENTITY.with(|id| id.get().0) as u32);
        // SAFETY: the object table guard keeps the live font allocated.
        let font = unsafe { &*(entry.obj_ptr as *const objects::GdiFont) };
        assert_eq!([font.height, font.width, font.escapement, font.orientation, font.weight],
                   [-25, 12, 900, 450, 700]);
        assert_eq!([font.italic, font.underline, font.strike_out, font.charset,
                    font.out_precision, font.clip_precision, font.quality, font.pitch_and_family],
                   [1, 1, 1, 2, 3, 4, 5, 6]);
        assert_eq!(String::from_utf16(&font.face_name[..13]).unwrap(), "Captured Face");
    });
    assert!(objects::gdi_delete_object(first));
    assert!(!objects::gdi_delete_object(first));
    assert!(objects::gdi_delete_object(second));
}

fn a_width(hdc: u64) -> i32 {
    let mut width = -1i32;
    allow(&mut width as *mut _ as u64, 4, true);
    assert_eq!(
        call(
            nt_gdi_get_char_width_w,
            &[hdc, b'A' as u64, 1, 0, 3, &mut width as *mut _ as u64]
        ),
        1
    );
    width
}

#[test]
fn memory_font_is_copied_selected_and_retained_after_removal() {
    let mut bytes = private_face("Private One");
    let resource = install(&bytes);
    bytes.fill(0); // The caller may immediately discard or overwrite its data.
    let hdc = dc::gre_create_compatible_dc(0);
    let logical = select(hdc, "pRIVATE oNE");
    // Selection pins the resource before the first metrics/outline request.
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[resource]), 1);
    assert_eq!(a_width(hdc), 9); // round(600 * 32 / 2048)
    let mut family = [0u16; 32];
    allow(family.as_mut_ptr() as u64, 64, true);
    assert_eq!(
        call(
            nt_gdi_get_text_face_w,
            &[hdc, 32, family.as_mut_ptr() as u64, 0]
        ),
        12
    );
    assert_eq!(String::from_utf16(&family[..11]).unwrap(), "Private One");
    assert_eq!(family[11], 0);
    let mut info = [24u32, 0, 0, 0, 0, 0];
    allow(info.as_mut_ptr() as u64, 24, true);
    assert_eq!(
        call(
            nt_gdi_get_realization_info,
            &[hdc, info.as_mut_ptr() as u64]
        ),
        1
    );
    let id = info[3] as u64;
    assert!(dc::gre_delete_dc(hdc));
    assert!(objects::gdi_delete_object(logical));
    let expected = private_face("Private One");
    let mut read = vec![0u8; expected.len()];
    let offset = 0u64;
    allow(&offset as *const _ as u64, 8, false);
    allow(read.as_mut_ptr() as u64, read.len(), true);
    assert_eq!(
        call(
            nt_gdi_get_font_file_data,
            &[
                id,
                0,
                &offset as *const _ as u64,
                read.as_mut_ptr() as u64,
                read.len() as u64
            ]
        ),
        1
    );
    assert_eq!(read, expected);
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[resource]), 0);
    let hdc = dc::gre_create_compatible_dc(0);
    let logical = select(hdc, "Private One");
    assert!(a_width(hdc) > 9); // New realizations substitute after uninstall.
    assert!(dc::gre_delete_dc(hdc));
    assert!(objects::gdi_delete_object(logical));
}

#[test]
fn memory_font_resource_and_realized_file_are_process_private() {
    let resource = install(&private_face("Private Two"));
    let hdc = dc::gre_create_compatible_dc(0);
    let logical = select(hdc, "Private Two");
    let mut info = [24u32, 0, 0, 0, 0, 0];
    allow(info.as_mut_ptr() as u64, 24, true);
    assert_eq!(
        call(
            nt_gdi_get_realization_info,
            &[hdc, info.as_mut_ptr() as u64]
        ),
        1
    );
    let original = ps::thread::IDENTITY.with(|identity| identity.replace((0x8899, 0x889a)));
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[resource]), 0);
    let other_dc = dc::gre_create_compatible_dc(0);
    let other_font = select(other_dc, "Private Two");
    assert!(a_width(other_dc) > 9);
    assert!(dc::gre_delete_dc(other_dc));
    assert!(objects::gdi_delete_object(other_font));
    let offset = 0u64;
    let mut bytes = [0u8; 4];
    allow(&offset as *const _ as u64, 8, false);
    allow(bytes.as_mut_ptr() as u64, 4, true);
    assert_eq!(
        call(
            nt_gdi_get_font_file_data,
            &[
                info[3] as u64,
                0,
                &offset as *const _ as u64,
                bytes.as_mut_ptr() as u64,
                4
            ]
        ),
        0
    );
    ps::thread::IDENTITY.with(|identity| identity.set(original));
    assert_eq!(a_width(hdc), 9);
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[resource]), 1);
    assert!(dc::gre_delete_dc(hdc));
    assert!(objects::gdi_delete_object(logical));
}

#[test]
fn memory_font_rejects_unreadable_files_and_unwritable_count_without_output() {
    let mut count = 0x12345678u32;
    let output = &mut count as *mut _ as u64;
    allow(output, 4, true);
    assert_eq!(
        call(nt_gdi_add_font_mem_resource_ex, &[1, 4096, 0, 0, output]),
        0
    );
    let data = [0u8; 64];
    allow(data.as_ptr() as u64, data.len(), false);
    assert_eq!(
        call(
            nt_gdi_add_font_mem_resource_ex,
            &[data.as_ptr() as u64, 64, 0, 0, output]
        ),
        0
    );
    assert_eq!(
        call(nt_gdi_add_font_mem_resource_ex, &[0, 0, 0, 0, output]),
        0
    );
    assert_eq!(
        call(
            nt_gdi_add_font_mem_resource_ex,
            &[0, u32::MAX as u64, 0, 0, output]
        ),
        0
    );
    assert_eq!(
        call(
            nt_gdi_add_font_mem_resource_ex,
            &[data.as_ptr() as u64, 64, 0, 0, 1]
        ),
        0
    );
    assert_eq!(count, 0x12345678);
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[0]), 0);
}

#[test]
fn removed_memory_font_still_produces_outlines_bitmap_coverage_and_text_pixels() {
    let resource = install(&private_face("Private Tri"));
    let fixture = crate::DrawingFixture::new();
    let logical = select(fixture.handle, "Private Tri");
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[resource]), 1);
    let matrix = [65536i32, 0, 0, 65536];
    let mut metrics = [0u8; 20];
    allow(matrix.as_ptr() as u64, 16, false);
    allow(metrics.as_mut_ptr() as u64, 20, true);
    let size = call(
        nt_gdi_get_glyph_outline,
        &[
            fixture.handle,
            b'A' as u64,
            6,
            metrics.as_mut_ptr() as u64,
            0,
            0,
            matrix.as_ptr() as u64,
            0,
        ],
    );
    assert!(size > 0 && size < 65536);
    assert_eq!(i16::from_le_bytes(metrics[16..18].try_into().unwrap()), 9);
    let mut bitmap = vec![0u8; size as usize];
    allow(bitmap.as_mut_ptr() as u64, bitmap.len(), true);
    assert_eq!(
        call(
            nt_gdi_get_glyph_outline,
            &[
                fixture.handle,
                b'A' as u64,
                6,
                metrics.as_mut_ptr() as u64,
                size,
                bitmap.as_mut_ptr() as u64,
                matrix.as_ptr() as u64,
                0
            ]
        ),
        size
    );
    assert!(bitmap.iter().any(|&pixel| pixel > 0));
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.bk_mode = dc::TRANSPARENT;
        dc.text_color = 0x000000ff;
    }
    let units = [b'A' as u16];
    allow(units.as_ptr() as u64, 2, false);
    assert_eq!(
        call(
            nt_gdi_ext_text_out,
            &[fixture.handle, 0, 0, 0, 0, units.as_ptr() as u64, 1, 0, 0]
        ),
        1
    );
    assert!((0..16).any(|y| (0..16).any(|x| fixture.pixel(x, y) != 0x00123456)));
    drop(fixture);
    assert!(objects::gdi_delete_object(logical));
}

#[test]
fn process_exit_revokes_private_handles_and_realization_ids_without_reuse() {
    let original = ps::thread::IDENTITY.with(|identity| identity.replace((0x778899, 0x77889a)));
    let bytes = private_face("Private End");
    let resource = install(&bytes);
    let hdc = dc::gre_create_compatible_dc(0);
    let logical = select(hdc, "Private End");
    let mut info = [24u32, 0, 0, 0, 0, 0];
    allow(info.as_mut_ptr() as u64, 24, true);
    assert_eq!(
        call(
            nt_gdi_get_realization_info,
            &[hdc, info.as_mut_ptr() as u64]
        ),
        1
    );
    let old_id = info[3];
    assert!(dc::gre_delete_dc(hdc));
    assert!(objects::gdi_delete_object(logical));
    crate::font_services::release_process(0x778899);
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[resource]), 0);
    let offset = 0u64;
    let mut data = [0u8; 4];
    allow(&offset as *const _ as u64, 8, false);
    allow(data.as_mut_ptr() as u64, 4, true);
    let read_old = || {
        call(
            nt_gdi_get_font_file_data,
            &[
                old_id as u64,
                0,
                &offset as *const _ as u64,
                data.as_ptr() as u64,
                4,
            ],
        )
    };
    assert_eq!(read_old(), 0);
    let new_resource = install(&bytes);
    let hdc = dc::gre_create_compatible_dc(0);
    let logical = select(hdc, "Private End");
    info[0] = 24;
    assert_eq!(
        call(
            nt_gdi_get_realization_info,
            &[hdc, info.as_mut_ptr() as u64]
        ),
        1
    );
    assert_ne!(info[3], old_id);
    assert_eq!(read_old(), 0);
    assert_eq!(call(nt_gdi_remove_font_mem_resource_ex, &[new_resource]), 1);
    assert!(dc::gre_delete_dc(hdc));
    assert!(objects::gdi_delete_object(logical));
    crate::font_services::release_process(0x778899);
    ps::thread::IDENTITY.with(|identity| identity.set(original));
}

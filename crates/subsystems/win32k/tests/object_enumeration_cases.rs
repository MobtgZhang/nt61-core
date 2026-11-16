use super::*;
use object_enumeration::nt_gdi_enum_objects as enumerate;

fn catalog(handle: u64, kind: u64, count: usize) -> Vec<u8> {
    assert_eq!(call(enumerate, &[handle, kind, 0, 0]), count as u64);
    let mut output = vec![0xa5u32; count * 4 + 4];
    let address = output.as_mut_ptr() as u64;
    mm::user_copy::allow(address, output.len() * 4, true);
    assert_eq!(call(enumerate, &[handle, kind, (output.len() * 4) as u64, address]), count as u64);
    assert_eq!(&output[count * 4..], &[0xa5; 4], "larger supplied buffer must retain its tail");
    output[..count * 4].iter().flat_map(|word| word.to_le_bytes()).collect()
}

#[test]
fn enum_objects_returns_real_device_catalog_in_original_order_and_layout() {
    let fixture = DrawingFixture::new();
    assert_eq!(call(dc::caps::nt_gdi_get_device_caps, &[fixture.handle, 24]), u32::MAX as u64);
    let original = include_bytes!("data/win7-enumobjects-x64.bin");
    let prefix = (1 + 2 + 8 + 16) * 12 * 16;
    let pens = catalog(fixture.handle, 1, 100);
    let brushes = catalog(fixture.handle, 2, 140);
    assert_eq!(&pens, &original[prefix..prefix + 1600]);
    assert_eq!(&brushes, &original[prefix + 1600..]);
    // Allocated user objects do not belong in the device catalog. Actual object
    // publication and deletion must leave the returned descriptions unchanged.
    let brush = objects::gdi_create_solid_brush(0x123456);
    let pen = objects::gdi_create_pen(objects::PenStyle::Dash, 3, 0x654321);
    assert_ne!(brush, 0);
    assert_ne!(pen, 0);
    assert_eq!(catalog(fixture.handle, 1, 100), pens);
    assert_eq!(catalog(fixture.handle, 2, 140), brushes);
    assert!(objects::gdi_delete_object(brush));
    assert!(objects::gdi_delete_object(pen));
    assert_eq!(catalog(fixture.handle, 1, 100), pens);
    let display = dc::get_dc_table().create(1, 0);
    assert_ne!(display, 0);
    assert_eq!(catalog(display, 1, 100), pens);
    assert_eq!(catalog(display, 2, 140), brushes);
    assert!(dc::gre_delete_dc(display));
}

#[test]
fn enum_objects_preserves_native_size_queries_and_rejects_partial_catalogs() {
    let fixture = DrawingFixture::new();
    for (kind, count) in [(1, 100), (2, 140)] {
        let length = count * 16;
        for size in [1, 2, 15] {
            // No pointer probing occurs on sub-record count queries, even when
            // the nonnull address is inaccessible or not DWORD aligned.
            assert_eq!(call(enumerate, &[fixture.handle, kind, size, 1]), count);
        }
        assert_eq!(call(enumerate, &[fixture.handle, kind, 0, 1]), 0);
        assert_eq!(call(enumerate, &[fixture.handle, kind, length, 0]), 0);
        let mut output = vec![0xa5u32; length as usize / 4];
        let address = output.as_mut_ptr() as u64;
        mm::user_copy::allow(address, length as usize, true);
        for short in [16, length - 1] {
            assert_eq!(call(enumerate, &[fixture.handle, kind, short, address]), 0);
            assert!(output.iter().all(|&word| word == 0xa5));
        }
        assert_eq!(call(enumerate, &[fixture.handle, kind, length, address]), count);
        // The argument types are DWORD; upper register bits are discarded.
        assert_eq!(call(enumerate, &[fixture.handle, kind | 1 << 32, length | 1 << 32, address]), count);
    }
}

#[test]
fn enum_objects_probes_entire_supplied_range_with_dword_alignment() {
    let fixture = DrawingFixture::new();
    let length = 1600;
    let mut output = vec![0xdeadbeefu32; length / 4 + 2];
    let address = output.as_mut_ptr() as u64;
    mm::user_copy::allow(address, length, true);
    // The actual catalog fits, but its supplied extra tail has not been allowed.
    assert_eq!(call(enumerate, &[fixture.handle, 1, length as u64 + 4, address]), 0);
    assert!(output.iter().all(|&word| word == 0xdeadbeef));
    mm::user_copy::allow(address, output.len() * 4, true);
    assert_eq!(call(enumerate, &[fixture.handle, 1, length as u64, address + 1]), 0);
    assert!(output.iter().all(|&word| word == 0xdeadbeef));
    assert_eq!(call(enumerate, &[fixture.handle, 1, length as u64, 1]), 0);
    let mut readonly = vec![0x7fu32; length / 4];
    let readonly_address = readonly.as_mut_ptr() as u64;
    mm::user_copy::allow(readonly_address, length, false);
    assert_eq!(call(enumerate, &[fixture.handle, 1, length as u64, readonly_address]), 0);
    assert!(readonly.iter().all(|&word| word == 0x7f));
    // Four-byte alignment is sufficient for LOGPEN records.
    assert_eq!(call(enumerate, &[fixture.handle, 1, length as u64, address + 4]), 100);

    // LOGBRUSH contains a 64-bit hatch, but the original service still only
    // requires DWORD alignment. Force an address that is not QWORD aligned.
    let mut brushes = vec![0xa5a5a5a5a5a5a5a5u64; 282];
    let brush_address = brushes.as_mut_ptr() as u64 + 4;
    assert_eq!(brush_address & 7, 4);
    mm::user_copy::allow(brush_address, 2240, true);
    assert_eq!(call(enumerate, &[fixture.handle, 2, 2240, brush_address]), 140);
    let original = include_bytes!("data/win7-enumobjects-x64.bin");
    let actual = unsafe { core::slice::from_raw_parts(brush_address as *const u8, 2240) };
    assert_eq!(actual, &original[original.len() - 2240..]);
    assert_eq!(brushes[0] as u32, 0xa5a5a5a5);
    assert_eq!(brushes[280] >> 32, 0xa5a5a5a5);
}

#[test]
fn enum_objects_rejects_invalid_types_handles_and_missing_device_backends() {
    let fixture = DrawingFixture::new();
    for kind in [0, 3, 4, 5, 6, 7, u32::MAX as u64] {
        assert_eq!(call(enumerate, &[fixture.handle, kind, 0, 0]), 0);
    }
    let brush = objects::gdi_create_solid_brush(0);
    assert_ne!(brush, 0);
    for invalid in [0, 1, brush, u64::MAX] {
        assert_eq!(call(enumerate, &[invalid, 1, 0, 0]), 0);
    }
    assert!(objects::gdi_delete_object(brush));
    for dc_type in [dc::DC_TYPE_METAFILE, dc::DC_TYPE_PRINTER] {
        let handle = dc::get_dc_table().create(dc_type, 0);
        assert_ne!(handle, 0);
        assert_eq!(call(enumerate, &[handle, 1, 0, 0]), 0);
        assert!(dc::gre_delete_dc(handle));
    }
    let stale = dc::gre_create_compatible_dc(0);
    assert!(dc::gre_delete_dc(stale));
    assert_eq!(call(enumerate, &[stale, 1, 0, 0]), 0);
}

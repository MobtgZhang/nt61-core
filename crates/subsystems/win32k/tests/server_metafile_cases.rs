//! Server metafile storage is consumed through real create/query/delete paths.
use super::*;
use server_metafile::{
    nt_gdi_create_server_meta_file as create, nt_gdi_get_server_meta_file_bits as get,
};

fn create_payload(kind: u32, bytes: &[u32]) -> u64 {
    mm::user_copy::allow(bytes.as_ptr() as u64, bytes.len() * 4, false);
    call(
        create,
        &[
            kind as u64,
            (bytes.len() * 4) as u64,
            bytes.as_ptr() as u64,
            0x1_ffff_ffff,
            0xffff_ff80,
            0x8000_0000,
        ],
    )
}

#[test]
fn server_metafile_roundtrip_exact_size_and_generic_deletion() {
    for kind in [0x5f50_464d, 0x5845_464d] {
        let bytes = [0x0102_0304, 0xdead_beef, 0x8765_4321];
        let handle = create_payload(kind, &bytes);
        assert_ne!(handle, 0);
        assert_eq!(call(get, &[handle, 0, 1, 1, 1, 1, 1]), 12);
        let mut output = [0u32; 3];
        let output_ptr = user_pointer(&mut output, true);
        let mut meta = [0xa5a5_a5a5u32; 4];
        let meta_ptr = user_pointer(&mut meta, true);
        for count in [1, 11, 13, u32::MAX as u64] {
            assert_eq!(
                call(
                    get,
                    &[
                        handle,
                        count,
                        output_ptr,
                        meta_ptr,
                        meta_ptr + 4,
                        meta_ptr + 8,
                        meta_ptr + 12
                    ]
                ),
                0
            );
            assert_eq!(meta, [0xa5a5_a5a5; 4]);
            assert_eq!(output, [0; 3]);
        }
        assert_eq!(
            call(
                get,
                &[
                    handle,
                    12,
                    output_ptr,
                    meta_ptr,
                    meta_ptr + 4,
                    meta_ptr + 8,
                    meta_ptr + 12
                ]
            ),
            12
        );
        assert_eq!(output, bytes);
        assert_eq!(meta, [kind, u32::MAX, 0xffff_ff80, 0x8000_0000]);
        assert!(objects::with_handle_table(|t| t
            .get(handle)
            .is_some_and(|e| e.process_id == 0)));
        let other = std::thread::spawn(move || {
            ps::thread::IDENTITY.with(|id| id.set((77, 78)));
            assert_eq!(call(get, &[handle, 0]), 12);
        });
        other.join().unwrap();
        assert!(objects::gdi_delete_object(handle));
        assert_eq!(call(get, &[handle, 0]), 0);
        assert!(!objects::gdi_delete_object(handle));
    }
}

#[test]
fn server_metafile_captures_opaque_bytes_and_accepts_unaligned_metadata() {
    let mut input = [0x8877_6655u32, 0x4433_2211];
    let input_ptr = user_pointer(&input, false);
    let handle = call(create, &[0x1_5845_464d, 0x1_0000_0005, input_ptr, 7, 8, 9]);
    assert_ne!(handle, 0);
    // Publication owns a snapshot; later caller writes must not alter it.
    input[0] = 0;
    input[1] = 0;
    assert_eq!(input, [0, 0]);
    let mut output = [0u32; 2];
    let output_ptr = user_pointer(&mut output, true);
    let mut metadata = [0xa5u8; 17];
    let metadata_ptr = user_pointer(&mut metadata, true) + 1;
    assert_eq!(
        call(
            get,
            &[
                handle,
                0x1_0000_0005,
                output_ptr,
                metadata_ptr,
                metadata_ptr + 4,
                metadata_ptr + 8,
                metadata_ptr + 12
            ]
        ),
        5
    );
    assert_eq!(output[0], 0x8877_6655);
    assert_eq!(output[1], 0x11);
    assert_eq!(metadata[0], 0xa5);
    let expected = [0x5845_464du32, 7, 8, 9].map(u32::to_le_bytes).concat();
    assert_eq!(&metadata[1..], expected);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn server_metafile_overlapping_metadata_has_native_last_store_wins_order() {
    let bytes = [0x1234_5678];
    let handle = create_payload(0x5845_464d, &bytes);
    assert_ne!(handle, 0);
    let mut shared = 0u32;
    let shared_ptr = user_pointer(&mut shared, true);
    let mut output = [0u32];
    let output_ptr = user_pointer(&mut output, true);
    assert_eq!(
        call(
            get,
            &[handle, 4, output_ptr, shared_ptr, shared_ptr, shared_ptr, shared_ptr]
        ),
        4
    );
    assert_eq!(shared, u32::MAX); // map mode is written after x, y and type.
    assert_eq!(output, bytes);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn server_metafile_empty_capture_requires_nonnull_but_never_probes_pointer() {
    assert_eq!(call(create, &[0x5845_464d, 0, 0]), 0);
    let handle = call(create, &[0x5845_464d, 0, 1]);
    assert_ne!(handle, 0);
    assert_eq!(call(get, &[handle, 0, 1, 1, 1, 1, 1]), 0);
    assert_eq!(call(get, &[handle, 1, 1, 1, 1, 1, 1]), 0);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn server_metafile_invalid_type_size_alignment_capture_and_allocation_fail_without_leaks() {
    let before = mm::pool::balance();
    for kind in [0, 1, 0x464d_4558] {
        assert_eq!(call(create, &[kind, 4, 1]), 0);
    }
    for count in [0x271_0000 - 0x30 + 1, 0xffff_ffff] {
        assert_eq!(call(create, &[0x5845_464d, count, 4]), 0);
    }
    for input in [1, 3, 4, u64::MAX - 3] {
        assert_eq!(call(create, &[0x5845_464d, 4, input]), 0);
    }
    let bytes = [1u32; 2];
    mm::pool::fail_after(0);
    assert_eq!(create_payload(0x5845_464d, &bytes), 0);
    assert_eq!(mm::pool::balance(), before);
    let brush = objects::gdi_create_solid_brush(0x123456);
    assert_eq!(call(get, &[brush, 0]), 0);
    assert!(objects::gdi_delete_object(brush));
    assert_eq!(call(get, &[u64::MAX, 0]), 0);
}

#[test]
fn server_metafile_output_faults_preserve_original_sequential_store_order() {
    let bytes = [0x1234_5678];
    let handle = create_payload(0x5845_464d, &bytes);
    assert_ne!(handle, 0);
    for fail in 0..=4 {
        let mut meta = [0xaaaa_aaaau32; 4];
        // Each DWORD gets a separate registration: omitted words must fault.
        let mut addresses = [1u64; 4];
        for index in 0..fail {
            addresses[index] = user_pointer(&mut meta[index], true);
        }
        // Native metadata order is x,y,type,mapmode.
        assert_eq!(
            call(
                get,
                &[
                    handle,
                    4,
                    1,
                    addresses[2],
                    addresses[3],
                    addresses[0],
                    addresses[1]
                ]
            ),
            0
        );
        for index in 0..4 {
            let expected = [0xffff_ff80, 0x8000_0000, 0x5845_464d, u32::MAX][index];
            assert_eq!(
                meta[index],
                if index < fail { expected } else { 0xaaaa_aaaa }
            );
        }
    }
    assert!(objects::gdi_delete_object(handle));
}

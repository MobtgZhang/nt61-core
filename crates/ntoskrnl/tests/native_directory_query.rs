//! Exercise the production byte encoder with the production raw NTFS type.
#![allow(dead_code, non_camel_case_types)]
extern crate alloc;
pub use ntoskrnl::ntdll::{status, types};
#[path = "../../fs/src/fs/ntfs/raw_directory_entry.rs"]
pub mod raw_directory_entry;
#[path = "../../libs/ntdll/src/ntdll/file/directory_query.rs"]
pub mod directory_query;
pub mod fs {
    pub mod ntfs { pub use crate::raw_directory_entry::RawDirEntry; }
}
pub mod libs { pub mod ntdll { pub use crate::status; } }
#[path = "../src/io/directory_search.rs"]
pub mod directory_search;
#[path = "native_directory_matches.rs"]
mod original_matches;
use directory_query::{serialize, Disposition};
use raw_directory_entry::RawDirEntry;

fn entry(name: &str) -> RawDirEntry {
    let mut raw = RawDirEntry::empty();
    let units: Vec<_> = name.encode_utf16().collect();
    raw.name[..units.len()].copy_from_slice(&units);
    raw.name_len = units.len() as u16;
    raw.creation_time = -123;
    raw.last_write_time = 0x1234_5678;
    raw.last_access_time = -456;
    raw.change_time = 0x1234;
    raw.file_attributes = 0x2123;
    raw.size = 0x1122_3344_5566_7788;
    raw.alloc_size = 0x1122_3344_5566_8000;
    raw.mft_ref = 0x8877_6655_4433_2211;
    raw
}
fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap())
}
fn qword(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset+8].try_into().unwrap())
}
const CLASSES: [(u32, usize); 6] = [(1,0x40),(2,0x44),(3,0x5e),(12,0xc),(37,0x68),(38,0x50)];

#[test]
fn actual_abi_offsets_are_distinct_from_structure_size() {
    use core::mem::{offset_of, size_of};
    assert_eq!(offset_of!(types::FileDirectoryInformation, file_name), 0x40);
    assert_eq!(offset_of!(types::FileBothDirectoryInformation, file_name), 0x5e);
    assert_eq!(offset_of!(types::FileIdBothDirectoryInformation, file_id), 0x60);
    assert_eq!(offset_of!(types::FileIdBothDirectoryInformation, file_name), 0x68);
    assert_eq!(size_of::<types::FileDirectoryInformation>(), 0x48);
    assert_eq!(size_of::<types::FileBothDirectoryInformation>(), 0x60);
    assert_eq!(size_of::<types::FileIdBothDirectoryInformation>(), 0x70);
}

#[test]
fn all_classes_encode_exact_name_offsets_and_available_metadata() {
    let raw = entry("中😀");
    for (class, prefix) in CLASSES {
        let value = serialize(&[raw], class, prefix + 6, false).unwrap();
        assert_eq!(value.disposition, Disposition::Complete);
        assert_eq!(value.entries_written, 1);
        assert_eq!(value.bytes.len(), prefix + 6);
        assert_eq!(dword(&value.bytes, 0), 0);
        assert_eq!(dword(&value.bytes, 4), 0);
        assert_eq!(&value.bytes[prefix..], &[0x2d,0x4e,0x3d,0xd8,0x00,0xde]);
        if class == 12 {
            assert_eq!(dword(&value.bytes, 8), 6);
        } else {
            assert_eq!(qword(&value.bytes, 8), (-123i64) as u64);
            assert_eq!(qword(&value.bytes, 0x10), (-456i64) as u64);
            assert_eq!(qword(&value.bytes, 0x18), 0x1234_5678);
            assert_eq!(qword(&value.bytes, 0x20), 0x1234);
            assert_eq!(qword(&value.bytes, 0x28), raw.size);
            assert_eq!(qword(&value.bytes, 0x30), raw.alloc_size);
            assert_eq!(dword(&value.bytes, 0x38), 0x2123);
            assert_eq!(dword(&value.bytes, 0x3c), 6);
            if class != 1 { assert_eq!(dword(&value.bytes, 0x40), 0); }
            if matches!(class, 3 | 37) { assert!(value.bytes[0x44..0x5e].iter().all(|b| *b == 0)); }
            if class == 37 { assert_eq!(qword(&value.bytes, 0x60), raw.mft_ref); }
            if class == 38 { assert_eq!(qword(&value.bytes, 0x48), raw.mft_ref); }
        }
    }
}

#[test]
fn unequal_names_link_by_preceding_record_size_and_leave_final_record_unpadded() {
    let entries = [entry("a"), entry("longer"), entry("xy")];
    for (class, prefix) in CLASSES {
        let result = serialize(&entries, class, 4096, false).unwrap();
        let second = (prefix + 2 + 7) & !7;
        let third = (second + prefix + 12 + 7) & !7;
        assert_eq!(result.entries_written, 3);
        assert_eq!(dword(&result.bytes, 0) as usize, second);
        assert_eq!(dword(&result.bytes, second) as usize, third - second);
        assert_eq!(dword(&result.bytes, third), 0);
        assert_eq!(result.bytes.len(), third + prefix + 4);
        assert!(result.bytes[prefix+2..second].iter().all(|b| *b == 0));
        assert_eq!(&result.bytes[third+prefix..], &[b'x',0,b'y',0]);
    }
}

#[test]
fn every_capacity_is_bounded_and_reports_header_partial_or_complete() {
    let raw = entry("file");
    for (class, prefix) in CLASSES {
        for capacity in 0..prefix + 8 + 10 {
            let value = serialize(&[raw], class, capacity, false).unwrap();
            assert!(value.bytes.len() <= capacity);
            if capacity < prefix {
                assert!(value.bytes.is_empty());
                assert_eq!(value.disposition, Disposition::HeaderTooSmall);
                assert_eq!(value.status(), status::STATUS_INFO_LENGTH_MISMATCH);
            } else if capacity < prefix + 8 {
                assert_eq!(value.disposition, Disposition::PartialName);
                assert_eq!(value.entries_written, 0);
                assert_eq!(value.bytes.len(), capacity);
                assert_eq!(value.status(), status::STATUS_BUFFER_OVERFLOW);
                assert_eq!(dword(&value.bytes, if class == 12 {8} else {0x3c}), 8);
                assert_eq!(dword(&value.bytes, 0), 0);
                assert_eq!(&value.bytes[prefix..], &b"f\0i\0l\0e\0"[..capacity-prefix]);
            } else {
                assert_eq!(value.disposition, Disposition::Complete);
                assert_eq!(value.entries_written, 1);
                assert_eq!(value.bytes.len(), prefix + 8);
            }
        }
    }
}

#[test]
fn a_full_first_record_stays_terminated_when_second_cannot_fit() {
    let entries = [entry("a"), entry("much longer")];
    for (class,prefix) in CLASSES {
        let first_end = prefix + 2;
        let second_start = (first_end + 7) & !7;
        for capacity in first_end..second_start + prefix + 22 {
            let value = serialize(&entries,class,capacity,false).unwrap();
            assert_eq!(value.entries_written,1);
            assert_eq!(value.disposition, Disposition::MoreEntries);
            assert_eq!(value.status(),status::STATUS_SUCCESS);
            assert_eq!(value.bytes.len(),first_end);
            assert_eq!(dword(&value.bytes,0),0);
        }
    }
}

#[test]
fn single_and_empty_input_do_not_allocate_unrequested_capacity() {
    let entries = [entry("a"), entry("second")];
    for (class,prefix) in CLASSES {
        let result = serialize(&entries,class,usize::MAX,true).unwrap();
        assert_eq!(result.entries_written,1);
        assert_eq!(result.bytes.len(),prefix+2);
        assert_eq!(result.disposition,Disposition::Complete);
        let empty = serialize(&[],class,usize::MAX,false).unwrap();
        assert!(empty.bytes.is_empty());
        assert_eq!(empty.entries_written,0);
        assert_eq!(empty.status(),status::STATUS_SUCCESS);
    }
}

#[test]
fn hostile_lengths_and_unsupported_classes_are_rejected_without_indexing_sources() {
    for name_len in [257, 511, u16::MAX] {
        let mut invalid = RawDirEntry::empty();
        invalid.name_len = name_len;
        assert_eq!(serialize(&[invalid],3,usize::MAX,false).unwrap_err(), status::STATUS_INVALID_PARAMETER);
    }
    for class in [0,4,36,39,u32::MAX] {
        assert_eq!(serialize(&[],class,usize::MAX,false).unwrap_err(),status::STATUS_INVALID_INFO_CLASS);
    }
    let mut max = RawDirEntry::empty();
    max.name_len = 256;
    max.name.fill(0xffff);
    assert_eq!(serialize(&[max],3,606,false).unwrap().bytes.len(),606);
}

#[test]
fn empty_names_and_directory_attribute_do_not_overrun_or_invent_name_bytes() {
    let mut raw = RawDirEntry::empty();
    raw.is_dir = true;
    for (class,prefix) in CLASSES {
        let result = serialize(&[raw],class,prefix,false).unwrap();
        assert_eq!(result.bytes.len(),prefix);
        assert_eq!(result.entries_written,1);
        if class != 12 { assert_eq!(dword(&result.bytes,0x38),0x10); }
    }
}

fn upcase_ascii(unit: u16) -> u16 {
    if (b'a' as u16..=b'z' as u16).contains(&unit) { unit - 32 } else { unit }
}

#[test]
fn nt_dos_wildcards_match_412_original_win7_instruction_results() {
    for &(expression, name, ignore_case, expected) in original_matches::ORIGINAL_MATCHES {
        let actual = directory_search::matches_expression(
            expression, name, |unit| if ignore_case { upcase_ascii(unit) } else { unit },
        ).unwrap();
        assert_eq!(actual, expected, "expression={expression:?}, name={name:?}, ignore={ignore_case}");
    }
}

#[test]
fn searches_capture_owned_expression_once_restart_and_keep_file_owners_independent() {
    use directory_search::SearchState;
    let mut state = SearchState::new();
    let mut source: Vec<_> = "*.txt".encode_utf16().collect();
    state.begin(Some(&source), false, upcase_ascii).unwrap();
    source.fill(b'?' as u16);
    assert_eq!(state.expression().unwrap(), &"*.TXT".encode_utf16().collect::<Vec<_>>());
    assert!(state.matches(&"Example.TxT".encode_utf16().collect::<Vec<_>>(),upcase_ascii).unwrap());
    assert!(!state.matches(&"Example.exe".encode_utf16().collect::<Vec<_>>(),upcase_ascii).unwrap());
    assert!(!state.returned_entry());
    state.commit(7, 1);
    state.begin(Some(&[b'*' as u16]), false, upcase_ascii).unwrap();
    assert_eq!(state.position(),7);
    assert!(state.returned_entry());
    state.begin(Some(&[b'*' as u16]), true, upcase_ascii).unwrap();
    assert_eq!(state.position(),0);
    assert_eq!(state.expression().unwrap(), &"*.TXT".encode_utf16().collect::<Vec<_>>());
    let mut other = SearchState::new();
    other.begin(None,false,upcase_ascii).unwrap();
    assert!(other.matches(&[b'x' as u16],upcase_ascii).unwrap());
    assert_eq!(other.position(),0);
    assert!(!other.returned_entry());
}

#[test]
fn cursor_changes_are_explicit_and_empty_pattern_is_distinct_from_no_pattern() {
    use directory_search::SearchState;
    let mut state = SearchState::new();
    assert_eq!(state.matches(&[1],|c|c).unwrap_err(),status::STATUS_INVALID_PARAMETER);
    state.begin(Some(&[]), false, |c|c).unwrap();
    assert!(!state.matches(&[1],|c|c).unwrap());
    assert!(state.matches(&[],|c|c).unwrap());
    state.commit(4,0);
    assert_eq!(state.position(),4);
    assert!(!state.returned_entry());
    state.begin(None,false,|c|c).unwrap();
    assert_eq!(state.expression(),Some(&[][..]));
}

#[test]
fn matcher_uses_caller_upcase_mapping_without_unicode_expansion_or_normalization() {
    let custom = |unit| if unit == 0xe4 { 0xc4 } else { unit };
    assert!(directory_search::matches_expression(&[0xc4], &[0xe4],custom).unwrap());
    assert!(!directory_search::matches_expression(&[0xc4], &[0xe4],|c|c).unwrap());
    assert!(!directory_search::matches_expression(&[b'S' as u16,b'S' as u16], &[0xdf],custom).unwrap());
    assert!(!directory_search::matches_expression(&[b'?' as u16], &[0xd83d,0xde00],|c|c).unwrap());
    assert!(directory_search::matches_expression(&[b'?' as u16,b'?' as u16], &[0xd83d,0xde00],|c|c).unwrap());
}

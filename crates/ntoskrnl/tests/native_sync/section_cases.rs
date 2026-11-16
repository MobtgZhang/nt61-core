use super::named_cases::named;
use super::*;
use ntdll::section::*;

fn create(path: &str, access: u32) -> HANDLE {
    let mut handle = core::ptr::null_mut();
    let mut size = 0x1234;
    named(path, 0, |attributes| unsafe {
        assert_eq!(
            nt_create_section(&mut handle, access, attributes, &mut size,
                4, 0x0800_0000, core::ptr::null_mut()),
            STATUS_SUCCESS
        );
    });
    handle
}

fn query(handle: HANDLE) -> Result<i64, NTSTATUS> {
    let mut information = SectionBasicInformation {
        base_address: core::ptr::null_mut(),
        attributes: 0,
        reserved: 0,
        maximum_size: 0,
    };
    let status = unsafe {
        nt_query_section(handle, 0, (&mut information as *mut SectionBasicInformation).cast(),
            core::mem::size_of_val(&information), core::ptr::null_mut())
    };
    if status < 0 { Err(status) } else { Ok(information.maximum_size) }
}

#[test]
fn named_sections_share_metadata_until_the_last_handle_closes() {
    let _guard = state_guard();
    let path = "\\BaseNamedObjects\\SharedSection";
    let first = create(path, 0xf001f);
    let mut opened = core::ptr::null_mut();
    named(path, 0x40, |attributes| unsafe {
        assert_eq!(nt_open_section(&mut opened, 1, attributes), STATUS_SUCCESS);
    });
    assert_eq!(query(opened), Ok(0x1234));
    assert!(ntdll::file::free_handle(first));
    assert_eq!(query(opened), Ok(0x1234));
    assert!(ntdll::file::free_handle(opened));
    named(path, 0, |attributes| unsafe {
        assert_eq!(nt_open_section(&mut opened, 1, attributes), STATUS_OBJECT_NAME_NOT_FOUND);
    });
}

#[test]
fn openif_returns_existing_section_without_replacing_its_metadata() {
    let _guard = state_guard();
    let path = "\\BaseNamedObjects\\OpenIfSection";
    let first = create(path, 1);
    let mut alias = core::ptr::null_mut();
    let mut size = 0x9876;
    named(path, 0, |attributes| unsafe {
        assert_eq!(nt_create_section(&mut alias, 1, attributes, &mut size,
            4, 0x0400_0000, core::ptr::null_mut()), STATUS_OBJECT_NAME_COLLISION);
        assert!(alias.is_null());
        attributes.attributes = 0x80;
        assert_eq!(nt_create_section(&mut alias, 1, attributes, &mut size,
            4, 0x0400_0000, core::ptr::null_mut()), STATUS_OBJECT_NAME_EXISTS);
    });
    assert_eq!(query(alias), Ok(0x1234));
    for handle in [first, alias] { assert!(ntdll::file::free_handle(handle)); }
}

#[test]
fn section_aliases_enforce_their_own_access_and_survive_source_close() {
    let _guard = state_guard();
    let first = create("\\BaseNamedObjects\\SectionRights", 0xf001f);
    let write_only = ntdll::file::duplicate_sync_handle_with_access(first, 2, 0, 0).unwrap();
    let query_only = ntdll::file::duplicate_sync_handle_with_access(first, 1, 0, 0).unwrap();
    assert_eq!(query(write_only), Err(STATUS_ACCESS_DENIED));
    assert_eq!(ntdll::file::duplicate_sync_handle_with_access(query_only, 2, 0, 0),
        Err(STATUS_ACCESS_DENIED));
    assert!(ntdll::file::free_handle(first));
    assert_eq!(query(query_only), Ok(0x1234));
    assert_eq!(ntdll::file::close_sync_handle(write_only), Ok(()));
    assert_eq!(ntdll::file::close_sync_handle(query_only), Ok(()));
    assert_eq!(query(query_only), Err(STATUS_INVALID_HANDLE));
}

#[test]
fn section_generic_access_mapping_matches_the_object_type() {
    let _guard = state_guard();
    for (generic, rights) in [(0x8000_0000, 0x20005), (0x4000_0000, 0x20012),
        (0x2000_0000, 0x20008), (0x1000_0000, 0xf001f)]
    {
        assert_eq!(ntdll::object::map_sync_access(ntdll::file::HandleKind::Section, generic), Ok(rights));
    }
    assert_eq!(ntdll::object::map_sync_access(ntdll::file::HandleKind::Section, 0x100000),
        Err(STATUS_ACCESS_DENIED));
}

#[test]
fn section_open_probes_attributes_and_does_not_publish_on_failure() {
    let _guard = state_guard();
    let mut output = 0xfeedusize as HANDLE;
    unsafe {
        assert_eq!(nt_open_section(&mut output, 1, 1usize as *mut ObjectAttributes),
            STATUS_ACCESS_VIOLATION);
        assert_eq!(nt_open_section(&mut output, 1, core::ptr::null_mut()),
            STATUS_OBJECT_NAME_INVALID);
    }
    assert_eq!(output as usize, 0xfeed);
}

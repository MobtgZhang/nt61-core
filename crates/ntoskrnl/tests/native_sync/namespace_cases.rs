use super::*;
use super::named_cases::named;
use ntdll::namespace::*;

fn directory(path: &str, access: u32) -> HANDLE {
    let mut handle = core::ptr::null_mut();
    named(path, 0, |attributes| unsafe {
        assert_eq!(nt_create_directory_object(&mut handle, access, attributes), STATUS_SUCCESS);
    });
    handle
}

fn event(path: &str) -> HANDLE {
    let mut handle = core::ptr::null_mut();
    named(path, 0, |attributes| unsafe {
        assert_eq!(nt_create_event(&mut handle, 0x1f0003, attributes, 1, 0), STATUS_SUCCESS);
    });
    handle
}

#[test]
fn directories_resolve_relative_children_and_enforce_object_types() {
    let _guard = state_guard();
    let parent = directory("\\BaseNamedObjects\\NamespaceParent", 0xf000f);
    let mut child = core::ptr::null_mut();
    let mut opened = core::ptr::null_mut();
    named("Child", 0, |attributes| unsafe {
        attributes.root_directory = parent;
        assert_eq!(nt_create_event(&mut child, 0x1f0003, attributes, 1, 1), STATUS_SUCCESS);
        assert_eq!(nt_open_event(&mut opened, 0x100000, attributes), STATUS_SUCCESS);
        assert_eq!(nt_wait_for_single_object_kernel(opened, 0, Some(0)), STATUS_SUCCESS);
        assert_eq!(nt_close_sync(opened), STATUS_SUCCESS);
        assert_eq!(nt_open_directory_object(&mut opened, 1, attributes), STATUS_OBJECT_TYPE_MISMATCH);
    });
    named("\\BaseNamedObjects\\NamespaceParent\\Missing\\Child", 0, |attributes| unsafe {
        assert_eq!(nt_open_event(&mut opened, 1, attributes), STATUS_OBJECT_PATH_NOT_FOUND);
    });
    named("\\BaseNamedObjects\\NamespaceParent\\Child\\Nested", 0, |attributes| unsafe {
        assert_eq!(nt_open_event(&mut opened, 1, attributes), STATUS_OBJECT_TYPE_MISMATCH);
    });
    assert!(ntdll::file::free_handle(child));
    assert!(ntdll::file::free_handle(parent));
}

#[test]
fn directory_enumeration_packs_user_pointers_and_preserves_resume_context() {
    let _guard = state_guard();
    let parent = directory("\\BaseNamedObjects\\Enumeration", 0xf000f);
    let first = event("\\BaseNamedObjects\\Enumeration\\Alpha");
    let second = event("\\BaseNamedObjects\\Enumeration\\Beta");
    let mut bytes = [0xccu8; 512];
    let mut context = 0xdeadbeef;
    let mut length = 0;
    let query = |bytes: &mut [u8], single, restart, context: &mut u32, length: &mut u32| unsafe {
        nt_query_directory_object(parent, bytes.as_mut_ptr().cast(), bytes.len() as u32,
            single, restart, context, length)
    };
    assert_eq!(query(&mut [], 1, 1, &mut context, &mut length), STATUS_BUFFER_TOO_SMALL);
    assert_eq!(context, 0xdeadbeef);
    let required = length as usize;
    assert_eq!(query(&mut bytes[..required - 1], 1, 1, &mut context, &mut length), STATUS_BUFFER_TOO_SMALL);
    assert_eq!(context, 0xdeadbeef);
    assert_eq!(query(&mut bytes[..required], 1, 1, &mut context, &mut length), STATUS_SUCCESS);
    assert_eq!(context, 1);
    let name = unsafe { bytes.as_ptr().cast::<UnicodeString>().read_unaligned() };
    assert_eq!(name.length, 10);
    assert!(name.buffer as usize >= bytes.as_ptr() as usize);
    assert!(name.buffer as usize + name.maximum_length as usize <= bytes.as_ptr() as usize + required);
    assert_eq!(unsafe { core::slice::from_raw_parts(name.buffer, 6) }, &[65, 108, 112, 104, 97, 0]);
    let header = 2 * core::mem::size_of::<UnicodeString>();
    assert!(bytes[header..header * 2].iter().all(|&byte| byte == 0));
    assert_eq!(query(&mut bytes, 1, 0, &mut context, &mut length), STATUS_SUCCESS);
    assert_eq!(context, 2);
    assert_eq!(query(&mut bytes, 1, 0, &mut context, &mut length), STATUS_NO_MORE_ENTRIES);
    assert_eq!(context, 2);
    assert_eq!(length as usize, header);
    assert_eq!(query(&mut bytes[..header], 0, 1, &mut context, &mut length), 0x105);
    assert_eq!(context, 0);
    assert_eq!(length as usize, header);
    assert_eq!(query(&mut bytes, 0, 1, &mut context, &mut length), STATUS_SUCCESS);
    assert_eq!(context, 2);
    for handle in [first, second, parent] { assert!(ntdll::file::free_handle(handle)); }
}

#[test]
fn empty_directory_query_clears_terminator_and_preserves_context() {
    let _guard = state_guard();
    let parent = directory("\\BaseNamedObjects\\EmptyDirectory", 1);
    let mut bytes = [0xccu8; 64];
    let mut context = 0xdeadbeef;
    let mut length = 0;
    unsafe {
        assert_eq!(nt_query_directory_object(parent, bytes.as_mut_ptr().cast(), 64, 0, 1,
            &mut context, &mut length), STATUS_NO_MORE_ENTRIES);
    }
    assert_eq!(context, 0xdeadbeef);
    assert_eq!(length as usize, 2 * core::mem::size_of::<UnicodeString>());
    assert!(bytes[..length as usize].iter().all(|&byte| byte == 0));
    assert!(bytes[length as usize..].iter().all(|&byte| byte == 0xcc));
    assert!(ntdll::file::free_handle(parent));
}

#[test]
fn symbolic_links_resolve_intermediate_and_final_components_and_query_counted_target() {
    let _guard = state_guard();
    let parent = directory("\\BaseNamedObjects\\LinkTarget", 0xf000f);
    let child = event("\\BaseNamedObjects\\LinkTarget\\Child");
    let mut target: Vec<u16> = "\\BaseNamedObjects\\LinkTarget".encode_utf16().collect();
    let target = UnicodeString { length: (target.len() * 2) as u16,
        maximum_length: (target.len() * 2) as u16, buffer: target.as_mut_ptr() };
    let mut link = core::ptr::null_mut();
    let mut opened = core::ptr::null_mut();
    named("\\BaseNamedObjects\\DirectoryAlias", 0, |attributes| unsafe {
        assert_eq!(nt_create_symbolic_link_object(&mut link, 0xf0001, attributes, &target), STATUS_SUCCESS);
        assert_eq!(nt_open_directory_object(&mut opened, 1, attributes), STATUS_SUCCESS);
        assert!(ntdll::file::free_handle(opened));
    });
    named("\\BaseNamedObjects\\DirectoryAlias\\Child", 0, |attributes| unsafe {
        assert_eq!(nt_open_event(&mut opened, 1, attributes), STATUS_SUCCESS);
        assert!(ntdll::file::free_handle(opened));
    });
    let mut buffer = [0xeeeeu16; 128];
    let mut output = UnicodeString { length: 0x1234, maximum_length: target.length, buffer: buffer.as_mut_ptr() };
    let mut returned = 0;
    unsafe {
        assert_eq!(nt_query_symbolic_link_object(link, &mut output, &mut returned), STATUS_BUFFER_TOO_SMALL);
        assert_eq!(output.length, 0x1234);
        assert_eq!(returned, u32::from(target.length) + 2);
        output.maximum_length += 2;
        assert_eq!(nt_query_symbolic_link_object(link, &mut output, &mut returned), STATUS_SUCCESS);
    }
    assert_eq!(output.length, target.length);
    assert_eq!(buffer[target.length as usize / 2], 0);
    for handle in [child, parent, link] { assert!(ntdll::file::free_handle(handle)); }
}

#[test]
fn directory_close_and_name_reuse_do_not_adopt_old_children() {
    let _guard = state_guard();
    let parent = directory("\\BaseNamedObjects\\RecreatedDirectory", 0xf000f);
    let child = event("\\BaseNamedObjects\\RecreatedDirectory\\Child");
    assert!(ntdll::file::free_handle(parent));
    let parent = directory("\\BaseNamedObjects\\RecreatedDirectory", 0xf000f);
    let mut opened = core::ptr::null_mut();
    named("\\BaseNamedObjects\\RecreatedDirectory\\Child", 0, |attributes| unsafe {
        assert_eq!(nt_open_event(&mut opened, 1, attributes), STATUS_OBJECT_NAME_NOT_FOUND);
    });
    let replacement = event("\\BaseNamedObjects\\RecreatedDirectory\\Child");
    assert_ne!(ntdll::file::lookup_handle(child).unwrap().identity,
        ntdll::file::lookup_handle(replacement).unwrap().identity);
    for handle in [child, replacement, parent] { assert!(ntdll::file::free_handle(handle)); }
}

#[test]
fn permanent_objects_survive_close_and_temporary_objects_require_delete_access() {
    let _guard = state_guard();
    let handle = event("\\BaseNamedObjects\\PermanentEvent");
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(false));
    assert_eq!(nt_make_permanent_object(handle), STATUS_PRIVILEGE_NOT_HELD);
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(true));
    assert_eq!(nt_make_permanent_object(handle), STATUS_SUCCESS);
    ntdll::privilege::ENABLED.with(|enabled| enabled.set(false));
    assert!(ntdll::file::free_handle(handle));
    let mut opened = core::ptr::null_mut();
    named("\\BaseNamedObjects\\PermanentEvent", 0, |attributes| unsafe {
        assert_eq!(nt_open_event(&mut opened, 1, attributes), STATUS_SUCCESS);
        assert_eq!(nt_make_temporary_object(opened), STATUS_ACCESS_DENIED);
        assert!(ntdll::file::free_handle(opened));
        assert_eq!(nt_open_event(&mut opened, 0x10000, attributes), STATUS_SUCCESS);
        assert_eq!(nt_make_temporary_object(opened), STATUS_SUCCESS);
        assert!(ntdll::file::free_handle(opened));
        assert_eq!(nt_open_event(&mut opened, 1, attributes), STATUS_OBJECT_NAME_NOT_FOUND);
    });
}

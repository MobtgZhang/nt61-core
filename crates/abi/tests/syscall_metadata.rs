use nt61_abi::{syscall_metadata::*, syscalls::*};

#[test]
fn native_metadata_matches_canonical_indices_and_supports_the_longest_call() {
    let mut x86_numbers = std::collections::BTreeSet::new();
    for (index, service) in NATIVE_SERVICE_METADATA.iter().enumerate() {
        assert_eq!(usize::from(service.number), index);
        assert_eq!(service.name, NATIVE_SYSCALLS[index].name);
        assert_eq!(service.number, NATIVE_SYSCALLS[index].number);
        assert!(x86_numbers.insert(service.x86_number));
        assert!(service.arguments <= 17);
    }
    assert_eq!(NATIVE_SERVICE_METADATA.iter().map(|s| s.arguments).max(), Some(17));
    assert_eq!(NATIVE_SERVICE_METADATA[0xc].name, "NtClose");
    assert_eq!(NATIVE_SERVICE_METADATA[0xc].arguments, 1);
    assert_eq!(NATIVE_SERVICE_METADATA[0x43].arguments, 0);
    assert_eq!(NATIVE_SERVICE_METADATA[0x52].arguments, 11);
}

#[test]
fn gui_stack_metadata_retains_original_large_signatures() {
    assert_eq!(WIN32K_STACK_ARGUMENTS.len(), WIN32K_SYSCALLS.len());
    assert_eq!(WIN32K_STACK_ARGUMENTS[0x008], 7); // NtGdiBitBlt: 11 total
    assert_eq!(WIN32K_STACK_ARGUMENTS[0x076], 11); // NtUserCreateWindowEx: 15
    assert_eq!(WIN32K_STACK_ARGUMENTS[0x012], 0); // NtGdiFlush
}

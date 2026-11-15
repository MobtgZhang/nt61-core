use nt61_boot_types::boot_driver::*;

#[test]
fn all_64_bit_architectures_share_the_private_boot_layout() {
    assert_eq!(core::mem::size_of::<BootDriverObject>(), 40);
    assert_eq!(core::mem::offset_of!(BootDriverObject, dispatch), 24);
    assert_eq!(core::mem::offset_of!(BootDriverObject, unload), 32);
    assert_eq!(core::mem::size_of::<BootDriverRegistryPath>(), 16);
    assert_eq!(core::mem::offset_of!(BootDriverRegistryPath, buffer), 8);
}

#[test]
fn private_boot_contract_rejects_version_size_and_magic_drift() {
    let mut object = BootDriverObject::new();
    assert!(object.has_supported_abi());
    object.magic[0] ^= 1;
    assert!(!object.has_supported_abi());
    object = BootDriverObject::new();
    object.version += 1;
    assert!(!object.has_supported_abi());
    object = BootDriverObject::new();
    object.size -= 1;
    assert!(!object.has_supported_abi());
}

#[test]
fn boot_info_v11_publishes_the_complete_stack_reservation() {
    use nt61_boot_types::boot_types::BootInfo;
    assert_eq!(BootInfo::VERSION, 11);
    assert_eq!(core::mem::offset_of!(BootInfo, kernel_stack_base),
        core::mem::offset_of!(BootInfo, _reserved_boot_records) + 4);
    assert_eq!(core::mem::offset_of!(BootInfo, kernel_stack_size),
        core::mem::offset_of!(BootInfo, kernel_stack_base) + 8);
    assert_eq!(core::mem::size_of::<BootInfo>(),
        core::mem::offset_of!(BootInfo, kernel_stack_size) + 8);
    let mut info = BootInfo::defaults();
    assert!(info.is_valid());
    info.version = 10;
    assert!(!info.is_valid());
    assert_eq!(info.kernel_stack_base, 0);
    assert_eq!(info.kernel_stack_size, 0);
}

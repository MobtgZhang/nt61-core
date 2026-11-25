use core::mem::{offset_of, size_of};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct ListEntry {
    pub next: u64,
    pub previous: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct MemoryDescriptor {
    pub links: ListEntry,
    pub kind: u32,
    pub reserved: u32,
    pub base_page: u64,
    pub page_count: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct NlsTables {
    pub ansi: u64,
    pub oem: u64,
    pub unicode_case: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct ConfigurationNode {
    pub parent: u64,
    pub child: u64,
    pub sibling: u64,
    pub class: u32,
    pub kind: u32,
    pub flags: u32,
    pub version: u16,
    pub revision: u16,
    pub key: u32,
    pub affinity: u32,
    pub data_length: u32,
    pub identifier_length: u32,
    pub identifier: u64,
    pub data: u64,
}

#[repr(C)]
pub(crate) struct AcpiConfiguration {
    pub resource_list: [u8; 0x1c],
    pub root_table: [u8; 8],
    pub range_count: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FirmwareInformation {
    pub is_efi: u32,
    pub reserved: u32,
    pub version: u32,
    pub padding: u32,
    pub runtime_services: u64,
    pub virtual_map_status: u32,
    pub missed_mappings: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct LoaderBlock {
    pub os_major: u32,
    pub os_minor: u32,
    pub size: u32,
    pub reserved: u32,
    pub modules: ListEntry,
    pub memory: ListEntry,
    pub drivers: ListEntry,
    pub kernel_stack: u64,
    pub prcb: u64,
    pub process: u64,
    pub thread: u64,
    pub registry_length: u32,
    pub registry_padding: u32,
    pub registry_base: u64,
    pub configuration_root: u64,
    pub arc_boot_device: u64,
    pub arc_hal_device: u64,
    pub nt_boot_path: u64,
    pub nt_hal_path: u64,
    pub load_options: u64,
    pub nls: u64,
    pub arc_disks: u64,
    pub oem_font: u64,
    pub extension: u64,
    pub architecture: [u64; 2],
    pub firmware: FirmwareInformation,
}

#[repr(C)]
pub(crate) struct LoaderExtension {
    pub size: u32,
    pub profile: [u16; 8],
    pub reserved_profile: u32,
    pub em_inf: u64,
    pub em_inf_size: u32,
    pub reserved_em_inf: u32,
    pub triage_dump: u64,
    pub loader_pages_spanned: u64,
    pub headless: u64,
    pub smbios: u64,
    pub driver_database: u64,
    pub driver_database_size: u32,
    pub reserved_driver_database: u32,
    pub network: u64,
    pub firmware_descriptors: ListEntry,
    pub acpi_table: u64,
    pub acpi_table_size: u32,
    pub boot_flags: u32,
    pub performance: u64,
    pub persistent_data: ListEntry,
    pub memory_diagnostic: u64,
    pub boot_identifier: [u8; 16],
    pub resume_pages: u64,
    pub dump_header: u64,
    pub boot_graphics: u64,
    pub numa_locality: u64,
    pub numa_group_assignment: u64,
    pub attached_hives: ListEntry,
    pub cache_requirement_count: u32,
    pub reserved_cache_requirements: u32,
    pub cache_requirements: u64,
    pub boot_entropy: [u64; 9],
    pub counter_frequency: u64,
}

const _: () = {
    assert!(size_of::<LoaderBlock>() == 0xf0);
    assert!(offset_of!(LoaderBlock, modules) == 0x10);
    assert!(offset_of!(LoaderBlock, memory) == 0x20);
    assert!(offset_of!(LoaderBlock, drivers) == 0x30);
    assert!(offset_of!(LoaderBlock, kernel_stack) == 0x40);
    assert!(offset_of!(LoaderBlock, prcb) == 0x48);
    assert!(offset_of!(LoaderBlock, registry_length) == 0x60);
    assert!(offset_of!(LoaderBlock, registry_base) == 0x68);
    assert!(offset_of!(LoaderBlock, configuration_root) == 0x70);
    assert!(offset_of!(LoaderBlock, nls) == 0xa0);
    assert!(offset_of!(LoaderBlock, arc_disks) == 0xa8);
    assert!(offset_of!(LoaderBlock, extension) == 0xb8);
    assert!(offset_of!(LoaderBlock, firmware) == 0xd0);
    assert!(size_of::<FirmwareInformation>() == 0x20);
    assert!(size_of::<NlsTables>() == 0x18);
    assert!(offset_of!(ConfigurationNode, identifier) == 0x38);
    assert!(offset_of!(ConfigurationNode, data) == 0x40);
    assert!(offset_of!(AcpiConfiguration, root_table) == 0x1c);
    assert!(offset_of!(AcpiConfiguration, range_count) == 0x24);
    assert!(size_of::<AcpiConfiguration>() == 0x2c);
    assert!(size_of::<MemoryDescriptor>() == 0x28);
    assert!(offset_of!(MemoryDescriptor, base_page) == 0x18);
    assert!(size_of::<LoaderExtension>() == 0x148);
    assert!(offset_of!(LoaderExtension, firmware_descriptors) == 0x60);
    assert!(offset_of!(LoaderExtension, acpi_table) == 0x70);
    assert!(offset_of!(LoaderExtension, attached_hives) == 0xd8);
    assert!(offset_of!(LoaderExtension, counter_frequency) == 0x140);
};

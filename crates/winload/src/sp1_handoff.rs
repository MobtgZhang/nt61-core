use crate::loader::{self, MappedBootImage};
use crate::sp1_layout::{AcpiConfiguration, ConfigurationNode, FirmwareInformation, ListEntry, LoaderBlock, LoaderExtension, MemoryDescriptor, NlsTables};
use alloc::vec::Vec;
use core::{arch::asm, mem::{offset_of, size_of}, ptr, slice};
use uefi::boot::{allocate_pages, AllocateType, MemoryType};

type BootError = &'static str;
type ProjectKernel = (u64, u64, u64, u64, u64);

const PAGE: usize = 4096;
const HIGH_BASE: u64 = 0xffff_f800_0000_0000;
const PAGE_ADDRESS: u64 = 0x000f_ffff_ffff_f000;
const PAGE_FLAGS: u64 = 3;
const SELF_MAP_SLOT: usize = 0x1ed;
const DESCRIPTOR_CAPACITY: usize = 1024;

fn reserve(bytes: usize) -> Result<u64, BootError> {
    let pages = bytes.div_ceil(PAGE).max(1);
    let address = allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .map_err(|_| "cannot allocate SP1 loader pages")?.as_ptr();
    unsafe { ptr::write_bytes(address, 0, pages * PAGE); }
    Ok(address as u64)
}

fn high(address: u64) -> Result<u64, BootError> {
    (address < (1 << 39)).then(|| HIGH_BASE + address).ok_or("SP1 payload outside the kernel virtual window")
}

struct Paging {
    root: u64,
}

impl Paging {
    fn new() -> Result<Self, BootError> {
        let (cr3, cr4): (u64, u64);
        unsafe { asm!("mov {}, cr3", "mov {}, cr4", out(reg) cr3, out(reg) cr4, options(nostack, nomem)); }
        if cr4 & (1 << 12) != 0 {
            return Err("SP1 requires four-level paging");
        }
        let active = (cr3 & PAGE_ADDRESS) as *const u64;
        let root = reserve(PAGE)?;
        let table = root as *mut u64;
        unsafe {
            ptr::copy_nonoverlapping(active, table, PAGE / size_of::<u64>());
            if *table.add((HIGH_BASE >> 39 & 0x1ff) as usize) != 0 {
                return Err("SP1 kernel virtual window is already occupied");
            }
            *table.add(SELF_MAP_SLOT) = root | PAGE_FLAGS;
        }
        Ok(Self { root })
    }

    fn map(&self, address: u64, bytes: usize) -> Result<u64, BootError> {
        if address & (PAGE as u64 - 1) != 0 || bytes == 0 {
            return Err("unaligned SP1 mapping");
        }
        let virtual_base = high(address)?;
        for page in 0..bytes.div_ceil(PAGE) {
            let physical = address + (page * PAGE) as u64;
            let virtual_address = high(physical)?;
            let mut table = self.root;
            for shift in [39, 30, 21] {
                let slot = (table as *mut u64).wrapping_add((virtual_address >> shift & 0x1ff) as usize);
                let mut descriptor = unsafe { *slot };
                if descriptor == 0 {
                    descriptor = reserve(PAGE)? | PAGE_FLAGS;
                    unsafe { *slot = descriptor; }
                }
                table = descriptor & PAGE_ADDRESS;
            }
            let slot = (table as *mut u64).wrapping_add((virtual_address >> 12 & 0x1ff) as usize);
            unsafe { *slot = physical | PAGE_FLAGS; }
        }
        Ok(virtual_base)
    }

    fn bytes(&self, data: &[u8]) -> Result<u64, BootError> {
        if data.is_empty() {
            return Err("empty SP1 boot payload");
        }
        let physical = reserve(data.len())?;
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), physical as *mut u8, data.len()); }
        self.map(physical, data.len())
    }

    fn strings(&self, text: &str) -> Result<(u64, u16), BootError> {
        let wide: Vec<u16> = text.encode_utf16().chain(core::iter::once(0)).collect();
        let len = u16::try_from(wide.len() * 2).map_err(|_| "SP1 module name too long")?;
        let bytes = unsafe { slice::from_raw_parts(wide.as_ptr().cast::<u8>(), usize::from(len)) };
        Ok((self.bytes(bytes)?, len))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UnicodeString {
    length: u16,
    capacity: u16,
    padding: u32,
    buffer: u64,
}

impl UnicodeString {
    fn new(paging: &Paging, text: &str) -> Result<Self, BootError> {
        let (buffer, capacity) = paging.strings(text)?;
        Ok(Self { length: capacity - 2, capacity, padding: 0, buffer })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Module {
    links: ListEntry,
    exception_table: u64,
    exception_size: u32,
    padding: u32,
    gp_value: u64,
    debug_info: u64,
    base: u64,
    entry: u64,
    image_size: u32,
    name_padding: u32,
    full_name: UnicodeString,
    base_name: UnicodeString,
    flags: u32,
    load_count: u16,
    signature: u16,
    section_pointer: u64,
    checksum: u32,
    coverage_size: u32,
    coverage: u64,
    imports: u64,
    spare: u64,
    unrounded_size: u32,
    timestamp: u32,
}

fn self_list(address: u64) -> ListEntry {
    ListEntry { next: address, previous: address }
}

fn list_item(base: u64, stride: usize, count: usize, index: usize, head: u64) -> ListEntry {
    ListEntry {
        previous: if index == 0 { head } else { base + ((index - 1) * stride) as u64 },
        next: if index + 1 == count { head } else { base + ((index + 1) * stride) as u64 },
    }
}

fn list_head(base: u64, stride: usize, count: usize, head: u64) -> ListEntry {
    if count == 0 {
        self_list(head)
    } else {
        ListEntry { next: base, previous: base + ((count - 1) * stride) as u64 }
    }
}

#[repr(C, packed)]
struct DescriptorTable {
    limit: u16,
    base: u64,
}

fn processor_tables(paging: &Paging, kernel_stack: u64) -> Result<DescriptorTable, BootError> {
    let physical = reserve(PAGE)?;
    let virtual_base = paging.map(physical, PAGE)?;
    let tss = virtual_base + 0x100;
    let tss_descriptor = (0x67_u64)
        | ((tss & 0x00ff_ffff) << 16)
        | (0x89 << 40)
        | ((tss & 0xff00_0000) << 32);
    let gdt = [
        0, 0, 0x00af_9b00_0000_ffff, 0x00cf_9300_0000_ffff,
        0x00cf_fb00_0000_ffff, 0x00cf_f300_0000_ffff, 0x00af_fb00_0000_ffff,
        0x00af_9b00_0000_ffff, tss_descriptor, tss >> 32, 0x00cf_f300_0000_ffff,
    ];
    unsafe {
        ptr::copy_nonoverlapping(gdt.as_ptr(), physical as *mut u64, gdt.len());
        ptr::write_unaligned((physical + 0x104) as *mut u64, kernel_stack);
        ptr::write_unaligned((physical + 0x166) as *mut u16, 0x68);
    }
    Ok(DescriptorTable { limit: (size_of::<[u64; 11]>() - 1) as u16, base: virtual_base })
}

fn required_file(paging: &Paging, path: &str, error: BootError) -> Result<(u64, u32), BootError> {
    let bytes = crate::open_system_file_data(path).ok_or(error)?;
    let size = u32::try_from(bytes.len()).map_err(|_| error)?;
    Ok((paging.bytes(&bytes)?, size))
}

fn acpi_root_table() -> Result<u64, BootError> {
    let address = crate::firmware_acpi_rsdp().ok_or("SP1 requires the EFI ACPI configuration table")?;
    let legacy = unsafe { slice::from_raw_parts(address as *const u8, 20) };
    let checksum_valid = |data: &[u8]| data.iter().copied().fold(0u8, u8::wrapping_add) == 0;
    if &legacy[..8] != b"RSD PTR " || !checksum_valid(legacy) {
        return Err("SP1 EFI ACPI RSDP has an invalid signature or checksum");
    }
    let [first, second, third, fourth] = [legacy[16], legacy[17], legacy[18], legacy[19]];
    let mut table = u64::from(u32::from_le_bytes([first, second, third, fourth]));
    if legacy[15] >= 2 {
        let extended = unsafe { slice::from_raw_parts(address as *const u8, 36) };
        let mut length = [0; 4];
        length.copy_from_slice(&extended[20..24]);
        let length = u32::from_le_bytes(length) as usize;
        if !(36..=PAGE).contains(&length)
            || !checksum_valid(unsafe { slice::from_raw_parts(address as *const u8, length) })
        {
            return Err("SP1 EFI ACPI RSDP has an invalid extended checksum or length");
        }
        let mut xsdt = [0; 8];
        xsdt.copy_from_slice(&extended[24..32]);
        let xsdt = u64::from_le_bytes(xsdt);
        if xsdt != 0 {
            table = xsdt;
        }
    }
    if table == 0 {
        return Err("SP1 EFI ACPI RSDP has no root table");
    }
    Ok(table)
}

fn configuration_tree(paging: &Paging) -> Result<u64, BootError> {
    const ACPI_IDENTIFIER: &[u8] = b"ACPI BIOS\0";
    let root_table = acpi_root_table()?;
    let physical = reserve(PAGE)?;
    let base = paging.map(physical, PAGE)?;
    let node_size = size_of::<ConfigurationNode>();
    let child = base + node_size as u64;
    let data_offset = node_size * 2;
    let identifier_offset = data_offset + size_of::<AcpiConfiguration>();
    let root = ConfigurationNode { child, ..ConfigurationNode::default() };
    let acpi = ConfigurationNode {
        parent: base,
        class: 3,
        kind: 12,
        data_length: size_of::<AcpiConfiguration>() as u32,
        identifier_length: ACPI_IDENTIFIER.len() as u32,
        identifier: base + identifier_offset as u64,
        data: base + data_offset as u64,
        ..ConfigurationNode::default()
    };
    let configuration = AcpiConfiguration {
        resource_list: [0; 0x1c],
        root_table: root_table.to_le_bytes(),
        range_count: 0,
        reserved: 0,
    };
    unsafe {
        let nodes = physical as *mut ConfigurationNode;
        nodes.write(root);
        nodes.add(1).write(acpi);
        ((physical + data_offset as u64) as *mut AcpiConfiguration).write(configuration);
        ptr::copy_nonoverlapping(ACPI_IDENTIFIER.as_ptr(), (physical + identifier_offset as u64) as *mut u8, ACPI_IDENTIFIER.len());
    }
    uefi::println!("[WINLOAD] SP1 ACPI configuration {:#x}; root table {:#x}", child, root_table);
    Ok(base)
}

fn memory_kind(efi: u32) -> u32 {
    match efi {
        1 | 2 => 7,
        3 | 4 => 5,
        7 => 2,
        8 => 3,
        9 | 10 => 6,
        _ => 6,
    }
}

fn finish(
    block: &mut LoaderBlock, block_virtual: u64, descriptors: u64, descriptor_virtual: u64,
) -> Result<(), BootError> {
    let mut map = [crate::MemoryMapEntry::new(0, 0, 0); DESCRIPTOR_CAPACITY];
    let virtual_head = block_virtual + offset_of!(LoaderBlock, memory) as u64;
    let system_table = uefi::table::system_table_raw().ok_or("missing SP1 EFI system table")?;
    let services = unsafe { &*system_table.as_ref().boot_services };
    let handle = uefi::boot::image_handle().as_ptr();
    for _ in 0..2 {
        let buffer = unsafe { &mut (*ptr::addr_of_mut!(crate::MEM_MAP_BUF)).0 };
        let (count, key, _) = crate::raw_get_memory_map_with_key(&mut map, buffer);
        if count == 0 || count == DESCRIPTOR_CAPACITY {
            return Err("cannot describe SP1 EFI memory map");
        }
        let entries = &mut map[..count];
        entries.sort_unstable_by_key(|entry| entry.base);
        block.memory = list_head(descriptor_virtual, size_of::<MemoryDescriptor>(), count, virtual_head);
        for (index, entry) in entries.iter().enumerate() {
            let record = MemoryDescriptor {
                links: list_item(descriptor_virtual, size_of::<MemoryDescriptor>(), count, index, virtual_head),
                kind: memory_kind(entry.memory_type),
                reserved: 0,
                base_page: entry.base / PAGE as u64,
                page_count: entry.length / PAGE as u64,
            };
            unsafe { (descriptors as *mut MemoryDescriptor).add(index).write(record); }
        }
        let status = unsafe { (services.exit_boot_services)(handle, key) };
        if status.is_success() {
            return Ok(());
        }
    }
    Err("SP1 ExitBootServices failed")
}

pub(crate) fn boot(images: &mut [MappedBootImage<'_>]) -> Result<ProjectKernel, BootError> {
    let paging = Paging::new()?;
    for image in images.iter_mut() {
        image.load_base = paging.map(image.load_base, image.header.image_size)?;
    }
    loader::link_boot_images(images).map_err(|(_, error)| error)?;
    let kernel_entry = images[0].load_base + u64::from(images[0].header.entry_rva);

    let (hive, hive_size) = required_file(&paging, "\\Windows\\System32\\config\\SYSTEM", "SP1 SYSTEM hive is missing")?;
    let (ansi, _) = required_file(&paging, "\\Windows\\System32\\c_1252.nls", "SP1 ANSI code page is missing")?;
    let (oem, _) = required_file(&paging, "\\Windows\\System32\\c_437.nls", "SP1 OEM code page is missing")?;
    let (unicode_case, _) = required_file(&paging, "\\Windows\\System32\\l_intl.nls", "SP1 Unicode case table is missing")?;
    let nls = reserve(PAGE)?;
    let nls_virtual = paging.map(nls, PAGE)?;
    unsafe { (nls as *mut NlsTables).write(NlsTables { ansi, oem, unicode_case }); }

    let block_physical = reserve(PAGE)?;
    let block_virtual = paging.map(block_physical, PAGE)?;
    let extension_offset = 0x100;
    let extension_virtual = block_virtual + extension_offset;
    let extension = unsafe { &mut *((block_physical + extension_offset) as *mut LoaderExtension) };
    extension.size = size_of::<LoaderExtension>() as u32;
    extension.firmware_descriptors = self_list(extension_virtual + offset_of!(LoaderExtension, firmware_descriptors) as u64);
    extension.persistent_data = self_list(extension_virtual + offset_of!(LoaderExtension, persistent_data) as u64);
    extension.attached_hives = self_list(extension_virtual + offset_of!(LoaderExtension, attached_hives) as u64);
    extension.boot_flags = 4;

    let module_bytes = size_of::<Module>() * images.len();
    let modules = reserve(module_bytes)?;
    let modules_virtual = paging.map(modules, module_bytes)?;
    let module_head = block_virtual + offset_of!(LoaderBlock, modules) as u64;
    for (index, image) in images.iter().enumerate() {
        let module = Module {
            links: list_item(modules_virtual, size_of::<Module>(), images.len(), index, module_head),
            base: image.load_base,
            entry: image.load_base + u64::from(image.header.entry_rva),
            image_size: image.header.image_size as u32,
            unrounded_size: image.header.image_size as u32,
            full_name: UnicodeString::new(&paging, &alloc::format!("\\SystemRoot\\system32\\{}", image.name))?,
            base_name: UnicodeString::new(&paging, image.name)?,
            load_count: 1,
            ..Module::default()
        };
        unsafe { (modules as *mut Module).add(index).write(module); }
    }

    let stack_bytes = 128 * 1024;
    let stack = paging.map(reserve(stack_bytes)?, stack_bytes)? + stack_bytes as u64;
    let gdt = processor_tables(&paging, stack)?;
    let descriptor_size = DESCRIPTOR_CAPACITY * size_of::<MemoryDescriptor>();
    let descriptors = reserve(descriptor_size)?;
    let descriptor_virtual = paging.map(descriptors, descriptor_size)?;
    let arc_disks_offset = 0x250;
    let arc_disks = block_virtual + arc_disks_offset;
    unsafe { ((block_physical + arc_disks_offset) as *mut ListEntry).write(self_list(arc_disks)); }
    let system_table = uefi::table::system_table_raw().ok_or("missing SP1 EFI system table")?;
    let system_table = unsafe { system_table.as_ref() };
    let block = unsafe { &mut *(block_physical as *mut LoaderBlock) };
    *block = LoaderBlock {
        os_major: 6,
        os_minor: 1,
        size: size_of::<LoaderBlock>() as u32,
        modules: list_head(modules_virtual, size_of::<Module>(), images.len(), module_head),
        drivers: self_list(block_virtual + offset_of!(LoaderBlock, drivers) as u64),
        kernel_stack: stack,
        registry_base: hive,
        registry_length: hive_size,
        configuration_root: configuration_tree(&paging)?,
        arc_boot_device: paging.bytes(b"multi(0)disk(0)rdisk(0)partition(2)\0")?,
        arc_hal_device: paging.bytes(b"multi(0)disk(0)rdisk(0)partition(1)\0")?,
        nt_boot_path: paging.bytes(b"\\Windows\\\0")?,
        nt_hal_path: paging.bytes(b"\\\0")?,
        load_options: paging.bytes(b" NOEXECUTE=OPTIN\0")?,
        arc_disks,
        nls: nls_virtual,
        extension: extension_virtual,
        firmware: FirmwareInformation {
            is_efi: 1,
            version: system_table.header.revision.0,
            virtual_map_status: u32::MAX,
            ..FirmwareInformation::default()
        },
        ..LoaderBlock::default()
    };
    uefi::println!("[WINLOAD] SP1 native entry {:#x}; loader block {:#x}; CR3 {:#x}", kernel_entry, block_virtual, paging.root);
    finish(block, block_virtual, descriptors, descriptor_virtual)?;
    unsafe { enter(paging.root, &gdt, stack, block_virtual, kernel_entry); }
}

unsafe fn enter(root: u64, gdt: &DescriptorTable, stack: u64, block: u64, entry: u64) -> ! {
    unsafe {
        asm!(
            "cli", "cld",
            "mov cr3, r9",
            "lgdt [r8]",
            "push 0x38", "lea r11, [rip + 2f]", "push r11", "retfq", "2:",
            "mov ax, 0x18", "mov ss, ax", "mov ds, ax", "mov es, ax",
            "mov ax, 0x40", "ltr ax",
            "mov rsp, rdx", "and rsp, -16", "sub rsp, 32", "xor ebp, ebp",
            "call r10", "ud2",
            in("r9") root,
            in("r8") gdt,
            in("rdx") stack,
            in("rcx") block,
            in("r10") entry,
            options(noreturn),
        );
    }
}

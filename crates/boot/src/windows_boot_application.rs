use alloc::string::String;
use alloc::{vec, vec::Vec};
use core::ptr::NonNull;
use nt61_bootfs::SectorReader;
use uefi::boot::{self, MemoryType};
use uefi::mem::memory_map::{MemoryMap, MemoryMapOwned};
use uefi::proto::device_path::{media::{HardDrive, PartitionSignature}, DevicePath};
use uefi::proto::media::block::BlockIO;

use crate::windows_boot_paging::{PageTableLayout, ADDRESS_MASK, TABLE_ENTRIES};
use crate::windows_boot_return::{
    data_pages, table_pages, valid_record_header, PhysicalPages, PAGE_SIZE,
};

const HEADER_SIZE: usize = 0x3c;
const MEMORY_HEADER_SIZE: usize = 0x14;
const MEMORY_DESCRIPTOR_SIZE: usize = 0x30;
const IMAGE_DESCRIPTOR: usize = HEADER_SIZE + MEMORY_HEADER_SIZE;
const PAGING_DESCRIPTOR: usize = IMAGE_DESCRIPTOR + MEMORY_DESCRIPTOR_SIZE;
const LOW_MEMORY_DESCRIPTOR: usize = PAGING_DESCRIPTOR + MEMORY_DESCRIPTOR_SIZE;
const RESERVED_MEMORY: u32 = 0xf000_0003;
const APPLICATION_HEADER_SIZE: usize = 0x2c;
const OPTION_HEADER_SIZE: usize = 0x18;
const FIRMWARE_SIZE: usize = 0x38;
const FIRMWARE_PAGE_TABLE_OFFSET: usize = 0x1c;
const RETURN_SIZE: usize = 0x1c;
const APPLICATION_DEVICE: u32 = 0x1100_0001;
const OS_DEVICE: u32 = 0x2100_0001;
const RAMDISK_PARENT_OFFSET: usize = 0x44;
const GPT_DEVICE_SIZE: usize = 0x48;

pub struct Application {
    pub guid: [u8; 16],
    pub options: Vec<(u32, Vec<u8>)>,
    pub boot_device: Vec<u8>,
    pub ramdisk_options: Option<Vec<(u32, Vec<u8>)>>,
    pub resume: bool,
}

pub struct Ramdisk {
    allocation: NonNull<u8>,
    pages: usize,
    device: Vec<u8>,
}

impl Drop for Ramdisk {
    fn drop(&mut self) {
        let _ = unsafe { boot::free_pages(self.allocation, self.pages) };
    }
}

#[derive(Default)]
pub struct PersistentData(Vec<PhysicalPages>);

fn range_type(map: &MemoryMapOwned, range: PhysicalPages) -> Option<MemoryType> {
    let end = range.base.checked_add((range.count * PAGE_SIZE) as u64)?;
    map.entries()
        .find(|descriptor| {
            descriptor.phys_start <= range.base
                && descriptor
                    .phys_start
                    .checked_add(descriptor.page_count.saturating_mul(PAGE_SIZE as u64))
                    .is_some_and(|limit| limit >= end)
        })
        .map(|descriptor| descriptor.ty)
}

fn validate_returned_pages(
    range: PhysicalPages,
    before: &MemoryMapOwned,
    after: &MemoryMapOwned,
) -> Result<(), &'static str> {
    let previous_type = range_type(before, range);
    let current_type = range_type(after, range);
    crate::debug_println!(
        "[WINDOWS-BOOT] Persistent data at {:#x}, pages={}, UEFI type {:?} -> {:?}",
        range.base,
        range.count,
        previous_type,
        current_type
    );
    if previous_type != Some(MemoryType::CONVENTIONAL)
        || current_type != Some(MemoryType::LOADER_DATA)
    {
        return Err("Windows persistent data does not lie in newly allocated loader data pages");
    }
    Ok(())
}

impl PersistentData {
    pub fn capture(arguments: &[u8], before: &MemoryMapOwned) -> Result<Self, &'static str> {
        let Some(range) = table_pages(arguments)? else {
            return Ok(Self::default());
        };
        let after = boot::memory_map(MemoryType::BOOT_SERVICES_DATA)
            .map_err(|_| "cannot get the firmware memory map after the diagnostic")?;
        validate_returned_pages(range, before, &after)?;
        let bytes = unsafe {
            core::slice::from_raw_parts(range.base as *const u8, range.count * PAGE_SIZE)
        };
        let mut reserved = Vec::new();
        for range in data_pages(range, bytes)? {
            validate_returned_pages(range.pages, before, &after)?;
            let header =
                unsafe { core::slice::from_raw_parts(range.pages.base as *const u8, 0x2c) };
            if !valid_record_header(header, range.length) {
                return Err("invalid Windows persistent data record header");
            }
            reserved.push(range.pages);
        }
        crate::debug_println!(
            "[WINDOWS-BOOT] Preserved {} returned data ranges",
            reserved.len()
        );
        Ok(Self(reserved))
    }
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn describe_memory(
    block: &mut [u8],
    descriptor: usize,
    base: u64,
    pages: u64,
    attributes: u32,
    kind: u32,
) {
    put_u64(block, descriptor + 0x10, base >> 12);
    put_u64(block, descriptor + 0x20, pages);
    put_u32(block, descriptor + 0x28, attributes);
    put_u32(block, descriptor + 0x2c, kind);
}

fn option(
    block: &mut Vec<u8>,
    start: usize,
    previous: &mut Option<usize>,
    kind: u32,
    data: &[u8],
    flags: u32,
) -> usize {
    let offset = block.len();
    let end = offset + OPTION_HEADER_SIZE + data.len();
    block.resize(end, 0);
    if let Some(last) = previous.replace(offset) {
        put_u32(block, last + 16, (offset - start) as u32);
    }
    put_u32(block, offset, kind);
    put_u32(block, offset + 4, OPTION_HEADER_SIZE as u32);
    put_u32(block, offset + 8, data.len() as u32);
    put_u32(block, offset + 20, flags);
    block[offset + OPTION_HEADER_SIZE..end].copy_from_slice(data);
    offset
}

fn append_associated_options(block: &mut Vec<u8>, parent: usize, options: &[(u32, Vec<u8>)]) {
    let start = block.len();
    put_u32(block, parent + 12, (start - parent) as u32);
    let mut previous = None;
    for (kind, data) in options {
        option(block, start, &mut previous, *kind, data, 0);
    }
}

fn qualified_partition(device: &[u8]) -> Option<&[u8]> {
    let partition = device.get(16..)?;
    (partition.len() == GPT_DEVICE_SIZE
        && partition[..4] == 6u32.to_le_bytes()
        && partition[8..12] == (GPT_DEVICE_SIZE as u32).to_le_bytes())
    .then_some(partition)
}

pub(crate) fn is_boot_partition(descriptor: &[u8]) -> bool {
    descriptor.len() == GPT_DEVICE_SIZE
        && descriptor[..4] == 5u32.to_le_bytes()
        && descriptor[8..12] == (GPT_DEVICE_SIZE as u32).to_le_bytes()
        && descriptor[4..8].iter().chain(&descriptor[12..]).all(|byte| *byte == 0)
}

fn read_ramdisk_file(descriptor: &[u8], path: &str) -> Result<Vec<u8>, &'static str> {
    if is_boot_partition(descriptor) {
        return crate::read_loaded_volume_file(path);
    }
    read_partition_file(descriptor, path).map_err(ApplicationImageError::message)
}

fn device_path(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut units = bytes
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]));
    let path: Vec<_> = units.by_ref().take_while(|&unit| unit != 0).collect();
    if path.first() != Some(&u16::from(b'\\')) || path.len() >= bytes.len() / 2 {
        return None;
    }
    String::from_utf16(&path).ok()
}

pub enum ApplicationImageError {
    FileUnavailable,
    Other(&'static str),
}

impl ApplicationImageError {
    pub fn message(self) -> &'static str {
        match self {
            Self::FileUnavailable => "file missing or unreadable on its BCD device",
            Self::Other(message) => message,
        }
    }
}

impl Application {
    pub fn resolve_boot_partition(&mut self, handle: uefi::Handle) {
        if !is_boot_partition(&self.boot_device) {
            return;
        }
        let Some(descriptor) = firmware_boot_partition(handle) else {
            return;
        };
        self.boot_device.copy_from_slice(&descriptor);
        crate::debug_println!("[WINDOWS-BOOT] Resolved boot device to the firmware GPT partition");
    }

    pub fn has_ramdisk(&self) -> bool {
        self.ramdisk_options.is_some()
    }

    fn device_option(&self, kind: u32) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|(id, _)| *id == kind)
            .map(|(_, data)| data.as_slice())
    }

    pub fn read_qualified_image(&self, path: &str) -> Result<Option<Vec<u8>>, ApplicationImageError> {
        let device = self
            .device_option(APPLICATION_DEVICE)
            .ok_or(ApplicationImageError::Other("missing Windows boot application device"))?;
        if device.get(16..20) != Some(&6u32.to_le_bytes()) {
            return Ok(None);
        }
        let partition =
            qualified_partition(device).ok_or(ApplicationImageError::Other("unsupported qualified Windows boot device"))?;
        read_partition_file(partition, path).map(Some)
    }

    pub fn read_recovery_image(
        &self,
        path: &str,
    ) -> Result<(Vec<u8>, Option<Ramdisk>), &'static str> {
        let os_device = self
            .device_option(OS_DEVICE)
            .ok_or("missing Windows recovery OS device")?;
        if qualified_partition(os_device).is_some() {
            return self
                .read_qualified_image(path).map_err(ApplicationImageError::message)?
                .map(|image| (image, None))
                .ok_or("Windows recovery loader requires a qualified GPT partition");
        }

        let application_device = self
            .device_option(APPLICATION_DEVICE)
            .ok_or("missing Windows recovery device")?;
        if os_device != application_device {
            return Err("Windows recovery requires matching application and OS ramdisks");
        }
        let options = self
            .ramdisk_options
            .as_deref()
            .ok_or("missing Windows recovery ramdisk options")?;
        let sdi_partition = options
            .iter()
            .find(|(kind, _)| *kind == 0x3100_0003)
            .and_then(|(_, device)| {
                qualified_partition(device).or_else(|| device.get(16..).filter(|part| is_boot_partition(part)))
            })
            .ok_or("unsupported Windows recovery SDI device")?;
        let sdi_path = options
            .iter()
            .find(|(kind, _)| *kind == 0x3200_0004)
            .and_then(|(_, path)| device_path(path))
            .ok_or("invalid Windows recovery SDI path")?;

        let fields = os_device
            .get(16..RAMDISK_PARENT_OFFSET)
            .ok_or("truncated Windows recovery device")?;
        let field =
            |offset: usize| u32::from_le_bytes(fields[offset..offset + 4].try_into().unwrap());
        if field(0) != 0
            || field(4) != 1
            || field(8) as usize != os_device.len() - 16
            || field(16) != 3
            || field(40) != 1
            || field(44) as usize != os_device.len() - 56
            || field(48) != 5
        {
            return Err("unsupported Windows recovery RAM disk descriptor");
        }
        let wim_partition = os_device
            .get(RAMDISK_PARENT_OFFSET..RAMDISK_PARENT_OFFSET + GPT_DEVICE_SIZE)
            .filter(|part| {
                (part[..4] == 6u32.to_le_bytes()
                    && part[8..12] == (GPT_DEVICE_SIZE as u32).to_le_bytes())
                    || is_boot_partition(part)
            })
            .ok_or("unsupported Windows recovery WIM device")?;
        let wim_path = os_device
            .get(RAMDISK_PARENT_OFFSET + GPT_DEVICE_SIZE..)
            .and_then(device_path)
            .ok_or("invalid Windows recovery WIM path")?;

        let sdi = read_ramdisk_file(sdi_partition, &sdi_path)?;
        if !sdi.starts_with(b"$SDI0001") || sdi.len() % PAGE_SIZE != 0 {
            return Err("invalid Windows recovery SDI image");
        }
        let wim = read_ramdisk_file(wim_partition, &wim_path)?;
        let image = crate::windows_wim::read_boot_file(&wim, path)
            .ok_or("cannot read the Windows recovery loader from the WIM boot image")?;
        let size = sdi
            .len()
            .checked_add(wim.len())
            .ok_or("Windows recovery RAM disk too large")?;
        let pages = size.div_ceil(PAGE_SIZE);
        let allocation = boot::allocate_pages(
            boot::AllocateType::MaxAddress(u32::MAX.into()),
            MemoryType::LOADER_DATA,
            pages,
        )
        .map_err(|_| "cannot allocate the Windows recovery RAM disk")?;
        let bytes =
            unsafe { core::slice::from_raw_parts_mut(allocation.as_ptr(), pages * PAGE_SIZE) };
        bytes[..sdi.len()].copy_from_slice(&sdi);
        bytes[sdi.len()..size].copy_from_slice(&wim);
        bytes[size..].fill(0);

        let mut device = os_device.to_vec();
        device[..24].fill(0);
        put_u64(&mut device, 0x24, allocation.as_ptr() as u64);
        put_u64(&mut device, 0x2c, size as u64);
        put_u32(&mut device, 0x34, sdi.len() as u32);
        crate::debug_println!(
            "[WINDOWS-BOOT] Windows RAM disk at {:#x}, SDI={:#x}, WIM={:#x}",
            allocation.as_ptr() as u64,
            sdi.len(),
            wim.len()
        );
        Ok((
            image,
            Some(Ramdisk {
                allocation,
                pages,
                device,
            }),
        ))
    }

    pub fn set_option(&mut self, kind: u32, data: &[u8]) {
        match self.options.binary_search_by_key(&kind, |(id, _)| *id) {
            Ok(index) => {
                self.options[index].1.clear();
                self.options[index].1.extend_from_slice(data);
            }
            Err(index) => self.options.insert(index, (kind, data.to_vec())),
        }
    }

    pub fn parameters(
        &self,
        image_base: u64,
        image_size: u32,
        image_handle: u64,
        system_table: u64,
        persistent: &PersistentData,
        ramdisk: Option<&Ramdisk>,
    ) -> (Vec<u8>, usize) {
        let memory = HEADER_SIZE;
        let first_persistent = LOW_MEMORY_DESCRIPTOR + MEMORY_DESCRIPTOR_SIZE;
        let ramdisk_descriptor = first_persistent + persistent.0.len() * MEMORY_DESCRIPTOR_SIZE;
        let application =
            ramdisk_descriptor + usize::from(ramdisk.is_some()) * MEMORY_DESCRIPTOR_SIZE;
        let options = application + APPLICATION_HEADER_SIZE;
        let mut block = vec![0; options];
        block[..8].copy_from_slice(b"BOOT APP");
        put_u32(&mut block, 8, 2);
        put_u32(&mut block, 0x10, 0x8664);
        put_u64(&mut block, 0x18, image_base);
        put_u32(&mut block, 0x20, image_size);
        put_u32(&mut block, 0x24, memory as u32);
        put_u32(&mut block, 0x28, application as u32);

        put_u32(&mut block, memory, 1);
        put_u32(&mut block, memory + 4, MEMORY_HEADER_SIZE as u32);
        put_u32(
            &mut block,
            memory + 8,
            3 + persistent.0.len() as u32 + u32::from(ramdisk.is_some()),
        );
        put_u32(&mut block, memory + 12, MEMORY_DESCRIPTOR_SIZE as u32);
        put_u32(&mut block, memory + 16, 0x10);
        describe_memory(
            &mut block,
            IMAGE_DESCRIPTOR,
            image_base,
            u64::from(image_size).div_ceil(4096),
            0xf,
            0xd000_0002,
        );
        describe_memory(&mut block, LOW_MEMORY_DESCRIPTOR, 0, 1, 0xf, RESERVED_MEMORY);
        for (index, pages) in persistent.0.iter().enumerate() {
            describe_memory(
                &mut block,
                first_persistent + index * MEMORY_DESCRIPTOR_SIZE,
                pages.base,
                pages.count as u64,
                0x1804_000f,
                0xe000_0004,
            );
        }
        if let Some(ramdisk) = ramdisk {
            describe_memory(
                &mut block,
                ramdisk_descriptor,
                ramdisk.allocation.as_ptr() as u64,
                ramdisk.pages as u64,
                0xf,
                0xd000_0008,
            );
        }

        block[application..application + 8].copy_from_slice(b"BTAPENT\0");
        put_u32(
            &mut block,
            application + 8,
            if ramdisk.is_some() {
                0x4000_0006
            } else if self.resume {
                0x0000_0100
            } else {
                0x0800_0004
            },
        );
        block[application + 12..application + 28].copy_from_slice(&self.guid);

        let associated = ramdisk.and(self.ramdisk_options.as_deref());
        let mut previous = None;
        for (kind, data) in &self.options {
            if let Some(associated) =
                associated.filter(|_| matches!(*kind, APPLICATION_DEVICE | OS_DEVICE))
            {
                let offset = option(&mut block, options, &mut previous, *kind, data, 1);
                append_associated_options(&mut block, offset, associated);
            } else {
                option(&mut block, options, &mut previous, *kind, data, 0);
            }
        }
        if let Some((ramdisk, associated)) = ramdisk.zip(associated) {
            for kind in [APPLICATION_DEVICE, OS_DEVICE] {
                let offset = option(&mut block, options, &mut previous, kind, &ramdisk.device, 0);
                append_associated_options(&mut block, offset, associated);
            }
        }

        let device = block.len();
        block.extend_from_slice(&self.boot_device);
        let firmware = block.len();
        let result = firmware + FIRMWARE_SIZE;
        let total = result + RETURN_SIZE;
        block.resize(total, 0);

        put_u32(&mut block, 0xc, total as u32);
        put_u32(&mut block, 0x2c, device as u32);
        put_u32(&mut block, 0x30, firmware as u32);
        put_u32(&mut block, 0x34, result as u32);
        put_u32(&mut block, firmware, 1);
        put_u64(&mut block, firmware + 8, image_handle);
        put_u64(&mut block, firmware + 16, system_table);
        put_u32(&mut block, result, 1);
        (block, result)
    }
}

fn device_protocol<P: uefi::proto::ProtocolPointer + ?Sized>(
    handle: uefi::Handle,
) -> Option<boot::ScopedProtocol<P>> {
    unsafe {
        boot::open_protocol::<P>(
            boot::OpenProtocolParams { handle, agent: boot::image_handle(), controller: None },
            boot::OpenProtocolAttributes::GetProtocol,
        ).ok()
    }
}

fn firmware_boot_partition(handle: uefi::Handle) -> Option<[u8; GPT_DEVICE_SIZE]> {
    let path = device_protocol::<DevicePath>(handle)?;
    let device = path.get()?;
    let partition = <&HardDrive>::try_from(device.node_iter().last()?).ok()?;
    let PartitionSignature::Guid(partition_id) = partition.partition_signature() else {
        return None;
    };
    let partition_id = partition_id.to_bytes();
    let partition_last = partition.partition_start().checked_add(partition.partition_size().checked_sub(1)?)?;
    let parent_len = device.as_bytes().len().checked_sub(42 + 4)?;
    let parent = &device.as_bytes()[..parent_len];
    for disk_handle in boot::find_handles::<BlockIO>().ok()? {
        let Some(disk_path) = device_protocol::<DevicePath>(disk_handle) else { continue };
        let Some(bytes) = disk_path.get().map(DevicePath::as_bytes) else { continue };
        if bytes.get(..bytes.len().saturating_sub(4)) != Some(parent) {
            continue;
        }
        let Some(block) = device_protocol::<BlockIO>(disk_handle) else { continue };
        let Some(block) = block.get() else { continue };
        let media = block.media();
        if media.is_logical_partition() || media.block_size() != 512 || !media.is_media_present() {
            continue;
        }
        let disk = Volume { block, first: 0, last: media.last_block() };
        let Some(header) = disk.read_sectors(1, 1) else { continue };
        let disk_id = &header[56..72];
        if disk_id == [0; 16]
            || find_gpt_partition(&disk, disk_id, &partition_id) != Some((partition.partition_start(), partition_last))
        {
            continue;
        }
        let mut descriptor = [0; GPT_DEVICE_SIZE];
        put_u32(&mut descriptor, 0, 6);
        put_u32(&mut descriptor, 8, GPT_DEVICE_SIZE as u32);
        descriptor[16..32].copy_from_slice(&partition_id);
        descriptor[40..56].copy_from_slice(disk_id);
        return Some(descriptor);
    }
    None
}

fn read_partition_file(partition: &[u8], path: &str) -> Result<Vec<u8>, ApplicationImageError> {
    let partition_id = &partition[16..32];
    let disk_id = &partition[40..56];
    let handles = boot::find_handles::<BlockIO>().map_err(|_| ApplicationImageError::Other("no disk handles"))?;
    for handle in handles {
        let Some(block) = device_protocol::<BlockIO>(handle) else {
            continue;
        };
        let Some(block) = block.get() else { continue };
        let media = block.media();
        if media.is_logical_partition() || media.block_size() != 512 {
            continue;
        }
        let disk = Volume {
            block,
            first: 0,
            last: media.last_block(),
        };
        if let Some((first, last)) = find_gpt_partition(&disk, disk_id, partition_id) {
            crate::debug_println!(
                "[WINDOWS-BOOT] BCD device resolved to GPT LBA {:#x}, file {}",
                first,
                path
            );
            let volume = Volume { block, first, last };
            return nt61_bootfs::ntfs::read_file(&volume, path)
                .or_else(|| nt61_bootfs::read_fat32_file(&volume, 0, path))
                .ok_or(ApplicationImageError::FileUnavailable);
        }
    }
    Err(ApplicationImageError::Other("BCD GPT device not found"))
}

struct Volume<'a> {
    block: &'a BlockIO,
    first: u64,
    last: u64,
}

impl SectorReader for Volume<'_> {
    fn read_sectors(&self, lba: u64, count: u32) -> Option<Vec<u8>> {
        let start = self.first.checked_add(lba)?;
        let end = start.checked_add(u64::from(count).checked_sub(1)?)?;
        if end > self.last {
            return None;
        }
        let media = self.block.media();
        crate::block_read::read_sectors(start, count, media.io_align() as usize, |offset, data| {
            self.block.read_blocks(media.media_id(), offset, data).ok()
        })
    }
}

fn find_gpt_partition(
    disk: &Volume<'_>,
    disk_id: &[u8],
    partition_id: &[u8],
) -> Option<(u64, u64)> {
    let header = disk.read_sectors(1, 1)?;
    if header.get(..8)? != b"EFI PART" || header.get(56..72)? != disk_id {
        return None;
    }
    let table = u64::from_le_bytes(header[72..80].try_into().ok()?);
    let count = u32::from_le_bytes(header[80..84].try_into().ok()?) as usize;
    let size = u32::from_le_bytes(header[84..88].try_into().ok()?) as usize;
    let bytes = count.checked_mul(size)?;
    if !(128..=4096).contains(&size) || bytes > 1024 * 1024 {
        return None;
    }
    let entries = disk.read_sectors(table, bytes.div_ceil(512) as u32)?;
    for entry in entries.get(..bytes)?.chunks_exact(size) {
        if entry[0..16] == [0; 16] || &entry[16..32] != partition_id {
            continue;
        }
        let first = u64::from_le_bytes(entry[32..40].try_into().ok()?);
        let last = u64::from_le_bytes(entry[40..48].try_into().ok()?);
        let first_usable = u64::from_le_bytes(header[40..48].try_into().ok()?);
        let last_usable = u64::from_le_bytes(header[48..56].try_into().ok()?);
        return (first >= first_usable
            && first <= last
            && last <= last_usable
            && last <= disk.last)
            .then_some((first, last));
    }
    None
}

pub unsafe fn start(
    entry: u64,
    parameters: &mut [u8],
    capture_memory_map: bool,
) -> Result<(u32, Option<MemoryMapOwned>), &'static str> {
    let original: u64;
    let cr4: u64;
    unsafe {
        core::arch::asm!("mov {original}, cr3", "mov {cr4}, cr4", original = out(reg) original, cr4 = out(reg) cr4, options(nostack, preserves_flags));
    }
    if cr4 & (1 << 12) != 0 {
        return Err("Windows 7 boot application requires four-level paging");
    }
    let firmware = u32::from_le_bytes(parameters[0x30..0x34].try_into().unwrap()) as usize;
    put_u64(parameters, firmware + FIRMWARE_PAGE_TABLE_OFFSET, original);
    let root = original & ADDRESS_MASK;
    let read_table = |physical| unsafe { (physical as *const [u64; TABLE_ENTRIES]).read_volatile() };
    let layout = PageTableLayout::discover(root, read_table)
        .ok_or("cannot enumerate firmware page tables")?;
    let page_count = layout.len();
    let table = uefi::boot::allocate_pages(
        uefi::boot::AllocateType::AnyPages,
        uefi::boot::MemoryType::LOADER_DATA,
        page_count,
    )
    .map_err(|_| "allocate Windows boot application page table")?;
    let address = table.as_ptr() as u64;
    describe_memory(
        parameters,
        PAGING_DESCRIPTOR,
        address,
        page_count as u64,
        0x2000_000f,
        0xd000_0006,
    );
    let tables = unsafe {
        table.as_ptr().write_bytes(0, page_count * PAGE_SIZE);
        core::slice::from_raw_parts_mut(table.as_ptr().cast::<[u64; TABLE_ENTRIES]>(), page_count)
    };
    if layout.copy_to(address, tables, read_table).is_none() {
        let _ = unsafe { uefi::boot::free_pages(table, page_count) };
        return Err("firmware page tables changed during Windows boot application setup");
    }
    let invoke: unsafe extern "efiapi" fn(*mut u8) -> u32 =
        unsafe { core::mem::transmute(entry as usize) };
    let before = capture_memory_map
        .then(|| boot::memory_map(MemoryType::BOOT_SERVICES_DATA).ok())
        .flatten();
    let status = unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) address, options(nostack, preserves_flags));
        let status = invoke(parameters.as_mut_ptr());
        core::arch::asm!("mov cr3, {}", in(reg) original, options(nostack, preserves_flags));
        let _ = uefi::boot::free_pages(table, page_count);
        status
    };
    Ok((status, before))
}

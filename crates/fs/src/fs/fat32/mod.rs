//! FAT32 File System Driver
//
//! Implements the FAT32 file system used by the EFI System Partition
//! and the small boot volume that holds `bootmgr.efi` and friends.
//
//! # Layout
//! The FAT32 layout is well-documented in Microsoft's
//! "FAT: General Overview of On-Disk Format" spec:
//
//! ```text
//!   Sector 0                     Reserved sectors (boot sector is at 0)
//!   Sector <reserved_sectors>    FAT region (typically <num_fats> copies)
//!   Sector <fat_end>             Data region (cluster 2 is the root dir)
//! ```
//
//! Cluster numbers in the FAT are 28 bits - the high 4 bits of every
//! FAT entry are reserved and must be preserved when writing. The
//! special values we need to recognise are `0x00000000` (free),
//! `0x00000001` (reserved), `0x0FFFFFF8..=0x0FFFFFFF` (end-of-chain)
//! and `0x0FFFFFF7` (bad cluster).
//
//! # Mounting
//! We expose two entry points: `mount` reads the boot sector and
//! registers a `Fat32FileSystem`; `Fat32FileSystem::read_file` reads
//! the bytes of a file by walking the cluster chain. mutations are shared by the system-volume VFS.
use crate::libs::ntdll::{
    status::{STATUS_IO_DEVICE_ERROR, STATUS_UNSUCCESSFUL},
    types::NTSTATUS,
};

extern crate alloc;

use crate::boot_println; // re-exported from rtl/klog.rs
use crate::drivers::storage::ahci;
use crate::drivers::storage::ataport::ata_port_initialize;
use crate::fs::{FileSystem, FileSystemDriver, FileSystemType};
use crate::ke::sync::Spinlock;
use alloc::vec::Vec;
use core::ptr::null_mut;

pub mod extended_api;
/// FAT32 pagefile support module
pub mod pagefile;
pub use extended_api::{
    count_free_clusters, get_volume_label, set_volume_label, verify_boot_sector,
    verify_directory_entries, verify_fat_tables,
};

/// FAT32 Boot Sector (BPB + EBR). Field layout matches the Microsoft
/// `FAT_GENERAL_SPEC` document.
#[repr(C, packed)]
pub struct Fat32BootSector {
    pub jump: [u8; 3],
    pub oem_name: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub root_entries: u16,
    pub total_sectors_16: u16,
    pub media_descriptor: u8,
    pub sectors_per_fat_16: u16,
    pub sectors_per_track: u16,
    pub num_heads: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,
    pub sectors_per_fat_32: u32,
    pub extended_flags: u16,
    pub fs_version: u16,
    pub root_cluster: u32,
    pub fs_info_sector: u16,
    pub backup_boot_sector: u16,
    pub drive_number: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fs_type: [u8; 8],
}

impl Fat32BootSector {
    pub fn is_valid(&self) -> bool {
        self.jump[0] == 0xEB || self.jump[0] == 0xE9
    }
    pub fn sector_size(&self) -> u32 {
        self.bytes_per_sector as u32
    }
    pub fn cluster_size(&self) -> u32 {
        self.sector_size() * self.sectors_per_cluster as u32
    }
    pub fn fat_size_sectors(&self) -> u32 {
        self.sectors_per_fat_32
    }
}

/// FAT32 directory entry - 32 bytes, packed.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FatDirectoryEntry {
    pub name: [u8; 11],
    pub attributes: u8,
    pub reserved: u8,
    pub creation_time_tenth: u8,
    pub creation_time: u16,
    pub creation_date: u16,
    pub last_access_date: u16,
    pub first_cluster_high: u16,
    pub modification_time: u16,
    pub modification_date: u16,
    pub first_cluster_low: u16,
    pub file_size: u32,
}

impl FatDirectoryEntry {
    pub fn is_valid(&self) -> bool {
        self.name[0] != 0x00 && self.name[0] != 0xE5
    }
    pub fn is_directory(&self) -> bool {
        (self.attributes & 0x10) != 0
    }
    pub fn is_long_name(&self) -> bool {
        self.attributes == 0x0F
    }
    pub fn is_volume_id(&self) -> bool {
        (self.attributes & 0x08) != 0
    }
    pub fn first_cluster(&self) -> u32 {
        ((self.first_cluster_high as u32) << 16) | (self.first_cluster_low as u32)
    }
    pub fn file_size(&self) -> u32 {
        self.file_size
    }
}

/// Directory entry attribute bits.
pub const ATTR_READ_ONLY: u8 = 0x01;
pub const ATTR_HIDDEN: u8 = 0x02;
pub const ATTR_SYSTEM: u8 = 0x04;
pub const ATTR_VOLUME_ID: u8 = 0x08;
pub const ATTR_DIRECTORY: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;
pub const ATTR_LONG_NAME: u8 = 0x0F;

/// FAT32 FSINFO sector.
#[repr(C)]
pub struct Fat32FsInfo {
    pub lead_signature: u32,
    pub reserved1: [u8; 480],
    pub structure_signature: u32,
    pub free_clusters: u32,
    pub next_free_cluster: u32,
    pub reserved2: [u8; 12],
    pub trail_signature: u32,
}

impl Fat32FsInfo {
    pub const LEAD_SIG: u32 = 0x41615252;
    pub const STRUCT_SIG: u32 = 0x61417272;
    pub const TRAIL_SIG: u32 = 0xAA550000;
}

/// Per-volume data.
pub struct Fat32Data {
    pub boot_sector: Fat32BootSector,
    pub fat_start_sector: u64,
    pub data_start_sector: u64,
    pub root_cluster: u32,
}

/// FAT32 file system instance.
pub struct Fat32FileSystem {
    pub base: FileSystem,
    pub fat_data: Fat32Data,
}

impl Fat32FileSystem {
    pub const fn new() -> Self {
        Self {
            base: FileSystem {
                driver: null_mut(),
                device: null_mut(),
                volume_name: [0; 64],
                fs_type: FileSystemType::Fat32,
                sector_size: 512,
                cluster_size: 4096,
                total_clusters: 0,
                free_clusters: 0,
            },
            fat_data: Fat32Data {
                boot_sector: Fat32BootSector {
                    jump: [0; 3],
                    oem_name: [0; 8],
                    bytes_per_sector: 0,
                    sectors_per_cluster: 0,
                    reserved_sectors: 0,
                    num_fats: 0,
                    root_entries: 0,
                    total_sectors_16: 0,
                    media_descriptor: 0,
                    sectors_per_fat_16: 0,
                    sectors_per_track: 0,
                    num_heads: 0,
                    hidden_sectors: 0,
                    total_sectors_32: 0,
                    sectors_per_fat_32: 0,
                    extended_flags: 0,
                    fs_version: 0,
                    root_cluster: 2,
                    fs_info_sector: 0,
                    backup_boot_sector: 0,
                    drive_number: 0,
                    boot_signature: 0,
                    volume_id: 0,
                    volume_label: [0; 11],
                    fs_type: [0; 8],
                },
                fat_start_sector: 0,
                data_start_sector: 0,
                root_cluster: 2,
            },
        }
    }
}

impl Default for Fat32FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// FAT32 partition offset (set by mount_fat32_partition in fs/mod.rs)
static FAT32_PARTITION_OFFSET: Spinlock<u64> = Spinlock::new(0);
/// Thread-safe mounted filesystem storage - stores raw pointer for safe mutable access
static mut FAT32_MOUNTED_FS_PTR: *mut Fat32FileSystem = core::ptr::null_mut();

/// Set the FAT32 partition offset (sector number where partition starts)
pub fn set_partition_offset(offset: u64) {
    *FAT32_PARTITION_OFFSET.lock() = offset;
}

/// Get the FAT32 partition offset
pub fn get_partition_offset() -> u64 {
    *FAT32_PARTITION_OFFSET.lock()
}

/// Set the FAT32 filesystem as mounted (atomically)
pub fn set_mounted_fs(fs: &'static mut Fat32FileSystem) {
    unsafe {
        FAT32_MOUNTED_FS_PTR = fs;
    }
}

/// Check if FAT32 is mounted (atomically)
pub fn is_mounted() -> bool {
    !unsafe { FAT32_MOUNTED_FS_PTR.is_null() }
}

/// Get the mounted FAT32 filesystem (for CMD use)
/// Returns a mutable reference to the mounted filesystem
/// Note: This is thread-safe via the original Spinlock pattern
pub fn get_mounted_fs() -> Option<&'static mut Fat32FileSystem> {
    unsafe {
        if FAT32_MOUNTED_FS_PTR.is_null() {
            None
        } else {
            Some(&mut *FAT32_MOUNTED_FS_PTR)
        }
    }
}

/// Clear the mounted filesystem reference
pub fn clear_mounted_fs() {
    unsafe {
        FAT32_MOUNTED_FS_PTR = core::ptr::null_mut();
    }
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// read a sector from the backing device using AHCI.
pub unsafe fn read_sector(device: *mut (), sector: u64, buffer: &mut [u8]) -> Result<(), NTSTATUS> {
    if buffer.len() < 512 {
        return Err(STATUS_UNSUCCESSFUL);
    }

    // Prefer the *active* partition mirror, when one is set by
    // the dispatcher in `fs::mod`. The dispatcher flips the
    // pointer to whichever mirror (ESP or System) it is currently
    // trying to mount. This is what lets the same FAT32 driver
    // mount the Z: (ESP) or the C: (System) drive as FAT32.
    //
    // Fallback: the ESP mirror registered by winload. This is the
    // historical path — kept so legacy call sites that don't go
    // through `mount_partition_detected` still see the ESP.
    if let Some(context) = unsafe { crate::fs::mount_context_from_device(device) } {
        let base = context.base;
        let off = (sector as usize) * 512;
        // Check if the read is within the ESP/System mirror bounds
        // to prevent stack faults when reading beyond the partition.
        let max_size = context.size;
        if off + 512 > max_size {
            crate::boot_println!(
                "[FAT32] read_sector: out of bounds (off=0x{:x} max=0x{:x} sector={})",
                off,
                max_size,
                sector
            );
            return Err(STATUS_UNSUCCESSFUL);
        }
        // Read directly from the mount context so a filesystem instance
        // always observes its own partition image. Volatile byte copies
        // avoid UC-memory issues in the boot-time mirror path.
        unsafe {
            let src = base.add(off);
            for index in 0..512 {
                let value = core::ptr::read_volatile(src.add(index));
                core::ptr::write_volatile(buffer.as_mut_ptr().add(index), value);
            }
        }
        return Ok(());
    }
    let n = crate::fs::esp_ramdisk_read(sector * 512, buffer);
    if n >= 512 {
        return Ok(());
    }

    // x86_64-only fallback to AHCI/ATA drivers.
    {
        let partition_offset = *FAT32_PARTITION_OFFSET.lock();
        let absolute_sector = partition_offset + sector;

        // Try AHCI first (channel 0, port 0)
        if crate::drivers::storage::read_ahci_sector(absolute_sector as u32, buffer) {
            // AHCI read succeeded
            return Ok(());
        }

        // Fallback: Try ATA/ATAPI
        crate::drivers::storage::ataport::ata_port_initialize(0);
        let mut words = [0u16; 256];
        if crate::drivers::storage::ataport::read_sector(0, absolute_sector as u32, &mut words) {
            // Convert u16 array to u8 array (little-endian)
            for i in 0..256 {
                buffer[i * 2] = words[i] as u8;
                buffer[i * 2 + 1] = (words[i] >> 8) as u8;
            }
            return Ok(());
        }
    }

    Err(STATUS_UNSUCCESSFUL)
}

/// # Safety
///
/// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
/// write a sector to the backing device.
pub unsafe fn write_sector(device: *mut (), sector: u64, buffer: &[u8]) -> Result<(), NTSTATUS> {
    if buffer.len() < 512 {
        return Err(STATUS_UNSUCCESSFUL);
    }

    if let Some(context) = unsafe { crate::fs::mount_context_from_device(device) } {
        let offset = sector as usize * 512;
        if offset + 512 > context.size {
            return Err(STATUS_UNSUCCESSFUL);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                buffer.as_ptr(),
                context.base.add(offset) as *mut u8,
                512,
            );
        }
        return Ok(());
    }

    // Try storage layer first (AHCI, ATA, RAM disk)
    if crate::drivers::storage::write_device_sector(0, sector as u32, buffer) {
        return Ok(());
    }

    // x86_64-only fallback to AHCI/ATA drivers.
    {
        let partition_offset = get_partition_offset();
        let absolute_sector = partition_offset + sector;

        // Try AHCI directly (channel 0, port 0)
        if ahci::write_sector(0, 0, absolute_sector as u32, buffer) {
            return Ok(());
        }

        // Fallback: Try ATA/ATAPI
        ata_port_initialize(0);
        let mut words = [0u16; 256];
        // Convert u8 buffer to u16 words (little-endian)
        for i in 0..256 {
            words[i] = (buffer[i * 2] as u16) | ((buffer[i * 2 + 1] as u16) << 8);
        }
        if crate::drivers::storage::ata::write_sector(0, absolute_sector as u32, &words) {
            return Ok(());
        }
    }

    Err(STATUS_UNSUCCESSFUL)
}

/// Update a FAT entry for a given cluster.
/// This function reads the FAT sector, modifies the entry, and writes it back.
/// For FAT32, we preserve the high 4 bits of each entry.
pub fn update_fat_entry(fs: &Fat32FileSystem, cluster: u32, value: u32) -> Result<(), NTSTATUS> {
    unsafe {
        let fat_start = fs.fat_data.fat_start_sector;
        let sector_size = fs.base.sector_size as u64;
        let bytes_per_sector = sector_size as usize;

        // Each FAT entry is 4 bytes (32-bit for FAT32)
        let entry_offset = (cluster as u64) * 4;
        let fat_sector = fat_start + (entry_offset / sector_size);
        let offset_in_sector = (entry_offset % sector_size) as usize;

        // read FAT sector
        let mut sector_buf = [0u8; 512];
        if { read_sector(fs.base.device, fat_sector, &mut sector_buf) }.is_err() {
            // kprintln!("[FAT32] update_fat_entry: failed to read FAT sector {}", fat_sector);
            return Err(STATUS_UNSUCCESSFUL);
        }

        // Make sure we have enough bytes for the entry
        if offset_in_sector + 4 > bytes_per_sector {
            // kprintln!("[FAT32] update_fat_entry: entry spans sector boundary");
            return Err(STATUS_UNSUCCESSFUL);
        }

        // read current value (preserve high 4 bits for FAT32)
        let old_value = u32::from_le_bytes([
            sector_buf[offset_in_sector],
            sector_buf[offset_in_sector + 1],
            sector_buf[offset_in_sector + 2],
            sector_buf[offset_in_sector + 3],
        ]);

        // FAT32 entries use only 28 bits; preserve high 4 bits
        let new_value = (old_value & 0xF000_0000) | (value & 0x0FFF_FFFF);

        // Update the FAT entry
        sector_buf[offset_in_sector..offset_in_sector + 4]
            .copy_from_slice(&new_value.to_le_bytes());

        // write FAT sector back
        if write_sector(fs.base.device, fat_sector, &sector_buf).is_err() {
            // kprintln!("[FAT32] update_fat_entry: failed to write FAT sector {}", fat_sector);
            return Err(STATUS_UNSUCCESSFUL);
        }

        // A mirror failure is an I/O failure, even if the primary was written.
        for copy in 1..fs.fat_data.boot_sector.num_fats as u64 {
            let backup = fat_sector + copy * fs.fat_data.boot_sector.sectors_per_fat_32 as u64;
            write_sector(fs.base.device, backup, &sector_buf)?;
        }

        Ok(())
    }
}

/// Allocate a free cluster and optionally link it from the current cluster.
/// Returns the new cluster number on success.
pub fn allocate_cluster(fs: &Fat32FileSystem, current_cluster: u32) -> Result<u32, NTSTATUS> {
    let total_clusters = fs.base.total_clusters as u32;
    if total_clusters == 0 {
        return Err(STATUS_UNSUCCESSFUL);
    }

    // Scan FAT to find a free cluster
    for cluster in 2..total_clusters.saturating_add(2) {
        let entry = read_fat_entry_checked(fs, cluster)?;
        if entry == FAT32_FREE {
            // Mark new cluster as end of chain
            if update_fat_entry(fs, cluster, FAT32_EOC).is_err() {
                continue;
            }

            // Link current cluster to new cluster if provided
            if current_cluster >= 2 && update_fat_entry(fs, current_cluster, cluster).is_err() {
                // Rollback: mark new cluster as free
                let _ = update_fat_entry(fs, cluster, FAT32_FREE);
                continue;
            }

            return Ok(cluster);
        }
    }

    // kprintln!("[FAT32] allocate_cluster: no free clusters available");
    Err(STATUS_UNSUCCESSFUL)
}

/// Free an entire cluster chain starting from the given cluster.
pub fn free_cluster_chain(fs: &Fat32FileSystem, start_cluster: u32) -> Result<(), NTSTATUS> {
    if start_cluster<2{return Ok(());}
    let mut chain=Vec::new();let mut current=start_cluster;
    while (2..0x0fff_fff8).contains(&current){
        if chain.len()>=fs.base.total_clusters as usize || chain.contains(&current){return Err(STATUS_UNSUCCESSFUL);}
        chain.push(current);current=read_fat_entry_checked(fs,current)?;
    }
    if current<0x0fff_fff8{return Err(STATUS_UNSUCCESSFUL);}
    for cluster in chain{update_fat_entry(fs,cluster,FAT32_FREE)?;}
    Ok(())
}

/// write data to a file given its starting cluster.
/// This handles cluster allocation and FAT table updates.
pub fn write_file(
    fs: &Fat32FileSystem, start_cluster: u32, offset: u32, data: &[u8],
) -> Result<usize, NTSTATUS> {
    let cluster_size=fs.base.cluster_size as usize;
    if cluster_size==0||cluster_size%512!=0||start_cluster<2{return Err(STATUS_UNSUCCESSFUL);}
    if data.is_empty(){return Ok(0);}
    let mut current=start_cluster;
    let mut visited=Vec::new();
    let mut remaining_skip=offset as usize/cluster_size;
    let mut within=offset as usize%cluster_size;
    let mut written=0;
    loop {
        if current<2||current as u64>=fs.base.total_clusters.saturating_add(2)||visited.contains(&current){return Err(STATUS_UNSUCCESSFUL);}
        visited.push(current);
        if remaining_skip==0 {
            while within<cluster_size&&written<data.len(){
                let mut sector=[0;512];
                let sector_index=(within/512)as u32;
                let position=within%512;
                let count=(512-position).min(data.len()-written);
                if position!=0||count!=512{read_cluster_sector(fs,current,sector_index,&mut sector)?;}
                sector[position..position+count].copy_from_slice(&data[written..written+count]);
                write_cluster_sector(fs,current,sector_index,&sector)?;
                written+=count;within+=count;
            }
            if written==data.len(){return Ok(written);}
        }else{remaining_skip-=1;}
        within=0;
        let next=read_fat_entry_checked(fs,current)?;
        current=if next>=0x0fff_fff8 {
            // Initialize before linking, so gaps and reused clusters never
            // expose bytes from a previously deleted file.
            let new=allocate_cluster(fs,0)?;
            let zero=alloc::vec![0;cluster_size];
            if let Err(error)=write_cluster(fs,new,&zero){let _=update_fat_entry(fs,new,FAT32_FREE);return Err(error);}
            if let Err(error)=update_fat_entry(fs,current,new){let _=update_fat_entry(fs,new,FAT32_FREE);return Err(error);}
            new
        }else{next};
    }
}

/// Decode a 4-byte FAT entry into a 28-bit cluster number.
pub fn decode_fat_entry(raw: u32) -> u32 {
    raw & 0x0FFF_FFFF
}

/// EOC marker for FAT32 cluster chains.
pub const FAT32_EOC: u32 = 0x0FFF_FFFF;
pub const FAT32_BAD: u32 = 0x0FFF_FFF7;
pub const FAT32_FREE: u32 = 0x0000_0000;

/// Mount a FAT32 volume. The device pointer is currently a stand-in
/// for "the in-memory filesystem image handed to us by the UEFI
/// stub" - the real implementation would call into the storage
/// stack to issue a read.
pub fn mount(
    device: *mut (),
    _path: &[u16],
    partition: Option<crate::fs::MountContext>,
) -> Option<&'static mut Fat32FileSystem> {
    unsafe {
        crate::boot_println!("[FAT32] mount: entering");
        let device = match partition {
            Some(context) => crate::fs::allocate_mount_context(context) as *mut (),
            None => device,
        };
        let mut buffer = [0u8; 512];
        crate::boot_println!("[FAT32] mount: about to read_sector 0");
        if { read_sector(device, 0, &mut buffer) }.is_err() {
            // kprintln!("[FAT32] Failed to read boot sector");
            crate::boot_println!("[FAT32] mount: read_sector failed, returning None");
            return None;
        }
        crate::boot_println!(
            "[FAT32] mount: read_sector ok, first 16 bytes = {:02x?}",
            &buffer[..16]
        );
        // The buffer-to-struct copy has to use a packed read because the
        // boot sector is misaligned in practice. We use `read_unaligned`
        // via a `core::ptr::read_unaligned` of the slice.
        let boot: Fat32BootSector =
            { core::ptr::read_unaligned(buffer.as_ptr() as *const Fat32BootSector) };
        crate::boot_println!(
            "[FAT32] mount: boot sector read, jump[0]=0x{:x}",
            boot.jump[0]
        );
        if !boot.is_valid() {
            // kprintln!("[FAT32] Invalid boot sector");
            crate::boot_println!("[FAT32] mount: boot sector invalid, returning None");
            return None;
        }
        crate::boot_println!("[FAT32] mount: boot sector valid");
        let reserved = boot.reserved_sectors as u64;
        let fats = boot.num_fats as u64;
        let fat_size = boot.sectors_per_fat_32 as u64;
        let fat_start = reserved;
        let data_start = reserved + fats * fat_size;
        let sector_sz = boot.sector_size();
        let cluster_sz = boot.cluster_size();
        let root_clu = boot.root_cluster;
        let total_sectors = if boot.total_sectors_16 != 0 { boot.total_sectors_16 as u64 } else { boot.total_sectors_32 as u64 };
        if sector_sz != 512 || boot.sectors_per_cluster == 0 || !boot.sectors_per_cluster.is_power_of_two()
            || fats == 0 || fat_size == 0 || total_sectors <= data_start { return None; }
        let total_clusters = (total_sectors - data_start) / boot.sectors_per_cluster as u64;
        if total_clusters == 0 || total_clusters + 2 > fat_size * sector_sz as u64 / 4
            || total_clusters + 2 > 0x0fff_fff0 || root_clu < 2 || root_clu as u64 >= total_clusters + 2 { return None; }
        // kprintln!("[FAT32] Mounting volume:");
        // kprintln!("      Bytes/sector:  {}", boot.bytes_per_sector);
        // kprintln!("      Sectors/cluster: {}", boot.sectors_per_cluster);
        // kprintln!("      FAT start sector: {}", fat_start);
        // kprintln!("      Data start sector: {}", data_start);
        // kprintln!("      Root cluster:    {}", root_clu);

        let fs_ptr = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<Fat32FileSystem>(),
        ) as *mut Fat32FileSystem;
        if fs_ptr.is_null() {
            return None;
        }
        // The pool allocator already zeroed the user region, so
        // we only need to set the fields that matter. We avoid
        // `core::ptr::write(fs_ptr, Fat32FileSystem::new())`
        // because the compiler emits a non-temporal SSE store
        // for the aggregate write, and the kernel pool can
        // back onto UnCacheable memory regions (UC-typed MTRRs
        // from the UEFI firmware) where those stores silently
        // fail. Field-by-field assignment is both safer and
        // explicit about the layout we are constructing.
        let new_fs = Fat32FileSystem::new();
        (*fs_ptr).base = new_fs.base;
        (*fs_ptr).fat_data = new_fs.fat_data;
        (*fs_ptr).base.device = device;
        (*fs_ptr).base.sector_size = sector_sz;
        (*fs_ptr).base.cluster_size = cluster_sz;
        (*fs_ptr).base.total_clusters = total_clusters;
        (*fs_ptr).fat_data.boot_sector = boot;
        (*fs_ptr).fat_data.fat_start_sector = fat_start;
        (*fs_ptr).fat_data.data_start_sector = data_start;
        (*fs_ptr).fat_data.root_cluster = root_clu;

        // Store in global for CMD access
        crate::boot_println!("[FAT32] mount: about to set_mounted_fs");
        set_mounted_fs(fs_ptr.as_mut().unwrap());
        crate::boot_println!("[FAT32] mount: set_mounted_fs done");

        // Initialize pagefile support
        crate::boot_println!("[FAT32] mount: about to init_pagefile");
        init_pagefile();
        crate::boot_println!("[FAT32] mount: init_pagefile returned");

        crate::boot_println!("[FAT32] mount: returning Some(fs)");
        fs_ptr.as_mut()
    }
}

/// Initialize pagefile on this FAT32 volume.
fn init_pagefile() {
    crate::boot_println!("[FAT32] init_pagefile entered");
    // Temporarily skip the pagefile open-or-create path while we
    // debug the kernel-phase bring-up. The pagefile module writes
    // through the FAT32 driver, which would in turn call
    // `write_sector` and recursively trigger the file-system bring-up
    // we are still inside. `cmd.exe` does not need the pagefile to
    // execute; once the FS path is verified we can re-enable this.
    crate::boot_println!("[FAT32] init_pagefile: SKIPPED (debug)");
    /* DISABLED (debug)
    kprintln_info!("FAT32", "Initializing pagefile support...");

    // Get block device ID (assume device 0 for now)
    let device_id = 0;

    // Get filesystem parameters from mounted volume
    let (fat_start, data_start, cluster_size, sectors_per_cluster) = if let Some(fs) = get_mounted_fs() {
        (
            fs.fat_data.fat_start_sector,
            fs.fat_data.data_start_sector,
            fs.base.cluster_size,
            fs.base.cluster_size / 512,
        )
    } else {
        kprintln_warn!("FAT32",
            "No mounted filesystem for pagefile init");
        return;
    };

    // Try to open or create pagefile
    let size_mb = crate::mm::pagefile::DEFAULT_PAGEFILE_SIZE_MB;

    if let Some(handle) = pagefile::open_or_create(
        0, // fs_start_sector
        sectors_per_cluster,
        cluster_size,
        fat_start,
        data_start,
        device_id,
        size_mb,
    ) {
        kprintln_info!("FAT32",
            "Pagefile initialized: {} clusters, {} bytes",
            handle.cluster_count, handle.size_bytes);
    } else {
        kprintln_warn!("FAT32",
            "Pagefile initialization failed");
    }
    */
}

/// Unmount a FAT32 volume.
pub fn unmount(_fs: *mut Fat32FileSystem) {
    // kprintln!("[FAT32] Volume unmounted");
}

/// Register the FAT32 driver with the I/O manager.
pub fn register_driver() {
    static mut FAT32_DRIVER: FileSystemDriver = FileSystemDriver {
        name: [
            b'F' as u16,
            b'a' as u16,
            b't' as u16,
            b'3' as u16,
            b'2' as u16,
            0,
            0,
            0,
        ],
        fs_type: FileSystemType::Fat32,
        mount: Some(mount_trampoline),
        unmount: Some(unmount_fs),
    };
    // kprintln!("    FAT32 driver registered");
    crate::fs::register(core::ptr::addr_of_mut!(FAT32_DRIVER));
}

/// `FileSystemDriver::mount` returns `*mut FileSystem` but the
/// real `mount` function returns `Option<&'static mut Fat32FileSystem>`.
/// Wrap that mismatch in a trampoline that translates the
/// `Option<&mut>` into a `*mut FileSystem`.
fn mount_trampoline(device: *mut (), path: &[u16]) -> *mut FileSystem {
    match mount(device, path, None) {
        Some(fs) => fs as *mut Fat32FileSystem as *mut FileSystem,
        None => core::ptr::null_mut(),
    }
}

/// `FileSystemDriver::unmount` is `Option<fn(*mut FileSystem)>`,
/// but `unmount` here takes `*mut Fat32FileSystem`. Wrap the
/// mismatch with a cast helper.
fn unmount_fs(fs: *mut FileSystem) {
    unmount(fs as *mut Fat32FileSystem);
}

/// read a file's bytes by walking its cluster chain. The file is
/// identified by its starting cluster (taken from a directory entry
/// search) and its size.
/// Read a byte range without allocating or reading the preceding file data.
pub fn read_file_at(fs:&Fat32FileSystem,start:u32,size:u64,offset:u64,out:&mut[u8])->Result<usize,NTSTATUS>{
    if offset>=size || out.is_empty(){return Ok(0);}
    let cluster_size=fs.base.cluster_size as usize;
    if cluster_size==0 || cluster_size%512!=0{return Err(STATUS_IO_DEVICE_ERROR);}
    let mut cluster=start;
    let skip=offset/cluster_size as u64;
    if skip>=fs.base.total_clusters{return Err(STATUS_IO_DEVICE_ERROR);}
    for _ in 0..skip {if !(2..0x0fff_fff8).contains(&cluster){return Err(STATUS_IO_DEVICE_ERROR);}cluster=read_fat_entry_checked(fs,cluster)?;}
    let length=out.len().min((size-offset)as usize);let mut copied=0;let mut within=(offset%cluster_size as u64)as usize;
    while copied<length{
        if !(2..0x0fff_fff8).contains(&cluster){return Err(STATUS_IO_DEVICE_ERROR);}
        let first=fs.fat_data.data_start_sector+(cluster-2)as u64*(cluster_size/512)as u64;
        while within<cluster_size && copied<length{
            let mut sector=[0;512];
            unsafe{read_sector(fs.base.device,first+(within/512)as u64,&mut sector)}?;
            let n=(512-within%512).min(length-copied);
            out[copied..copied+n].copy_from_slice(&sector[within%512..within%512+n]);copied+=n;within+=n;
        }
        within=0;if copied<length{cluster=read_fat_entry_checked(fs,cluster)?;}
    }
    Ok(copied)
}

pub fn read_file(
    fs: &Fat32FileSystem,
    start_cluster: u32,
    file_size: u32,
    out: &mut [u8],
) -> Result<usize, NTSTATUS> {
    let cluster_size = fs.base.cluster_size as usize;
    crate::boot_println!(
        "[FAT32] read_file: start_cluster={} file_size={} cluster_size={}",
        start_cluster,
        file_size,
        cluster_size
    );
    let mut current = start_cluster;
    let mut written = 0usize;
    // Safety limit to prevent infinite loops
    let mut iterations = 0u32;
    let max_iterations = (file_size / cluster_size as u32 + 1).min(65536);

    while (2..0x0fff_fff8).contains(&current)
        && written < file_size as usize
        && iterations < max_iterations
    {
        let first_sector = fs.fat_data.data_start_sector
            + ((current - 2) as u64) * (fs.base.cluster_size / fs.base.sector_size) as u64;
        crate::boot_println!(
            "[FAT32] read_file: current={} first_sector={} written={}",
            current,
            first_sector,
            written
        );
        let to_copy = core::cmp::min(cluster_size, file_size as usize - written);
        let to_copy = core::cmp::min(to_copy, out.len() - written);
        if to_copy == 0 {
            break;
        }
        // read one cluster's worth of sectors.
        let sector_size = fs.base.sector_size as usize;
        for s in 0..(fs.base.cluster_size / fs.base.sector_size) {
            let mut sector = [0u8; 512];
            let rs = unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) };
            if rs.is_err() {
                return Err(STATUS_IO_DEVICE_ERROR);
            }

            let sector_offset = s as usize * sector_size;
            let src_start = core::cmp::min(sector_offset, to_copy);
            let src_end = core::cmp::min(sector_offset + sector_size, to_copy);
            if src_start >= src_end {
                break;
            }
            let copy_len = src_end - src_start;
            out[written + src_start..written + src_end].copy_from_slice(&sector[..copy_len]);
        }
        crate::boot_println!(
            "[FAT32] read_file: cluster loop done, about to written += {}",
            to_copy
        );
        written += to_copy;
        crate::boot_println!("[FAT32] read_file: written now {}", written);

        // Walk the FAT to find the next cluster.
        // Use read_fat_entry to get the correct next cluster from the FAT table.
        if file_size as usize <= cluster_size {
            crate::boot_println!(
                "[FAT32] read_file: file_size {} <= cluster_size {}, breaking",
                file_size,
                cluster_size
            );
            break;
        }

        crate::boot_println!(
            "[FAT32] read_file: about to read_fat_entry for cluster {}",
            current
        );
        let next_cluster = read_fat_entry(fs, current);
        crate::boot_println!(
            "[FAT32] read_file: next_cluster after {} = {} (written={})",
            current,
            next_cluster,
            written
        );

        // Check for FAT corruption (loop detection)
        if next_cluster == current || next_cluster == FAT32_BAD {
            // kprintln!("[FAT32] read_file: FAT corruption at cluster {} (next={})", current, next_cluster);
            break;
        }

        current = next_cluster;
        iterations += 1;
    }
    Ok(written)
}

/// Look up a directory entry by 8.3 name in the root directory.
pub fn find_file_in_root(fs: &Fat32FileSystem, short_name: &[u8; 11]) -> Option<FatDirectoryEntry> {
    let cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let first_sector = fs.fat_data.data_start_sector
        + ((fs.fat_data.root_cluster - 2) as u64) * (cluster_size / sector_size) as u64;
    let sectors_per_cluster = cluster_size / sector_size;
    for s in 0..sectors_per_cluster {
        let mut sector = [0u8; 512];
        if unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) }.is_err() {
            continue;
        }
        for e in 0..sector_size / core::mem::size_of::<FatDirectoryEntry>() {
            let entry: FatDirectoryEntry = unsafe {
                core::ptr::read_unaligned(
                    sector
                        .as_ptr()
                        .add(e * core::mem::size_of::<FatDirectoryEntry>())
                        as *const FatDirectoryEntry,
                )
            };
            if !entry.is_valid() || entry.is_long_name() || entry.is_volume_id() {
                continue;
            }
            if &entry.name == short_name {
                return Some(entry);
            }
        }
    }
    None
}

/// Directory entry info for CMD
#[derive(Copy, Clone)]
pub struct FatDirEntry {
    pub name: [u8; 13], // 8.3 name, null-terminated
    pub is_dir: bool,
    pub size: u32,
    pub cluster: u32,
    pub mod_date: u16,
    pub mod_time: u16,
}

impl FatDirEntry {
    pub fn new() -> Self {
        Self {
            name: [0; 13],
            is_dir: false,
            size: 0,
            cluster: 0,
            mod_date: 0,
            mod_time: 0,
        }
    }
}

impl Default for FatDirEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// List directory entries in a cluster (and follow FAT chain if needed).
///
/// This function implements proper cluster-based directory traversal:
/// 1. Reads directory data from the starting cluster
/// 2. Parses directory entries (including long filenames)
/// 3. Follows the FAT chain to read subsequent clusters if the directory spans multiple clusters
///
/// # Arguments
///
/// * `fs` - The FAT32 filesystem
/// * `start_cluster` - The first cluster of the directory
/// * `entries` - Output buffer for directory entries
///
/// # Returns
///
/// The number of directory entries found
pub fn list_directory_cluster(
    fs: &Fat32FileSystem,
    start_cluster: u32,
    entries: &mut [FatDirEntry],
) -> usize {
    if start_cluster < 2 {
        // kprintln!("[FAT32] list_directory_cluster: invalid start cluster {}", start_cluster);
        return 0;
    }

    let cluster_size = fs.base.cluster_size as usize;
    let mut buffer = alloc::vec![0u8; cluster_size];
    let mut current_cluster = start_cluster;
    let mut count = 0;
    let mut long_name_buffer: alloc::vec::Vec<u8> = alloc::vec::Vec::new();

    // Follow the FAT chain
    while (2..0x0fff_fff8).contains(&current_cluster) {
        // read this cluster
        match read_cluster_data(fs, current_cluster, &mut buffer) {
            Ok(_) => {
                // Parse directory entries in this cluster
                let mut offset = 0;
                while offset + 32 <= cluster_size {
                    if count >= entries.len() {
                        return count;
                    }

                    let entry = unsafe {
                        core::ptr::read_unaligned(
                            buffer.as_ptr().add(offset) as *const FatDirectoryEntry
                        )
                    };

                    if entry.name[0] == 0x00 {
                        // End of directory marker
                        return count;
                    }

                    if entry.name[0] == 0xE5 {
                        // Deleted entry - skip and clear long name buffer
                        long_name_buffer.clear();
                        offset += 32;
                        continue;
                    }

                    if entry.is_long_name() {
                        // Preserve the entire 32-byte LFN record. The name
                        // field is only 11 bytes and contains checksum/type
                        // bytes in the middle of the record.
                        long_name_buffer.extend_from_slice(&buffer[offset..offset + 32]);
                        offset += 32;
                        continue;
                    }

                    // Short name entry - process it
                    let name = if long_name_buffer.is_empty() {
                        // Convert 8.3 name to string
                        convert_83_name(&entry.name, entry.is_directory())
                    } else {
                        // Decode accumulated long name
                        decode_long_name(&long_name_buffer)
                    };

                    entries[count].name = name;
                    entries[count].is_dir = entry.is_directory();
                    entries[count].size = entry.file_size();
                    entries[count].cluster = entry.first_cluster();
                    entries[count].mod_date = entry.modification_date;
                    entries[count].mod_time = entry.modification_time;
                    count += 1;

                    // Clear long name buffer for next entry
                    long_name_buffer.clear();
                    offset += 32;
                }
            }
            Err(_) => {
                // kprintln!("[FAT32] list_directory_cluster: failed to read cluster {}", current_cluster);
                break;
            }
        }

        // Get next cluster from FAT chain
        let next_cluster = read_fat_entry(fs, current_cluster);
        if next_cluster == current_cluster {
            // FAT corruption - avoid infinite loop
            // kprintln!("[FAT32] list_directory_cluster: FAT corruption detected at cluster {}", current_cluster);
            break;
        }
        current_cluster = next_cluster;
    }

    count
}

/// read a cluster's worth of data.
pub fn read_cluster_sector(
    fs: &Fat32FileSystem,
    cluster: u32,
    sector_offset: u32,
    buffer: &mut [u8],
) -> Result<(), NTSTATUS> {
    unsafe {
        if buffer.len() < 512 {
            return Err(STATUS_UNSUCCESSFUL);
        }

        let data_start = fs.fat_data.data_start_sector;
        let sectors_per_cluster = fs.base.cluster_size / fs.base.sector_size;
        let first_sector = data_start + ((cluster - 2) as u64) * (sectors_per_cluster as u64);
        let sector_num = first_sector + (sector_offset as u64);

        read_sector(fs.base.device, sector_num, buffer)
    }
}

/// read a FAT entry for a given cluster number.
/// Returns the next cluster in the chain, or FAT32_EOC if end of chain or error.
pub fn read_fat_entry(fs: &Fat32FileSystem, cluster: u32) -> u32 {
    read_fat_entry_checked(fs,cluster).unwrap_or(FAT32_EOC)
}

/// Checked FAT lookup for callers that must distinguish I/O failure from EOF.
fn read_fat_entry_checked(fs:&Fat32FileSystem,cluster:u32)->Result<u32,NTSTATUS>{
    if fs.base.sector_size!=512||cluster as u64>=fs.base.total_clusters.saturating_add(2){return Err(STATUS_IO_DEVICE_ERROR);}
    let mut sector=[0;512];let offset=cluster as usize*4;
    unsafe{read_sector(fs.base.device,fs.fat_data.fat_start_sector+(offset/512)as u64,&mut sector)}?;
    Ok(decode_fat_entry(u32::from_le_bytes(sector[offset%512..offset%512+4].try_into().map_err(|_|STATUS_IO_DEVICE_ERROR)?)))
}

/// read an entire cluster's worth of data.
fn read_cluster_data(
    fs: &Fat32FileSystem,
    cluster: u32,
    buffer: &mut [u8],
) -> Result<usize, NTSTATUS> {
    {
        let cluster_size = fs.base.cluster_size as usize;
        if buffer.len() < cluster_size {
            // kprintln!("[FAT32] read_cluster_data: buffer too small ({} < {})", buffer.len(), cluster_size);
            return Err(STATUS_UNSUCCESSFUL);
        }

        let sector_size = fs.base.sector_size as u64;
        let sectors_per_cluster = (cluster_size / sector_size as usize) as u64;
        let data_start = fs.fat_data.data_start_sector;
        // Cluster 2 is the first data cluster
        let first_sector = data_start + ((cluster as u64) - 2) * sectors_per_cluster;

        let mut offset = 0usize;
        for i in 0..sectors_per_cluster {
            let mut sector_buf = [0u8; 512];
            if unsafe { read_sector(fs.base.device, first_sector + i, &mut sector_buf) }.is_err() {
                // kprintln!("[FAT32] read_cluster_data: failed to read sector {}", first_sector + i);
                return Err(STATUS_UNSUCCESSFUL);
            }
            buffer[offset..offset + 512].copy_from_slice(&sector_buf);
            offset += 512;
        }

        Ok(offset)
    }
}

/// Convert an 8.3 filename to a null-terminated string.
fn convert_83_name(name_bytes: &[u8; 11], is_directory: bool) -> [u8; 13] {
    let mut name = [0u8; 13];
    let mut j = 0;

    // Copy base name (first 8 bytes), stripping spaces
    for i in 0..8 {
        if name_bytes[i] != 0x20 {
            name[j] = name_bytes[i];
            j += 1;
        }
    }

    // Add extension if present and not a directory
    if !is_directory && name_bytes[8] != 0x20 {
        name[j] = b'.';
        j += 1;
        for i in 8..11 {
            if name_bytes[i] != 0x20 {
                name[j] = name_bytes[i];
                j += 1;
            }
        }
    }

    name
}

/// Decode accumulated long filename bytes to UTF-16 vector.
/// This properly handles UTF-16 encoding and returns a full Unicode string.
///
/// Long filename entries are stored in reverse order. Each entry is 32 bytes:
/// - Byte 0: Sequence number (with last-entry and deleted flags)
/// - Bytes 1-10: First 5 UTF-16 characters (offset 0, 2, 4, 6, 8)
/// - Byte 11: attributes (always 0x0F for long name entries)
/// - Byte 12: Reserved (checksum)
/// - Bytes 13-20: Next 4 UTF-16 characters (offset 12, 14, 16, 18)
/// - Byte 21: Reserved
/// - Bytes 22-25: Next 3 UTF-16 characters (offset 22, 24)
/// - Bytes 26-27: Reserved
/// - Bytes 28-31: Reserved
pub fn decode_long_name_utf16(long_name_buffer: &[u8]) -> Vec<u16> {
    let mut result = Vec::new();

    if long_name_buffer.is_empty() {
        return result;
    }

    // Long name entries are stored in reverse order, so we need to process them backwards
    let mut entry_start = long_name_buffer.len();

    while entry_start >= 32 {
        entry_start = entry_start.saturating_sub(32);

        // Each entry contains 13 characters (26 bytes for UTF-16)
        // LFN fields: five chars before the attributes, four between the
        // attribute/checksum bytes, three before the first-cluster fields,
        // and two in the final reserved field.
        let offsets = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];

        for &off in &offsets {
            let pos = entry_start + off;
            if pos + 1 < long_name_buffer.len() {
                let c1 = long_name_buffer[pos];
                let c2 = long_name_buffer[pos + 1];

                // Skip padding bytes (0xFF)
                if c1 != 0xFF && c2 != 0xFF {
                    let ch = (c2 as u16) << 8 | c1 as u16;

                    // Skip Unicode null terminator
                    if ch != 0 {
                        result.push(ch);
                    }
                }
            }
        }
    }

    result
}

/// Decode accumulated long filename bytes to a null-terminated string (ASCII fallback).
/// This is kept for backwards compatibility with code that expects ASCII output.
fn decode_long_name(long_name_buffer: &[u8]) -> [u8; 13] {
    let mut name = [0u8; 13];

    if long_name_buffer.is_empty() {
        return name;
    }

    // Get UTF-16 representation
    let utf16_name = decode_long_name_utf16(long_name_buffer);

    // Convert UTF-16 to ASCII (using lossy conversion)
    let mut out_idx = 0usize;
    for &ch in utf16_name.iter() {
        if out_idx >= 12 {
            break;
        }
        // Convert UTF-16 to ASCII (or use lossy conversion)
        if ch < 128 {
            name[out_idx] = ch as u8;
            out_idx += 1;
        } else if (0xC0..=0x24F).contains(&ch) {
            // Latin characters - take lower byte
            name[out_idx] = (ch & 0xFF) as u8;
            out_idx += 1;
        } else {
            // Non-ASCII character - use underscore
            name[out_idx] = b'_';
            out_idx += 1;
        }
    }

    name
}

/// Convert UTF-16 string to UTF-8 bytes
/// This properly handles all Unicode characters in UTF-16 encoding
pub fn utf16_to_utf8(utf16: &[u16]) -> Vec<u8> {
    let mut result = Vec::new();

    for &ch in utf16.iter() {
        if ch == 0 {
            break; // Null terminator
        }

        if ch < 0x80 {
            // ASCII: 0xxxxxxx
            result.push(ch as u8);
        } else if ch < 0x800 {
            // Two bytes: 110xxxxx 10xxxxxx
            result.push(0xC0 | ((ch >> 6) as u8));
            result.push(0x80 | ((ch & 0x3F) as u8));
        } else {
            // Three bytes: 1110xxxx 10xxxxxx 10xxxxxx
            // This covers all remaining u16 values (0x800-0xFFFF)
            result.push(0xE0 | ((ch >> 12) as u8));
            result.push(0x80 | (((ch >> 6) & 0x3F) as u8));
            result.push(0x80 | ((ch & 0x3F) as u8));
        }
    }

    result
}

/// Calculate FAT32 long filename checksum
/// Used to verify long filename integrity against the short name
pub fn calc_long_name_checksum(short_name: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;

    for &byte in short_name.iter() {
        // Rotate right and add byte
        sum = sum.rotate_right(1).wrapping_add(byte);
    }

    sum
}

/// Verify long filename integrity by checking the checksum
/// Returns true if the checksum matches
pub fn verify_long_name_checksum(short_name: &[u8; 11], checksum: u8) -> bool {
    calc_long_name_checksum(short_name) == checksum
}

/// read directory entries from the root directory
pub fn list_root_directory(fs: &Fat32FileSystem, entries: &mut [FatDirEntry]) -> usize {
    let cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let first_sector = fs.fat_data.data_start_sector
        + ((fs.fat_data.root_cluster - 2) as u64) * (cluster_size / sector_size) as u64;
    let sectors_per_cluster = cluster_size / sector_size;
    let mut count = 0;

    for s in 0..sectors_per_cluster {
        let mut sector = [0u8; 512];
        if unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) }.is_err() {
            continue;
        }
        for e in 0..sector_size / core::mem::size_of::<FatDirectoryEntry>() {
            if count >= entries.len() {
                return count;
            }
            let dir_entry: FatDirectoryEntry = unsafe {
                core::ptr::read_unaligned(
                    sector
                        .as_ptr()
                        .add(e * core::mem::size_of::<FatDirectoryEntry>())
                        as *const FatDirectoryEntry,
                )
            };
            if !dir_entry.is_valid() {
                continue;
            }
            if dir_entry.is_long_name() || dir_entry.is_volume_id() {
                continue;
            }
            // Convert 8.3 name to string
            let name_bytes = dir_entry.name;
            let mut name = [0u8; 13];
            let mut j = 0;
            for i in 0..8 {
                if name_bytes[i] != 0x20 {
                    name[j] = name_bytes[i];
                    j += 1;
                }
            }
            if !dir_entry.is_directory() && name_bytes[8] != 0x20 {
                name[j] = b'.';
                j += 1;
                for i in 8..11 {
                    if name_bytes[i] != 0x20 {
                        name[j] = name_bytes[i];
                        j += 1;
                    }
                }
            }

            entries[count].name = name;
            entries[count].is_dir = dir_entry.is_directory();
            entries[count].size = dir_entry.file_size();
            entries[count].cluster = dir_entry.first_cluster();
            entries[count].mod_date = dir_entry.modification_date;
            entries[count].mod_time = dir_entry.modification_time;
            count += 1;
        }
    }
    count
}

// ============================================================================
// File/Directory write Operations
// ============================================================================

/// write data to a specific sector in a cluster
pub fn write_cluster_sector(
    fs: &Fat32FileSystem,
    cluster: u32,
    sector_offset: u32,
    data: &[u8],
) -> Result<(), NTSTATUS> {
    unsafe {
        let data_start = fs.fat_data.data_start_sector;
        let sectors_per_cluster = fs.base.cluster_size / fs.base.sector_size;
        let first_sector = data_start + ((cluster - 2) as u64) * (sectors_per_cluster as u64);
        let sector_num = first_sector + (sector_offset as u64);

        write_sector(fs.base.device, sector_num, data)
    }
}

/// write an entire cluster's worth of data
pub fn write_cluster(fs: &Fat32FileSystem, cluster: u32, data: &[u8]) -> Result<usize, NTSTATUS> {
    let cluster_size = fs.base.cluster_size as usize;
    if data.len() < cluster_size {
        return Err(STATUS_UNSUCCESSFUL);
    }

    let sectors_per_cluster = fs.base.cluster_size / fs.base.sector_size;
    let mut written = 0usize;

    for i in 0..sectors_per_cluster {
        let sector_buf: &[u8; 512] = &data[i as usize * 512..(i as usize + 1) * 512]
            .try_into()
            .unwrap();
        if write_cluster_sector(fs, cluster, i, sector_buf).is_err() {
            return Err(STATUS_UNSUCCESSFUL);
        }
        written += 512;
    }

    Ok(written)
}

/// Find a directory entry by name in the root directory
/// Returns (cluster, offset_in_cluster) if found, None otherwise
pub fn find_file_in_root_ex(fs: &Fat32FileSystem, short_name: &[u8; 11]) -> Option<(u32, usize)> {
    {
        let cluster_size = fs.base.cluster_size as usize;
        let sector_size = fs.base.sector_size as usize;
        let first_sector = fs.fat_data.data_start_sector
            + ((fs.fat_data.root_cluster - 2) as u64) * (cluster_size / sector_size) as u64;
        let sectors_per_cluster = cluster_size / sector_size;

        for s in 0..sectors_per_cluster {
            let mut sector = [0u8; 512];
            if unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) }.is_err()
            {
                continue;
            }
            for e in 0..sector_size / core::mem::size_of::<FatDirectoryEntry>() {
                let entry: FatDirectoryEntry = unsafe {
                    core::ptr::read_unaligned(
                        sector
                            .as_ptr()
                            .add(e * core::mem::size_of::<FatDirectoryEntry>())
                            as *const FatDirectoryEntry,
                    )
                };
                if !entry.is_valid() || entry.is_long_name() || entry.is_volume_id() {
                    continue;
                }
                if &entry.name == short_name {
                    let offset = s * sector_size + (e * core::mem::size_of::<FatDirectoryEntry>());
                    return Some((fs.fat_data.root_cluster, offset));
                }
            }
        }
        None
    }
}

/// Find a free directory entry slot in the root directory
/// Returns (cluster, byte_offset) of the free slot
pub fn find_free_dir_slot(fs: &Fat32FileSystem) -> Option<(u32, usize)> {
    let cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let first_sector = fs.fat_data.data_start_sector
        + ((fs.fat_data.root_cluster - 2) as u64) * (cluster_size / sector_size) as u64;
    let sectors_per_cluster = cluster_size / sector_size;

    for s in 0..sectors_per_cluster {
        let mut sector = [0u8; 512];
        if unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) }.is_err() {
            continue;
        }
        for e in 0..sector_size / core::mem::size_of::<FatDirectoryEntry>() {
            let entry: FatDirectoryEntry = unsafe {
                core::ptr::read_unaligned(
                    sector
                        .as_ptr()
                        .add(e * core::mem::size_of::<FatDirectoryEntry>())
                        as *const FatDirectoryEntry,
                )
            };
            if entry.name[0] == 0x00 || entry.name[0] == 0xE5 {
                let offset = s * sector_size + (e * core::mem::size_of::<FatDirectoryEntry>());
                return Some((fs.fat_data.root_cluster, offset));
            }
        }
    }
    None
}

/// Find a free directory entry slot in any directory cluster chain.
pub fn find_free_dir_slot_in(fs: &Fat32FileSystem, dir_cluster: u32) -> Option<(u32, usize)> {
    let cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let sectors_per_cluster = cluster_size / sector_size;
    let mut current_cluster = dir_cluster;
    let mut visited = 0u32;

    while (2..0x0fff_fff8).contains(&current_cluster) && visited < 0x0FFF_FFFF {
        let first_sector = fs.fat_data.data_start_sector
            + ((current_cluster - 2) as u64) * (sectors_per_cluster as u64);
        for s in 0..sectors_per_cluster {
            let mut sector = [0u8; 512];
            if unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) }.is_err()
            {
                continue;
            }
            for e in 0..sector_size / core::mem::size_of::<FatDirectoryEntry>() {
                let entry: FatDirectoryEntry = unsafe {
                    core::ptr::read_unaligned(
                        sector
                            .as_ptr()
                            .add(e * core::mem::size_of::<FatDirectoryEntry>())
                            as *const FatDirectoryEntry,
                    )
                };
                if entry.name[0] == 0x00 || entry.name[0] == 0xE5 {
                    let offset = s * sector_size + (e * core::mem::size_of::<FatDirectoryEntry>());
                    return Some((current_cluster, offset));
                }
            }
        }

        let next = read_fat_entry(fs, current_cluster);
        if next == current_cluster || next >= 0x0fff_fff8 {
            {
                break;
            }
        }
        current_cluster = next;
        visited += 1;
    }

    None
}

/// Find a directory entry location by short name in any directory.
type DirEntryLocation = (u32, usize, FatDirectoryEntry);

pub fn find_dir_entry_location(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    short_name: &[u8; 11],
) -> Option<DirEntryLocation> {
    let cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let sectors_per_cluster = cluster_size / sector_size;
    let mut current_cluster = dir_cluster;
    let mut visited = 0u32;

    while (2..0x0fff_fff8).contains(&current_cluster) && visited < 0x0FFF_FFFF {
        let first_sector = fs.fat_data.data_start_sector
            + ((current_cluster - 2) as u64) * (sectors_per_cluster as u64);
        for s in 0..sectors_per_cluster {
            let mut sector = [0u8; 512];
            if unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) }.is_err()
            {
                continue;
            }
            for e in 0..sector_size / core::mem::size_of::<FatDirectoryEntry>() {
                let entry: FatDirectoryEntry = unsafe {
                    core::ptr::read_unaligned(
                        sector
                            .as_ptr()
                            .add(e * core::mem::size_of::<FatDirectoryEntry>())
                            as *const FatDirectoryEntry,
                    )
                };
                if !entry.is_valid() || entry.is_long_name() || entry.is_volume_id() {
                    continue;
                }
                if &entry.name == short_name {
                    let offset = s * sector_size + (e * core::mem::size_of::<FatDirectoryEntry>());
                    return Some((current_cluster, offset, entry));
                }
            }
        }

        let next = read_fat_entry(fs, current_cluster);
        if next == current_cluster || next >= 0x0fff_fff8 {
            break;
        }
        current_cluster = next;
        visited += 1;
    }

    None
}

/// Update the short name of an entry at a known directory location.
fn rename_entry_at(
    fs: &Fat32FileSystem,
    cluster: u32,
    byte_offset: usize,
    new_name: &[u8; 11],
) -> Result<(), NTSTATUS> {
    let sector_size = fs.base.sector_size as usize;
    let sector_index = byte_offset / sector_size;
    let offset_in_sector = byte_offset % sector_size;
    let mut buffer = [0u8; 512];
    if read_cluster_sector(fs, cluster, sector_index as u32, &mut buffer).is_err() {
        return Err(STATUS_UNSUCCESSFUL);
    }

    buffer[offset_in_sector..offset_in_sector + 11].copy_from_slice(new_name);
    write_cluster_sector(fs, cluster, sector_index as u32, &buffer)
}

/// Update a directory entry at a specific location
pub fn update_dir_entry(
    fs: &Fat32FileSystem,
    cluster: u32,
    byte_offset: usize,
    entry: &FatDirectoryEntry,
) -> Result<(), NTSTATUS> {
    let _cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let sector_index = byte_offset / sector_size;
    let offset_in_sector = byte_offset % sector_size;

    // read the sector
    let mut buffer = [0u8; 512];
    if read_cluster_sector(fs, cluster, sector_index as u32, &mut buffer).is_err() {
        return Err(STATUS_UNSUCCESSFUL);
    }

    // Update the entry
    let entry_ptr = entry as *const FatDirectoryEntry as *const u8;
    let entry_size = core::mem::size_of::<FatDirectoryEntry>();
    for i in 0..entry_size {
        buffer[offset_in_sector + i] = unsafe { *entry_ptr.add(i) };
    }

    // write back
    write_cluster_sector(fs, cluster, sector_index as u32, &buffer)
}

/// Mark a file as deleted by setting the first byte to 0xE5
pub fn delete_file_entry(
    fs: &Fat32FileSystem,
    cluster: u32,
    byte_offset: usize,
) -> Result<(), NTSTATUS> {
    let _cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let sector_index = byte_offset / sector_size;
    let offset_in_sector = byte_offset % sector_size;

    // read the sector
    let mut buffer = [0u8; 512];
    if read_cluster_sector(fs, cluster, sector_index as u32, &mut buffer).is_err() {
        return Err(STATUS_UNSUCCESSFUL);
    }

    let entry: FatDirectoryEntry = unsafe {
        core::ptr::read_unaligned(buffer.as_ptr().add(offset_in_sector) as *const FatDirectoryEntry)
    };
    let start_cluster =
        ((entry.first_cluster_high as u32) << 16) | (entry.first_cluster_low as u32);

    // Mark as deleted
    buffer[offset_in_sector] = 0xE5;

    // write back
    if write_cluster_sector(fs, cluster, sector_index as u32, &buffer).is_err() {
        return Err(STATUS_UNSUCCESSFUL);
    }

    free_cluster_chain(fs, start_cluster)
}

/// Create a file or directory entry in the directory starting at
/// `dir_cluster`, supporting any directory rather than just the root.
fn create_dir_entry_in_dir(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    name: &[u8; 11],
    start_cluster: u32,
    size: u32,
    attributes: u8,
) -> Result<(), NTSTATUS> {
    let (cluster, byte_offset) =
        find_free_dir_slot_in(fs, dir_cluster).ok_or(STATUS_UNSUCCESSFUL)?;
    let entry = FatDirectoryEntry {
        name: *name,
        attributes,
        reserved: 0,
        creation_time_tenth: 0,
        creation_time: 0,
        creation_date: 0,
        last_access_date: 0,
        first_cluster_high: (start_cluster >> 16) as u16,
        modification_time: 0,
        modification_date: 0,
        first_cluster_low: (start_cluster & 0xFFFF) as u16,
        file_size: size,
    };
    update_dir_entry(fs, cluster, byte_offset, &entry)
}

fn lfn_name_utf16(long_name: &[u8]) -> alloc::vec::Vec<u16> {
    core::str::from_utf8(long_name)
        .unwrap_or("")
        .encode_utf16()
        .collect()
}

fn short_name_for_long(long_name:&[u8],index:u8)->[u8;11]{
    let mut name=[b' ';11];
    let text=core::str::from_utf8(long_name).unwrap_or("");
    let (stem,extension)=text.rsplit_once('.').unwrap_or((text,""));
    let suffix=alloc::format!("~{index}");let limit=8-suffix.len();
    let mut count=0;
    for c in stem.chars(){
        if c==' '||c=='.'{continue;}
        if count==limit{break;}
        name[count]=if c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'$'|'!'){c.to_ascii_uppercase()as u8}else{b'_'};count+=1;
    }
    if count==0{name[0]=b'_';count=1;}
    name[count..count+suffix.len()].copy_from_slice(suffix.as_bytes());
    for(i,c)in extension.chars().take(3).enumerate(){name[8+i]=if c.is_ascii_alphanumeric(){c.to_ascii_uppercase()as u8}else{b'_'};}
    name
}

fn lfn_entry(sequence:u8,checksum:u8,name:&[u16],start:usize)->FatDirectoryEntry{
    let mut bytes=[0xff;32];bytes[0]=sequence;bytes[11]=ATTR_LONG_NAME;bytes[12]=0;bytes[13]=checksum;bytes[26]=0;bytes[27]=0;
    for (i,offset)in [1,3,5,7,9,14,16,18,20,22,24,28,30].into_iter().enumerate(){
        let unit=if start+i<name.len(){name[start+i]}else if start+i==name.len(){0}else{0xffff};
        bytes[offset..offset+2].copy_from_slice(&unit.to_le_bytes());
    }
    // SAFETY: the buffer is exactly one packed integer-only FAT directory record.
    unsafe{core::ptr::read_unaligned(bytes.as_ptr()as *const FatDirectoryEntry)}
}

fn find_contiguous_free_dir_slots(fs:&Fat32FileSystem,parent:u32,count:usize)->Result<Vec<(u32,usize)>,NTSTATUS>{
    if count==0||count>21||fs.base.cluster_size==0||fs.base.cluster_size%512!=0{return Err(STATUS_UNSUCCESSFUL);}
    let mut cluster=parent;let mut visited=Vec::new();let mut slots=Vec::new();let mut end=false;
    let mut buffer=alloc::vec![0;fs.base.cluster_size as usize];
    loop{
        if cluster<2||cluster as u64>=fs.base.total_clusters.saturating_add(2)||visited.contains(&cluster){return Err(STATUS_UNSUCCESSFUL);}
        visited.push(cluster);read_cluster_data(fs,cluster,&mut buffer)?;
        for offset in (0..buffer.len()).step_by(32){
            if buffer[offset]==0&&!end{
                end=true;buffer[offset..].fill(0);write_cluster(fs,cluster,&buffer)?;
            }
            if end||buffer[offset]==0xe5{slots.push((cluster,offset));if slots.len()==count{return Ok(slots);}}
            else{slots.clear();}
        }
        let next=read_fat_entry_checked(fs,cluster)?;
        if next>=0x0fff_fff8{
            let new=allocate_cluster(fs,0)?;buffer.fill(0);
            if let Err(error)=write_cluster(fs,new,&buffer){let _=update_fat_entry(fs,new,FAT32_FREE);return Err(error);}
            if let Err(error)=update_fat_entry(fs,cluster,new){let _=update_fat_entry(fs,new,FAT32_FREE);return Err(error);}
            cluster=new;
        }else{cluster=next;}
        if end{
            // All records after the first 0 marker are unused. Clear stale
            // bytes in following clusters before publishing any new entry.
            buffer.fill(0);write_cluster(fs,cluster,&buffer)?;
        }
    }
}



/// Remove a validated LFN chain, including records crossing cluster boundaries.
fn unlink_named_entry(fs:&Fat32FileSystem,parent:u32,entry:&NamedDirectoryEntry)->Result<(),NTSTATUS>{
    let mut cluster=parent;let mut visited=Vec::new();let mut pending:Vec<(u32,usize)>=Vec::new();
    let mut buffer=alloc::vec![0;fs.base.cluster_size as usize];
    while (2..0x0fff_fff8).contains(&cluster){
        if visited.contains(&cluster)||visited.len()>=fs.base.total_clusters as usize{return Err(STATUS_UNSUCCESSFUL);}
        visited.push(cluster);read_cluster_data(fs,cluster,&mut buffer)?;
        for (i,raw)in buffer.chunks_exact(32).enumerate(){
            if cluster==entry.directory_cluster&&i*32==entry.byte_offset{
                // Unlink the live short entry first. A failed cleanup leaves
                // orphan LFN records rather than a live entry with a freed chain.
                delete_file_entry_without_freeing_chain(fs,cluster,i*32)?;
                if pending.iter().all(|(c,o)|{
                    let mut sector=[0;512];read_cluster_sector(fs,*c,(*o/512)as u32,&mut sector).is_ok()
                        && sector[*o%512+13]==calc_long_name_checksum(&entry.raw.name)
                }){
                    for (c,o)in pending{delete_file_entry_without_freeing_chain(fs,c,o)?;}
                }
                return Ok(());
            }
            if raw[0]==0{return Err(STATUS_UNSUCCESSFUL);}
            if raw[0]!=0xe5&&raw[11]==ATTR_LONG_NAME{
                if raw[0]&0x40!=0{pending.clear();}pending.push((cluster,i*32));
                if pending.len()>20{return Err(STATUS_UNSUCCESSFUL);}
            }else{pending.clear();}
        }
        cluster=read_fat_entry_checked(fs,cluster)?;
    }
    Err(STATUS_UNSUCCESSFUL)
}

pub fn create_entry_long(fs:&Fat32FileSystem,parent:u32,name:&[u8],start:u32,size:u32,attributes:u8)->Result<(),NTSTATUS>{
    create_entry_long_impl(fs,parent,name,start,size,attributes,None)
}

fn create_entry_long_impl(fs:&Fat32FileSystem,parent:u32,name:&[u8],start:u32,size:u32,attributes:u8,exclude:Option<(u32,usize)>)->Result<(),NTSTATUS>{
    let text=core::str::from_utf8(name).map_err(|_|STATUS_UNSUCCESSFUL)?;
    if text.is_empty()||text.ends_with([' ','.'])||text.chars().any(|c|c<' '||matches!(c,'\\'|'/'|':'|'*'|'?'|'"'|'<'|'>'|'|')){return Err(STATUS_UNSUCCESSFUL);}
    let units:Vec<_>=text.encode_utf16().collect();if units.len()>255{return Err(STATUS_UNSUCCESSFUL);}
    let entries=directory_entries(fs,parent)?;
    if entries.iter().any(|e|Some((e.directory_cluster,e.byte_offset))!=exclude&&e.name.eq_ignore_ascii_case(text)){return Err(0xc0000035u32 as NTSTATUS);}
    let short=(1..=255).map(|i|short_name_for_long(name,i)).find(|short|!entries.iter().any(|e|e.raw.name==*short)).ok_or(STATUS_UNSUCCESSFUL)?;
    let count=units.len().div_ceil(13);let slots=find_contiguous_free_dir_slots(fs,parent,count+1)?;
    for (ordinal,&(cluster,offset))in slots[..count].iter().enumerate(){
        let number=count-ordinal;let sequence=number as u8|if ordinal==0{0x40}else{0};
        update_dir_entry(fs,cluster,offset,&lfn_entry(sequence,calc_long_name_checksum(&short),&units,(number-1)*13))?;
    }
    // SAFETY: packed integer fields permit zero initialization.
    let mut raw:FatDirectoryEntry=unsafe{core::mem::zeroed()};
    raw.name=short;raw.attributes=attributes;raw.first_cluster_high=(start>>16)as u16;raw.first_cluster_low=start as u16;raw.file_size=size;
    let (cluster,offset)=slots[count];update_dir_entry(fs,cluster,offset,&raw)
}

pub fn create_file_in_dir_long(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    long_name: &[u8],
    start_cluster: u32,
    size: u32,
) -> Result<(), NTSTATUS> {
    create_entry_long(fs, dir_cluster, long_name, start_cluster, size, 0x20)
}

pub fn create_dir_in_dir_long(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    long_name: &[u8],
    start_cluster: u32,
) -> Result<(), NTSTATUS> {
    create_entry_long(fs, dir_cluster, long_name, start_cluster, 0, 0x10)
}

/// Delete a file or empty directory using its resolved LFN, never a guessed alias.
pub fn delete_file_in_dir_long(fs:&Fat32FileSystem,parent:u32,name:&[u8])->Result<(),NTSTATUS>{
    let name=core::str::from_utf8(name).map_err(|_|STATUS_UNSUCCESSFUL)?;
    if matches!(name,"."|".."){return Err(STATUS_UNSUCCESSFUL);}
    let entry=directory_entries(fs,parent)?.into_iter().find(|e|e.name.eq_ignore_ascii_case(name)).ok_or(STATUS_UNSUCCESSFUL)?;
    if entry.raw.is_directory()&&directory_entries(fs,entry.raw.first_cluster())?.iter().any(|e|!matches!(e.name.as_str(),"."|"..")){
        return Err(0xc0000101u32 as NTSTATUS);
    }
    unlink_named_entry(fs,parent,&entry)?;
    free_cluster_chain(fs,entry.raw.first_cluster())
}

/// Create the replacement name before unlinking the original, retaining the
/// data chain, timestamps and attributes. Same-parent rename only.
pub fn rename_file_in_dir_long(fs:&Fat32FileSystem,parent:u32,old:&[u8],new:&[u8])->Result<(),NTSTATUS>{
    let old=core::str::from_utf8(old).map_err(|_|STATUS_UNSUCCESSFUL)?;
    let new_text=core::str::from_utf8(new).map_err(|_|STATUS_UNSUCCESSFUL)?;
    let entries=directory_entries(fs,parent)?;
    let entry=entries.iter().find(|e|e.name.eq_ignore_ascii_case(old)).ok_or(STATUS_UNSUCCESSFUL)?;
    if old==new_text{return Ok(());}
    if entries.iter().any(|e|e.name.eq_ignore_ascii_case(new_text)&&(e.directory_cluster,e.byte_offset)!=(entry.directory_cluster,entry.byte_offset)){return Err(0xc0000035u32 as NTSTATUS);}
    create_entry_long_impl(fs,parent,new,entry.raw.first_cluster(),entry.raw.file_size(),entry.raw.attributes,Some((entry.directory_cluster,entry.byte_offset)))?;
    let replacement=directory_entries(fs,parent)?.into_iter().find(|e|e.name==new_text).ok_or(STATUS_UNSUCCESSFUL)?;
    let mut raw=entry.raw;raw.name=replacement.raw.name;
    update_dir_entry(fs,replacement.directory_cluster,replacement.byte_offset,&raw)?;
    unlink_named_entry(fs,parent,entry)
}

/// Mark a short-name entry deleted without freeing its cluster chain.
/// Rename uses this only after the replacement entry has been created.
fn delete_file_entry_without_freeing_chain(
    fs: &Fat32FileSystem,
    cluster: u32,
    byte_offset: usize,
) -> Result<(), NTSTATUS> {
    let sector_size = fs.base.sector_size as usize;
    let sector_index = byte_offset / sector_size;
    let offset_in_sector = byte_offset % sector_size;
    let mut buffer = [0u8; 512];
    if read_cluster_sector(fs, cluster, sector_index as u32, &mut buffer).is_err() {
        return Err(STATUS_UNSUCCESSFUL);
    }
    buffer[offset_in_sector] = 0xE5;
    write_cluster_sector(fs, cluster, sector_index as u32, &buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ke::sync::Spinlock;

    static FAT32_TEST_LOCK: Spinlock<()> = Spinlock::new(());

    #[test]
    fn fat32_nested_directories_and_long_names_use_lfn_entries() {
        let _guard = FAT32_TEST_LOCK.lock();
        let image = alloc::boxed::Box::leak(alloc::vec![0u8; 256 * 512].into_boxed_slice());

        let mut fs = Fat32FileSystem::new();
        fs.fat_data.fat_start_sector = 1;
        fs.fat_data.data_start_sector = 65;
        fs.fat_data.root_cluster = 2;
        fs.base.total_clusters = 254;
        let context = alloc::boxed::Box::leak(alloc::boxed::Box::new(
            crate::fs::MountContext::new(image.as_ptr(), image.len()),
        ));
        fs.base.device = context as *mut crate::fs::MountContext as *mut ();

        assert!(update_fat_entry(&fs, 2, FAT32_EOC).is_ok());
        assert!(update_fat_entry(&fs, 3, FAT32_EOC).is_ok());
        assert!(update_fat_entry(&fs, 4, FAT32_EOC).is_ok());

        let subdir_name = b"SUBDIR     ";
        assert!(create_dir_in_dir(&fs, 2, subdir_name, 3).is_ok());

        let long_name = b"a_very_long_nested_file_name.txt";
        assert!(create_file_in_dir_long(&fs, 3, long_name, 4, 17).is_ok());
        let short_name = short_name_for_long(long_name, 1);
        let (_, _, entry) = find_dir_entry_location(&fs, 3, &short_name).unwrap();
        assert_eq!(entry.first_cluster(), 4);
        assert_eq!(entry.file_size(), 17);

        let mut found_lfn = false;
        let mut buffer = [0u8; 512];
        assert!(read_cluster_sector(&fs, 3, 0, &mut buffer).is_ok());
        for offset in (0..512).step_by(32) {
            if buffer[offset + 11] == 0x0F {
                found_lfn = true;
                assert_eq!(buffer[offset + 13], calc_long_name_checksum(&short_name));
            }
        }
        assert!(found_lfn);

        let renamed = b"renamed_long_file_name.dat";
        assert!(rename_file_in_dir_long(&fs, 3, long_name, renamed).is_ok());
        let renamed_short = short_name_for_long(renamed, 1);
        assert!(find_dir_entry_location(&fs, 3, &renamed_short).is_some());

        assert!(delete_file_in_dir_long(&fs, 3, renamed).is_ok());
        assert!(read_fat_entry(&fs, 4) == FAT32_FREE);
        assert!(find_dir_entry_location(&fs, 3, &renamed_short).is_none());

        let _ = subdir_name;
    }
}

/// Create a file in a directory other than the root.
pub fn create_file_in_dir(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    name: &[u8; 11],
    start_cluster: u32,
    size: u32,
) -> Result<(), NTSTATUS> {
    create_dir_entry_in_dir(fs, dir_cluster, name, start_cluster, size, 0x20)
}

/// Create a directory in a directory other than the root.
pub fn create_dir_in_dir(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    name: &[u8; 11],
    start_cluster: u32,
) -> Result<(), NTSTATUS> {
    create_dir_entry_in_dir(fs, dir_cluster, name, start_cluster, 0, 0x10)
}

/// Rename an entry in a directory other than the root.
pub fn rename_file_in_dir(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    old_name: &[u8; 11],
    new_name: &[u8; 11],
) -> Result<(), NTSTATUS> {
    let (cluster, byte_offset, _) =
        find_dir_entry_location(fs, dir_cluster, old_name).ok_or(STATUS_UNSUCCESSFUL)?;
    rename_entry_at(fs, cluster, byte_offset, new_name)
}

/// create a new file in the root directory
pub fn create_file_in_root(
    fs: &Fat32FileSystem,
    name: &[u8; 11],
    start_cluster: u32,
    size: u32,
) -> Result<(), NTSTATUS> {
    // Find a free slot
    let (cluster, byte_offset) = find_free_dir_slot(fs).ok_or(STATUS_UNSUCCESSFUL)?;

    // create the directory entry
    let entry = FatDirectoryEntry {
        name: *name,
        attributes: 0x20, // Archive
        reserved: 0,
        creation_time_tenth: 0,
        creation_time: 0,
        creation_date: 0,
        last_access_date: 0,
        first_cluster_high: (start_cluster >> 16) as u16,
        modification_time: 0,
        modification_date: 0,
        first_cluster_low: (start_cluster & 0xFFFF) as u16,
        file_size: size,
    };

    update_dir_entry(fs, cluster, byte_offset, &entry)
}

/// create a new directory in the root directory
pub fn create_dir_in_root(
    fs: &Fat32FileSystem,
    name: &[u8; 11],
    start_cluster: u32,
) -> Result<(), NTSTATUS> {
    // Find a free slot
    let (cluster, byte_offset) = find_free_dir_slot(fs).ok_or(STATUS_UNSUCCESSFUL)?;

    // create the directory entry
    let entry = FatDirectoryEntry {
        name: *name,
        attributes: 0x10, // Directory
        reserved: 0,
        creation_time_tenth: 0,
        creation_time: 0,
        creation_date: 0,
        last_access_date: 0,
        first_cluster_high: (start_cluster >> 16) as u16,
        modification_time: 0,
        modification_date: 0,
        first_cluster_low: (start_cluster & 0xFFFF) as u16,
        file_size: 0,
    };

    update_dir_entry(fs, cluster, byte_offset, &entry)
}

/// Convert a filename string to 8.3 format
pub fn name_to_83(name: &str) -> [u8; 11] {
    let mut result = [b' '; 11];

    // handle path with backslash
    let name_only = if let Some(pos) = name.rfind('\\') {
        &name[pos + 1..]
    } else if let Some(pos) = name.rfind('/') {
        &name[pos + 1..]
    } else {
        name
    };

    // Find extension position
    let dot_pos = name_only.find('.').unwrap_or(name_only.len());
    let base = &name_only[..dot_pos];
    let ext = if dot_pos < name_only.len() {
        &name_only[dot_pos + 1..]
    } else {
        ""
    };

    // Copy base name (up to 8 chars), uppercased
    for (i, c) in base.bytes().take(8).enumerate() {
        result[i] = if c.is_ascii_lowercase() { c - 32 } else { c };
    }

    // Copy extension (up to 3 chars), uppercased
    for (i, c) in ext.bytes().take(3).enumerate() {
        let uc = if c.is_ascii_lowercase() { c - 32 } else { c };
        result[8 + i] = uc;
    }

    result
}

/// Rename a file in the root directory
pub fn rename_file_in_root(
    fs: &Fat32FileSystem,
    old_name: &[u8; 11],
    new_name: &[u8; 11],
) -> Result<(), NTSTATUS> {
    if let Some((cluster, byte_offset)) = find_file_in_root_ex(fs, old_name) {
        // read current entry
        let _cluster_size = fs.base.cluster_size as usize;
        let sector_size = fs.base.sector_size as usize;
        let sector_index = byte_offset / sector_size;
        let offset_in_sector = byte_offset % sector_size;

        let mut buffer = [0u8; 512];
        if read_cluster_sector(fs, cluster, sector_index as u32, &mut buffer).is_err() {
            return Err(STATUS_UNSUCCESSFUL);
        }

        // Update the name (first 11 bytes of the entry)
        buffer[offset_in_sector..(11 + offset_in_sector)].copy_from_slice(&new_name[..11]);

        // write back
        write_cluster_sector(fs, cluster, sector_index as u32, &buffer)
    } else {
        Err(STATUS_UNSUCCESSFUL)
    }
}

/// Check if a file exists in the root directory
pub fn file_exists_in_root(fs: &Fat32FileSystem, short_name: &[u8; 11]) -> bool {
    find_file_in_root_ex(fs, short_name).is_some()
}

/// Search a single directory cluster for a file or subdirectory by
/// short name. Returns the matched `FatDirectoryEntry` (preserving the
/// on-disk bytes for callers that need attributes and clusters). The
/// search is purely by short (8.3) name — long filename support is
/// not needed for the kernel-side batch loader, which receives
/// uppercased names from the BAT parser.
fn find_in_dir_by_short(
    fs: &Fat32FileSystem,
    dir_cluster: u32,
    short_name: &[u8; 11],
) -> Option<FatDirectoryEntry> {
    let cluster_size = fs.base.cluster_size as usize;
    let sector_size = fs.base.sector_size as usize;
    let sectors_per_cluster = cluster_size / sector_size;
    let mut current_cluster = dir_cluster;
    // Walk the FAT chain so multi-cluster directories are supported.
    let mut visited: u32 = 0;
    let mut iter_count: u32 = 0;
    while (2..0x0fff_fff8).contains(&current_cluster) && visited < 0x0FFF_FFFF {
        let first_sector = fs.fat_data.data_start_sector
            + ((current_cluster - 2) as u64) * (sectors_per_cluster as u64);
        for s in 0..sectors_per_cluster {
            let mut sector = [0u8; 512];
            if unsafe { read_sector(fs.base.device, first_sector + s as u64, &mut sector) }.is_err()
            {
                continue;
            }
            for e in 0..sector_size / core::mem::size_of::<FatDirectoryEntry>() {
                iter_count += 1;
                if iter_count <= 4 {
                    boot_println!("[FAT32] find_in_dir_by_short: checking entry {} (cluster={} sector={} entry_idx={})",
                        iter_count, current_cluster, first_sector + s as u64, e);
                }
                let entry: FatDirectoryEntry = unsafe {
                    core::ptr::read_unaligned(
                        sector
                            .as_ptr()
                            .add(e * core::mem::size_of::<FatDirectoryEntry>())
                            as *const FatDirectoryEntry,
                    )
                };
                if !entry.is_valid() {
                    if iter_count <= 4 {
                        boot_println!(
                            "[FAT32] find_in_dir_by_short:   entry {} invalid",
                            iter_count
                        );
                    }
                    continue;
                }
                if entry.is_long_name() || entry.is_volume_id() {
                    if iter_count <= 4 {
                        boot_println!(
                            "[FAT32] find_in_dir_by_short:   entry {} is LFN/vol, skipped",
                            iter_count
                        );
                    }
                    continue;
                }
                if iter_count <= 8 {
                    boot_println!(
                        "[FAT32] find_in_dir_by_short:   entry {} name={:?} == target={:?}? {}",
                        iter_count,
                        entry.name,
                        short_name,
                        &entry.name == short_name
                    );
                }
                if &entry.name == short_name {
                    boot_println!("[FAT32] find_in_dir_by_short:   FOUND entry!");
                    return Some(entry);
                }
            }
        }
        let next = read_fat_entry(fs, current_cluster);
        if next == current_cluster || next >= 0x0fff_fff8 {
            break;
        }
        current_cluster = next;
        visited += 1;
    }
    boot_println!(
        "[FAT32] find_in_dir_by_short: not found after {} entries",
        iter_count
    );
    None
}

/// Resolve a Windows-style path against the FAT32 volume and return
/// the directory entry of the requested file.
///
/// The path is split on `\` (any embedded `\` segments are walked
/// against subdirectories); a final segment that is not `.bat` or
/// `.cmd` is allowed because callers occasionally pass arbitrary
/// batch scripts. If any segment of the path is missing the function
/// returns `None` rather than descending into the root, so a
/// halfway-mounted system partition never silently runs a stale
/// script from `C:\autoexec.bat`.
/// Complete UTF-8 names and on-disk locations for VFS consumers.
/// Unlike the legacy FatDirEntry this does not truncate LFNs to 12 bytes.
pub struct NamedDirectoryEntry {
    pub name: alloc::string::String,
    pub raw: FatDirectoryEntry,
    pub directory_cluster: u32,
    pub byte_offset: usize,
}

pub fn directory_entries(fs: &Fat32FileSystem, start: u32) -> Result<Vec<NamedDirectoryEntry>, NTSTATUS> {
    let cluster_size = fs.base.cluster_size as usize;
    if start < 2 || cluster_size == 0 || cluster_size > 65536 { return Err(STATUS_UNSUCCESSFUL); }
    let mut buffer = alloc::vec![0; cluster_size];
    let mut entries = Vec::new();
    let mut visited = Vec::new();
    let mut cluster = start;
    let mut lfn = Vec::new();
    while (2..0x0fff_fff8).contains(&cluster) {
        if visited.len() >= 4096 || visited.contains(&cluster) { return Err(STATUS_UNSUCCESSFUL); }
        visited.push(cluster);
        read_cluster_data(fs, cluster, &mut buffer)?;
        for (index, bytes) in buffer.chunks_exact(32).enumerate() {
            if bytes[0] == 0 { return Ok(entries); }
            if bytes[0] == 0xE5 { lfn.clear(); continue; }
            if bytes[11] == ATTR_LONG_NAME {
                if bytes[0] & 0x40 != 0 { lfn.clear(); }
                if lfn.len() < 20 * 32 { lfn.extend_from_slice(bytes); }
                continue;
            }
            // SAFETY: bytes contains a full packed directory record, read by value.
            let raw = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const FatDirectoryEntry) };
            if raw.is_volume_id() { lfn.clear(); continue; }
            let mut units = Vec::new();
            let count = lfn.len() / 32;
            let valid = count != 0 && lfn.chunks_exact(32).enumerate().all(|(i, rec)|
                usize::from(rec[0] & 0x1F) == count-i && rec[13] == calc_long_name_checksum(&raw.name));
            if valid {
                'name: for record in lfn.chunks_exact(32).rev() {
                    for offset in [1,3,5,7,9,14,16,18,20,22,24,28,30] {
                        let unit = u16::from_le_bytes([record[offset], record[offset+1]]);
                        if unit == 0 || unit == 0xFFFF { break 'name; }
                        units.push(unit);
                    }
                }
            }
            let name = if units.is_empty() {
                let short = convert_83_name(&raw.name, raw.is_directory());
                let length = short.iter().position(|b| *b==0).unwrap_or(short.len());
                alloc::string::String::from_utf8_lossy(&short[..length]).into_owned()
            } else { alloc::string::String::from_utf16_lossy(&units) };
            lfn.clear();
            entries.push(NamedDirectoryEntry { name, raw, directory_cluster: cluster, byte_offset:index*32 });
            if entries.len() >= 8192 { return Err(STATUS_UNSUCCESSFUL); }
        }
        cluster = read_fat_entry_checked(fs, cluster)?;
    }
    if cluster < 0x0fff_fff8 { return Err(STATUS_IO_DEVICE_ERROR); }
    Ok(entries)
}

pub fn find_file_at_path(fs: &Fat32FileSystem, path: &str) -> Option<FatDirectoryEntry> {
    let path = path.strip_prefix("C:").or_else(||path.strip_prefix("c:")).unwrap_or(path);
    let mut segments = path.split(['\\','/']).filter(|s| !s.is_empty()).peekable();
    let mut cluster = fs.fat_data.root_cluster;
    while let Some(segment) = segments.next() {
        if segment == "." || segment == ".." { return None; }
        let entry = directory_entries(fs, cluster).ok()?.into_iter()
            .find(|entry| {
                let short = convert_83_name(&entry.raw.name, entry.raw.is_directory());
                let n = short.iter().position(|b|*b==0).unwrap_or(short.len());
                entry.name.eq_ignore_ascii_case(segment) || short[..n].eq_ignore_ascii_case(segment.as_bytes())
            })?;
        if segments.peek().is_none() { return Some(entry.raw); }
        if !entry.raw.is_directory() { return None; }
        cluster = entry.raw.first_cluster();
    }
    None
}

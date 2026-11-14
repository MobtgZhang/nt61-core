//! DiskPart command backend. Focus is caller-owned; unsupported operations return errors.
//! Filesystem formatting and dynamic disks are not implemented.

use crate::drivers::{partmgr, storage::disk, volmgr};
use crate::fs::mount;
use crate::hal::serial;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct DiskPartState {
    selected_disk: Option<usize>,
    selected_partition: Option<u32>,
    selected_volume: Option<usize>,
}

impl DiskPartState {
    pub fn new() -> Self {
        Self {
            selected_disk: None,
            selected_partition: None,
            selected_volume: None,
        }
    }
}

impl Default for DiskPartState {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskPartState {
    pub fn from_focus(focus:[u32;3])->Result<Self,&'static str>{
        let selected_disk=(focus[0]!=u32::MAX).then_some(focus[0]as usize);
        let selected_partition=(focus[1]!=u32::MAX).then_some(focus[1]);
        let selected_volume=(focus[2]!=u32::MAX).then_some(focus[2]as usize);
        if selected_disk.is_some_and(|d|disk::get_disk(d).is_none())
            ||selected_volume.is_some_and(|v|!volmgr::list_all().get(v).is_some_and(|v|v.valid))
            ||selected_partition.is_some_and(|p|selected_disk.is_none_or(|d|!partmgr::list_for_disk(d).iter().any(|v|v.valid&&v.partition_number==p))){return Err("The selected object no longer exists. Select it again.");}
        Ok(Self{selected_disk,selected_partition,selected_volume})
    }
    pub fn focus(&self)->[u32;3]{[self.selected_disk.map_or(u32::MAX,|v|v as u32),self.selected_partition.unwrap_or(u32::MAX),self.selected_volume.map_or(u32::MAX,|v|v as u32)]}
}
/// The interactive input loop belongs to the console host, which calls execute_command.
pub fn run_interactive()->bool{print_str("DiskPart requires an interactive console host.\r\n");false}

pub fn execute_command(cmd: &str, state: &mut DiskPartState) -> Result<String, String> {
    // Serialize table changes across console sessions and script invocations.
    static COMMAND_LOCK: spin::Mutex<()> = spin::Mutex::new(());
    let _guard=COMMAND_LOCK.lock();
    let cmd_upper = cmd.trim().to_uppercase();
    let parts: Vec<&str> = cmd_upper.split_whitespace().collect();

    if parts.is_empty() {
        return Ok(String::new());
    }

    if matches!(parts[0],"ACTIVE"|"INACTIVE"|"CLEAN"|"EXIT"|"QUIT") && parts.len()!=1 {return Err("Unsupported command options.".into());}
    if parts[0]=="ATTRIBUTES" {return Err("Disk and volume attribute queries/changes are not implemented.".into());}
    if matches!(parts[0],"CREATE"|"DELETE"|"ACTIVE"|"INACTIVE"|"CLEAN"|"CONVERT"|"EXTEND"|"SHRINK") {
        if let Some(disk_index)=state.selected_disk.or_else(||state.selected_volume.and_then(|i|volmgr::list_all().get(i).map(|v|v.disk_index))){
            if disk_has_mounted_volume(disk_index){return Err("The disk contains a mounted volume. Offline it before changing its partition table.".into());}
            let mut mbr=[0;512];if disk::disk_read(disk_index,0,512,&mut mbr)!=512{return Err("Cannot read the partition table.".into());}
            if mbr[510..512]!=[0x55,0xaa] && parts[0]!="CLEAN" {return Err("The disk has no valid MBR. Initialize it before creating partitions.".into());}
            if (0..4).any(|i|mbr[446+i*16+4]==0xee){return Err("GPT partition-table modifications are not implemented.".into());}
        }
    }
    match parts[0] {
        "LIST" => execute_list(&parts[1..], state),
        "SELECT" => execute_select(&parts[1..], state),
        "DETAIL" => execute_detail(&parts[1..], state),
        "CREATE" => execute_create(&parts[1..], state),
        "DELETE" => execute_delete(&parts[1..], state),
        "FORMAT" => execute_format(&parts[1..], state),
        "ASSIGN" | "REMOVE" => Err(String::from("Changing drive letters is not supported by the live mount manager.")),
        "ACTIVE" => execute_active(state),
        "INACTIVE" => execute_inactive(state),
        "CLEAN" => execute_clean(state),
        "CONVERT" => execute_convert(&parts[1..], state),
        "EXTEND" => execute_extend(&parts[1..], state),
        "SHRINK" => execute_shrink(&parts[1..], state),
        
        "RESCAN" if parts.len()==1 => {
            partmgr::init();
            for index in 0..disk::disk_count(){volmgr::refresh_disk(index);}
            if DiskPartState::from_focus(state.focus()).is_err(){state.selected_partition=None;state.selected_volume=None;}
            Ok("DiskPart has finished scanning your configuration.".into())
        },
        "HELP" | "?" => Ok(get_help()),
        "EXIT" | "QUIT" => Err(String::from("EXIT")),
        _ => Err(format!("Unknown command: {}", parts[0])),
    }
}

/// The boot path mounts RAM mirrors without creating mount-manager records.
/// Match their boot sectors too so an unregistered boot disk cannot be cleaned.
fn disk_has_mounted_volume(index: usize) -> bool {
    if mount::list_all_mount_points().iter().any(|m| m.mounted && m.disk_index == index)
        || volmgr::list_all().iter().any(|v| v.valid && v.mounted && v.disk_index == index) { return true; }
    let mut system=[0u8;512];let mut esp=[0u8;512];
    let have_system=crate::fs::sys_ramdisk_read(0,&mut system)==512;
    let have_esp=crate::fs::esp_ramdisk_read(0,&mut esp)==512;
    if !have_system && !have_esp {return false;}
    partmgr::list_for_disk(index).iter().any(|p| {
        let mut sector=[0u8;512];
        disk::disk_read(index,p.starting_offset.saturating_mul(512),512,&mut sector)==512
            && ((have_system && sector==system)||(have_esp && sector==esp))
    })
}

fn execute_list(args: &[&str], state: &DiskPartState) -> Result<String, String> {
    if args.len() != 1 {
        return Err(String::from("Specify exactly one target: DISK, PARTITION, or VOLUME"));
    }

    match args[0] {
        "DISK" => list_disks(),
        "PARTITION" => list_partitions(state),
        "VOLUME" => list_volumes(),
        _ => Err(format!("Invalid LIST target: {}", args[0])),
    }
}

fn list_disks() -> Result<String, String> {
    let mut output = String::new();
    output.push_str("\r\n");
    output.push_str("  Disk ###  Status         Size     Free     Dyn  Gpt\r\n");
    output.push_str("  --------  -------------  -------  -------  ---  ---\r\n");

    let disk_count = disk::disk_count();
    for i in 0..disk_count {
        output.push_str(&format!("  Disk {}     ", i));

        if let Some(info) = disk::get_disk(i) {
            output.push_str("Online         ");

            let size_gb = info.sector_count / (2 * 1024 * 1024); // Convert sectors to GB
            if size_gb < 1024 {
                output.push_str(&format!("{} GB    ", size_gb));
            } else {
                output.push_str(&format!("{} TB    ", size_gb / 1024));
            }

            let partitions = partmgr::list_for_disk(i);
            let used_sectors: u64 = partitions.iter().map(|p| p.sector_count).sum();
            let free_sectors = info.sector_count.saturating_sub(used_sectors);
            let free_gb = free_sectors / (2 * 1024 * 1024);

            if free_gb < 1024 {
                output.push_str(&format!("{} GB  ", free_gb));
            } else {
                output.push_str(&format!("{} TB  ", free_gb / 1024));
            }

            output.push_str("     ");
            output.push_str("   "); // Dyn (not dynamic)
            let mut mbr=[0u8;512];
            let gpt=disk::disk_read(i,0,512,&mut mbr)==512
                && mbr[510..512]==[0x55,0xaa] && (0..4).any(|slot|mbr[446+slot*16+4]==0xee);
            output.push_str(if gpt{" *  "}else{"    "});
        } else {
            output.push_str("Offline        ");
        }

        output.push_str("\r\n");
    }

    Ok(output)
}

fn list_partitions(state: &DiskPartState) -> Result<String, String> {
    if state.selected_disk.is_none() {
        return Err(String::from(
            "There is no disk selected to list partitions.\r\nPlease select a disk and try again.",
        ));
    }

    let disk_index = state.selected_disk.unwrap();
    let mut output = String::new();

    output.push_str("\r\n");
    output.push_str("  Partition ###  Type              Size     Offset\r\n");
    output.push_str("  -------------  ----------------  -------  -------\r\n");

    let partitions = partmgr::list_for_disk(disk_index);

    for part in partitions {
        if !part.valid {
            continue;
        }

        output.push_str(&format!("  Partition {}     ", part.partition_number));

        let part_type = match part.partition_type {
            0x07 => "NTFS",
            0x0B | 0x0C => "FAT32",
            0x83 => "Linux",
            0x82 => "Linux swap",
            0x05 | 0x0F => "Extended",
            _ => "Primary",
        };

        output.push_str(&format!("{:<16}  ", part_type));

        let size_mb = part.sector_count / 2048;
        if size_mb < 1024 {
            output.push_str(&format!("{} MB    ", size_mb));
        } else {
            output.push_str(&format!("{} GB    ", size_mb / 1024));
        }

        let offset_kb = part.starting_offset / 2;
        output.push_str(&format!("{} KB", offset_kb));

        output.push_str("\r\n");
    }

    Ok(output)
}

/// Mount-manager names identify partitions; volume-manager names identify slots.
/// Join on the backing partition instead of comparing those different names.
fn mounted_volume<'a>(vol:&volmgr::Volume,points:&'a [mount::MountPoint])->Option<&'a mount::MountPoint>{
    let partitions=partmgr::list_for_disk(vol.disk_index);
    points.iter().find(|m|m.mounted && m.disk_index==vol.disk_index
        && partitions.iter().any(|p|p.valid && p.partition_number==m.partition_index && p.starting_offset==vol.starting_lba))
}
fn volume_heading(output:&mut String){
    output.push_str("  Volume ###  Ltr  Label        Fs     Type        Size     Status     Info\r\n");
    output.push_str("  ----------  ---  -----------  -----  ----------  -------  ---------  --------\r\n");
}
fn volume_row(output:&mut String,index:usize,vol:&volmgr::Volume,points:&[mount::MountPoint]){
    let mp=mounted_volume(vol,points);
    let letter=mp.map_or(' ',|m|m.drive_letter);
    let label=mp.map_or_else(String::new,|m|m.volume_label.chars().take(11).collect());
    let fs=mp.map_or(vol.fs_name.as_str(),|m|m.filesystem_type.as_str());
    let size_mb=vol.sector_count/2048;
    let size=if size_mb<1024{format!("{} MB",size_mb)}else{format!("{} GB",size_mb/1024)};
    output.push_str(&format!("  Volume {:<4}  {}    {:<11}  {:<5}  Partition   {:<7}  {:<9}  {}\r\n",
        index,letter,label,fs,size,if mp.is_some()||vol.mounted{"Mounted"}else{"Unmounted"},
        if mp.is_some_and(|m|m.bootable){"Active"}else{""}));
}
fn list_volumes()->Result<String,String>{
    let mut output=String::from("\r\n");volume_heading(&mut output);
    let points=mount::list_all_mount_points();
    for(index,vol)in volmgr::list_all().iter().enumerate().filter(|(_,v)|v.valid){volume_row(&mut output,index,vol,&points);}
    Ok(output)
}

fn execute_select(args: &[&str], state: &mut DiskPartState) -> Result<String, String> {
    if args.len() != 2 {
        return Err(String::from(
            "Invalid SELECT syntax. Use: SELECT DISK|PARTITION|VOLUME <number>",
        ));
    }

    let index: usize = args[1]
        .parse()
        .map_err(|_| String::from("Invalid number"))?;

    match args[0] {
        "DISK" => {
            if disk::get_disk(index).is_some() {
                state.selected_disk = Some(index);
                state.selected_partition = None;state.selected_volume=None;
                Ok(format!("Disk {} is now the selected disk.", index))
            } else {
                Err(String::from("There is no disk with that number."))
            }
        }
        "PARTITION" => {
            if state.selected_disk.is_none() {
                return Err(String::from(
                    "There is no disk selected. Please select a disk first.",
                ));
            }

            let disk_index = state.selected_disk.unwrap();
            let partitions = partmgr::list_for_disk(disk_index);

            if partitions
                .iter()
                .any(|p| p.valid && u32::try_from(index).ok() == Some(p.partition_number))
            {
                state.selected_partition = Some(index as u32);state.selected_volume=None;
                Ok(format!(
                    "Partition {} is now the selected partition.",
                    index
                ))
            } else {
                Err(String::from("There is no partition with that number."))
            }
        }
        "VOLUME" => {
            let volumes = volmgr::list_all();
            if index < volumes.len() && volumes[index].valid {
                state.selected_volume = Some(index);state.selected_disk=Some(volumes[index].disk_index);state.selected_partition=None;
                Ok(format!("Volume {} is now the selected volume.", index))
            } else {
                Err(String::from("There is no volume with that number."))
            }
        }
        _ => Err(format!("Invalid SELECT target: {}", args[0])),
    }
}

fn execute_detail(args: &[&str], state: &DiskPartState) -> Result<String, String> {
    if args.len() != 1 {
        return Err(String::from("Specify exactly one target: DISK, PARTITION, or VOLUME"));
    }

    match args[0] {
        "DISK" => detail_disk(state),
        "PARTITION" => detail_partition(state),
        "VOLUME" => detail_volume(state),
        _ => Err(format!("Invalid DETAIL target: {}", args[0])),
    }
}

fn detail_disk(state: &DiskPartState) -> Result<String, String> {
    if state.selected_disk.is_none() {
        return Err(String::from("There is no disk selected."));
    }

    let disk_index = state.selected_disk.unwrap();
    let mut output = String::new();

    if let Some(info) = disk::get_disk(disk_index) {
        let mut mbr=[0u8;512];
        if disk::disk_read(disk_index,0,512,&mut mbr)!=512{return Err("Cannot read disk information.".into());}
        let signature=u32::from_le_bytes(mbr[440..444].try_into().unwrap());
        output.push_str(&format!("\r\nDisk {}\r\nDisk ID: {:08X}\r\nSize: {} bytes\r\nStatus : Online\r\n",disk_index,signature,info.sector_count.saturating_mul(512)));
        output.push_str(&format!("Path   : \\Device\\Harddisk{}\r\n\r\n",disk_index));
        volume_heading(&mut output);
        let points=mount::list_all_mount_points();
        for(index,vol)in volmgr::list_all().iter().enumerate().filter(|(_,v)|v.valid&&v.disk_index==disk_index){
            volume_row(&mut output,index,vol,&points);
        }

        Ok(output)
    } else {
        Err(String::from("Cannot get disk information."))
    }
}

fn detail_partition(state: &DiskPartState) -> Result<String, String> {
    if state.selected_partition.is_none() {
        return Err(String::from("There is no partition selected."));
    }

    let disk_index = state.selected_disk.unwrap();
    let part_num = state.selected_partition.unwrap();

    let partitions = partmgr::list_for_disk(disk_index);
    let partition = partitions.iter().find(|p| p.partition_number == part_num);

    if let Some(part) = partition {
        let mut output = String::new();
        output.push_str(&format!("\r\nPartition {}\r\n", part.partition_number));
        output.push_str(&format!("Type  : {:02X}\r\n", part.partition_type));
        output.push_str("Hidden: No\r\n");
        output.push_str(&format!(
            "Active: {}\r\n",
            if part.boot_indicator == 0x80 {
                "Yes"
            } else {
                "No"
            }
        ));
        output.push_str(&format!(
            "Offset in Bytes: {}\r\n\r\n",
            part.starting_offset * 512
        ));

        volume_heading(&mut output);
        let points=mount::list_all_mount_points();
        for(index,vol)in volmgr::list_all().iter().enumerate().filter(|(_,v)|v.valid&&v.disk_index==disk_index&&v.starting_lba==part.starting_offset){
            volume_row(&mut output,index,vol,&points);
        }

        Ok(output)
    } else {
        Err(String::from("Cannot find partition information."))
    }
}

fn detail_volume(state: &DiskPartState) -> Result<String, String> {
    if state.selected_volume.is_none() {
        return Err(String::from("There is no volume selected."));
    }

    let vol_index = state.selected_volume.unwrap();
    let volumes = volmgr::list_all();

    if vol_index < volumes.len() && volumes[vol_index].valid {
        let vol = &volumes[vol_index];
        let mut output = String::new();

        output.push_str(&format!("\r\n{}\r\n", vol.name));
        output.push_str(&format!("Volume ###  {}\r\n", vol_index));

        let mount_points = mount::list_all_mount_points();
        let mp = mounted_volume(vol,&mount_points);

        if let Some(mp) = mp {
            output.push_str(&format!("Drive Letter: {}\r\n", mp.drive_letter));
        } else {
            output.push_str("Drive Letter: (none)\r\n");
        }

        output.push_str(&format!("File System: {}\r\n", mp.map_or(vol.fs_name.as_str(),|m|m.filesystem_type.as_str())));
        output.push_str(&format!("Label      : {}\r\n",mp.map_or("",|m|m.volume_label.as_str())));
        output.push_str("Type       : Partition\r\n");
        output.push_str(&format!(
            "Status     : {}\r\n",
            if mp.is_some()||vol.mounted { "Mounted" } else { "Unmounted" }
        ));

        let size_gb = vol.sector_count / (2 * 1024 * 1024);
        output.push_str(&format!("Capacity   : {} GB\r\n", size_gb));



        Ok(output)
    } else {
        Err(String::from("Cannot find volume information."))
    }
}

fn execute_create(args: &[&str], state: &mut DiskPartState) -> Result<String, String> {
    if args.is_empty() {
        return Err(String::from("Specify PARTITION PRIMARY|EXTENDED|LOGICAL"));
    }

    if args[0] != "PARTITION" {
        return Err(String::from("Only CREATE PARTITION is supported"));
    }

    if state.selected_disk.is_none() {
        return Err(String::from("There is no disk selected."));
    }

    let part_kind = if args.len() > 1 { args[1] } else { "PRIMARY" };
    if part_kind != "PRIMARY" {
        return Err(format!("Only MBR PRIMARY partitions are supported, requested: {}", part_kind));
    }

    let disk_index = state.selected_disk.unwrap();
    let sector_count = disk::disk_count();
    if disk_index >= sector_count {
        return Err(String::from("Disk index out of range."));
    }

    let existing = partmgr::list_for_disk(disk_index);
    if existing.iter().filter(|p| p.valid).count() >= 4 {
        return Err(String::from(
            "Disk already contains the maximum number of MBR primary partitions.",
        ));
    }

    // Find the first free slot and the end of the last existing partition.
    let mut free_slot = 0usize;
    let mut next_lba = 2048u64;
    for p in &existing {
        if p.valid {
            next_lba = (p.starting_offset + p.sector_count).max(next_lba);
        }
    }
    next_lba=next_lba.div_ceil(2048)*2048;
    // Skip over used slots so we don't overwrite a valid entry.
    while free_slot < 4 {
        let used = existing
            .iter()
            .any(|p| p.valid && p.partition_number as usize == free_slot + 1);
        if !used {
            break;
        }
        free_slot += 1;
    }
    if free_slot >= 4 {
        return Err(String::from("No free MBR partition slot available."));
    }

    let part_type: u8 = match part_kind {
        "EXTENDED" => 0x05,
        "LOGICAL" => 0x0B,
        _ => 0x07,
    };

    // Read the MBR, build the new entry, and write it back.
    let mut mbr = [0u8; 512];
    if disk::disk_read(disk_index, 0, 512, &mut mbr) != 512 {
        return Err(String::from("Failed to read MBR from disk."));
    }
    let entry_offset = 446 + free_slot * 16;
    if mbr[entry_offset..entry_offset + 16].iter().any(|&b| b != 0) {
        return Err(String::from("MBR slot is not empty."));
    }
    // Status byte: non-bootable.
    mbr[entry_offset] = 0x00;
    // Start head/sector/cylinder fields are not reliably derivable for
    // modern disks; the kernel uses LBA so CHS fields are set to zero
    // (standard practice for large-sector disks).
    mbr[entry_offset + 1] = 0x00;
    mbr[entry_offset + 2] = 0x00;
    mbr[entry_offset + 3] = 0x00;
    mbr[entry_offset + 4] = part_type;
    mbr[entry_offset + 5] = 0x00;
    mbr[entry_offset + 6] = 0x00;
    mbr[entry_offset + 7] = 0x00;
    // Start LBA (little-endian, 4 bytes).
    let mut requested_size=None;
    for token in args.iter().skip(if args.len()>1&&args[1]=="PRIMARY"{2}else{1}){
        if let Some(size)=token.strip_prefix("SIZE="){let mb=size.parse::<u64>().map_err(|_|String::from("Invalid SIZE in megabytes."))?;requested_size=Some(mb.checked_mul(2048).filter(|v|*v!=0).ok_or_else(||String::from("Invalid SIZE."))?);}
        else {return Err(format!("Unsupported CREATE option: {}",token));}
    }
    if next_lba>u32::MAX as u64{return Err("Partition start exceeds MBR limits.".into());}
    let start = (next_lba as u32).to_le_bytes();
    mbr[entry_offset + 8..entry_offset + 12].copy_from_slice(&start);
    // Size in sectors (little-endian, 4 bytes): use the remaining free
    // space, capped at the maximum u32 value.
    let total_sectors = disk::get_disk(disk_index)
        .map(|d| d.sector_count)
        .unwrap_or(0);
    let available=total_sectors.saturating_sub(next_lba).min(u32::MAX as u64-next_lba);
    let size=requested_size.unwrap_or(available);if size==0||size>available{return Err("There is not enough usable free space.".into());}
    let size_bytes = (size as u32).to_le_bytes();
    mbr[entry_offset + 12..entry_offset + 16].copy_from_slice(&size_bytes);

    if disk::disk_write(disk_index, 0, 512, &mbr) != 512 {
        return Err(String::from("Failed to write MBR to disk."));
    }

    // Re-read the partition table so the new partition is visible.
    partmgr::init();volmgr::refresh_disk(disk_index);state.selected_partition=Some((free_slot+1)as u32);state.selected_volume=None;

    Ok(format!(
        "DiskPart successfully created the specified partition (slot {}, start LBA {}, size {} sectors).",
        free_slot + 1, next_lba, size
    ))
}

fn execute_delete(args: &[&str], state: &mut DiskPartState) -> Result<String, String> {
    if args.len() != 1 {
        return Err(String::from("Specify exactly one target: PARTITION or VOLUME"));
    }

    match args[0] {
        "PARTITION" => {
            if state.selected_partition.is_none() {
                return Err(String::from("There is no partition selected."));
            }
            let disk_index = state.selected_disk.unwrap_or(0);
            let part_num = state.selected_partition.unwrap();
            let partitions = partmgr::list_for_disk(disk_index);
            let Some(_part) = partitions
                .iter()
                .find(|p| p.valid && p.partition_number == part_num)
            else {
                return Err(String::from("Selected partition not found on disk."));
            };

            let mut mbr = [0u8; 512];
            if disk::disk_read(disk_index, 0, 512, &mut mbr) != 512 {
                return Err(String::from("Failed to read MBR from disk."));
            }
            // Find the MBR entry for this partition (partition_number is
            // 1-based, MBR entries are 0-based).
            let slot = (part_num as usize).saturating_sub(1);
            if slot >= 4 {
                return Err(String::from(
                    "Cannot delete extended/logical partition entries.",
                ));
            }
            let entry_offset = 446 + slot * 16;
            if mbr[entry_offset..entry_offset + 16].iter().all(|&b| b == 0) {
                return Err(String::from("Partition table entry is already empty."));
            }
            for b in mbr[entry_offset..entry_offset + 16].iter_mut() {
                *b = 0;
            }
            if disk::disk_write(disk_index, 0, 512, &mbr) != 512 {
                return Err(String::from("Failed to write MBR to disk."));
            }
            partmgr::init();volmgr::refresh_disk(disk_index);state.selected_partition=None;state.selected_volume=None;
            Ok(format!(
                "DiskPart successfully deleted the partition (slot {}).",
                part_num
            ))
        }
        "VOLUME" => {
            if state.selected_volume.is_none() {
                return Err(String::from("There is no volume selected."));
            }
            // Deleting a volume is equivalent to deleting the underlying
            // partition. The volume manager rebuilds from the partition
            // table after `partmgr::init()`.
            let vol_index = state.selected_volume.unwrap();
            let volumes = volmgr::list_all();
            let Some(vol) = volumes.get(vol_index) else {
                return Err(String::from("Selected volume not found."));
            };
            if !vol.valid {
                return Err(String::from("Selected volume is not valid."));
            }
            // Look up the partition that backs this volume.
            let disk_index = vol.disk_index;
            let partitions = partmgr::list_for_disk(disk_index);
            let Some(part) = partitions.iter().find(|p| p.valid && p.starting_offset == vol.starting_lba) else {
                return Err(String::from(
                    "Cannot find the backing partition for this volume.",
                ));
            };
            state.selected_disk = Some(disk_index);
            state.selected_partition = Some(part.partition_number);
            execute_delete(&["PARTITION"], state)
        }
        _ => Err(format!("Invalid DELETE target: {}", args[0])),
    }
}

fn execute_format(args: &[&str], state: &DiskPartState) -> Result<String, String> {
    if state.selected_volume.is_none() {
        return Err(String::from("There is no volume selected."));
    }

    let mut _fs_type = "NTFS";
    let mut _quick = false;
    let mut label = "";

    for i in 0..args.len() {
        if args[i] == "FS" && i + 1 < args.len() {
            _fs_type = args[i + 1];
        } else if args[i] == "QUICK" {
            _quick = true;
        } else if args[i] == "LABEL" && i + 1 < args.len() {
            label = args[i + 1];
        }
    }

    let _ = args;
    let _ = state;
    // Filesystem formatting requires a full filesystem driver with
    // block-level write support. The kernel currently provides read
    // access to FAT32 and NTFS; the format operation would need to
    // initialise the superblock, MFT/file allocation table, and root
    // directory. Until that write path is available in the mounted
    // filesystem, report a precise error instead of pretending success.
    if label.is_empty() {
        Err(String::from(
            "Format requires filesystem-level write access that is not yet available. \
             Boot into the target OS and run format from there.",
        ))
    } else {
        Err(format!(
            "Format of volume with label '{}' requires filesystem-level write access \
             that is not yet available.",
            label
        ))
    }
}

fn execute_active(state: &mut DiskPartState) -> Result<String, String> {
    if state.selected_partition.is_none() {
        return Err(String::from("There is no partition selected."));
    }

    let disk_index = state.selected_disk.unwrap_or(0);
    let part_num = state.selected_partition.unwrap();
    let partitions = partmgr::list_for_disk(disk_index);
    let Some(_part) = partitions
        .iter()
        .find(|p| p.valid && p.partition_number == part_num)
    else {
        return Err(String::from("Selected partition not found on disk."));
    };

    let mut mbr = [0u8; 512];
    if disk::disk_read(disk_index, 0, 512, &mut mbr) != 512 {
        return Err(String::from("Failed to read MBR from disk."));
    }
    let slot = (part_num as usize).saturating_sub(1);
    if slot >= 4 {
        return Err(String::from(
            "Cannot mark extended/logical partitions as active.",
        ));
    }
    let entry_offset = 446 + slot * 16;
    if mbr[entry_offset..entry_offset + 16].iter().all(|&b| b == 0) {
        return Err(String::from("Partition table entry is empty."));
    }
    // Clear the boot indicator on all other partitions (only one can
    // be active on an MBR disk), then set this one to 0x80.
    for other in 0..4 {
        mbr[446 + other * 16] = if other == slot { 0x80 } else { 0x00 };
    }
    if disk::disk_write(disk_index, 0, 512, &mbr) != 512 {
        return Err(String::from("Failed to write MBR to disk."));
    }
    partmgr::init();volmgr::refresh_disk(disk_index);
    Ok(format!("DiskPart marked partition {} as active.", part_num))
}

fn execute_inactive(state: &mut DiskPartState) -> Result<String, String> {
    if state.selected_partition.is_none() {
        return Err(String::from("There is no partition selected."));
    }

    let disk_index = state.selected_disk.unwrap_or(0);
    let part_num = state.selected_partition.unwrap();
    let partitions = partmgr::list_for_disk(disk_index);
    let Some(_part) = partitions
        .iter()
        .find(|p| p.valid && p.partition_number == part_num)
    else {
        return Err(String::from("Selected partition not found on disk."));
    };

    let mut mbr = [0u8; 512];
    if disk::disk_read(disk_index, 0, 512, &mut mbr) != 512 {
        return Err(String::from("Failed to read MBR from disk."));
    }
    let slot = (part_num as usize).saturating_sub(1);
    if slot >= 4 {
        return Err(String::from(
            "Cannot mark extended/logical partitions as inactive.",
        ));
    }
    let entry_offset = 446 + slot * 16;
    if mbr[entry_offset] != 0x80 {
        return Err(String::from("The partition is not marked as active."));
    }
    mbr[entry_offset] = 0x00;
    if disk::disk_write(disk_index, 0, 512, &mbr) != 512 {
        return Err(String::from("Failed to write MBR to disk."));
    }
    partmgr::init();volmgr::refresh_disk(disk_index);
    Ok(format!(
        "DiskPart marked partition {} as inactive.",
        part_num
    ))
}

fn execute_clean(state: &mut DiskPartState) -> Result<String, String> {
    if state.selected_disk.is_none() {
        return Err(String::from("There is no disk selected."));
    }

    let disk_index = state.selected_disk.unwrap();
    let mut mbr = [0u8; 512];
    if disk::disk_read(disk_index, 0, 512, &mut mbr) != 512 {
        return Err(String::from("Failed to read MBR from disk."));
    }
    // Preserve the boot signature so the disk remains a valid MBR disk
    // even after cleaning. Zero the partition table and the MBR code
    // area (offsets 0-445) but keep the 0x55 0xAA signature.
    for b in mbr[..446].iter_mut() {
        *b = 0;
    }
    for b in mbr[446..510].iter_mut() {
        *b = 0;
    }
    mbr[510..512].copy_from_slice(&[0x55, 0xaa]);
    if disk::disk_write(disk_index, 0, 512, &mbr) != 512 {
        return Err(String::from("Failed to write MBR to disk."));
    }
    partmgr::init();volmgr::refresh_disk(disk_index);
    state.selected_partition = None;
    state.selected_volume = None;
    Ok(format!(
        "DiskPart succeeded in cleaning disk {}.",
        disk_index
    ))
}

fn execute_convert(args: &[&str], state: &DiskPartState) -> Result<String, String> {
    if args.is_empty() {
        return Err(String::from("Specify MBR or GPT"));
    }

    if state.selected_disk.is_none() {
        return Err(String::from("There is no disk selected."));
    }

    match args[0] {
        "MBR" => Err(String::from(
            "Conversion to MBR is not implemented.",
        )),
        "GPT" => Err(String::from(
            "Conversion to GPT is not implemented.",
        )),
        _ => Err(format!("Invalid CONVERT target: {}", args[0])),
    }
}

fn execute_extend(_args: &[&str], state: &DiskPartState) -> Result<String, String> {
    if state.selected_volume.is_none() {
        return Err(String::from("There is no volume selected."));
    }

    // Extending a volume requires the filesystem to be resized online.
    // The NTFS and FAT32 filesystem drivers in this kernel support
    // read-only mounting; growing the filesystem would require writing
    // new cluster chains and updating the filesystem metadata.
    Err(String::from(
        "Extend requires online filesystem resize support that is not yet \
         available in the read-only filesystem mount path.",
    ))
}

fn execute_shrink(_args: &[&str], state: &DiskPartState) -> Result<String, String> {
    if state.selected_volume.is_none() {
        return Err(String::from("There is no volume selected."));
    }

    // Shrinking a volume requires moving files to the beginning of the
    // volume and updating the filesystem metadata. This is not yet
    // supported by the read-only filesystem mount path.
    Err(String::from(
        "Shrink requires online filesystem resize support that is not yet \
         available in the read-only filesystem mount path.",
    ))
}

fn get_help() -> String {
    String::from("LIST DISK|PARTITION|VOLUME; SELECT DISK|PARTITION|VOLUME n; DETAIL DISK|PARTITION|VOLUME; CREATE PARTITION PRIMARY [SIZE=MB]; DELETE PARTITION; ACTIVE; INACTIVE; CLEAN; EXIT.\r\nPartition writes require an unmounted MBR disk. GPT modification, format, resize and drive-letter changes are not implemented.")
}

fn print_str(s: &str) {
    for &b in s.as_bytes() {
        serial::write_char(b);
    }
}

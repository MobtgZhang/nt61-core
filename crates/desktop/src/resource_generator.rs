//! Build-time generator for the Windows 7 desktop resource DLLs.
//!
//! The source payloads are the icon files extracted from Windows 7 and
//! stored under `assets/windows7/icons`. This module packages those
//! payloads into a real PE32+ resource directory; it does not copy any
//! Microsoft DLL.

extern crate std;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FILE_ALIGNMENT: u32 = 0x200;
const SECTION_ALIGNMENT: u32 = 0x1000;
const TEXT_RVA: u32 = SECTION_ALIGNMENT;
const RDATA_RVA: u32 = SECTION_ALIGNMENT * 2;
const RSRC_RVA: u32 = SECTION_ALIGNMENT * 3;

const RESOURCE_DIRECTORY_SIZE: usize = 0x10;
const RESOURCE_ENTRY_SIZE: usize = 0x08;
const DATA_ENTRY_SIZE: usize = 0x10;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

const ICON_DIRECTORIES: [&str; 2] = ["imageres", "shell32"];

const SPECIAL_ICON_IDS: [(&str, u16); 2] = [
    ("SHIDI_FLIP3D.ico", 63012),
    ("SHIDI_SHIELD_INTERNAL.ico", 63013),
];

#[derive(Debug, Clone)]
struct IconImage {
    width: u8,
    height: u8,
    color_count: u8,
    planes: u16,
    bit_count: u16,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct IconGroup {
    id: u16,
    first_icon_id: u16,
    images: Vec<IconImage>,
}

#[derive(Debug)]
struct ResourceEntry {
    type_id: u16,
    id: u16,
    name: Option<String>,
    bytes: Vec<u8>,
}

fn align_up_u32(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) & !(alignment - 1)
}

fn put_u16(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn resource_directory_header(out: &mut [u8], offset: usize, named_count: usize, id_count: usize) {
    put_u32(out, offset, 0);
    put_u32(out, offset + 4, 0);
    put_u16(out, offset + 8, 0);
    put_u16(out, offset + 10, 0);
    put_u16(out, offset + 12, named_count as u16);
    put_u16(out, offset + 14, id_count as u16);
}

fn resource_directory_entry(out: &mut [u8], offset: usize, id: u32, child_offset: usize) {
    put_u32(out, offset, id);
    put_u32(out, offset + 4, 0x8000_0000 | child_offset as u32);
}

fn ico_error(path: &Path, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}: {}", path.display(), message),
    )
}

fn icon_group_id(path: &Path) -> Result<u16, io::Error> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ico_error(path, "icon file name is not valid UTF-8"))?;
    if let Some((_, id)) = SPECIAL_ICON_IDS.iter().find(|(name, _)| *name == file_name) {
        return Ok(*id);
    }
    let id_part = file_name
        .strip_prefix("ICON")
        .and_then(|rest| rest.split('_').next())
        .ok_or_else(|| ico_error(path, "icon file name does not contain an ID"))?;
    id_part
        .parse::<u16>()
        .map_err(|_| ico_error(path, "icon file ID is not a valid u16"))
}

fn icon_group_key(path: &Path) -> Result<String, io::Error> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ico_error(path, "icon file name is not valid UTF-8"))?;
    if let Some((name, _)) = SPECIAL_ICON_IDS.iter().find(|(name, _)| *name == file_name) {
        return Ok(name.strip_suffix(".ico").unwrap_or(name).to_string());
    }
    let id_part = file_name
        .strip_prefix("ICON")
        .and_then(|rest| rest.split('_').next())
        .ok_or_else(|| ico_error(path, "icon file name does not contain an ID"))?;
    Ok(id_part.to_string())
}

fn load_icon_order(directory: &Path) -> Result<Vec<(String, u16)>, io::Error> {
    let path = directory.join("icon-order.csv");
    let contents = fs::read_to_string(&path)?;
    let mut order = Vec::new();
    for (line_index, line) in contents.lines().enumerate() {
        if line_index == 0 && line.starts_with("group,") {
            continue;
        }
        let mut parts = line.split(',');
        let group = parts.next().unwrap_or_default().trim().to_string();
        let first_icon_id = parts
            .next()
            .unwrap_or_default()
            .trim()
            .parse::<u16>()
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: invalid icon ID", path.display()),
                )
            })?;
        if group.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: missing icon group", path.display()),
            ));
        }
        order.push((group, first_icon_id));
    }
    Ok(order)
}

fn parse_ico(path: &Path, bytes: &[u8]) -> Result<IconGroup, io::Error> {
    if bytes.len() < 6 {
        return Err(ico_error(path, "ICO header is truncated"));
    }
    if bytes[0..2] != [0, 0] {
        return Err(ico_error(path, "ICO reserved field is not zero"));
    }
    let type_id = u16::from_le_bytes([bytes[2], bytes[3]]);
    if type_id != 1 {
        return Err(ico_error(path, "ICO type is not an icon"));
    }
    let image_count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    if image_count == 0 {
        return Err(ico_error(path, "ICO does not contain images"));
    }

    let mut images = Vec::with_capacity(image_count);
    for index in 0..image_count {
        let entry_offset = 6 + index * 16;
        if entry_offset + 16 > bytes.len() {
            return Err(ico_error(path, "ICO image directory is truncated"));
        }
        let width = bytes[entry_offset];
        let height = bytes[entry_offset + 1];
        let color_count = bytes[entry_offset + 2];
        let planes = u16::from_le_bytes([bytes[entry_offset + 4], bytes[entry_offset + 5]]);
        let bit_count = u16::from_le_bytes([bytes[entry_offset + 6], bytes[entry_offset + 7]]);
        let image_size = u32::from_le_bytes([
            bytes[entry_offset + 8],
            bytes[entry_offset + 9],
            bytes[entry_offset + 10],
            bytes[entry_offset + 11],
        ]) as usize;
        let image_offset = u32::from_le_bytes([
            bytes[entry_offset + 12],
            bytes[entry_offset + 13],
            bytes[entry_offset + 14],
            bytes[entry_offset + 15],
        ]) as usize;
        if image_size == 0 {
            return Err(ico_error(path, "ICO image has zero size"));
        }
        let image_end = image_offset
            .checked_add(image_size)
            .ok_or_else(|| ico_error(path, "ICO image end overflows"))?;
        if image_end > bytes.len() {
            return Err(ico_error(path, "ICO image extends beyond the file"));
        }
        images.push(IconImage {
            width,
            height,
            color_count,
            planes,
            bit_count,
            bytes: bytes[image_offset..image_end].to_vec(),
        });
    }

    Ok(IconGroup {
        id: icon_group_id(path)?,
        first_icon_id: 0,
        images,
    })
}

fn load_icon_directory(directory_name: &str) -> Result<Vec<IconGroup>, io::Error> {
    let assets_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("windows7")
        .join("icons");
    let directory = assets_directory.join(directory_name);
    let icon_order: BTreeMap<String, u16> = load_icon_order(&directory)?.into_iter().collect();
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("ico")
        {
            paths.push(path);
        }
    }
    paths.sort();

    let mut groups = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path)?;
        let mut group = parse_ico(&path, &bytes)?;
        let key = icon_group_key(&path)?;
        group.first_icon_id = *icon_order.get(&key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: missing icon order entry", path.display()),
            )
        })?;
        groups.push(group);
    }
    if groups.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no Windows 7 icons found in {}", directory_name),
        ));
    }
    groups.sort_by_key(|group| group.first_icon_id);
    let duplicate = groups
        .windows(2)
        .find(|pair| pair[0].id == pair[1].id)
        .map(|pair| pair[0].id);
    if let Some(id) = duplicate {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "duplicate Windows 7 icon group ID {} in {}",
                id, directory_name
            ),
        ));
    }
    Ok(groups)
}

fn group_icon_payload(group: &IconGroup, icon_ids: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(6 + group.images.len() * 14);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(group.images.len() as u16).to_le_bytes());
    for (image, icon_id) in group.images.iter().zip(icon_ids) {
        bytes.extend_from_slice(&[image.width, image.height, image.color_count, 0]);
        bytes.extend_from_slice(&image.planes.to_le_bytes());
        bytes.extend_from_slice(&image.bit_count.to_le_bytes());
        bytes.extend_from_slice(&(image.bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&icon_id.to_le_bytes());
    }
    bytes
}

fn icon_resources(groups: &[IconGroup]) -> Vec<ResourceEntry> {
    let mut resources = Vec::new();
    for group in groups {
        let mut icon_ids = Vec::with_capacity(group.images.len());
        let mut icon_id = group.first_icon_id;
        for image in &group.images {
            icon_ids.push(icon_id);
            resources.push(ResourceEntry {
                type_id: RT_ICON,
                id: icon_id,
                name: None,
                bytes: image.bytes.clone(),
            });
            icon_id = icon_id.saturating_add(1);
        }
        resources.push(ResourceEntry {
            type_id: RT_GROUP_ICON,
            id: group.id,
            name: SPECIAL_ICON_IDS
                .iter()
                .find(|(_, id)| *id == group.id)
                .map(|(file_name, _)| {
                    file_name
                        .strip_suffix(".ico")
                        .unwrap_or(file_name)
                        .to_string()
                }),
            bytes: group_icon_payload(group, &icon_ids),
        });
    }
    resources.sort_by_key(|resource| (resource.type_id, resource.name.is_none(), resource.id));
    resources
}

fn resource_section(entries: &[ResourceEntry]) -> Vec<u8> {
    let icon_count = entries
        .iter()
        .filter(|entry| entry.type_id == RT_ICON)
        .count();
    let group_count = entries
        .iter()
        .filter(|entry| entry.type_id == RT_GROUP_ICON)
        .count();

    let root_directory_size = RESOURCE_DIRECTORY_SIZE + 2 * RESOURCE_ENTRY_SIZE;
    let icon_directory_size = RESOURCE_DIRECTORY_SIZE + icon_count * RESOURCE_ENTRY_SIZE;
    let group_directory_size = RESOURCE_DIRECTORY_SIZE + group_count * RESOURCE_ENTRY_SIZE;
    let name_strings_size = entries
        .iter()
        .filter_map(|entry| entry.name.as_ref())
        .map(|name| align_up_u32((2 + name.len() * 2) as u32, 4) as usize)
        .sum::<usize>();
    let language_directory_count = entries.len();
    let language_directory_size = RESOURCE_DIRECTORY_SIZE + RESOURCE_ENTRY_SIZE;
    let language_directories_size = language_directory_count * language_directory_size;
    let data_entries_size = entries.len() * DATA_ENTRY_SIZE;
    let payload_offset = root_directory_size
        + icon_directory_size
        + group_directory_size
        + name_strings_size
        + language_directories_size
        + data_entries_size;

    let payload_offset = (payload_offset + 3) & !3;
    let payload_size = entries.iter().map(|entry| entry.bytes.len()).sum::<usize>();
    let section_size = align_up_u32((payload_offset + payload_size) as u32, SECTION_ALIGNMENT);
    let mut section = vec![0u8; section_size as usize];

    resource_directory_header(&mut section, 0, 0, 2);
    resource_directory_entry(&mut section, 0x10, RT_ICON as u32, root_directory_size);
    resource_directory_entry(
        &mut section,
        0x18,
        RT_GROUP_ICON as u32,
        root_directory_size + icon_directory_size,
    );

    let icon_directory_offset = root_directory_size;
    resource_directory_header(&mut section, icon_directory_offset, 0, icon_count);
    let group_directory_offset = icon_directory_offset + icon_directory_size;
    let named_group_count = entries
        .iter()
        .filter(|entry| entry.type_id == RT_GROUP_ICON && entry.name.is_some())
        .count();
    resource_directory_header(
        &mut section,
        group_directory_offset,
        named_group_count,
        group_count - named_group_count,
    );

    let name_strings_offset = group_directory_offset + group_directory_size;
    let mut name_cursor = name_strings_offset;
    let mut name_offsets = BTreeMap::new();
    for entry in entries {
        if let Some(name) = entry.name.as_ref() {
            name_offsets.insert(entry.id, name_cursor);
            put_u16(&mut section, name_cursor, name.len() as u16);
            for (index, unit) in name.encode_utf16().enumerate() {
                put_u16(&mut section, name_cursor + 2 + index * 2, unit);
            }
            name_cursor =
                align_up_u32(name_cursor as u32 + (2 + name.len() * 2) as u32, 4) as usize;
        }
    }

    let mut language_offset = name_cursor;
    let mut data_entry_offset = language_offset + language_directories_size;
    let mut payload_cursor = payload_offset;
    let mut icon_index = 0usize;
    let mut group_index = 0usize;
    let mut current_type = entries.first().map_or(RT_ICON, |entry| entry.type_id);

    for entry in entries {
        if entry.type_id != current_type {
            icon_index = 0;
            group_index = 0;
            current_type = entry.type_id;
        }
        let directory_offset = if entry.type_id == RT_ICON {
            let offset = icon_directory_offset + 0x10 + icon_index * RESOURCE_ENTRY_SIZE;
            icon_index += 1;
            offset
        } else {
            let offset = group_directory_offset + 0x10 + group_index * RESOURCE_ENTRY_SIZE;
            group_index += 1;
            offset
        };
        let directory_name = entry
            .name
            .as_ref()
            .map(|_| 0x8000_0000u32 | name_offsets[&entry.id] as u32)
            .unwrap_or(entry.id as u32);
        put_u32(&mut section, directory_offset, directory_name);
        put_u32(
            &mut section,
            directory_offset + 4,
            0x8000_0000 | language_offset as u32,
        );

        resource_directory_header(&mut section, language_offset, 0, 1);
        let mut language_entry = [0u8; 8];
        put_u32(&mut language_entry, 0, 0x409);
        put_u32(&mut language_entry, 4, data_entry_offset as u32);
        let language_entry_offset = language_offset + RESOURCE_DIRECTORY_SIZE;
        section[language_entry_offset..language_entry_offset + 8].copy_from_slice(&language_entry);

        put_u32(
            &mut section,
            data_entry_offset,
            RSRC_RVA + payload_cursor as u32,
        );
        put_u32(
            &mut section,
            data_entry_offset + 4,
            entry.bytes.len() as u32,
        );
        put_u32(&mut section, data_entry_offset + 8, 0);
        put_u32(&mut section, data_entry_offset + 12, 0);

        let payload_end = payload_cursor + entry.bytes.len();
        section[payload_cursor..payload_end].copy_from_slice(&entry.bytes);

        language_offset += language_directory_size;
        data_entry_offset += DATA_ENTRY_SIZE;
        payload_cursor = payload_end;
    }

    section
}

fn machine_for_arch(arch: &str) -> u16 {
    match arch {
        "aarch64" | "arm64" => 0xAA64,
        "riscv64" => 0x5064,
        "loongarch64" => 0x6264,
        _ => 0x8664,
    }
}

fn image_base_for_arch(arch: &str) -> u64 {
    match arch {
        "aarch64" | "arm64" => 0x0000_0000_0040_0000,
        "riscv64" => 0x0000_0000_0041_0000,
        "loongarch64" => 0x0000_0000_0042_0000,
        _ => 0x0000_0000_0043_0000,
    }
}

fn resource_pe(arch: &str, image_name: &str, entries: &[ResourceEntry]) -> Vec<u8> {
    let text = [0xC3u8, 0xC3, 0xC3, 0xC3];
    let rsrc = resource_section(entries);
    let export_name = image_name.as_bytes();
    let rdata_size = 0x100usize + export_name.len() + 1;
    let mut rdata = vec![0u8; rdata_size];
    let dll_name_offset = 0x0D0;
    rdata[dll_name_offset..dll_name_offset + export_name.len()].copy_from_slice(export_name);

    let section_table_offset = 0x188u32;
    let text_raw_size = align_up_u32(text.len() as u32, FILE_ALIGNMENT);
    let rdata_raw_size = align_up_u32(rdata.len() as u32, FILE_ALIGNMENT);
    let rsrc_raw_size = align_up_u32(rsrc.len() as u32, FILE_ALIGNMENT);
    let text_raw_offset = align_up_u32(section_table_offset + 3 * 40, FILE_ALIGNMENT);
    let rdata_raw_offset = text_raw_offset + text_raw_size;
    let rsrc_raw_offset = rdata_raw_offset + rdata_raw_size;
    let total_size = rsrc_raw_offset + rsrc_raw_size;
    let headers_size = align_up_u32(section_table_offset + 3 * 40, FILE_ALIGNMENT);

    let mut out = vec![0u8; total_size as usize];
    out[0..2].copy_from_slice(b"MZ");
    put_u32(&mut out, 0x3C, 0x80);
    let pe_offset = 0x80;
    out[pe_offset..pe_offset + 4].copy_from_slice(b"PE\x00\x00");
    let coff = pe_offset + 4;
    put_u16(&mut out, coff, machine_for_arch(arch));
    put_u16(&mut out, coff + 2, 3);
    put_u16(&mut out, coff + 0x10, 240);
    put_u16(&mut out, coff + 0x12, 0x2022);

    let optional = coff + 20;
    put_u16(&mut out, optional, 0x020B);
    put_u16(&mut out, optional + 2, 14);
    put_u32(&mut out, optional + 6, text_raw_size);
    put_u32(&mut out, optional + 0x0A, rdata_raw_size);
    put_u32(&mut out, optional + 0x0E, rsrc_raw_size);
    put_u32(&mut out, optional + 0x10, TEXT_RVA);
    put_u32(&mut out, optional + 0x14, TEXT_RVA);
    put_u64(&mut out, optional + 0x18, image_base_for_arch(arch));
    put_u32(&mut out, optional + 0x20, SECTION_ALIGNMENT);
    put_u32(&mut out, optional + 0x24, FILE_ALIGNMENT);
    put_u16(&mut out, optional + 0x28, 10);
    put_u16(&mut out, optional + 0x2A, 0);
    put_u16(&mut out, optional + 0x2C, 0);
    put_u16(&mut out, optional + 0x2E, 0);
    put_u16(&mut out, optional + 0x30, 10);
    put_u16(&mut out, optional + 0x32, 0);
    put_u32(&mut out, optional + 0x38, SECTION_ALIGNMENT * 4);
    put_u32(&mut out, optional + 0x3C, headers_size);
    put_u16(&mut out, optional + 0x44, 0x0002);
    put_u16(&mut out, optional + 0x46, 0x0160);
    put_u64(&mut out, optional + 0x48, 0x0010_0000);
    put_u64(&mut out, optional + 0x50, 0x0000_1000);
    put_u64(&mut out, optional + 0x58, 0x0010_0000);
    put_u64(&mut out, optional + 0x60, 0x0000_1000);
    put_u32(&mut out, optional + 0x68, 0);
    put_u32(&mut out, optional + 0x6C, 16);

    let data_directory = optional + 0x70;
    put_u32(&mut out, data_directory, RDATA_RVA);
    put_u32(&mut out, data_directory + 4, rdata.len() as u32);
    put_u32(&mut out, data_directory + 16, RSRC_RVA);
    put_u32(&mut out, data_directory + 20, rsrc.len() as u32);

    let sections = section_table_offset as usize;
    for (index, section) in [
        (
            ".text",
            text.len(),
            TEXT_RVA,
            text_raw_size,
            text_raw_offset,
            0x6000_0020,
        ),
        (
            ".rdata",
            rdata.len(),
            RDATA_RVA,
            rdata_raw_size,
            rdata_raw_offset,
            0x4000_0040,
        ),
        (
            ".rsrc",
            rsrc.len(),
            RSRC_RVA,
            rsrc_raw_size,
            rsrc_raw_offset,
            0x4000_0040,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = sections + index * 40;
        out[offset..offset + section.0.len()].copy_from_slice(section.0.as_bytes());
        put_u32(&mut out, offset + 8, section.1 as u32);
        put_u32(&mut out, offset + 0x0C, section.2);
        put_u32(&mut out, offset + 0x10, section.3);
        put_u32(&mut out, offset + 0x14, section.4);
        put_u32(&mut out, offset + 0x24, section.5);
    }

    let text_raw = text_raw_offset as usize;
    let rdata_raw = rdata_raw_offset as usize;
    let rsrc_raw = rsrc_raw_offset as usize;
    out[text_raw..text_raw + text.len()].copy_from_slice(&text);
    out[rdata_raw..rdata_raw + rdata.len()].copy_from_slice(&rdata);
    out[rsrc_raw..rsrc_raw + rsrc.len()].copy_from_slice(&rsrc);
    out
}

pub fn generate_desktop_resources(output_directory: &Path, arch: &str) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(output_directory)?;
    let mut written = Vec::new();
    for name in ICON_DIRECTORIES {
        let groups = load_icon_directory(name)?;
        let entries = icon_resources(&groups);
        let image_name = name.strip_suffix(".dll").unwrap_or(name);
        let image_name = alloc::format!("{}.dll", image_name);
        let bytes = resource_pe(arch, &image_name, &entries);
        let path = output_directory.join(&image_name);
        fs::write(&path, bytes)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows7_icons_are_loaded_and_have_unique_ids() {
        for name in ICON_DIRECTORIES {
            let groups = load_icon_directory(name).expect("icons load");
            assert!(!groups.is_empty());
            let mut ids = groups.iter().map(|group| group.id).collect::<Vec<_>>();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), groups.len());
        }
    }

    #[test]
    fn windows7_icon_catalogs_match_the_original_resource_counts() {
        let cases = [("imageres", 2406, 219), ("shell32", 2338, 306)];
        for (name, icon_count, group_count) in cases {
            let groups = load_icon_directory(name).expect("icons load");
            assert_eq!(groups.len(), group_count);
            assert_eq!(
                groups.iter().map(|group| group.images.len()).sum::<usize>(),
                icon_count
            );
        }
    }

    #[test]
    fn generated_resource_dlls_have_real_resource_directories() {
        for arch in ["x86_64", "aarch64", "riscv64", "loongarch64"] {
            for name in ICON_DIRECTORIES {
                let groups = load_icon_directory(name).expect("icons load");
                let entries = icon_resources(&groups);
                let image = resource_pe(arch, name, &entries);
                assert!(image.starts_with(b"MZ"));
                let pe_offset = u32::from_le_bytes(image[0x3C..0x40].try_into().unwrap()) as usize;
                assert_eq!(&image[pe_offset..pe_offset + 4], b"PE\x00\x00");
                let machine =
                    u16::from_le_bytes(image[pe_offset + 4..pe_offset + 6].try_into().unwrap());
                assert_eq!(machine, machine_for_arch(arch));
                let optional_header = pe_offset + 4 + 20;
                let resource_size = u32::from_le_bytes(
                    image[optional_header + 0x80..optional_header + 0x84]
                        .try_into()
                        .unwrap(),
                );
                assert!(resource_size > 0);
            }
        }
    }

    #[test]
    fn named_group_icon_entries_are_counted_separately() {
        let entries = icon_resources(&load_icon_directory("imageres").expect("icons load"));
        let section = resource_section(&entries);
        let icon_count = entries
            .iter()
            .filter(|entry| entry.type_id == RT_ICON)
            .count();
        let root_directory_size = RESOURCE_DIRECTORY_SIZE + 2 * RESOURCE_ENTRY_SIZE;
        let icon_directory_size = RESOURCE_DIRECTORY_SIZE + icon_count * RESOURCE_ENTRY_SIZE;
        let group_header = root_directory_size + icon_directory_size;
        let named_count = u16::from_le_bytes(
            section[group_header + 12..group_header + 14]
                .try_into()
                .unwrap(),
        );
        let id_count = u16::from_le_bytes(
            section[group_header + 14..group_header + 16]
                .try_into()
                .unwrap(),
        );
        assert_eq!(named_count, 2);
        assert_eq!(
            usize::from(id_count),
            entries
                .iter()
                .filter(|entry| entry.type_id == RT_GROUP_ICON && entry.name.is_none())
                .count()
        );
    }
}

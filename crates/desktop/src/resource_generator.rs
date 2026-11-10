//! Build-time generator for NT 6.1 desktop resource files.
//!
//! This module is intentionally host-only. It emits real PE32+ resource
//! DLLs plus a binary Aero theme descriptor under the selected build
//! directory. No generated artifact is committed to the source tree.

extern crate std;

use crate::resources::ImageResource;
use alloc::vec;
use alloc::vec::Vec;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const CLOSE_ICON: &[u8] = include_bytes!("../assets/7css/gui/icon/close.png");
const MINIMIZE_ICON: &[u8] = include_bytes!("../assets/7css/gui/icon/minimize.png");
const MAXIMIZE_ICON: &[u8] = include_bytes!("../assets/7css/gui/icon/maximize.png");
const RESTORE_ICON: &[u8] = include_bytes!("../assets/7css/gui/icon/restore.png");
const COMPUTER_ICON: &[u8] = include_bytes!("../assets/imageres/computer.png");
const FOLDER_ICON: &[u8] = include_bytes!("../assets/imageres/folder.png");
const DOCUMENT_ICON: &[u8] = include_bytes!("../assets/imageres/document.png");
const NOTEPAD_ICON: &[u8] = include_bytes!("../assets/imageres/notepad.png");
const CMD_ICON: &[u8] = include_bytes!("../assets/imageres/cmd.png");
const RECYCLE_BIN_ICON: &[u8] = include_bytes!("../assets/imageres/recycle-bin.png");
const NETWORK_ICON: &[u8] = include_bytes!("../assets/imageres/network.png");
const CONTROL_PANEL_ICON: &[u8] = include_bytes!("../assets/imageres/control-panel.png");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceBlob {
    pub resource: ImageResource,
    pub bytes: &'static [u8],
}

pub fn resource_blobs() -> Vec<ResourceBlob> {
    vec![
        ResourceBlob {
            resource: ImageResource::Computer,
            bytes: COMPUTER_ICON,
        },
        ResourceBlob {
            resource: ImageResource::Folder,
            bytes: FOLDER_ICON,
        },
        ResourceBlob {
            resource: ImageResource::Document,
            bytes: DOCUMENT_ICON,
        },
        ResourceBlob {
            resource: ImageResource::Notepad,
            bytes: NOTEPAD_ICON,
        },
        ResourceBlob {
            resource: ImageResource::Cmd,
            bytes: CMD_ICON,
        },
        ResourceBlob {
            resource: ImageResource::RecycleBin,
            bytes: RECYCLE_BIN_ICON,
        },
        ResourceBlob {
            resource: ImageResource::Network,
            bytes: NETWORK_ICON,
        },
        ResourceBlob {
            resource: ImageResource::ControlPanel,
            bytes: CONTROL_PANEL_ICON,
        },
        ResourceBlob {
            resource: ImageResource::WindowClose,
            bytes: CLOSE_ICON,
        },
        ResourceBlob {
            resource: ImageResource::WindowMinimize,
            bytes: MINIMIZE_ICON,
        },
        ResourceBlob {
            resource: ImageResource::WindowMaximize,
            bytes: MAXIMIZE_ICON,
        },
        ResourceBlob {
            resource: ImageResource::WindowRestore,
            bytes: RESTORE_ICON,
        },
    ]
}

const FILE_ALIGNMENT: u32 = 0x200;
const SECTION_ALIGNMENT: u32 = 0x1000;
const TEXT_RVA: u32 = SECTION_ALIGNMENT;
const RDATA_RVA: u32 = SECTION_ALIGNMENT * 2;
const RSRC_RVA: u32 = SECTION_ALIGNMENT * 3;

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

fn resource_section(blobs: &[ResourceBlob]) -> Vec<u8> {
    let type_directory_size = 0x18u32;
    let name_directory_size = blobs.len() as u32 * 8;
    let language_directory_size = blobs.len() as u32 * 0x18;
    let data_entries_offset = type_directory_size + name_directory_size + language_directory_size;
    let data_entry_size = blobs.len() as u32 * 0x10;
    let payload_offset = data_entries_offset + data_entry_size;

    let mut payload_rvas = Vec::new();
    let mut cursor = payload_offset;
    for blob in blobs {
        payload_rvas.push(cursor);
        cursor += blob.bytes.len() as u32;
    }
    let section_size = align_up_u32(cursor, SECTION_ALIGNMENT);
    let mut section = vec![0u8; section_size as usize];

    put_u32(&mut section, 0x00, 0);
    put_u32(&mut section, 0x04, 0);
    put_u16(&mut section, 0x08, 1);
    put_u16(&mut section, 0x0A, 0);
    put_u16(&mut section, 0x0C, 0);
    put_u16(&mut section, 0x0E, 1);
    put_u32(&mut section, 0x10, 3);
    put_u32(&mut section, 0x14, 0x8000_0018);

    for (index, blob) in blobs.iter().enumerate() {
        let entry_offset = 0x18 + index as u32 * 8;
        let language_directory_offset = 0x18 + name_directory_size + index as u32 * 0x18;
        put_u32(
            &mut section,
            entry_offset as usize,
            blob.resource.resource_id() as u32,
        );
        put_u32(
            &mut section,
            entry_offset as usize + 4,
            language_directory_offset | 0x8000_0000,
        );

        put_u32(
            &mut section,
            entry_offset as usize,
            blob.resource.resource_id() as u32,
        );
        put_u32(&mut section, language_directory_offset as usize, 0x409);
        put_u32(
            &mut section,
            language_directory_offset as usize + 4,
            data_entries_offset + index as u32 * 0x10,
        );

        let data_entry_offset = data_entries_offset + index as u32 * 0x10;
        put_u32(
            &mut section,
            data_entry_offset as usize,
            RSRC_RVA + payload_rvas[index],
        );
        put_u32(
            &mut section,
            data_entry_offset as usize + 4,
            blob.bytes.len() as u32,
        );
        put_u16(&mut section, data_entry_offset as usize + 8, 0);
        put_u16(&mut section, data_entry_offset as usize + 10, 0);

        let payload_start = payload_rvas[index] as usize;
        let bytes = blob.bytes;
        section[payload_start..payload_start + bytes.len()].copy_from_slice(bytes);
    }

    section
}

fn resource_pe(arch: &str, image_name: &str) -> Vec<u8> {
    let text = [0xC3u8, 0xC3, 0xC3, 0xC3];
    let blobs = resource_blobs();
    let rsrc = resource_section(&blobs);

    let mut export_name = [0u8; 32];
    let image_bytes = image_name.as_bytes();
    export_name[..image_bytes.len()].copy_from_slice(image_bytes);
    export_name[image_bytes.len()] = 0;
    let export_name_len = export_name
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(image_bytes.len() + 1);
    let theme_data = if image_name == "aero.msstyles" {
        Some(aero_theme_payload())
    } else {
        None
    };
    let theme_offset = if theme_data.is_some() { 0x100 } else { 0 };
    let rdata_size = 0x100 + theme_data.as_ref().map_or(0, |data| data.len());
    let mut rdata = vec![0u8; rdata_size];
    if let Some(data) = theme_data {
        rdata[theme_offset..theme_offset + data.len()].copy_from_slice(&data);
    }
    put_u32(&mut rdata, 0x00, 0);
    put_u32(&mut rdata, 0x04, 0);
    put_u16(&mut rdata, 0x08, 6);
    put_u16(&mut rdata, 0x0A, 1);
    let dll_name_offset = 0x0D0;
    rdata[dll_name_offset..dll_name_offset + export_name_len]
        .copy_from_slice(&export_name[..export_name_len]);
    put_u32(&mut rdata, 0x0C, RDATA_RVA + dll_name_offset as u32);
    put_u32(&mut rdata, 0x0C + 4, 1);
    put_u32(&mut rdata, 0x0C + 8, 0);
    put_u32(&mut rdata, 0x18, 2);
    put_u32(&mut rdata, 0x1C, 2);
    put_u32(&mut rdata, 0x20, RDATA_RVA + 0x080);
    put_u32(&mut rdata, 0x24, RDATA_RVA + 0x090);
    put_u32(&mut rdata, 0x28, RDATA_RVA + 0x0A0);
    put_u32(&mut rdata, 0x080, TEXT_RVA);
    put_u32(&mut rdata, 0x084, TEXT_RVA + 4);
    put_u32(&mut rdata, 0x090, RDATA_RVA + 0x0B0);
    put_u32(&mut rdata, 0x094, RDATA_RVA + 0x0BB);
    put_u16(&mut rdata, 0x0A0, 0);
    put_u16(&mut rdata, 0x0A2, 1);
    rdata[0x0B0..0x0B8].copy_from_slice(b"DllMain\x00");
    rdata[0x0BB..0x0CB].copy_from_slice(b"ResourceQuery\x00\x00\x00");

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
    put_u16(&mut out, coff + 0x02, 3);
    put_u16(&mut out, coff + 0x10, 240);
    put_u16(&mut out, coff + 0x12, 0x2022);

    let optional = coff + 20;
    put_u16(&mut out, optional, 0x020B);
    put_u16(&mut out, optional + 0x02, 14);
    put_u32(&mut out, optional + 0x06, text_raw_size);
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
    put_u16(&mut out, optional + 0x44, 0x000E);
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
    put_u32(&mut out, data_directory + 8, RSRC_RVA);
    put_u32(&mut out, data_directory + 12, rsrc.len() as u32);

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

fn aero_theme_payload() -> Vec<u8> {
    let mut theme = Vec::new();
    theme.extend_from_slice(b"AERO7SP1");
    theme.extend_from_slice(&6u16.to_le_bytes());
    theme.extend_from_slice(&1u16.to_le_bytes());
    theme.extend_from_slice(&7601u32.to_le_bytes());
    theme.extend_from_slice(&[0x11, 0x1A, 0x2C, 0x40, 0x55, 0x66]);
    theme.extend_from_slice(b"windows7/aero/basic");
    theme
}

pub fn generate_desktop_resources(output_directory: &Path, arch: &str) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(output_directory)?;
    let mut written = Vec::new();
    for (name, bytes) in [
        ("imageres.dll", resource_pe(arch, "imageres.dll")),
        ("shell32.dll", resource_pe(arch, "shell32.dll")),
        ("aero.msstyles", resource_pe(arch, "aero.msstyles")),
    ] {
        let path = output_directory.join(name);
        fs::write(&path, bytes)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_resources_are_real_pe_images() {
        for arch in ["x86_64", "aarch64", "riscv64", "loongarch64"] {
            let blobs = resource_blobs();
            assert_eq!(blobs.len(), 12);
            for blob in &blobs {
                assert!(blob.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
            }
            let image = resource_pe(arch, "imageres.dll");
            assert!(image.starts_with(b"MZ"));
            let pe_offset = u32::from_le_bytes(image[0x3C..0x40].try_into().unwrap()) as usize;
            assert_eq!(&image[pe_offset..pe_offset + 4], b"PE\x00\x00");
            let machine =
                u16::from_le_bytes(image[pe_offset + 4..pe_offset + 6].try_into().unwrap());
            assert_eq!(machine, machine_for_arch(arch));
            let optional_header = pe_offset + 4 + 20;
            let resource_size = u32::from_le_bytes(
                image[optional_header + 0x78..optional_header + 0x7c]
                    .try_into()
                    .unwrap(),
            );
            assert!(resource_size > 0);
        }
    }

    #[test]
    fn theme_is_real_pe_with_aero_payload() {
        let theme = resource_pe("x86_64", "aero.msstyles");
        assert!(theme.starts_with(b"MZ"));
        assert!(theme
            .windows(8)
            .any(|window| window.starts_with(b"AERO7SP1")));
        assert!(theme.len() > 0x1000);
    }
}

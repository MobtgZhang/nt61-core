use alloc::{collections::BTreeMap, string::String, vec::Vec};

use super::{
    parse_headers, read_data_directory_pe32, read_data_directory_pe32plus, ImageDatabase,
    IMAGE_ORDINAL_FLAG64,
};

fn bytes<const N: usize>(image: &[u8], offset: usize) -> Option<[u8; N]> {
    image.get(offset..offset.checked_add(N)?)?.try_into().ok()
}

fn dword(image: &[u8], offset: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes(image, offset)?) as usize)
}

fn string(image: &[u8], offset: usize) -> Option<&str> {
    let tail = image.get(offset..)?;
    let end = tail.iter().position(|&byte| byte == 0)?;
    core::str::from_utf8(&tail[..end]).ok()
}

fn directory(image: &[u8], optional: usize, pe32_plus: bool, index: usize) -> (usize, usize) {
    let (rva, size) = if pe32_plus {
        read_data_directory_pe32plus(image, optional, index)
    } else {
        read_data_directory_pe32(image, optional, index)
    };
    (rva as usize, size as usize)
}

pub(super) fn bind_imports(image: &mut [u8], database: &ImageDatabase) -> Result<(), &'static str> {
    prepare_imports(image, database)?.apply(image);
    Ok(())
}

pub(super) struct ImportBindings {
    width: usize,
    writes: Vec<(usize, u64)>,
}

impl ImportBindings {
    pub(super) fn apply(self, image: &mut [u8]) {
        for (offset, address) in self.writes {
            image[offset..offset + self.width]
                .copy_from_slice(&address.to_le_bytes()[..self.width]);
        }
    }
}

pub(super) fn prepare_imports(
    image: &[u8],
    database: &ImageDatabase,
) -> Result<ImportBindings, &'static str> {
    let (_, _, magic, optional) = parse_headers(image).ok_or("invalid mapped PE headers")?;
    let pe32_plus = magic == 0x20b;
    let width = if pe32_plus { 8 } else { 4 };
    let (start, size) = directory(image, optional, pe32_plus, 1);
    let writes = if start == 0 && size == 0 {
        Vec::new()
    } else {
        import_writes(image, database, start, size, pe32_plus)
            .ok_or("invalid or unresolved PE imports")?
    };
    Ok(ImportBindings { width, writes })
}

fn import_writes(
    image: &[u8],
    database: &ImageDatabase,
    start: usize,
    size: usize,
    pe32_plus: bool,
) -> Option<Vec<(usize, u64)>> {
    if start == 0 {
        return None;
    }
    let descriptors = image.get(start..start.checked_add(size)?)?;
    let (width, ordinal_flag) = if pe32_plus {
        (8, IMAGE_ORDINAL_FLAG64)
    } else {
        (4, 1 << 31)
    };
    let mut writes = Vec::new();
    for descriptor in descriptors.chunks_exact(20) {
        if descriptor == [0; 20] {
            return Some(writes);
        }
        let name_rva = dword(descriptor, 12)?;
        let mut address_slot = dword(descriptor, 16)?;
        if name_rva == 0 || address_slot == 0 {
            return None;
        }
        let dll = string(image, name_rva)?;
        let mut lookup = dword(descriptor, 0)?;
        if lookup == 0 {
            lookup = address_slot;
        }
        loop {
            let entry = if pe32_plus {
                u64::from_le_bytes(bytes(image, lookup)?)
            } else {
                dword(image, lookup)? as u64
            };
            if entry == 0 {
                break;
            }
            let end = address_slot.checked_add(width)?;
            if end > image.len() || writes.len() >= image.len() / width {
                return None;
            }
            let address = if entry & ordinal_flag != 0 {
                database.lookup_ordinal(dll, u16::try_from(entry ^ ordinal_flag).ok()?)?
            } else {
                let hint = u32::try_from(entry).ok()? as usize;
                database.lookup(dll, string(image, hint.checked_add(2)?)?)?
            };
            if !pe32_plus && address > u64::from(u32::MAX) {
                return None;
            }
            writes.push((address_slot, address));
            address_slot = end;
            lookup = lookup.checked_add(width)?;
        }
    }
    None
}

#[derive(Default)]
pub(super) struct ImageExports {
    ordinal_base: usize,
    slots: Vec<Option<ExportTarget>>,
    names: BTreeMap<String, usize>,
}

pub(super) enum ExportTarget {
    Address(u64),
    Forwarder(String),
}

#[derive(Clone, Copy)]
pub(super) enum ExportQuery<'a> {
    Name(&'a str),
    Ordinal(u16),
}

impl ImageExports {
    pub(super) fn parse(image: &[u8], load_base: u64) -> Option<Self> {
        let (_, _, magic, optional) = parse_headers(image)?;
        let image_size = dword(image, optional + 0x38)?;
        let (start, size) = directory(image, optional, magic == 0x20b, 0);
        if start == 0 || size < 40 {
            return None;
        }
        let end = start.checked_add(size)?;
        let export = image.get(start..end)?;
        let ordinal_base = dword(export, 16)?;
        let function_count = dword(export, 20)?;
        let name_count = dword(export, 24)?;
        if function_count > usize::from(u16::MAX) + 1 {
            return None;
        }
        let eat = dword(export, 28)?;
        let names_table = dword(export, 32)?;
        let ordinals = dword(export, 36)?;
        let functions = image.get(eat..eat.checked_add(function_count * 4)?)?;
        let names = image.get(names_table..names_table.checked_add(name_count.checked_mul(4)?)?)?;
        let ordinals = image.get(ordinals..ordinals.checked_add(name_count.checked_mul(2)?)?)?;
        let slots = functions
            .chunks_exact(4)
            .map(|slot| {
                let rva = dword(slot, 0)?;
                if rva == 0 || rva >= image_size {
                    return None;
                }
                if (start..end).contains(&rva) {
                    let target = string(&image[..end], rva)?;
                    return Some(ExportTarget::Forwarder(String::from(target)));
                }
                load_base.checked_add(rva as u64).map(ExportTarget::Address)
            })
            .collect();
        let mut exports = Self {
            ordinal_base,
            slots,
            names: BTreeMap::new(),
        };
        for (name, ordinal) in names.chunks_exact(4).zip(ordinals.chunks_exact(2)) {
            let index = usize::from(u16::from_le_bytes(ordinal.try_into().ok()?));
            let Some(Some(_)) = exports.slots.get(index) else {
                continue;
            };
            exports
                .names
                .insert(String::from(string(image, dword(name, 0)?)?), index);
        }
        Some(exports)
    }

    pub(super) fn get(&self, query: ExportQuery<'_>) -> Option<&ExportTarget> {
        let index = match query {
            ExportQuery::Name(name) => *self.names.get(name)?,
            ExportQuery::Ordinal(ordinal) => usize::from(ordinal).checked_sub(self.ordinal_base)?,
        };
        self.slots.get(index)?.as_ref()
    }
}

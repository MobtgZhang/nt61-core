//! Bounded PE resources in executable, DLL and data-only MUI mappings.
use super::*;
use super::super::{u32_at, MappedImage, ModuleMetadata, Region, READ};
use alloc::collections::BTreeSet;

const INVALID_DATA: u32 = 13;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Resource {
    pub owner: u64,
    info: u64,
    data: u64,
    size: usize,
}

enum Name<'a> {
    Id(u16),
    Text(&'a str),
}

fn write(memory: &mut dyn GuestMemory, address: u64, bytes: &[u8]) -> Result<(), ApiError> {
    if !memory.probe_write(address, bytes.len()) || !memory.write_virt(address, bytes) {
        return Err(NOACCESS.into());
    }
    Ok(())
}

struct Resources<'a> {
    region: &'a Region,
    root: usize,
    size: usize,
}

impl Resources<'_> {
    fn data(&self, offset: usize, length: usize) -> Result<&[u8], ApiError> {
        if offset.checked_add(length).filter(|end| *end <= self.size).is_none() {
            return Err(INVALID_DATA.into());
        }
        self.image_data(self.root + offset, length)
    }

    fn image_data(&self, rva: usize, length: usize) -> Result<&[u8], ApiError> {
        if length > MAX_BUFFER {
            return Err(INVALID_DATA.into());
        }
        let address = self.region.base.checked_add(rva as u64).ok_or(INVALID_DATA)?;
        let range = self.region.range(address, length, READ).ok_or(INVALID_DATA)?;
        Ok(&self.region.bytes[range])
    }

    fn entries(&self, offset: usize, include_names: bool) -> Result<Vec<(u32, u32)>, ApiError> {
        let header = self.data(offset, 16)?;
        let named = u16::from_le_bytes([header[12], header[13]]) as usize;
        let ids = u16::from_le_bytes([header[14], header[15]]) as usize;
        let count = named + ids;
        let bytes = self.data(offset + 16, count * 8)?;
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, entry) in bytes.chunks_exact(8).enumerate() {
            let name = u32::from_le_bytes(entry[..4].try_into().map_err(|_| INVALID_DATA)?);
            let target = u32::from_le_bytes(entry[4..].try_into().map_err(|_| INVALID_DATA)?);
            if (name & 0x80000000 != 0) != (index < named) {
                return Err(INVALID_DATA.into());
            }
            if index >= named || include_names {
                if (index >= named && name > u16::MAX as u32)
                    || !seen.insert(name) {
                    return Err(INVALID_DATA.into());
                }
                result.push((name, target));
            }
        }
        Ok(result)
    }

    fn directory(&self, offset: usize, id: u32, missing: u32) -> Result<usize, ApiError> {
        let target = self.entries(offset, false)?.iter().find(|(name, _)| *name == id)
            .map(|(_, target)| *target).ok_or(missing)?;
        if target & 0x80000000 == 0 { return Err(INVALID_DATA.into()); }
        Ok((target & 0x7fffffff) as usize)
    }

    fn named_directory(&self, offset: usize, name: &Name<'_>, missing: u32) -> Result<usize, ApiError> {
        let requested = match name {
            Name::Id(id) => return self.directory(offset, u32::from(*id), missing),
            Name::Text(name) => name,
        };
        for (name, target) in self.entries(offset, true)? {
            if name & 0x80000000 == 0 { continue; }
            let start = (name & 0x7fffffff) as usize;
            let length = u16::from_le_bytes(self.data(start, 2)?.try_into().map_err(|_| INVALID_DATA)?);
            let encoded = self.data(start + 2, usize::from(length) * 2)?;
            let units = encoded.chunks_exact(2).map(|unit| u16::from_le_bytes([unit[0], unit[1]]));
            let name = core::char::decode_utf16(units).collect::<Result<String, _>>().map_err(|_| INVALID_DATA)?;
            let equal = name.chars().flat_map(char::to_uppercase).eq(requested.chars().flat_map(char::to_uppercase));
            if equal {
                if target & 0x80000000 == 0 { return Err(INVALID_DATA.into()); }
                return Ok((target & 0x7fffffff) as usize);
            }
        }
        Err(missing.into())
    }

    fn leaf(&self, target: u32) -> Result<(usize, &[u8]), ApiError> {
        if target & 0x80000000 != 0 { return Err(INVALID_DATA.into()); }
        let entry = self.data(target as usize, 16)?;
        let rva = u32::from_le_bytes(entry[..4].try_into().map_err(|_| INVALID_DATA)?) as usize;
        let size = u32::from_le_bytes(entry[4..8].try_into().map_err(|_| INVALID_DATA)?) as usize;
        Ok((rva, self.image_data(rva, size)?))
    }

    fn mui(&self) -> Result<Option<Mui>, ApiError> {
        let mut directory = None;
        for (name, target) in self.entries(0, true)? {
            if name & 0x80000000 == 0 { continue; }
            let offset = (name & 0x7fffffff) as usize;
            if self.data(offset, 2)? == [3, 0] && self.data(offset + 2, 6)? == b"M\0U\0I\0" {
                if directory.is_some() || target & 0x80000000 == 0 {
                    return Err(INVALID_DATA.into());
                }
                directory = Some((target & 0x7fffffff) as usize);
            }
        }
        let Some(directory) = directory else { return Ok(None); };
        let name = self.directory(directory, 1, 1814)?;
        let languages = self.entries(name, false)?;
        let [(_, target)] = languages.as_slice() else { return Err(INVALID_DATA.into()); };
        let (_, data) = self.leaf(*target)?;
        Mui::parse(data).map(Some)
    }

    fn find(&self, kind: &Name<'_>, name: &Name<'_>, language: u16, exact: bool) -> Result<u32, ApiError> {
        let kind = self.named_directory(0, kind, 1813)?;
        let block = self.named_directory(kind, name, 1814)?;
        let languages = self.entries(block, false)?;
        if exact {
            return languages.iter().find(|(lang, _)| *lang == u32::from(language))
                .map(|(_, target)| *target).ok_or(1815u32.into());
        }
        if let Some((_, target)) = languages.iter().find(|(lang, _)| *lang == 0)
            .or_else(|| languages.iter().find(|(lang, _)| *lang == u32::from(language))) {
            Ok(*target)
        } else if languages.len() == 1 { Ok(languages[0].1) }
        else if languages.is_empty() { Err(1815u32.into()) }
        else { Err(ApiError::Unsupported) }
    }

    fn string(&self, id: u16, language: u16, exact: bool) -> Result<(u64, Vec<u8>), ApiError> {
        let target = self.find(&Name::Id(6), &Name::Id(id / 16 + 1), language, exact)?;
        let (rva, bytes) = self.leaf(target)?;
        let mut offset = 0usize;
        for index in 0..=id % 16 {
            let length = bytes.get(offset..offset + 2).ok_or(INVALID_DATA)?;
            let length = u16::from_le_bytes([length[0], length[1]]) as usize;
            offset += 2;
            let end = offset.checked_add(length * 2).ok_or(INVALID_DATA)?;
            let text = bytes.get(offset..end).ok_or(INVALID_DATA)?;
            if index == id % 16 {
                return Ok((self.region.base + rva as u64 + offset as u64, text.to_vec()));
            }
            offset = end;
        }
        Err(INVALID_DATA.into())
    }
}

struct Mui {
    kind: u32,
    checksums: [u8; 32],
    language: Option<u16>,
    external_fallback: Option<u16>,
}

impl Mui {
    fn parse(data: &[u8]) -> Result<Self, ApiError> {
        let field = |offset| u32_at(data, offset).map_err(|_| INVALID_DATA);
        let size = field(4)? as usize;
        if field(0)? != 0xfecd_fecd || field(8)? != 0x0001_0000 || size < 0x84 || size > data.len() {
            return Err(INVALID_DATA.into());
        }
        let language = |offset| -> Result<Option<u16>, ApiError> {
            let start = field(offset)? as usize;
            let length = field(offset + 4)? as usize;
            if start == 0 && length == 0 { return Ok(None); }
            let end = start.checked_add(length).ok_or(INVALID_DATA)?;
            if start < 0x84 || end > size || length < 2 || length % 2 != 0 {
                return Err(INVALID_DATA.into());
            }
            let Some(text) = data[start..end].strip_suffix(&[0, 0]) else {
                return Err(INVALID_DATA.into());
            };
            Ok(nls::UI_LANGUAGES.iter().find(|(_, tag, _)| text.len() == tag.len() * 2
                && text.chunks_exact(2).zip(tag.bytes()).all(|(unit, byte)|
                    unit[1] == 0 && unit[0].eq_ignore_ascii_case(&byte)))
                .map(|(language, _, _)| *language))
        };
        Ok(Self {
            kind: field(0x10)? & 0xf,
            checksums: data[0x1c..0x3c].try_into().map_err(|_| INVALID_DATA)?,
            language: language(0x74)?,
            external_fallback: if field(0x18)? == 2 { language(0x7c)? } else { None },
        })
    }
}

fn resources(region: &Region, (root, size): (u32, u32)) -> Result<Resources<'_>, ApiError> {
    if root == 0 || size == 0 {
        return Err(1812u32.into());
    }
    if (root as usize).checked_add(size as usize).filter(|end| *end <= region.bytes.len()).is_none() {
        return Err(INVALID_DATA.into());
    }
    Ok(Resources { region, root: root as usize, size: size as usize })
}

fn module_resources(image: &GuestImage, module: ModuleMetadata) -> Result<Resources<'_>, ApiError> {
    let region = image.memory.regions.iter().find(|region| region.base == module.base)
        .ok_or(INVALID_DATA)?;
    resources(region, module.directories[2])
}

pub(super) fn file_version(file: &[u8], language: u16) -> Result<Vec<u8>, ApiError> {
    let mapped = MappedImage::parse(file, true).map_err(|_| 193u32)?;
    let resources = resources(&mapped.region, mapped.directories[2])?;
    let target = resources.find(&Name::Id(16), &Name::Id(1), language, false)?;
    Ok(resources.leaf(target)?.1.to_vec())
}

fn satellite(image: &mut GuestImage, services: &mut dyn GuestServices, owner: ModuleMetadata,
    mui: &Mui, language: u16) -> Result<Option<ModuleMetadata>, ApiError> {
    if let Some((_, _, metadata)) = image.resource_satellites.iter()
        .find(|(base, lang, _)| *base == owner.base && *lang == language) {
        return Ok(Some(*metadata));
    }
    let Some((_, tag, _)) = nls::UI_LANGUAGES.iter().find(|(id, _, _)| *id == language) else {
        return Ok(None);
    };
    let owner_path = if owner.base == image.image_base {
        Some(image.process_image_path.as_str())
    } else {
        image.win32.module_paths.iter().find(|(base, _)| *base == owner.base)
            .map(|(_, path)| path.as_str())
    };
    let path = if let Some((directory, name)) = owner_path.and_then(|path| path.rsplit_once(['\\', '/'])) {
        alloc::format!("{directory}\\{tag}\\{name}.mui")
    } else if owner.base == image.image_base {
        return Ok(None);
    } else {
        let Some((name, _)) = image.win32.modules.iter().find(|(_, base)| *base == owner.base) else {
            return Ok(None);
        };
        alloc::format!("{tag}\\{name}.mui")
    };
    let file = if owner_path.is_none() && owner.system_library_search {
        services.read_system_library(&path, owner.mode)
    } else {
        services.read_library(&path, owner.mode)
    };
    let file = match file {
        Ok(bytes) => bytes,
        Err(0xc000_0034 | 0xc000_003a | 0xc000_000f | 0xc000_0135) => return Ok(None),
        Err(_) => return Err(INVALID_DATA.into()),
    };
    let mapped = MappedImage::parse(&file, true).map_err(|_| INVALID_DATA)?;
    let size = mapped.region.bytes.len();
    if mapped.entry != 0 || size > MAX_BUFFER
        || mapped.directories[2].0 == 0 || mapped.directories[1].0 != 0
        || mapped.directories[9].0 != 0 || mapped.directories[13].0 != 0
        || mapped.region.permissions.iter().any(|flags| flags & EXECUTE != 0)
    {
        return Err(INVALID_DATA.into());
    }
    let Some(header) = resources(&mapped.region, mapped.directories[2])?.mui()? else { return Ok(None); };
    if header.kind != 2 || header.language != Some(language) || header.checksums != mui.checksums {
        return Ok(None);
    }
    image.resource_satellites.try_reserve(1).map_err(|_| 8u32)?;
    let base = allocate(image, 0, size as u64, READ)?;
    let region = image.memory.regions.last_mut().ok_or(INVALID_DATA)?;
    region.bytes.copy_from_slice(&mapped.region.bytes);
    for ((original, permissions), protection) in mapped.region.permissions.iter()
        .zip(&mut region.permissions).zip(&mut region.protections) {
        *permissions &= *original;
        *protection = win_protection(*permissions);
    }
    image.win32.allocations.pop();
    let mut metadata = ModuleMetadata::from_image(&mapped);
    metadata.base = base;
    image.resource_satellites.push((owner.base, language, metadata));
    Ok(Some(metadata))
}

fn select<T>(image: &mut GuestImage, services: &mut dyn GuestServices, metadata: ModuleMetadata,
    explicit: u16, lookup: impl Fn(&Resources<'_>, u16, bool) -> Result<T, ApiError>) -> Result<T, ApiError> {
    let resources = module_resources(image, metadata)?;
    let primary = if explicit != 0 { explicit } else {
        image.win32.local.ui_languages.first().copied().unwrap_or(image.win32.ui_language)
    };
    let local = lookup(&resources, primary, explicit != 0);
    let Some(mui) = resources.mui()?.filter(|mui| mui.kind == 1) else { return local; };
    let mut languages = [0; nls::UI_LANGUAGES.len() + 2];
    if explicit != 0 {
        languages[0] = explicit;
    } else {
        let preferred = &image.win32.local.ui_languages;
        languages[..preferred.len()].copy_from_slice(preferred);
        languages[preferred.len()] = image.win32.ui_language;
        languages[preferred.len() + 1] = mui.external_fallback.unwrap_or_default();
    }
    for (index, language) in languages.into_iter().enumerate() {
        if language == 0 || languages[..index].contains(&language) { continue; }
        if let Some(satellite) = satellite(image, services, metadata, &mui, language)? {
            match lookup(&module_resources(image, satellite)?, language, explicit != 0) {
                Err(ApiError::Windows(1812..=1815)) => continue,
                result => return result,
            }
        }
    }
    local
}

fn owner(image: &GuestImage, base: u64) -> Result<ModuleMetadata, ApiError> {
    let base = if base == 0 { image.image_base } else { base };
    image.module_metadata.iter().find(|module| module.base == base).copied()
        .ok_or(ApiError::Windows(126))
}

fn name<'a>(value: u64, text: &'a str) -> Result<Name<'a>, ApiError> {
    if value <= u16::MAX as u64 { return Ok(Name::Id(value as u16)); }
    if let Some(number) = text.strip_prefix('#') {
        return number.parse::<u16>().map(Name::Id).map_err(|_| INVALID_PARAMETER.into());
    }
    Ok(Name::Text(text))
}

pub(super) fn template(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    module: u64,
    kind: u16,
    item: u64,
    limit: usize,
) -> Result<Vec<u8>, ApiError> {
    let metadata = owner(image, module)?;
    let text = if item > u16::MAX as u64 {
        string(&mut image.memory, item, true)?
    } else {
        String::new()
    };
    let item = name(item, &text)?;
    template_for_name(image, services, metadata, kind, item, limit)
}

pub(super) fn menu_template(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    module: u64,
    resource: Result<u16, &str>,
) -> Result<Vec<u8>, ApiError> {
    let metadata = owner(image, module)?;
    let item = match resource {
        Ok(id) => Name::Id(id),
        Err(text) => name(0x10000, text)?,
    };
    template_for_name(image, services, metadata, 4, item, 65536)
}

fn template_for_name(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    metadata: ModuleMetadata,
    kind: u16,
    item: Name<'_>,
    limit: usize,
) -> Result<Vec<u8>, ApiError> {
    select(image, services, metadata, 0, |resources, language, exact| {
        let offset = resources.find(&Name::Id(kind), &item, language, exact)?;
        let (_, data) = resources.leaf(offset)?;
        if data.len() > limit {
            return Err(INVALID_DATA.into());
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(data.len()).map_err(|_| 8u32)?;
        bytes.extend_from_slice(data);
        Ok(bytes)
    })
}

pub(super) struct ImageFrame {
    pub width: u16,
    pub height: u16,
    pub bytes: Vec<u8>,
}

pub(super) fn image_frame(
    image: &mut GuestImage,
    services: &mut dyn GuestServices,
    module: u64,
    item: u64,
    cursor: bool,
    desired: (u16, u16),
) -> Result<ImageFrame, ApiError> {
    let metadata = owner(image, module)?;
    let text = if item > u16::MAX as u64 {
        string(&mut image.memory, item, true)?
    } else {
        String::new()
    };
    let item = name(item, &text)?;
    let (group_type, frame_type, signature) = if cursor { (12, 1, 2) } else { (14, 3, 1) };
    select(image, services, metadata, 0, |resources, language, exact| {
        let offset = resources.find(&Name::Id(group_type), &item, language, exact)?;
        let (_, group) = resources.leaf(offset)?;
        let count = group.get(4..6).map(|field| u16::from_le_bytes([field[0], field[1]]) as usize)
            .ok_or(INVALID_DATA)?;
        if group[..4] != [0, 0, signature, 0] || count == 0 || count > 256
            || group.len() != 6 + count * 14
        {
            return Err(INVALID_DATA.into());
        }
        let dimension = |value: u8| if value == 0 { 256 } else { u16::from(value) };
        let read_word = |entry: &[u8], at| u16::from_le_bytes([entry[at], entry[at + 1]]);
        let entry = group[6..].chunks_exact(14).filter_map(|entry| {
            let (width, height) = if cursor {
                (read_word(entry, 0), read_word(entry, 2) / 2)
            } else {
                (dimension(entry[0]), dimension(entry[1]))
            };
            let size = u32_at(entry, 8).ok()? as usize;
            let distance = width.abs_diff(desired.0) as u32 + height.abs_diff(desired.1) as u32;
            let undersized = width < desired.0 || height < desired.1;
            (width != 0 && height != 0 && size <= MAX_BUFFER).then_some((
                (distance, undersized, core::cmp::Reverse(read_word(entry, 6))),
                width, height, read_word(entry, 12), size,
            ))
        }).min_by_key(|entry| entry.0).ok_or(INVALID_DATA)?;
        let (_, width, height, frame_id, size) = entry;
        let offset = resources.find(&Name::Id(frame_type), &Name::Id(frame_id), language, exact)?;
        let (_, data) = resources.leaf(offset)?;
        let bitmap = data.get(if cursor { 4.. } else { 0.. }).ok_or(INVALID_DATA)?;
        if data.len() != size || bitmap.len() < 12 {
            return Err(INVALID_DATA.into());
        }
        let valid_dimensions = if bitmap.starts_with(b"\x89PNG\r\n\x1a\n") {
            bitmap.get(12..24).is_some_and(|header| {
                header[..4] == *b"IHDR"
                    && header[4..8] == u32::from(width).to_be_bytes()
                    && header[8..12] == u32::from(height).to_be_bytes()
            })
        } else {
            match u32_at(bitmap, 0) {
                Ok(12) => read_word(bitmap, 4) == width
                    && u32::from(read_word(bitmap, 6)) == 2 * u32::from(height),
                Ok(header @ (40 | 52 | 56 | 108 | 124)) => {
                    (header as usize) <= bitmap.len()
                        && u32_at(bitmap, 4).ok() == Some(u32::from(width))
                        && u32_at(bitmap, 8).ok() == Some(2 * u32::from(height))
                }
                _ => false,
            }
        };
        if !valid_dimensions {
            return Err(INVALID_DATA.into());
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(data.len()).map_err(|_| 8u32)?;
        bytes.extend_from_slice(data);
        Ok(ImageFrame { width, height, bytes })
    })
}

pub(super) fn invoke(image: &mut GuestImage, api: &str, args: &[u64], services: &mut dyn GuestServices)
    -> Result<u64, ApiError> {
    match api {
        "FindResourceA" | "FindResourceW" | "FindResourceExA" | "FindResourceExW" => {
            let metadata = owner(image, args[0])?;
            let extended = matches!(api, "FindResourceExA" | "FindResourceExW");
            let (kind, item, language) = if extended { (args[1], args[2], args[3] as u16) }
                else { (args[2], args[1], 0) };
            let mut read_name = |pointer| -> Result<String, ApiError> {
                if pointer > u16::MAX as u64 { Ok(string(&mut image.memory, pointer, api.ends_with('W'))?) }
                else { Ok(String::new()) }
            };
            let kind_text = read_name(kind)?;
            let item_text = read_name(item)?;
            let kind = name(kind, &kind_text)?;
            let item = name(item, &item_text)?;
            let resource = select(image, services, metadata, language, |resources, language, exact| {
                let target = resources.find(&kind, &item, language, exact)?;
                let (rva, data) = resources.leaf(target)?;
                Ok(Resource {
                    owner: metadata.base,
                    info: resources.region.base + resources.root as u64 + u64::from(target),
                    data: resources.region.base + rva as u64,
                    size: data.len(),
                })
            })?;
            if !image.win32.resources.contains(&resource) {
                image.win32.resources.try_reserve(1).map_err(|_| 8u32)?;
                image.win32.resources.push(resource);
            }
            Ok(resource.info)
        }
        "LoadResource" | "SizeofResource" => {
            let module = owner(image, args[0])?;
            let resource = image.win32.resources.iter().find(|resource|
                resource.owner == module.base && resource.info == args[1]).ok_or(6u32)?;
            Ok(if api == "LoadResource" { resource.data } else { resource.size as u64 })
        }
        "LockResource" => image.win32.resources.iter().find(|resource| resource.data == args[0])
            .map(|resource| resource.data).ok_or(6u32.into()),
        _ => Err(ApiError::Unsupported),
    }
}

pub(super) fn load_string_w(image: &mut GuestImage, args: &[u64], services: &mut dyn GuestServices) -> Result<u64, ApiError> {
    let a = args;
    let capacity = a[3] as u32 as i32;
    if a[2] == 0 { return Err(INVALID_PARAMETER.into()); }
    // Negative INT sizes cannot describe a checked destination buffer.
    if capacity < 0 { return Err(INVALID_PARAMETER.into()); }
    let metadata = owner(image, a[0])?;
    let id = a[1] as u16;
    let loaded = select(image, services, metadata, 0, |resources, language, exact| resources.string(id, language, exact));
    let (pointer, text) = match loaded {
        Ok(value) => value,
        Err(e) => {
            if capacity > 0 { write(&mut image.memory, a[2], &[0, 0])?; }
            return Err(e);
        }
    };
    if capacity == 0 {
        let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
        write(&mut image.memory, a[2], &pointer.to_le_bytes()[..width])?;
        return Ok((text.len() / 2) as u64);
    }
    let length = (text.len() / 2).min(capacity as usize - 1);
    let mut output = text[..length * 2].to_vec();
    output.extend_from_slice(&[0, 0]);
    write(&mut image.memory, a[2], &output)?;
    Ok(length as u64)
}

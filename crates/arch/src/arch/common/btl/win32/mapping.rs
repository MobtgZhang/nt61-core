use super::{allocate, free, status_error, ApiError, DecodeMode, GuestImage, GuestServices, PAGE, READ, WRITE};
use alloc::vec::Vec;

const ALLOCATION_GRANULARITY: u64 = 0x10000;
const MAX_VIEW: u64 = 64 * 1024 * 1024;
const MAX_SECTIONS: usize = 128;
const MAX_VIEWS: usize = 128;
const READ_ONLY: u32 = 2;
const READ_WRITE: u32 = 4;
const WRITE_COPY: u32 = 8;

#[derive(Default)]
pub(super) struct State {
    sections: Vec<Section>,
    views: Vec<View>,
}

struct Section {
    id: u64,
    open: bool,
    file_backed: bool,
    size: u64,
    protection: u32,
}

struct View {
    section: u64,
    base: u64,
    length: usize,
    offset: u64,
    original: Option<Vec<u8>>,
}

impl State {
    pub(super) fn contains_view(&self, base: u64) -> bool {
        self.views.iter().any(|view| view.base == base)
    }

    fn prune(&mut self, services: &mut dyn GuestServices, id: u64) {
        if let Some(index) = self.sections.iter().position(|section| section.id == id && !section.open) {
            if !self.views.iter().any(|view| view.section == id) {
                self.sections.swap_remove(index);
                services.release_file_mapping(id);
            }
        }
    }
}

pub(in crate::arch::common::btl::pe) fn close(image: &mut GuestImage, services: &mut dyn GuestServices, id: u64) -> bool {
    let state = &mut image.win32.mappings;
    let Some(section) = state.sections.iter_mut().find(|section| section.id == id && section.open) else {
        return false;
    };
    section.open = false;
    state.prune(services, id);
    true
}

fn create(image: &mut GuestImage, services: &mut dyn GuestServices, args: &[u64]) -> Result<u64, u32> {
    let pseudo = if image.thread.context.mode == DecodeMode::X86 { u32::MAX.into() } else { u64::MAX };
    let file = (args[0] != pseudo).then_some(args[0]);
    let protection = args[2] as u32 & !0x0800_0000;
    if args[1] != 0 || args[5] != 0 || !matches!(protection, READ_ONLY | READ_WRITE | WRITE_COPY) {
        return Err(120);
    }
    let size = (u64::from(args[3] as u32) << 32) | u64::from(args[4] as u32);
    let state = &mut image.win32.mappings;
    if state.sections.len() >= MAX_SECTIONS { return Err(4); }
    state.sections.try_reserve(1).map_err(|_| 8u32)?;
    let (id, file_size) = services.retain_file_mapping(file, protection == READ_WRITE).map_err(status_error)?;
    let size = if size == 0 { file_size } else { size };
    let failure = if size == 0 { Some(if file.is_some() { 1006 } else { 87 }) }
        else if file.is_none() || size <= file_size { None }
        else { Some(if protection == READ_WRITE { 120 } else { 5 }) };
    if let Some(error) = failure {
        services.release_file_mapping(id);
        return Err(error);
    }
    if file.is_none() && size > MAX_VIEW {
        services.release_file_mapping(id);
        return Err(8);
    }
    state.sections.push(Section { id, open: true, file_backed: file.is_some(), size, protection });
    Ok(id)
}

fn map(image: &mut GuestImage, services: &mut dyn GuestServices, args: &[u64], requested: u64) -> Result<u64, u32> {
    let id = args[0];
    let state = &mut image.win32.mappings;
    let section = state.sections.iter().find(|section| section.id == id && section.open).ok_or(6u32)?;
    let (file_backed, size) = (section.file_backed, section.size);
    let protection = match args[1] as u32 {
        4 => READ_ONLY,
        2 | 6 | 0xf001f if section.protection == READ_WRITE => READ_WRITE,
        1 => WRITE_COPY,
        2 | 6 | 0xf001f => return Err(5),
        _ => return Err(120),
    };
    let offset = (u64::from(args[2] as u32) << 32) | u64::from(args[3] as u32);
    if !offset.is_multiple_of(ALLOCATION_GRANULARITY) || !requested.is_multiple_of(ALLOCATION_GRANULARITY) {
        return Err(1132);
    }
    let remaining = size.checked_sub(offset).ok_or(87u32)?;
    let length = if args[4] == 0 { remaining } else { args[4] };
    if length == 0 || length > remaining { return Err(87); }
    if length > MAX_VIEW || state.views.len() >= MAX_VIEWS { return Err(8); }
    if state.views.iter().any(|view| view.section == id) { return Err(120); }
    state.views.try_reserve(1).map_err(|_| 8u32)?;
    let length = length as usize;
    let snapshots = state.views.iter().filter_map(|view| view.original.as_ref()).map(Vec::len).sum::<usize>();
    let snapshot = file_backed && protection == READ_WRITE;
    if image.memory.mapped_bytes() + snapshots + length.div_ceil(PAGE) * PAGE
        + if snapshot { length } else { 0 } > super::MAX_PROCESS_MEMORY {
        return Err(8);
    }
    let mut original = if snapshot {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| 8u32)?;
        Some(bytes)
    } else { None };
    let permissions = if protection == READ_ONLY { READ } else { READ | WRITE };
    let base = allocate(image, requested, length as u64, permissions)?;
    let region = image.memory.regions.last_mut().filter(|region| region.base == base).ok_or(487u32)?;
    let loaded = (|| {
        if file_backed {
            for (index, chunk) in region.bytes[..length].chunks_mut(65536).enumerate() {
                let bytes = services.read_file_mapping(id, offset + (index * 65536) as u64, chunk.len()).map_err(status_error)?;
                if bytes.len() != chunk.len() { return Err(38u32); }
                chunk.copy_from_slice(&bytes);
                if let Some(original) = &mut original { original.extend_from_slice(&bytes); }
            }
        }
        region.protections.fill(protection);
        region.allocation_protection = protection;
        Ok(())
    })();
    if let Err(error) = loaded {
        free(image, base)?;
        return Err(error);
    }
    image.win32.allocations.pop();
    image.win32.mappings.views.push(View { section: id, base, length, offset, original });
    Ok(base)
}

fn flush(image: &mut GuestImage, services: &mut dyn GuestServices, address: u64, count: u64) -> Result<(), u32> {
    let view = image.win32.mappings.views.iter_mut().find(|view| {
        address.checked_sub(view.base).is_some_and(|offset| offset < view.length as u64)
    }).ok_or(487u32)?;
    let start = (address - view.base) as usize;
    let length = if count == 0 { view.length - start } else { usize::try_from(count).map_err(|_| 87u32)? };
    let end = start.checked_add(length).filter(|end| *end <= view.length).ok_or(87u32)?;
    let region = image.memory.regions.iter().find(|region| region.base == view.base).ok_or(487u32)?;
    if let Some(original) = &mut view.original {
        for page in start / PAGE..end.div_ceil(PAGE) {
            let first = page * PAGE;
            let last = (first + PAGE).min(view.length);
            let mut cursor = first;
            while cursor < last {
                if region.bytes[cursor] == original[cursor] {
                    cursor += 1;
                    continue;
                }
                let start = cursor;
                while cursor < last && region.bytes[cursor] != original[cursor] { cursor += 1; }
                let current = &region.bytes[start..cursor];
                let written = services.write_file_mapping(view.section, view.offset + start as u64, current).map_err(status_error)?;
                if written != current.len() { return Err(1117); }
                original[start..cursor].copy_from_slice(current);
            }
        }
    }
    Ok(())
}

fn unmap(image: &mut GuestImage, services: &mut dyn GuestServices, address: u64) -> Result<(), u32> {
    let index = image.win32.mappings.views.iter().position(|view| view.base == address).ok_or(487u32)?;
    flush(image, services, address, 0)?;
    let index_region = image.memory.regions.iter().position(|region| region.base == address).ok_or(487u32)?;
    image.memory.regions.swap_remove(index_region);
    let view = image.win32.mappings.views.swap_remove(index);
    image.win32.mappings.prune(services, view.section);
    Ok(())
}

pub(in crate::arch::common::btl::pe) fn finish(image: &mut GuestImage, services: &mut dyn GuestServices) -> Result<(), u32> {
    let mut failure = None;
    for index in 0..image.win32.mappings.views.len() {
        let base = image.win32.mappings.views[index].base;
        if let Err(error) = flush(image, services, base, 0) { failure.get_or_insert(error); }
    }
    for section in image.win32.mappings.sections.drain(..) {
        services.release_file_mapping(section.id);
    }
    failure.map_or(Ok(()), Err)
}

pub(super) fn invoke(image: &mut GuestImage, services: &mut dyn GuestServices, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    Ok(match name {
        "CreateFileMappingA" | "CreateFileMappingW" => create(image, services, args)?,
        "MapViewOfFile" => map(image, services, args, 0)?,
        "MapViewOfFileEx" => map(image, services, args, args[5])?,
        "FlushViewOfFile" => { flush(image, services, args[0], args[1])?; 1 }
        "UnmapViewOfFile" => { unmap(image, services, args[0])?; 1 }
        _ => return Err(ApiError::Unsupported),
    })
}

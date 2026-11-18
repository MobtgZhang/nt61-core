//! Bounded PE import/export linking into the isolated guest address space.
//! A resolved address names readable code or data in a same-ISA guest DLL;
//! instruction fetch independently enforces executable page permissions.

use super::{u16_at, u32_at, u64_at, GuestImage, LoadError, MappedImage, READ};
use crate::arch::common::btl::ir::DecodeMode;
use alloc::{format, string::String, vec::Vec};
use core::fmt;

const MAX_MODULES: usize = 128;
const MAX_DEPTH: usize = 64;
const MAX_RELOCATIONS: usize = 1_048_576;
const MAX_SYMBOLS: usize = 16_384;
const MAX_MAPPED_BYTES: usize = 128 * 1024 * 1024;

/// The resolver chooses a guest filesystem view, never a host DLL loader.
pub trait ImportResolver {
    fn read_library(&mut self, name: &str, mode: DecodeMode) -> Result<Vec<u8>, u32>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportSymbol {
    Name(String),
    Ordinal(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    Image(LoadError),
    Library {
        name: String,
        status: u32,
    },
    LibraryImage {
        name: String,
        error: LoadError,
    },
    WrongMachine {
        name: String,
    },
    MissingExport {
        module: String,
        symbol: ImportSymbol,
    },
    InitializationRequired {
        name: String,
    },
    AddressConflict {
        name: String,
    },
    LimitExceeded,
}

impl LinkError {
    /// Preserve the Windows loader error class at the command/process boundary.
    pub fn ntstatus(&self) -> u32 {
        match self {
            Self::Library { status, .. } => match status {
                0xc000_0034 | 0xc000_003a | 0xc000_000f => 0xc000_0135, // DLL_NOT_FOUND
                status => *status,
            },
            Self::MissingExport { .. } => 0xc000_0139, // ENTRYPOINT_NOT_FOUND
            Self::InitializationRequired { .. }
            | Self::Image(LoadError::TlsRequired | LoadError::ImportsRequired)
            | Self::LibraryImage {
                error: LoadError::TlsRequired | LoadError::ImportsRequired,
                ..
            } => 0xc000_0002,
            Self::LimitExceeded
            | Self::Image(LoadError::ImageTooLarge | LoadError::OutOfMemory)
            | Self::LibraryImage {
                error: LoadError::ImageTooLarge | LoadError::OutOfMemory,
                ..
            } => 0xc000_0017,
            _ => 0xc000_007b,
        }
    }
}
impl From<LoadError> for LinkError {
    fn from(error: LoadError) -> Self {
        Self::Image(error)
    }
}
impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Image(error) => write!(f, "PE image: {error:?}"),
            Self::Library { name, status } => {
                write!(f, "cannot read guest DLL {name} (0x{status:08X})")
            }
            Self::LibraryImage { name, error } => write!(f, "guest DLL {name}: {error:?}"),
            Self::WrongMachine { name } => {
                write!(f, "guest DLL {name} has the wrong instruction set")
            }
            Self::MissingExport { module, symbol } => {
                write!(f, "guest DLL {module} has no export {symbol:?}")
            }
            Self::InitializationRequired { name } => {
                write!(f, "guest DLL {name} requires a DLL initialization callback")
            }
            Self::AddressConflict { name } => write!(
                f,
                "guest DLL {name} requires relocation (preferred address occupied)"
            ),
            Self::LimitExceeded => f.write_str("guest DLL link resource/depth limit exceeded"),
        }
    }
}

struct Module {
    name: String,
    image: MappedImage,
    dependencies: Vec<usize>,
}
struct Linker<'a> {
    resolver: &'a mut dyn ImportResolver,
    modules: Vec<Module>,
    mode: DecodeMode,
    mapped_bytes: usize,
    imports: usize,
    occupied: Vec<(u64, u64)>,
    dynamic: bool,
    address_end: u64,
}

pub(super) fn load(
    file: &[u8],
    resolver: &mut dyn ImportResolver,
) -> Result<GuestImage, LinkError> {
    let image = MappedImage::parse(file, false)?;
    let mut linker = Linker {
        resolver,
        mode: image.mode,
        address_end: image.user_address_end()?,
        mapped_bytes: image.region.bytes.len(),
        imports: 0,
        occupied: Vec::new(),
        dynamic: false,
        modules: alloc::vec![Module {
            name: String::new(),
            image,
            dependencies: Vec::new(),
        }],
    };
    linker.bind(0, 0)?;
    // Include forwarded-export dependencies, not just the order in which
    // files happened to be read. Publish-before-bind permits import cycles;
    // the visiting mark also prevents recursive initialization of a cycle.
    fn visit(index: usize, modules: &[Module], marks: &mut [bool], order: &mut Vec<usize>) {
        if marks[index] {
            return;
        }
        marks[index] = true;
        for &dependency in &modules[index].dependencies {
            visit(dependency, modules, marks, order);
        }
        if index != 0 {
            order.push(index);
        }
    }
    let mut order = Vec::new();
    visit(
        0,
        &linker.modules,
        &mut alloc::vec![false; linker.modules.len()],
        &mut order,
    );
    let mut modules: Vec<_> = linker.modules.into_iter().map(Some).collect();
    let executable = modules[0].take().ok_or(LoadError::InvalidImage)?.image;
    let mut libraries = Vec::new();
    let mut identities = Vec::new();
    for index in order {
        let module = modules[index].take().ok_or(LoadError::InvalidImage)?;
        identities.push((module.name, module.image.region.base));
        libraries.push(module.image);
    }
    let mut image = GuestImage::from_mapped(executable, libraries)?;
    image.win32.process_dependencies.extend(identities.iter().map(|(_, base)| *base));
    image.win32.modules.extend(identities);
    Ok(image)
}

/// Stage against checked copies of existing mappings. The live image is only
/// extended after every dependency, export and initialization constraint passes.
pub(super) fn load_library(
    guest: &mut GuestImage,
    name: &str,
    resolver: &mut dyn ImportResolver,
) -> Result<u64, LinkError> {
    let name = canonical_module(name)?;
    if let Some((_, base)) = guest.win32.modules.iter().find(|(n, _)| *n == name) {
        return Ok(*base);
    }
    if guest.module_metadata.len() >= MAX_MODULES {
        return Err(LinkError::LimitExceeded);
    }
    let mut linker = Linker {
        resolver,
        mode: guest.thread.context.mode,
        address_end: guest.user_address_end,
        mapped_bytes: 0,
        imports: 0,
        modules: Vec::new(),
        occupied: Vec::new(),
        dynamic: true,
    };
    linker
        .modules
        .try_reserve_exact(guest.module_metadata.len())
        .map_err(|_| LoadError::OutOfMemory)?;
    for meta in &guest.module_metadata {
        linker.mapped_bytes = linker
            .mapped_bytes
            .checked_add(meta.size)
            .ok_or(LinkError::LimitExceeded)?;
        if linker.mapped_bytes > MAX_MAPPED_BYTES {
            return Err(LinkError::LimitExceeded);
        }
        let name = guest
            .win32
            .modules
            .iter()
            .find(|(_, base)| *base == meta.base)
            .map(|(name, _)| name.clone())
            .ok_or(LoadError::InvalidImage)?;
        linker.modules.push(Module {
            name,
            image: meta.snapshot(&guest.memory)?,
            dependencies: Vec::new(),
        });
    }
    linker
        .occupied
        .try_reserve_exact(guest.memory.regions.len())
        .map_err(|_| LoadError::OutOfMemory)?;
    for region in &guest.memory.regions {
        linker.occupied.push((
            region.base,
            region
                .base
                .checked_add(region.bytes.len() as u64)
                .ok_or(LoadError::InvalidImage)?,
        ));
    }
    let existing = linker.modules.len();
    let library = linker.library(&name, 0)?;
    let handle = linker.modules[library].image.region.base;
    let new_count = linker.modules.len() - existing;
    guest
        .memory
        .regions
        .try_reserve_exact(new_count)
        .map_err(|_| LoadError::OutOfMemory)?;
    guest
        .module_metadata
        .try_reserve_exact(new_count)
        .map_err(|_| LoadError::OutOfMemory)?;
    guest
        .win32
        .modules
        .try_reserve_exact(new_count)
        .map_err(|_| LoadError::OutOfMemory)?;
    // Record imports and forward edges from both new DLLs and existing
    // facades used by those imports. Reserve before publishing any mappings.
    let mut dependencies = Vec::new();
    for module in &linker.modules {
        let owner = module.image.region.base;
        for &index in &module.dependencies {
            let edge = (owner, linker.modules[index].image.region.base);
            if !guest.win32.module_dependencies.contains(&edge) && !dependencies.contains(&edge) {
                dependencies.try_reserve(1).map_err(|_| LoadError::OutOfMemory)?;
                dependencies.push(edge);
            }
        }
    }
    guest.win32.module_dependencies.try_reserve_exact(dependencies.len())
        .map_err(|_| LoadError::OutOfMemory)?;
    guest.win32.module_dependencies.extend(dependencies);
    for module in linker.modules.into_iter().skip(existing) {
        guest.module_metadata.push(super::ModuleMetadata::from_image(&module.image));
        guest
            .win32
            .modules
            .push((module.name, module.image.region.base));
        guest.memory.regions.push(module.image.region);
    }
    guest.memory.regions.sort_by_key(|region| region.base);
    guest.thread.invalidate_code();
    guest.threads.invalidate_code();
    Ok(handle)
}

fn canonical_module(name: &str) -> Result<String, LoadError> {
    // Imports may name a DLL, not select arbitrary filesystem paths.
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        || name.contains("..")
    {
        return Err(LoadError::InvalidImage);
    }
    let name = name.to_ascii_lowercase();
    Ok(if name.ends_with(".dll") {
        name
    } else {
        format!("{name}.dll")
    })
}

impl MappedImage {
    fn rebase(&mut self, base: u64, limit: u64) -> Result<(), LoadError> {
        let bad = LoadError::InvalidImage;
        if base < 0x10000 || base & 0xffff != 0
            || base.checked_add(self.region.bytes.len() as u64).is_none_or(|end| end > limit) {
            return Err(bad);
        }
        let (rva, size) = self.directories[5];
        if (rva, size) == (0, 0) && self.forwarders_only()? {
            // API-set facade DLLs have no code, initialization, absolute data
            // fixups or relocation directory: their exports are RVA strings.
            self.region.base = base;
            return Ok(());
        }
        if rva == 0
            || size < 8
        {
            return Err(bad);
        }
        // Validate the entire directory and targets before applying any fixup.
        let directory = self.data(rva as usize, size as usize)?;
        let mut cursor = 0;
        let mut fixups = Vec::new();
        while cursor < directory.len() {
            let block = directory.get(cursor..).ok_or(bad)?;
            let page = u32_at(block, 0)? as usize;
            let length = u32_at(block, 4)? as usize;
            if page & 0xfff != 0 || length < 8 || length & 1 != 0 || length > block.len() {
                return Err(bad);
            }
            for at in (8..length).step_by(2) {
                let entry = u16_at(block, at)?;
                let width = match entry >> 12 {
                    0 => continue,
                    3 => 4,
                    10 if self.mode == DecodeMode::X64 => 8,
                    _ => return Err(bad),
                };
                let target = page.checked_add((entry & 0xfff) as usize).ok_or(bad)?;
                self.data(target, width)?;
                if fixups.len() >= MAX_RELOCATIONS {
                    return Err(LoadError::ImageTooLarge);
                }
                fixups.try_reserve(1).map_err(|_| LoadError::OutOfMemory)?;
                fixups.push((target, width));
            }
            cursor += length;
        }
        fixups.sort_unstable();
        if fixups.windows(2).any(|p| p[0].0 + p[0].1 > p[1].0) {
            return Err(bad);
        }
        let delta = base.wrapping_sub(self.region.base);
        for (offset, width) in fixups {
            if width == 4 {
                let value = u32_at(&self.region.bytes, offset)?.wrapping_add(delta as u32);
                self.region.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            } else {
                let value = u64_at(&self.region.bytes, offset)?.wrapping_add(delta);
                self.region.bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        self.region.base = base;
        Ok(())
    }

    fn forwarders_only(&self) -> Result<bool, LoadError> {
        if self.entry != 0
            || self.directories[1] != (0, 0)
            || self.directories[9] != (0, 0)
            || self.directories[10] != (0, 0)
            || self
                .region
                .permissions
                .iter()
                .any(|p| p & super::WRITE != 0)
        {
            return Ok(false);
        }
        let (rva, size) = self.directories[0];
        if rva == 0 || size < 40 {
            return Ok(false);
        }
        let table = self.data(rva as usize, 40)?;
        let count = u32_at(table, 20)? as usize;
        if count == 0 || count > MAX_SYMBOLS {
            return Ok(false);
        }
        let functions = self.data(u32_at(table, 28)? as usize, count * 4)?;
        let end = (rva as usize)
            .checked_add(size as usize)
            .ok_or(LoadError::InvalidImage)?;
        for i in 0..count {
            let target = u32_at(functions, i * 4)? as usize;
            if target != 0 {
                if !(rva as usize..end).contains(&target) {
                    return Ok(false);
                }
                self.string(target, (end - target).min(512))?;
            }
        }
        Ok(true)
    }

    fn data(&self, rva: usize, len: usize) -> Result<&[u8], LoadError> {
        let va = self
            .region
            .base
            .checked_add(rva as u64)
            .ok_or(LoadError::InvalidImage)?;
        let range = self
            .region
            .range(va, len, READ)
            .ok_or(LoadError::InvalidImage)?;
        Ok(&self.region.bytes[range])
    }
    fn string(&self, rva: usize, limit: usize) -> Result<String, LoadError> {
        let mut name = Vec::new();
        for offset in 0..limit {
            let b = self.data(rva.checked_add(offset).ok_or(LoadError::InvalidImage)?, 1)?[0];
            if b == 0 {
                if name.is_empty() {
                    return Err(LoadError::InvalidImage);
                }
                return String::from_utf8(name).map_err(|_| LoadError::InvalidImage);
            }
            if !(0x20..0x7f).contains(&b) {
                return Err(LoadError::InvalidImage);
            }
            name.push(b);
        }
        Err(LoadError::InvalidImage)
    }
}

impl Linker<'_> {
    fn library(&mut self, name: &str, depth: usize) -> Result<usize, LinkError> {
        if depth >= MAX_DEPTH {
            return Err(LinkError::LimitExceeded);
        }
        let name = canonical_module(name)?;
        if let Some(index) = self.modules.iter().position(|module| module.name == name) {
            return Ok(index);
        }
        if self.modules.len() >= MAX_MODULES {
            return Err(LinkError::LimitExceeded);
        }
        let file = self
            .resolver
            .read_library(&name, self.mode)
            .map_err(|status| LinkError::Library {
                name: name.clone(),
                status,
            })?;
        let mut image =
            MappedImage::parse(&file, true).map_err(|error| LinkError::LibraryImage {
                name: name.clone(),
                error,
            })?;
        if image.mode != self.mode {
            return Err(LinkError::WrongMachine { name });
        }
        let tls = super::tls::Plan::parse(&image).map_err(|error| LinkError::LibraryImage {
            name: name.clone(),
            error,
        })?;
        if self.dynamic && (image.entry != 0 || tls.is_some()) {
            return Err(LinkError::InitializationRequired { name });
        }
        let conflicts = |start: u64, end: u64| {
            self.modules.iter().any(|module| {
                let region = &module.image.region;
                start < region.base + region.bytes.len() as u64 && region.base < end
            }) || self
                .occupied
                .iter()
                .any(|&(base, limit)| start < limit && base < end)
        };
        if image.region.base + image.region.bytes.len() as u64 > self.address_end || conflicts(
            image.region.base,
            image.region.base + image.region.bytes.len() as u64,
        ) {
            // Application DLLs often share a linker-default preferred base.
            // Rebase only checked PE relocation records, never scan/patch words.
            let mut base: u64 = if self.mode == DecodeMode::X86 {
                0x4000_0000
            } else {
                0x2_0000_0000
            };
            loop {
                let end = base.checked_add(image.region.bytes.len() as u64)
                    .filter(|end| *end <= self.address_end)
                    .ok_or_else(|| LinkError::AddressConflict { name: name.clone() })?;
                if !conflicts(base, end) { break; }
                base = base.checked_add((image.region.bytes.len() as u64 + 0xffff) & !0xffff)
                    .ok_or_else(|| LinkError::AddressConflict { name: name.clone() })?;
            }
            if image.directories[5] == (0, 0) && !image.forwarders_only()? {
                return Err(LinkError::AddressConflict { name });
            }
            image
                .rebase(base, self.address_end)
                .map_err(|error| LinkError::LibraryImage {
                    name: name.clone(),
                    error,
                })?;
        }
        // TLS pointers include absolute VAs and must be checked after rebasing.
        super::tls::Plan::parse(&image).map_err(|error| LinkError::LibraryImage {
            name: name.clone(),
            error,
        })?;
        self.mapped_bytes += image.region.bytes.len();
        if self.mapped_bytes > MAX_MAPPED_BYTES {
            return Err(LinkError::LimitExceeded);
        }
        let index = self.modules.len();
        // Publish before binding so ordinary circular imports terminate.
        self.modules.push(Module {
            name,
            image,
            dependencies: Vec::new(),
        });
        self.bind(index, depth + 1).map_err(|error| match error {
            LinkError::Image(error) => LinkError::LibraryImage {
                name: self.modules[index].name.clone(),
                error,
            },
            other => other,
        })?;
        // Loading a DLL binds its imports, not every export. Sparse ordinal
        // slots and unavailable, unused forwarders must not reject the load.
        // Static imports resolve through export(); GetProcAddress loads a
        // forward target on demand through the guest resolver in win32.rs.
        Ok(index)
    }

    fn bind(&mut self, index: usize, depth: usize) -> Result<(), LinkError> {
        let (rva, size) = self.modules[index].image.directories[1];
        if (rva, size) == (0, 0) {
            return Ok(());
        }
        if rva == 0 || size < 20 || size as usize / 20 > MAX_SYMBOLS {
            return Err(LoadError::InvalidImage.into());
        }
        for offset in (0..size as usize - 19).step_by(20) {
            let image = &self.modules[index].image;
            let desc = image.data(rva as usize + offset, 20)?;
            if desc.iter().all(|b| *b == 0) {
                return Ok(());
            }
            let names = u32_at(desc, 0)? as usize;
            let iat = u32_at(desc, 16)? as usize;
            let name_rva = u32_at(desc, 12)? as usize;
            if name_rva == 0 || iat == 0 {
                return Err(LoadError::InvalidImage.into());
            }
            let name = image.string(name_rva, 129)?;
            let library = self.library(&name, depth + 1)?;
            self.modules[index].dependencies.push(library);
            self.thunks(
                index,
                library,
                if names == 0 { iat } else { names },
                iat,
                depth,
            )?;
        }
        Err(LoadError::InvalidImage.into()) // Missing terminating descriptor.
    }

    fn thunks(
        &mut self,
        index: usize,
        library: usize,
        names: usize,
        iat: usize,
        depth: usize,
    ) -> Result<(), LinkError> {
        let width = if self.mode == DecodeMode::X86 { 4 } else { 8 };
        let ordinal_flag = 1u64 << (width * 8 - 1);
        for offset in (0..=MAX_SYMBOLS * width).step_by(width) {
            let image = &self.modules[index].image;
            let slot = image.data(
                names.checked_add(offset).ok_or(LoadError::InvalidImage)?,
                width,
            )?;
            let thunk = if width == 4 {
                u32_at(slot, 0)? as u64
            } else {
                u64_at(slot, 0)?
            };
            if thunk == 0 {
                return Ok(());
            }
            self.imports += 1;
            if self.imports > MAX_SYMBOLS {
                return Err(LinkError::LimitExceeded);
            }
            let symbol = if thunk & ordinal_flag != 0 {
                if thunk & !(ordinal_flag | 0xffff) != 0 {
                    return Err(LoadError::InvalidImage.into());
                }
                ImportSymbol::Ordinal(thunk as u16)
            } else {
                let hint = usize::try_from(thunk).map_err(|_| LoadError::InvalidImage)?;
                image.data(hint, 2)?;
                ImportSymbol::Name(
                    image.string(hint.checked_add(2).ok_or(LoadError::InvalidImage)?, 512)?,
                )
            };
            let address = self.export(library, &symbol, depth + 1)?;
            let target = iat.checked_add(offset).ok_or(LoadError::InvalidImage)?;
            let image = &mut self.modules[index].image;
            // Loader fixup of read-only .rdata is allowed before publishing the
            // address space; guest execution still sees final page permissions.
            image.data(target, width)?;
            image.region.bytes[target..target + width]
                .copy_from_slice(&address.to_le_bytes()[..width]);
        }
        Err(LinkError::LimitExceeded)
    }

    fn export(
        &mut self,
        index: usize,
        symbol: &ImportSymbol,
        depth: usize,
    ) -> Result<u64, LinkError> {
        self.export_inner(index, symbol, depth)
            .map_err(|error| match error {
                LinkError::Image(error) => LinkError::LibraryImage {
                    name: format!("{} export {:?}", self.modules[index].name, symbol),
                    error,
                },
                other => other,
            })
    }

    fn export_inner(
        &mut self,
        index: usize,
        symbol: &ImportSymbol,
        depth: usize,
    ) -> Result<u64, LinkError> {
        if depth >= MAX_DEPTH {
            return Err(LinkError::LimitExceeded);
        }
        let module = &self.modules[index];
        let missing = || LinkError::MissingExport {
            module: module.name.clone(),
            symbol: symbol.clone(),
        };
        let image = &module.image;
        let (rva, size) = image.directories[0];
        if rva == 0 || size < 40 {
            return Err(missing());
        }
        let end = (rva as usize)
            .checked_add(size as usize)
            .ok_or(LoadError::InvalidImage)?;
        let table = image.data(rva as usize, 40)?;
        let base = u32_at(table, 16)?;
        let count = u32_at(table, 20)? as usize;
        let named = u32_at(table, 24)? as usize;
        if count > MAX_SYMBOLS || named > MAX_SYMBOLS {
            return Err(LinkError::LimitExceeded);
        }
        let functions = u32_at(table, 28)? as usize;
        let names = u32_at(table, 32)? as usize;
        let ordinals = u32_at(table, 36)? as usize;
        let ordinal = match symbol {
            ImportSymbol::Ordinal(n) => (*n as u32).checked_sub(base).map(|n| n as usize),
            ImportSymbol::Name(name) => {
                let pointers = image.data(names, named * 4)?;
                let ords = image.data(ordinals, named * 2)?;
                let mut result = None;
                for i in 0..named {
                    if image.string(u32_at(pointers, i * 4)? as usize, 512)? == *name {
                        result = Some(u16_at(ords, i * 2)? as usize);
                        break;
                    }
                }
                result
            }
        }
        .filter(|ordinal| *ordinal < count)
        .ok_or_else(missing)?;
        let functions = image.data(functions, count * 4)?;
        let target = u32_at(functions, ordinal * 4)? as usize;
        if target == 0 {
            return Err(missing());
        }
        if (rva as usize..end).contains(&target) {
            let forward = image.string(target, (end - target).min(512))?;
            let (name, function) = forward.rsplit_once('.').ok_or(LoadError::InvalidImage)?;
            let symbol = if let Some(ordinal) = function.strip_prefix('#') {
                ImportSymbol::Ordinal(ordinal.parse().map_err(|_| LoadError::InvalidImage)?)
            } else {
                ImportSymbol::Name(String::from(function))
            };
            let library = self.library(name, depth + 1)?;
            self.modules[index].dependencies.push(library);
            return self.export(library, &symbol, depth + 1);
        }
        let address = image
            .region
            .base
            .checked_add(target as u64)
            .ok_or(LoadError::InvalidImage)?;
        // PE exports include data (for example MSVC's std::_Raise_handler).
        // Linking validates a readable mapped address; execution separately
        // enforces EXECUTE permissions when the guest actually fetches code.
        if image.region.range(address, 1, READ).is_none() {
            return Err(LoadError::InvalidImage.into());
        }
        Ok(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocation_fixup_limit_is_checked_before_applying_or_growing_without_bound() {
        for mode in [DecodeMode::X86, DecodeMode::X64] {
            let length = 8 + (MAX_RELOCATIONS + 1) * 2;
            let mut region =
                super::super::Region::new(0x4000_0000, 0x1000 + length, READ | super::super::WRITE)
                    .unwrap();
            region.bytes[..8].copy_from_slice(&0x12345678u64.to_le_bytes());
            let block = &mut region.bytes[0x1000..];
            block[4..8].copy_from_slice(&(length as u32).to_le_bytes());
            let kind = if mode == DecodeMode::X86 {
                0x3000u16
            } else {
                0xa000u16
            };
            for entry in block[8..].chunks_exact_mut(2) {
                entry.copy_from_slice(&kind.to_le_bytes());
            }
            let mut image = MappedImage {
                region,
                mode,
                entry: 0,
                directories: [(0, 0); 16],
            };
            image.directories[5] = (0x1000, length as u32);
            assert_eq!(image.rebase(0x4100_0000, 0xffff_0000), Err(LoadError::ImageTooLarge));
            assert_eq!(image.region.base, 0x4000_0000);
            assert_eq!(&image.region.bytes[..8], &0x12345678u64.to_le_bytes());
        }
    }
}

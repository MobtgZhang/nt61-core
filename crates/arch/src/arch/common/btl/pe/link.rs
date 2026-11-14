//! Bounded PE import/export linking into the isolated guest address space.
//! A resolved address always names executable bytes of a same-ISA guest DLL.

use super::{u16_at, u32_at, u64_at, GuestImage, LoadError, MappedImage, EXECUTE, READ};
use crate::arch::common::btl::ir::DecodeMode;
use alloc::{format, string::String, vec::Vec};
use core::fmt;

const MAX_MODULES: usize = 32;
const MAX_DEPTH: usize = 32;
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
}
struct Linker<'a> {
    resolver: &'a mut dyn ImportResolver,
    modules: Vec<Module>,
    mode: DecodeMode,
    mapped_bytes: usize,
    imports: usize,
}

pub(super) fn load(
    file: &[u8],
    resolver: &mut dyn ImportResolver,
) -> Result<GuestImage, LinkError> {
    let image = MappedImage::parse(file, false)?;
    let mut linker = Linker {
        resolver,
        mode: image.mode,
        mapped_bytes: image.region.bytes.len(),
        imports: 0,
        modules: alloc::vec![Module {
            name: String::new(),
            image
        }],
    };
    linker.bind(0, 0)?;
    let mut modules = linker.modules.into_iter();
    let executable = modules.next().ok_or(LoadError::InvalidImage)?.image;
    let libraries = modules.map(|module| module.image.region).collect();
    GuestImage::from_mapped(executable, libraries).map_err(Into::into)
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
        let image = MappedImage::parse(&file, true).map_err(|error| LinkError::LibraryImage {
            name: name.clone(),
            error,
        })?;
        if image.mode != self.mode {
            return Err(LinkError::WrongMachine { name });
        }
        if image.directories[9] != (0, 0) {
            return Err(LinkError::LibraryImage {
                name,
                error: LoadError::TlsRequired,
            });
        }
        // Silently skipping DllMain would make a correctly linked image broken.
        if image.entry != 0 {
            return Err(LinkError::InitializationRequired { name });
        }
        let start = image.region.base;
        let end = start + image.region.bytes.len() as u64;
        if self.modules.iter().any(|module| {
            let region = &module.image.region;
            start < region.base + region.bytes.len() as u64 && region.base < end
        }) {
            return Err(LinkError::AddressConflict { name });
        }
        self.mapped_bytes += image.region.bytes.len();
        if self.mapped_bytes > MAX_MAPPED_BYTES {
            return Err(LinkError::LimitExceeded);
        }
        let index = self.modules.len();
        // Publish before binding so ordinary circular imports terminate.
        self.modules.push(Module { name, image });
        self.bind(index, depth + 1)?;
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
            return self.export(library, &symbol, depth + 1);
        }
        let address = image
            .region
            .base
            .checked_add(target as u64)
            .ok_or(LoadError::InvalidImage)?;
        if image.region.range(address, 1, EXECUTE).is_none() {
            return Err(LoadError::InvalidImage.into());
        }
        Ok(address)
    }
}

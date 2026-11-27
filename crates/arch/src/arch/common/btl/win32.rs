//! Win32 ABI services for the isolated x86/x64 runtime DLLs.
//! Addresses and handles belong to one GuestImage. No guest pointer is cast to
//! a host pointer. Unsupported APIs stop explicitly rather than return success.
#[path = "win32/eventlog.rs"]
mod eventlog;
#[path = "win32/directory.rs"]
mod directory;
#[path = "win32/process.rs"]
mod process;
#[path = "win32/advapi.rs"]
mod advapi;
#[path = "win32/gui.rs"]
mod gui;
#[path = "win32/dialogs.rs"]
mod dialogs;
#[path = "win32/printing.rs"]
mod printing;
#[path = "win32/font_enum.rs"]
mod font_enum;
#[path = "win32/text_layout.rs"]
mod text_layout;
#[path = "win32/user.rs"]
mod user;
#[path = "win32/sync.rs"]
pub(super) mod sync;
#[path = "win32/winsock.rs"]
mod winsock;
#[path = "win32/runonce.rs"]
mod runonce;
#[path = "win32/virtual_memory.rs"]
mod virtual_memory;
#[path = "win32/mapping.rs"]
pub(super) mod mapping;
#[path = "win32/context.rs"]
mod context;
#[path = "win32/function_table.rs"]
mod function_table;
#[path = "win32/rtl_integer.rs"]
mod rtl_integer;
#[path = "../../../../../libs/ntdll/src/ntdll/rtl_epoch.rs"]
mod rtl_epoch;
#[path = "win32/unwind.rs"]
mod unwind;
#[path = "win32/nls.rs"]
mod nls;
#[path = "win32/environment.rs"]
mod environment;
#[path = "win32/com.rs"]
mod com;
#[path = "win32/shell.rs"]
mod shell;
#[path = "win32/resources.rs"]
mod resources;
#[path = "win32/version.rs"]
mod version;
#[path = "win32/crt.rs"]
mod crt;
#[path = "win32/crt_startup.rs"]
pub(super) mod crt_startup;
#[path = "win32/library.rs"]
pub(super) mod library;
#[path = "win32/catalog.rs"]
pub mod catalog;
use crate::arch::common::btl::{
    abi::GuestAbi,
    io::guest_file_path,
    ir::DecodeMode,
    memory::GuestMemory,
    nt61::{self, GuestServices, Service, ServiceResult},
    pe::{GuestImage, Region, EXECUTE, MAX_PROCESS_MEMORY, PAGE, READ, WRITE},
    runtime::StopReason,
};
use alloc::{string::String, vec, vec::Vec};

const INVALID_PARAMETER: u32 = 87;
const NOACCESS: u32 = 998;
const MAX_BUFFER: usize = 1024 * 1024;
const HEAP: u64 = 0x7f00_0000;

struct VirtualAllocation {
    base: u64,
    length: usize,
    committed: Vec<bool>,
}

struct PrivateHeap {
    handle: u64,
    blocks: Vec<(u64, usize)>,
}

pub struct Runtime {
    pub modules: Vec<(String, u64)>,
    pub(super) module_paths: Vec<(u64, String)>,
    /// Dependencies retained by the executable's initial import graph.
    pub(super) process_dependencies: Vec<u64>,
    /// Import and resolved forward-export edges owned by mapped DLLs.
    pub(super) module_dependencies: Vec<(u64, u64)>,
    /// References acquired by successful Win32 LoadLibrary calls. Module
    /// lookup and GetModuleHandle do not acquire a reference.
    library_loads: Vec<(u64, u32)>,
    /// GET_MODULE_HANDLE_EX_FLAG_PIN retains a module until process teardown.
    pinned_modules: Vec<u64>,
    allocations: Vec<(u64, usize)>,
    heap_allocations: Vec<(u64, usize)>,
    private_heaps: Vec<PrivateHeap>,
    virtual_allocations: Vec<VirtualAllocation>,
    mappings: mapping::State,
    tls: Vec<bool>,
    pub(super) local: ThreadLocal,
    ui_language: u16,
    next_address: u64,
    pub last_api: Option<u32>,
    pub last_arguments: Vec<u64>,
    /// Bounded diagnostics for failed runtime calls; never changes guest results.
    pub failures: Vec<(u32, u32, String)>,
    pub exit_context: Option<(&'static str, &'static str, u64)>,
    pub last_library_error: Option<String>,
    advapi: advapi::State,
    tokens: advapi::token::State,
    com: com::State,
    eventlog: eventlog::State,
    directory: directory::State,
    resources: Vec<resources::Resource>,
    gui: gui::State,
    printing: printing::State,
    user: user::State,
    sync: sync::State,
    winsock_startups: u32,
    winsock_address_lists: Vec<u64>,
    environment: environment::State,
    shell: shell::State,
    crt: crt_startup::State,
    shutdown_parameters: (u32, u32),
    terminate_on_heap_corruption: bool,
    /// Process-local top-level filter registration. Exception dispatch remains
    /// unsupported; a registered callback is never called as a host pointer.
    exception_filter: u64,
}
impl Runtime {
    pub fn set_ui_language(&mut self, tag: &str) -> bool {
        let Some(&(language, _, _)) = nls::UI_LANGUAGES.iter()
            .find(|(_, candidate, _)| candidate.eq_ignore_ascii_case(tag)) else {
            return false;
        };
        self.ui_language = language;
        true
    }

    pub(super) fn new(base: u64) -> Self {
        Self {
            modules: vec![(String::new(), base)],
            module_paths: Vec::new(),
            process_dependencies: Vec::new(),
            module_dependencies: Vec::new(),
            library_loads: Vec::new(),
            pinned_modules: Vec::new(),
            allocations: Vec::new(),
            heap_allocations: Vec::new(),
            private_heaps: Vec::new(),
            virtual_allocations: Vec::new(),
            mappings: mapping::State::default(),
            tls: Vec::new(),
            local: ThreadLocal::default(),
            ui_language: nls::DEFAULT_UI_LANGUAGE,
            next_address: 0x2000_0000,
            last_api: None,
            last_arguments: Vec::new(),
            failures: Vec::new(),
            exit_context: None,
            last_library_error: None,
            advapi: advapi::State::new(),
            tokens: advapi::token::State::default(),
            com: com::State::default(),
            eventlog: eventlog::State::default(),
            directory: directory::State::default(),
            resources: Vec::new(),
            gui: gui::State::new(),
            printing: printing::State::new(),
            user: user::State::new(),
            sync: sync::State::default(),
            winsock_startups: 0,
            winsock_address_lists: Vec::new(),
            environment: environment::State::default(),
            shell: shell::State::default(),
            crt: crt_startup::State::default(),
            shutdown_parameters: (0x280, 0),
            terminate_on_heap_corruption: false,
            exception_filter: 0,
        }
    }

    fn heap_blocks(&self, handle: u64) -> Option<&[(u64, usize)]> {
        if handle == HEAP {
            return Some(&self.heap_allocations);
        }
        self.private_heaps.iter().find(|heap| heap.handle == handle)
            .map(|heap| heap.blocks.as_slice())
    }

    fn heap_blocks_mut(&mut self, handle: u64) -> Result<&mut Vec<(u64, usize)>, u32> {
        if handle == HEAP {
            return Ok(&mut self.heap_allocations);
        }
        self.private_heaps.iter_mut().find(|heap| heap.handle == handle)
            .map(|heap| &mut heap.blocks).ok_or(INVALID_PARAMETER)
    }
}
#[derive(Default)]
pub(super) struct ThreadLocal {
    pub(super) tls: Vec<u64>,
    pub(super) fs_redirection_disabled: bool,
    ui_languages: Vec<u16>,
    user: user::ThreadState,
    com: com::ThreadState,
}

pub(super) fn cleanup_thread(image: &mut GuestImage) {
    com::cleanup_thread(image);
    sync::abandon_mutexes(image);
}

pub enum Outcome {
    Return(u64),
    Exit(u32),
    ExitThread(u32),
    Park(super::threads::Wait),
    ParkRetry(super::threads::Wait),
    Retry,
    Continue,
}

fn read(m: &mut dyn GuestMemory, p: u64, n: usize) -> Result<Vec<u8>, u32> {
    if n > MAX_BUFFER {
        return Err(INVALID_PARAMETER);
    }
    let mut b = vec![0; n];
    if m.read_virt(p, &mut b) != n {
        return Err(NOACCESS);
    }
    Ok(b)
}
fn word(m: &mut dyn GuestMemory, p: u64, n: usize) -> Result<u64, u32> {
    let b = read(m, p, n)?;
    let mut v = [0; 8];
    v[..n].copy_from_slice(&b);
    Ok(u64::from_le_bytes(v))
}

pub fn delay_load_fault(image: &mut GuestImage) -> Option<(String, String)> {
    let api = catalog::resolve(image.win32.last_api?)?;
    let [code, _, 1, arguments] = image.win32.last_arguments.as_slice() else {
        return None;
    };
    if api.name != "RaiseException" || !matches!(*code as u32, 0xc06d007e | 0xc06d007f) {
        return None;
    }

    let width = match image.thread.context.mode {
        DecodeMode::X86 => 4,
        DecodeMode::X64 => 8,
    };
    let memory = &mut image.memory;
    let info = word(memory, *arguments, width).ok()?;
    if word(memory, info, 4).ok()? != 9 * width as u64 {
        return None;
    }
    let mut field = |index, size| word(memory, info.checked_add(index * width as u64)?, size).ok();
    let dll = field(3, width)?;
    let by_name = field(4, 4)?;
    let symbol = field(5, if by_name == 0 { 4 } else { width })?;
    let dll = delay_load_name(memory, dll)?;
    let symbol = if by_name == 0 {
        alloc::format!("#{}", u16::try_from(symbol).ok()?)
    } else {
        delay_load_name(memory, symbol)?
    };
    Some((dll, symbol))
}

fn delay_load_name(memory: &mut dyn GuestMemory, pointer: u64) -> Option<String> {
    let mut name = String::new();
    for offset in 0..128 {
        let mut byte = [0];
        if memory.read_virt(pointer.checked_add(offset)?, &mut byte) != 1 {
            return None;
        }
        match byte[0] {
            0 => return (!name.is_empty()).then_some(name),
            0x21..=0x7e => name.push(char::from(byte[0])),
            _ => return None,
        }
    }
    None
}

fn put(m: &mut dyn GuestMemory, p: u64, v: u64, n: usize) -> Result<(), u32> {
    if m.write_virt(p, &v.to_le_bytes()[..n]) {
        Ok(())
    } else {
        Err(NOACCESS)
    }
}
fn string(m: &mut dyn GuestMemory, p: u64, wide: bool) -> Result<String, u32> {
    let width = if wide { 2 } else { 1 };
    let mut units = Vec::new();
    for i in 0..32768 {
        let v = word(m, p.checked_add(i * width).ok_or(NOACCESS)?, width as usize)? as u16;
        if v == 0 {
            return if wide {
                String::from_utf16(&units).map_err(|_| 1113)
            } else {
                Ok(units.iter().map(|v| char::from(*v as u8)).collect())
            };
        }
        if !wide && v > 0x7f {
            return Err(1113);
        }
        units.push(v)
    }
    Err(INVALID_PARAMETER)
}
fn write_string(m: &mut dyn GuestMemory, p: u64, s: &str, wide: bool) -> Result<usize, u32> {
    let b: Vec<u8> = if wide {
        s.encode_utf16()
            .chain(core::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        if !s.is_ascii() {
            return Err(1113);
        }
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        b
    };
    if !m.write_virt(p, &b) {
        return Err(NOACCESS);
    }
    Ok(if wide { b.len() / 2 - 1 } else { b.len() - 1 })
}
fn wide_z_bytes(memory: &mut dyn GuestMemory, pointer: u64) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    for offset in (0..65_536).step_by(2) {
        let address = pointer.checked_add(offset).ok_or(ApiError::MemoryFault)?;
        let unit = word(memory, address, 2).map_err(|_| ApiError::MemoryFault)? as u16;
        bytes.extend_from_slice(&unit.to_le_bytes());
        if unit == 0 {
            return Ok(bytes);
        }
    }
    Err(ApiError::MemoryFault)
}
fn error(image: &mut GuestImage, e: u32) {
    let off = if image.thread.context.mode == DecodeMode::X86 {
        0x34
    } else {
        0x68
    };
    let _ = put(&mut image.memory, image.teb + off, e as u64, 4);
}
fn protection(p: u32) -> Result<u8, u32> {
    Ok(match p {
        1 => 0,
        2 => READ,
        4 => READ | WRITE,
        0x10 => EXECUTE,
        0x20 => READ | EXECUTE,
        0x40 => READ | WRITE | EXECUTE,
        // WRITECOPY protections apply to mapped file views, not VirtualAlloc
        // private pages. This runtime has no file-view/COW implementation;
        // accepting them as ordinary writable pages would claim false success.
        _ => return Err(INVALID_PARAMETER),
    })
}
pub(super) fn win_protection(p: u8) -> u32 {
    match p & (READ | WRITE | EXECUTE) {
        0 => 1,
        1 => 2,
        3 => 4,
        4 => 0x10,
        5 => 0x20,
        7 => 0x40,
        _ => 1,
    }
}
fn aligned_size(n: u64) -> Result<usize, u32> {
    let n = usize::try_from(n).map_err(|_| 8u32)?;
    if n == 0 || n > 64 * 1024 * 1024 {
        return Err(8);
    }
    n.checked_add(PAGE - 1).map(|v| v & !(PAGE - 1)).ok_or(8)
}
pub(super) fn allocate(image: &mut GuestImage, requested: u64, n: u64, permissions: u8) -> Result<u64, u32> {
    let requested_base = requested & !0xffff;
    let len = aligned_size(n.checked_add(requested - requested_base).ok_or(8u32)?)?;
    let mut base = if requested == 0 {
        image.win32.next_address
    } else {
        requested_base
    };
    if requested == 0 {
        loop {
            let end = base.checked_add(len as u64).ok_or(8u32)?;
            let conflict = image
                .memory
                .regions
                .iter()
                .find(|r| base < r.base + r.bytes.len() as u64 && r.base < end);
            match conflict {
                Some(r) => base = (r.base + r.bytes.len() as u64 + 0xffff) & !0xffff,
                None => break,
            }
        }
    }
    let end = base.checked_add(len as u64).ok_or(8u32)?;
    if base < 0x10000
        || end > image.user_address_end
        || image.win32.allocations.len() >= 4096
        || image.memory.mapped_bytes().saturating_add(len) > MAX_PROCESS_MEMORY
    {
        return Err(8);
    }
    if image
        .memory
        .regions
        .iter()
        .any(|r| base < r.base + r.bytes.len() as u64 && r.base < end)
    {
        return Err(487);
    }
    image
        .memory
        .regions
        .push(Region::new(base, len, permissions).map_err(|_| 8u32)?);
    image.win32.allocations.push((base, len));
    image.win32.next_address = (end + 0xffff) & !0xffff;
    Ok(base)
}
pub(super) fn free(image: &mut GuestImage, p: u64) -> Result<(), u32> {
    let i = image
        .win32
        .allocations
        .iter()
        .position(|a| a.0 == p)
        .ok_or(INVALID_PARAMETER)?;
    image.win32.allocations.remove(i);
    let i = image
        .memory
        .regions
        .iter()
        .position(|r| r.base == p)
        .ok_or(INVALID_PARAMETER)?;
    image.memory.regions.remove(i);
    Ok(())
}
fn virtual_range(image: &GuestImage, p: u64, n: u64) -> Result<(usize, usize, usize), u32> {
    if n == 0 {
        return Err(INVALID_PARAMETER);
    }
    let end = p.checked_add(n).ok_or(INVALID_PARAMETER)?;
    let (index, allocation) = image
        .win32
        .virtual_allocations
        .iter()
        .enumerate()
        .find(|(_, a)| p >= a.base && end <= a.base + a.length as u64)
        .ok_or(487u32)?;
    Ok((
        index,
        ((p - allocation.base) as usize) / PAGE,
        ((end - 1 - allocation.base) as usize) / PAGE,
    ))
}
fn change_commit(
    image: &mut GuestImage,
    p: u64,
    n: u64,
    bits: u8,
    commit: bool,
) -> Result<(), u32> {
    let (index, first, last) = virtual_range(image, p, n)?;
    let allocation = &mut image.win32.virtual_allocations[index];
    let region = image
        .memory
        .regions
        .iter_mut()
        .find(|r| r.base == allocation.base)
        .ok_or(487u32)?;
    for page in first..=last {
        if !commit || !allocation.committed[page] {
            let end = ((page + 1) * PAGE).min(region.bytes.len());
            region.bytes[page * PAGE..end].fill(0);
        }
        // SP1 preserves contents on duplicate commit but applies the requested
        // protection even to pages that were already committed.
        region.permissions[page] = if commit { bits } else { 0 };
        region.protections[page] = if commit { win_protection(bits) } else { 0 };
        allocation.committed[page] = commit;
    }
    image.thread.invalidate_code();
    Ok(())
}
fn protect(image: &mut GuestImage, p: u64, n: u64, flags: u32) -> Result<u32, u32> {
    let bits = protection(flags)?;
    if n == 0 {
        return Err(INVALID_PARAMETER);
    }
    let end = p.checked_add(n).ok_or(INVALID_PARAMETER)?;
    for a in &image.win32.virtual_allocations {
        if p < a.base + a.length as u64 && a.base < end {
            let (_, first, last) = virtual_range(image, p, n)?;
            if a.committed[first..=last].iter().any(|v| !v) {
                return Err(487);
            }
        }
    }
    let r = image
        .memory
        .regions
        .iter_mut()
        .find(|r| p >= r.base && end <= r.base + r.bytes.len() as u64)
        .ok_or(487u32)?;
    if image.win32.mappings.contains_view(r.base) && (bits & EXECUTE != 0
        || (bits & WRITE != 0 && r.allocation_protection == 2)) {
        return Err(5);
    }
    let first = ((p - r.base) as usize) / PAGE;
    let last = ((end - 1 - r.base) as usize) / PAGE;
    let old = r.protections[first];
    r.permissions[first..=last].fill(bits);
    r.protections[first..=last].fill(flags);
    Ok(old)
}
fn heap_allocate(image: &mut GuestImage, n: u64) -> Result<u64, u32> {
    heap_allocate_in(image, HEAP, n)
}
fn heap_allocate_in(image: &mut GuestImage, handle: u64, n: u64) -> Result<u64, u32> {
    image.win32.heap_blocks_mut(handle)?.try_reserve(1).map_err(|_| 8u32)?;
    let p = allocate(image, 0, n.max(1), READ | WRITE)?;
    image.win32.heap_blocks_mut(handle)?.push((p, n as usize));
    Ok(p)
}
fn heap_free(image: &mut GuestImage, p: u64) -> Result<(), u32> {
    heap_free_in(image, HEAP, p)
}
fn heap_free_in(image: &mut GuestImage, handle: u64, p: u64) -> Result<(), u32> {
    let blocks = image.win32.heap_blocks_mut(handle)?;
    if p == 0 {
        return Ok(());
    }
    let index = blocks.iter().position(|block| block.0 == p).ok_or(INVALID_PARAMETER)?;
    free(image, p)?;
    image.win32.heap_blocks_mut(handle)?.remove(index);
    Ok(())
}
fn heap_move(image: &mut GuestImage, old: u64, length: usize, size: u64) -> Result<u64, u32> {
    heap_move_in(image, HEAP, old, length, size)
}
fn heap_move_in(image: &mut GuestImage, handle: u64, old: u64, length: usize, size: u64) -> Result<u64, u32> {
    let destination = heap_allocate_in(image, handle, size)?;
    let result = (|| {
        let count = length.min(size as usize);
        let mut cursor = 0;
        while cursor < count {
            let chunk = (count - cursor).min(MAX_BUFFER);
            let bytes = read(&mut image.memory, old + cursor as u64, chunk)?;
            if !image.memory.write_virt(destination + cursor as u64, &bytes) {
                return Err(NOACCESS);
            }
            cursor += chunk;
        }
        heap_free_in(image, handle, old)
    })();
    if let Err(error) = result {
        let _ = heap_free_in(image, handle, destination);
        return Err(error);
    }
    Ok(destination)
}

fn local_reallocate(image: &mut GuestImage, address: u64, size: u64, flags: u32) -> Result<u64, u32> {
    const MOVEABLE: u32 = 0x02;
    const ZERO_INIT: u32 = 0x40;
    const MODIFY: u32 = 0x80;
    const DISCARDABLE: u32 = 0x0f00;
    if flags & !0x0ff2 != 0 || flags & DISCARDABLE != 0 && flags & MODIFY == 0 {
        return Err(INVALID_PARAMETER);
    }
    let index = image.win32.heap_allocations.iter().position(|entry| entry.0 == address).ok_or(8u32)?;
    if flags & MODIFY != 0 {
        return Ok(address);
    }
    let previous_size = image.win32.heap_allocations[index].1;
    let capacity = image.win32.allocations.iter().find(|entry| entry.0 == address).ok_or(8u32)?.1;
    if size > capacity as u64 {
        if flags & MOVEABLE == 0 {
            return Err(8);
        }
        return heap_move(image, address, previous_size, size);
    }
    let size = size as usize;
    if flags & ZERO_INIT != 0 && size > previous_size {
        if !image.memory.probe_write(address + previous_size as u64, size - previous_size) {
            return Err(NOACCESS);
        }
        let region = image.memory.regions.iter_mut().find(|region| region.base == address).ok_or(8u32)?;
        region.bytes[previous_size..size].fill(0);
    }
    image.win32.heap_allocations[index].1 = size;
    Ok(address)
}

fn module(image: &GuestImage, name: &str) -> Option<u64> {
    let name = name.rsplit(['\\', '/']).next()?;
    let lower = name.to_ascii_lowercase();
    let lower = if !lower.contains('.') {
        alloc::format!("{lower}.dll")
    } else {
        lower
    };
    image
        .win32
        .modules
        .iter()
        .find(|m| m.0.eq_ignore_ascii_case(&lower))
        .map(|m| m.1)
}
#[derive(Clone, Copy)]
enum LibrarySearch<'a> {
    Default,
    System32,
    AlteredDirectory(&'a str),
}

fn load_library(
    image: &mut GuestImage,
    name: &str,
    services: &mut dyn GuestServices,
    search: LibrarySearch<'_>,
    run_entry_points: bool,
) -> Result<u64, u32> {
    // Import descriptors remain restricted to basenames. LoadLibrary accepts
    // paths, however: keep the module identity separate from the path used for
    // its first read, and resolve its dependencies normally.
    let identity = name.rsplit(['\\', '/']).next().ok_or(INVALID_PARAMETER)?;
    struct Resolver<'a> {
        services: &'a mut dyn GuestServices,
        requested: &'a str,
        first: bool,
        search: LibrarySearch<'a>,
    }
    impl super::ImportResolver for Resolver<'_> {
        fn read_library(&mut self, name: &str, mode: DecodeMode) -> Result<Vec<u8>, u32> {
            self.resolve_library(name, mode).map(|library| library.bytes)
        }

        fn resolve_library(&mut self, name: &str, mode: DecodeMode) -> Result<super::ResolvedLibrary, u32> {
            if self.first {
                self.first = false;
                if self.requested.contains(['\\', '/']) {
                    let path = if self.requested.rsplit(['\\', '/']).next().is_some_and(|s| !s.contains('.')) {
                        alloc::format!("{}.dll", self.requested)
                    } else { self.requested.into() };
                    return self.services.resolve_library(&path, mode, false);
                }
            }
            match self.search {
                LibrarySearch::Default => self.services.resolve_library(name, mode, false),
                LibrarySearch::System32 => self.services.resolve_library(name, mode, true),
                LibrarySearch::AlteredDirectory(directory) => {
                    let path = alloc::format!("{directory}\\{name}");
                    match self.services.resolve_library(&path, mode, false) {
                        Err(0xc000_0034 | 0xc000_003a | 0xc000_000f | 0xc000_0135) => {
                            self.services.resolve_library(name, mode, true)
                        }
                        result => result,
                    }
                }
            }
        }
    }
    let mut resolver = Resolver { services, requested: name, first: true, search };
    super::link::load_library(image, identity, &mut resolver, matches!(search, LibrarySearch::System32), run_entry_points)
        .map_err(|e| {
            image.win32.last_library_error = Some(alloc::format!("{}: {}", name, e));
            match e.ntstatus() {
                0xc0000135 => 126,
                0xc0000139 => 127,
                0xc000007b => 193,
                0xc0000017 => 8,
                _ => 120,
            }
        })
}
// A failed lazy forward lookup must roll back newly mapped targets and
// dependency edges. Keep pre-existing explicit references and memory intact.
fn proc_address(
    image: &mut GuestImage,
    base: u64,
    name: Option<&str>,
    ordinal: u16,
    depth: usize,
    services: &mut dyn GuestServices,
) -> Result<u64, u32> {
    let mut previous = Vec::new();
    previous.try_reserve_exact(image.module_metadata.len()).map_err(|_| 8u32)?;
    previous.extend(image.module_metadata.iter().map(|m| m.base));
    let edge_count = image.win32.module_dependencies.len();
    let result = proc_address_inner(image, base, name, ordinal, depth, services);
    if result.is_err() {
        image.memory.regions.retain(|r|
            previous.contains(&r.base) || !image.module_metadata.iter().any(|m| m.base == r.base));
        image.module_metadata.retain(|m| previous.contains(&m.base));
        image.win32.modules.retain(|m| previous.contains(&m.1));
        image.win32.module_dependencies.truncate(edge_count);
        image.thread.invalidate_code();
        image.threads.invalidate_code();
    }
    result
}
fn proc_address_inner(
    image: &mut GuestImage,
    base: u64,
    name: Option<&str>,
    ordinal: u16,
    depth: usize,
    services: &mut dyn GuestServices,
) -> Result<u64, u32> {
    if depth >= 32 || !image.win32.modules.iter().any(|m| m.1 == base) {
        return Err(127);
    }
    let m = &mut image.memory;
    let pe = word(m, base + 0x3c, 4)?;
    let opt = base + pe + 24;
    let dir = opt + if word(m, opt, 2)? == 0x10b { 96 } else { 112 };
    let rva = word(m, dir, 4)?;
    let size = word(m, dir + 4, 4)?;
    if rva == 0 || size < 40 {
        return Err(127);
    }
    let t = base + rva;
    let first = word(m, t + 16, 4)?;
    let count = word(m, t + 20, 4)?;
    let named = word(m, t + 24, 4)?;
    if count > 16384 || named > 16384 {
        return Err(127);
    }
    let functions = base + word(m, t + 28, 4)?;
    let names = base + word(m, t + 32, 4)?;
    let ords = base + word(m, t + 36, 4)?;
    let index = if let Some(n) = name {
        let mut found = None;
        for i in 0..named {
            let p = base + word(m, names + i * 4, 4)?;
            if string(m, p, false)? == n {
                found = Some(word(m, ords + i * 2, 2)?);
                break;
            }
        }
        found.ok_or(127u32)?
    } else {
        (ordinal as u64).checked_sub(first).ok_or(127u32)?
    };
    if index >= count {
        return Err(127);
    }
    let target = word(m, functions + index * 4, 4)?;
    if target == 0 {
        return Err(127);
    }
    if target >= rva && target < rva + size {
        let f = string(m, base + target, false)?;
        let (dll, symbol) = f.rsplit_once('.').ok_or(127u32)?;
        let system_only = image.module_metadata.iter()
            .any(|metadata| metadata.base == base && metadata.system_library_search);
        let b = if let Some(base) = module(image, dll) {
            base
        } else {
            let search = if system_only { LibrarySearch::System32 } else { LibrarySearch::Default };
            load_library(image, dll, services, search, false)?
        };
        let result = if let Some(n) = symbol.strip_prefix('#') {
            proc_address_inner(image, b, None, n.parse().map_err(|_| 127u32)?, depth + 1, services)
        } else {
            proc_address_inner(image, b, Some(symbol), 0, depth + 1, services)
        };
        if result.is_ok() && b != base {
            let edge = (base, b);
            if !image.win32.module_dependencies.contains(&edge) {
                image.win32.module_dependencies.try_reserve(1).map_err(|_| 8u32)?;
                image.win32.module_dependencies.push(edge);
            }
        }
        return result;
    }
    let mut byte = [0];
    // A PE export may name readable data as well as code. Returning that
    // address does not grant execute permission; instruction fetch checks it.
    if image.memory.read_virt(base + target, &mut byte) != 1 {
        return Err(127);
    }
    Ok(base + target)
}
fn nt_service(name: &str) -> Option<Service> {
    Some(match name.trim_start_matches("Zw") {
        "NtClose" | "Close" => Service::Close,
        "NtTerminateProcess" | "TerminateProcess" => Service::TerminateProcess,
        "NtTerminateThread" | "TerminateThread" => Service::TerminateThread,
        "NtDelayExecution" | "DelayExecution" => Service::DelayExecution,
        "NtYieldExecution" | "YieldExecution" => Service::YieldExecution,
        "NtQuerySystemTime" | "QuerySystemTime" => Service::QuerySystemTime,
        "NtQueryPerformanceCounter" | "QueryPerformanceCounter" => Service::QueryPerformanceCounter,
        "NtReadFile" | "ReadFile" => Service::ReadFile,
        "NtWriteFile" | "WriteFile" => Service::WriteFile,
        "NtCreateFile" | "CreateFile" => Service::CreateFile,
        "NtQuerySecurityObject" | "QuerySecurityObject" => Service::QuerySecurityObject,
        "NtSetSecurityObject" | "SetSecurityObject" => Service::SetSecurityObject,
        "NtAllocateVirtualMemory" | "AllocateVirtualMemory" => Service::AllocateVirtualMemory,
        "NtQueryInformationProcess" | "QueryInformationProcess" => Service::QueryInformationProcess,
        "NtOpenProcess" | "OpenProcess" => Service::OpenProcess,
        "NtWaitForSingleObject" | "WaitForSingleObject" => Service::WaitForSingleObject,
        _ => return None,
    })
}

pub(super) fn close_token(image: &mut GuestImage, handle: u64) -> Option<u32> {
    image.win32.tokens.close(handle).map(|closed| {
        if closed { 0 } else { 0xc000_0008 }
    })
}

pub fn dispatch(
    image: &mut GuestImage,
    number: u32,
    services: &mut dyn GuestServices,
) -> Result<Outcome, StopReason> {
    let id = number & 0xffff;
    if id==user::CALLBACK_RETURN{return user::callback_return(image,services);}
    if id == user::DIALOG_GATEWAY { return user::poll_dialog(image, services); }
    if id == user::browse::GATEWAY { return user::browse::poll(image, services); }
    let api = catalog::resolve(id).ok_or(StopReason::UnsupportedApi { id })?;
    image.win32.last_api = Some(id);
    let abi = if image.thread.context.mode == DecodeMode::X86 {
        GuestAbi::X86_32
    } else {
        GuestAbi::X86_64
    };
    let count = if abi == GuestAbi::X86_32 {
        api.x86_arguments
    } else {
        api.arguments
    } as usize;
    let a = nt61::capture_arguments(abi, &image.thread.context.regs, count, &mut image.memory)
        .map_err(|_| StopReason::DataFault {
            pc: image.thread.context.rip,
        })?;
    image.win32.last_arguments.clear();
    image.win32.last_arguments.extend_from_slice(&a[..count]);
    if api.dll == "shell32.dll" && api.name == "SHBrowseForFolderW" {
        return Ok(user::browse::start(image, services, a[0]));
    }
    if matches!(api.name, "ExitProcess" | "NtTerminateProcess" | "TerminateProcess")
        || (api.dll == "msvcrt.dll" && matches!(api.name, "exit" | "_exit" | "_amsg_exit" | "abort"))
    {
        let width = if abi == GuestAbi::X86_32 { 4 } else { 8 };
        let stack = image.thread.context.reg(crate::arch::common::btl::ir::X86Reg::Rsp);
        let caller = word(&mut image.memory, stack, width).unwrap_or(0);
        image.win32.exit_context = Some((api.dll, api.name, caller));
    }
    if api.dll == "ntdll.dll" {
        if let Some(service) = nt_service(api.name) {
            if service == Service::Close {
                if mapping::close(image, services, a[0]) {
                    return Ok(Outcome::Return(0));
                }
                if let Some(status) = close_token(image, a[0]) {
                    return Ok(Outcome::Return(u64::from(status)));
                }
            }
            return Ok(
                match services.call(abi, service, &a[..count], &mut image.memory) {
                    ServiceResult::Return(v) => Outcome::Return(v as u64),
                    ServiceResult::ExitProcess(c) => Outcome::Exit(c),
                    ServiceResult::ExitThread(c) => Outcome::ExitThread(c),
                },
            );
        }
    }
    if api.name == "ExitProcess" { return Ok(Outcome::Exit(a[0] as u32)); }
    if api.name == "ExitThread" { return Ok(Outcome::ExitThread(a[0] as u32)); }
    if api.dll == "kernel32.dll" && api.name == "FreeLibraryAndExitThread" {
        let code = a[1] as u32;
        return match library::free(image, a[0], Some(code)) {
            Ok(_) if image.pending_library.is_some() => Ok(Outcome::Continue),
            Ok(_) | Err(ApiError::Windows(126)) => Ok(Outcome::ExitThread(code)),
            Err(_) => Err(StopReason::UnsupportedApi { id }),
        };
    }
    if api.dll == "msvcrt.dll" {
        match api.name {
            "exit" | "_exit" => return Ok(Outcome::Exit(a[0] as u32)),
            "_amsg_exit" => return Ok(Outcome::Exit(255)),
            _ => {}
        }
    }
    let folder_message = if api.dll == "user32.dll" && matches!(api.name, "SendMessageA" | "SendMessageW") {
        user::browse::send_message(image, services, &a)
    } else {
        None
    };
    match folder_message.unwrap_or_else(|| invoke(image, api, &a, services)) {
        Ok(v) => {
            if let Some((procedure,args))=image.win32.local.user.callback.take(){
                user::enter_callback(image,procedure,args).map_err(|_|StopReason::DataFault{pc:image.thread.context.rip})?;
                return Ok(Outcome::Continue);
            }
            if abi == GuestAbi::X86_32 && matches!(api.name, "GetTickCount64" | "VerSetConditionMask") {
                image
                    .thread
                    .context
                    .set_reg(crate::arch::common::btl::ir::X86Reg::Rdx, v >> 32);
            }
            Ok(Outcome::Return(v))
        }
        Err(ApiError::Windows(e)) => {
            let detail = if api.name.starts_with("LoadLibrary") {
                let library = image.win32.last_library_error.take().unwrap_or_else(|| {
                    string(&mut image.memory, a[0], api.name.ends_with('W')).unwrap_or_default()
                });
                if api.name.starts_with("LoadLibraryEx") {
                    alloc::format!("{library} file={:#x} flags={:#x}", a[1], a[2] as u32)
                } else {
                    library
                }
            } else if matches!(api.name, "CreateFileA" | "CreateFileW") {
                let width = if abi == GuestAbi::X86_32 { 4 } else { 8 };
                let mut attributes = [0; 24];
                let security = if a[3] != 0 && image.memory.read_virt(a[3], &mut attributes[..width * 3]) == width * 3 {
                    let length = u32::from_le_bytes(attributes[..4].try_into().unwrap());
                    let descriptor = attributes[width..width * 2].iter().any(|byte| *byte != 0);
                    let inherit = attributes[width * 2..width * 2 + 4].iter().any(|byte| *byte != 0);
                    alloc::format!("length={length} descriptor={descriptor} inherit={inherit}")
                } else {
                    alloc::format!("{:#x}", a[3])
                };
                alloc::format!("{} access={:#x} share={:#x} security=[{}] disposition={} flags={:#x} template={:#x}",
                    string(&mut image.memory, a[0], api.name.ends_with('W')).unwrap_or_default(),
                    a[1] as u32, a[2] as u32, security, a[4] as u32, a[5] as u32, a[6])
            } else if matches!(api.name, "CreateDirectoryW" | "RemoveDirectoryW" | "FindFirstFileW" | "FindFirstFileExW" | "GetFileAttributesW" | "SetFileAttributesW") {
                string(&mut image.memory, a[0], true).unwrap_or_default()
            } else if matches!(api.name, "ReadFile" | "WriteFile") {
                alloc::format!("length={} overlapped={}", a[2] as u32, a[4] != 0)
            } else if api.name == "GetTokenInformation" {
                alloc::format!("information_class={} length={}", a[1] as u32, a[3] as u32)
            } else if api.name == "GetProcAddress" {
                if a[1] <= 0xffff {
                    alloc::format!("ordinal {}", a[1])
                } else {
                    string(&mut image.memory, a[1], false).unwrap_or_default()
                }
            } else {
                String::new()
            };
            if image.win32.failures.len() == 16 {
                image.win32.failures.remove(0);
            }
            image.win32.failures.push((id, e, detail));
            error(image, e);
            if api.dll == "ws2_32.dll" {
                return Ok(Outcome::Return(winsock::failure_value(api.name, e, abi == GuestAbi::X86_32)));
            }
            Ok(Outcome::Return(match api.name {
                "DialogBoxParamW" => if abi == GuestAbi::X86_32 { u32::MAX as u64 } else { u64::MAX },
                "GetThreadPriority" => i32::MAX as u64,
                "SetAbortProc" => u32::MAX as u64,
                "SetFilePointer" | "GetFileAttributesA" | "GetFileAttributesW" => u32::MAX as u64,
                "GetMenuState" | "EnableMenuItem" | "CheckMenuItem" => u32::MAX as u64,
                "TlsAlloc" | "ResumeThread" | "SuspendThread" | "WaitForSingleObject" | "WaitForSingleObjectEx"
                | "WaitForMultipleObjects" | "WaitForMultipleObjectsEx" | "FindNLSString" => u32::MAX as u64,
                "GlobalFree" | "LocalFree" | "FreeSid" => a[0],
                "CreateFileA" | "CreateFileW" | "HeapSize" | "FindFirstFileA" | "FindFirstFileW" | "FindFirstFileExW" => {
                    if abi == GuestAbi::X86_32 {
                        u32::MAX as u64
                    } else {
                        u64::MAX
                    }
                }
                _ => 0,
            }))
        }
        Err(ApiError::Pending(wait)) => Ok(Outcome::Park(wait)),
        Err(ApiError::Hresult(code, detail)) => {
            // HRESULT failures are returned to the caller without touching
            // Win32 LastError. They are not successful backend operations.
            if image.win32.failures.len() == 16 { image.win32.failures.remove(0); }
            image.win32.failures.push((id, code, String::from(detail)));
            Ok(Outcome::Return(u64::from(code)))
        }
        Err(ApiError::Retry) => Ok(Outcome::Retry),
        Err(ApiError::PendingRetry(wait)) => Ok(Outcome::ParkRetry(wait)),
        Err(ApiError::CorruptHeap) => Ok(Outcome::Exit(0xc000_0374)),
        Err(ApiError::Unsupported) => Err(StopReason::UnsupportedApi { id }),
        Err(ApiError::MemoryFault) => Err(StopReason::DataFault {
            pc: image.thread.context.rip,
        }),
    }
}
pub(super) enum ApiError {
    Retry,
    PendingRetry(super::threads::Wait),
    Pending(super::threads::Wait),
    Windows(u32),
    Hresult(u32, &'static str),
    CorruptHeap,
    Unsupported,
    MemoryFault,
}
impl From<u32> for ApiError {
    fn from(e: u32) -> Self {
        Self::Windows(e)
    }
}
fn invoke(
    image: &mut GuestImage,
    api: &catalog::Api,
    a: &[u64],
    services: &mut dyn GuestServices,
) -> Result<u64, ApiError> {
    match (api.dll, api.name) {
        ("kernel32.dll", "GetDiskFreeSpaceA" | "GetDiskFreeSpaceW"
            | "GetDiskFreeSpaceExA" | "GetDiskFreeSpaceExW") => {
            return volume_space(image, services, api.name, a);
        }
        ("kernel32.dll", "CreateFileMappingA" | "CreateFileMappingW" | "MapViewOfFile"
            | "MapViewOfFileEx" | "FlushViewOfFile" | "UnmapViewOfFile") => {
            return mapping::invoke(image, services, api.name, a);
        }
        ("kernel32.dll", "FlushFileBuffers") => {
            services.flush_file_buffers(a[0]).map_err(status_error)?;
            return Ok(1);
        }
        ("kernel32.dll", "LockFile" | "UnlockFile") => {
            let offset = u64::from(a[1] as u32) | (u64::from(a[2] as u32) << 32);
            let length = u64::from(a[3] as u32) | (u64::from(a[4] as u32) << 32);
            services.set_file_range_lock(a[0], offset, length, api.name == "LockFile").map_err(status_error)?;
            return Ok(1);
        }
        ("ws2_32.dll", _) => return winsock::invoke(image, services, api.name, a),
        ("version.dll", _) => return version::invoke(image, api.name, a, services),
        ("ntdll.dll", "WinSqmAddToStream" | "WinSqmIncrementDWORD") => return Ok(0),
        ("comctl32.dll", "TaskDialogIndirect") | ("comdlg32.dll", "PrintDlgExW") => {
            return Err(ApiError::Hresult(0x80004001, "dialog backend unavailable"));
        }
        ("comdlg32.dll", "FindTextW" | "ReplaceTextW") => {
            return gui::unavailable_dialog(image);
        }
        ("winspool.drv", "OpenPrinterW") => return Err(1801u32.into()),
        ("winspool.drv", "ClosePrinter" | "GetPrinterDriverW") => return Err(6u32.into()),
        ("shell32.dll", "DragAcceptFiles") => return user::accept_file_drops(image, a[0], a[1] != 0),
        ("shell32.dll", "SHAddToRecentDocs") => return Err(120u32.into()),
        ("shell32.dll", "DragFinish" | "DragQueryFileW") => return Err(6u32.into()),
        ("shell32.dll", "ShellAboutW" | "ShellExecuteExW") => return Err(120u32.into()),
        ("shell32.dll", "SHCreateItemFromParsingName") => {
            return Err(ApiError::Hresult(0x80004001, "shell-item backend unavailable"));
        }
        ("ole32.dll", "CoCreateInstance") => return com::create_instance(image, a),
        _ => {}
    }
    if api.dll == "kernel32.dll" && matches!(api.name, "FindResourceA" | "FindResourceW"
        | "FindResourceExA" | "FindResourceExW" | "LoadResource" | "LockResource" | "SizeofResource") {
        return resources::invoke(image, api.name, a, services);
    }
    if api.dll == "ntdll.dll" && api.name == "RtlCaptureContext" {
        return context::capture(image, a[0]);
    }
    if api.dll == "ntdll.dll" && api.name == "RtlLookupFunctionEntry" {
        return function_table::lookup(image, a);
    }
    if api.dll == "ntdll.dll" && api.name == "RtlVirtualUnwind" {
        return unwind::unwind(image, a);
    }
    if api.dll == "ntdll.dll" && matches!(api.name, "RtlCharToInteger" | "RtlUnicodeStringToInteger"
        | "RtlIntegerToChar" | "RtlIntegerToUnicodeString") {
        return rtl_integer::invoke(image, api.name, a);
    }
    if api.dll == "ntdll.dll" {
        match api.name {
            "RtlTimeToSecondsSince1970" | "RtlTimeToSecondsSince1980" => {
                let epoch = if api.name == "RtlTimeToSecondsSince1970" { rtl_epoch::EPOCH_1970 } else { rtl_epoch::EPOCH_1980 };
                let time = word(&mut image.memory, a[0], 8).map_err(|_| ApiError::MemoryFault)? as i64;
                let Some(seconds) = rtl_epoch::to_seconds(time, epoch) else {
                    return Ok(0);
                };
                put(&mut image.memory, a[1], u64::from(seconds), 4).map_err(|_| ApiError::MemoryFault)?;
                return Ok(1);
            }
            "RtlSecondsSince1970ToTime" | "RtlSecondsSince1980ToTime" => {
                let epoch = if api.name == "RtlSecondsSince1970ToTime" { rtl_epoch::EPOCH_1970 } else { rtl_epoch::EPOCH_1980 };
                let time = rtl_epoch::from_seconds(a[0] as u32, epoch);
                put(&mut image.memory, a[1], time as u64, 8).map_err(|_| ApiError::MemoryFault)?;
                return Ok(0);
            }
            _ => {}
        }
    }
    if (api.dll == "ntdll.dll" && api.name == "NtQueryVirtualMemory")
        || (api.dll == "kernel32.dll" && matches!(api.name, "VirtualQuery" | "VirtualQueryEx")) {
        return virtual_memory::invoke(image, api.name, a);
    }
    if api.dll == "kernel32.dll" && matches!(api.name,
        "WerRegisterRuntimeExceptionModule" | "WerUnregisterRuntimeExceptionModule") {
        // The Win7 contract returns HRESULT. No out-of-process WER service,
        // registry allow-list or crash callback host is implemented yet.
        return Err(ApiError::Hresult(0x80004001, "WER out-of-process backend unavailable"));
    }
    if api.dll == "oleaut32.dll" || (api.dll == "ole32.dll" && matches!(api.name,
        "CoInitialize" | "CoInitializeEx" | "CoUninitialize" | "CoGetContextToken"
            | "OleInitialize" | "OleUninitialize" | "IIDFromString")) {
        return com::invoke(image, api.name, a);
    }
    if matches!(api.dll, "shell32.dll" | "ole32.dll" | "shlwapi.dll") {
        return shell::invoke(image, api.name, a, services);
    }
    if api.dll == "ntdll.dll" && matches!(api.name, "RtlAllocateHeap" | "RtlReAllocateHeap" | "RtlFreeHeap") {
        let allowed = if api.name == "RtlFreeHeap" { 1 } else { 9 };
        if a[1] & !allowed != 0 { return Err(ApiError::Unsupported); }
        let name = match api.name {
            "RtlAllocateHeap" => "HeapAlloc", "RtlReAllocateHeap" => "HeapReAlloc", _ => "HeapFree",
        };
        let target = catalog::find("kernel32.dll", name).ok_or(ApiError::Unsupported)?;
        let mut args = a.to_vec();
        args[1] &= !1; // serialization belongs to the process's guest scheduler
        return match invoke(image, target, &args, services) {
            Err(ApiError::Windows(_)) => Ok(0),
            result => result,
        };
    }
    if api.dll == "ntdll.dll" && api.name == "RtlSizeHeap" {
        // Native heap queries preserve Win32 LastError, including failures.
        // Only process-owned heaps and the documented HEAP_NO_SERIALIZE flag
        // are accepted; there is no host-pointer dereference.
        let failure = if image.thread.context.mode == DecodeMode::X86 { u32::MAX as u64 } else { u64::MAX };
        if a[1] & !1 != 0 { return Err(ApiError::Unsupported); }
        return Ok(image.win32.heap_blocks(a[0])
            .and_then(|blocks| blocks.iter().find(|block| block.0 == a[2]))
            .map_or(failure, |block| block.1 as u64));
    }
    if api.dll == "ntdll.dll" && crt::handles(api.name) {
        return crt::invoke(image, api.name, a);
    }
    if api.dll == "bcrypt.dll" && api.name == "BCryptGenRandom" {
        // CNG reports NTSTATUS directly and never sets Win32 LastError.
        if a[0] != 0 || a[3] as u32 & !2 != 0 { return Ok(0xc000000d); }
        if a[3] as u32 != 2 { return Ok(0xc00000bb); }
        let len = a[2] as u32 as usize;
        if len > MAX_BUFFER { return Ok(0xc000000d); }
        if len != 0 && !image.memory.probe_write(a[1], len) { return Ok(0xc0000005); }
        let mut bytes = vec![0; len];
        if len != 0 {
            if let Err(status) = services.random_bytes(&mut bytes) { return Ok(u64::from(status)); }
            if !image.memory.write_virt(a[1], &bytes) { return Ok(0xc0000005); }
        }
        return Ok(0);
    }
    if api.dll == "comdlg32.dll" || api.dll == "comctl32.dll" {
        return gui::invoke(image, api.dll, api.name, a, services);
    }
    if api.dll == "user32.dll" { return user::invoke(image, api.name, a, services); }
    if api.dll == "msvcrt.dll" { return crt_startup::invoke(image, api.name, a); }
    if api.dll == "gdi32.dll" {
        match api.name {
            "CreateCompatibleDC" => return Ok(services.create_memory_dc(a[0]).unwrap_or(0)),
            "GetDeviceCaps" if !printing::owns_dc(image, a[0]) => {
                return Ok(u64::from(services.device_caps(a[0], a[1] as i32).unwrap_or(0)));
            }
            "CreateFontIndirectW" => {
                let mut logical = [0; 92];
                if image.memory.read_virt(a[0], &mut logical) != logical.len() { return Ok(0); }
                return Ok(services.create_font_w(&logical).unwrap_or(0));
            }
            "SelectObject" => return Ok(services.select_font(a[0], a[1]).unwrap_or(0)),
            "DeleteObject" => return Ok(u64::from(services.delete_font(a[0])
                || (!image.win32.user.owns_client_dc(a[0]) && services.delete_memory_dc(a[0])))),
            "EnumFontsW" => return font_enum::start(image, a, services),
            "GetTextFaceW" => {
                let count = a[1] as i32;
                if count < 0 { return Ok(0); }
                let Some(face) = services.text_face_w(a[0]) else { return Ok(0); };
                if a[2] == 0 { return Ok(face.len() as u64 + 1); }
                if count == 0 { return Ok(0); }
                let copied = (count as usize).min(face.len() + 1);
                let mut bytes = Vec::with_capacity(copied * 2);
                for unit in &face[..copied - 1] {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
                bytes.extend_from_slice(&0u16.to_le_bytes());
                return Ok(if image.memory.probe_write(a[2], bytes.len())
                    && image.memory.write_virt(a[2], &bytes) { (copied - 1) as u64 } else { 0 });
            }
            "GetTextMetricsW" => {
                let Some(bytes) = services.text_metrics_w(a[0]) else { return Ok(0); };
                let output = a[1];
                return Ok(u64::from(image.memory.probe_write(output, bytes.len())
                    && image.memory.write_virt(output, &bytes)));
            }
            "GetTextExtentPoint32W" => {
                let count = a[2] as i32;
                if !(0..=65_536).contains(&count) {
                    return Err(ApiError::Windows(INVALID_PARAMETER));
                }
                if !image.memory.probe_write(a[3], 8) {
                    return Ok(0);
                }
                if count == 0 {
                    return Ok(u64::from(image.memory.write_virt(a[3], &[0; 8])));
                }
                let Ok(bytes) = read(&mut image.memory, a[1], count as usize * 2) else {
                    return Ok(0);
                };
                let text: Vec<_> = bytes.chunks_exact(2)
                    .map(|unit| u16::from_le_bytes([unit[0], unit[1]])).collect();
                let Some([width, height]) = services.text_extent_w(a[0], &text) else {
                    return Err(ApiError::Windows(INVALID_PARAMETER));
                };
                let mut size = [0; 8];
                size[..4].copy_from_slice(&width.to_le_bytes());
                size[4..].copy_from_slice(&height.to_le_bytes());
                return Ok(u64::from(image.memory.write_virt(a[3], &size)));
            }
            "SetBkMode" => return services.set_bk_mode(a[0], a[1] as i32)
                .map(|old| u64::from(old as u32)).ok_or(ApiError::Windows(INVALID_PARAMETER)),
            "SetMapMode" => {
                return services.set_map_mode(a[0], a[1] as i32)
                    .map(|old| u64::from(old as u32))
                    .ok_or(ApiError::Windows(INVALID_PARAMETER));
            }
            "SetWindowExtEx" | "SetViewportExtEx" => {
                let output = a[3];
                if output != 0 && !image.memory.probe_write(output, 8) {
                    return Ok(0);
                }
                let kind = if api.name == "SetWindowExtEx" {
                    nt61::DcExtent::Window
                } else {
                    nt61::DcExtent::Viewport
                };
                let Some((success, [old_x, old_y])) =
                    services.set_dc_extent(a[0], kind, [a[1] as i32, a[2] as i32])
                else {
                    return Err(ApiError::Windows(INVALID_PARAMETER));
                };
                if output != 0 {
                    let mut bytes = [0; 8];
                    bytes[..4].copy_from_slice(&old_x.to_le_bytes());
                    bytes[4..].copy_from_slice(&old_y.to_le_bytes());
                    if !image.memory.write_virt(output, &bytes) {
                        return Ok(0);
                    }
                }
                return Ok(u64::from(success));
            }
            "LPtoDP" => {
                let count = a[2] as i32;
                if count <= 0 { return Ok(1); }
                let len = count as usize * 8;
                let address = a[1];
                if count > 65_536 || !image.memory.probe_write(address, len) { return Ok(0); }
                let mut points = Vec::new();
                if points.try_reserve_exact(len).is_err() { return Ok(0); }
                points.resize(len, 0);
                return Ok(u64::from(image.memory.read_virt(address, &mut points) == len
                    && services.logical_to_device(a[0], &mut points)
                    && image.memory.write_virt(address, &points)));
            }
            "DeleteDC" if image.win32.user.owns_client_dc(a[0]) => return Ok(0),
            "DeleteDC" if services.delete_memory_dc(a[0]) => return Ok(1),
            _ => return printing::invoke(image, api.name, a, services),
        }
    }
    if api.dll == "advapi32.dll" && api.name == "SystemFunction036" {
        let len = a[1] as u32 as usize;
        if len > MAX_BUFFER || (len != 0 && !image.memory.probe_write(a[0], len)) { return Ok(0); }
        if len == 0 { return Ok(1); }
        let mut bytes = vec![0; len];
        if services.random_bytes(&mut bytes).is_err() { return Ok(0); }
        return Ok(u64::from(image.memory.write_virt(a[0], &bytes)));
    }
    if api.dll == "advapi32.dll" && eventlog::handles(api.name) {
        return eventlog::invoke(image, api.name, a, services);
    }
    if api.dll == "advapi32.dll" {
        if matches!(api.name, "GetFileSecurityW" | "SetFileSecurityW") {
            return advapi::file_security(image, api.name, a, services);
        }
        if let Some(result) = advapi::token::invoke(image, api.name, a) {
            return result.map_err(ApiError::Windows);
        }
        return advapi::invoke(image, api.name, a)
            .ok_or(ApiError::Unsupported)?
            .map_err(ApiError::Windows);
    }
    let x86 = image.thread.context.mode == DecodeMode::X86;
    let width = if x86 { 4 } else { 8 };
    let abi = if x86 {
        GuestAbi::X86_32
    } else {
        GuestAbi::X86_64
    };
    let name = api.name;
    if runonce::handles(name) {
        return runonce::invoke(image, api, a);
    }
    if api.dll == "kernel32.dll" && matches!(name, "GetFullPathNameA" | "GetFullPathNameW") {
        return directory::full_path_name(image, a, name.ends_with('W'));
    }
    if api.dll == "kernel32.dll" && matches!(name, "CreateDirectoryA" | "CreateDirectoryW" | "RemoveDirectoryA" | "RemoveDirectoryW" |
        "FindFirstFileA" | "FindFirstFileW" | "FindFirstFileExW" | "FindNextFileA" | "FindNextFileW" |
        "FindClose" | "GetFileAttributesA" | "GetFileAttributesW" | "GetFileAttributesExA" | "GetFileAttributesExW"
        | "SetFileAttributesW" | "DeleteFileA" | "DeleteFileW" | "MoveFileA" | "MoveFileW") {
        return directory::invoke(image, name, a, services);
    }
    if api.dll == "kernel32.dll" && nls::handles(name) {
        return nls::invoke(image, name, a);
    }
    if sync::handles(name) {
        return sync::invoke(image, name, a, services);
    }
    Ok(match name {
        "InterlockedExchange" if x86 => image.memory.exchange_u32(a[0], a[1] as u32)
            .map_err(|_| ApiError::MemoryFault)?,
        "InterlockedCompareExchange" if x86 => image.memory.locked_compare_exchange(a[0], 4, a[2], a[1])
            .map_err(|_| ApiError::MemoryFault)?,
        "InterlockedExchangeAdd" if x86 => image.memory.locked_fetch_add(a[0], 4, a[1])
            .map_err(|_| ApiError::MemoryFault)?,
        "InterlockedIncrement" | "InterlockedDecrement" if x86 => {
            let decrement = name == "InterlockedDecrement";
            let old = image.memory.locked_inc_dec(a[0], 4, decrement)
                .map_err(|_| ApiError::MemoryFault)? as u32;
            u64::from(old.wrapping_add(if decrement { u32::MAX } else { 1 }))
        }
        "Wow64DisableWow64FsRedirection" | "Wow64RevertWow64FsRedirection"
        | "Wow64EnableWow64FsRedirection" | "RtlWow64EnableFsRedirection"
        | "RtlWow64EnableFsRedirectionEx" => wow64_fs_redirection(image, name, a)?,
        "VerifyVersionInfoW" => version::verify(image, a[0], a[1] as u32, if x86 {a[2] | a[3] << 32} else {a[2]})?,
        "VerSetConditionMask" => {
            let (mask, types, condition) = if x86 {
                (a[0] | a[1] << 32, a[2] as u32, a[3] as u8)
            } else {
                (a[0], a[1] as u32, a[2] as u8)
            };
            let selected = types & 0xff;
            // NT gives the highest supported type bit priority and ORs its
            // three-bit condition into the existing 64-bit mask.
            if selected == 0 { mask } else {
                mask | (u64::from(condition & 7) << ((31 - selected.leading_zeros()) * 3))
            }
        }
        "SetUnhandledExceptionFilter" => {
            core::mem::replace(&mut image.win32.exception_filter, a[0])
        }
        "GetVersionExA" | "GetVersionExW" => {
            let wide = name.ends_with('W');
            let basic = 20 + 128 * if wide {2} else {1};
            let size = word(&mut image.memory, a[0], 4).map_err(|_| ApiError::MemoryFault)? as usize;
            if size != basic && size != basic + 8 { return Err(INVALID_PARAMETER.into()); }
            if !image.memory.probe_write(a[0], size) { return Err(ApiError::MemoryFault); }
            // Same Win7 SP1 compatibility identity as GetVersion and the PEB.
            // GetVersionEx on NT 6.1 is not affected by the Win8.1 manifest rule.
            let mut bytes = vec![0; size];
            for (offset, value) in [(0, size as u32), (4, 6), (8, 1), (12, 7601), (16, 2)] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            if wide {
                for (index, unit) in "Service Pack 1".encode_utf16().enumerate() {
                    bytes[20 + 2 * index..22 + 2 * index].copy_from_slice(&unit.to_le_bytes());
                }
            } else { bytes[20..34].copy_from_slice(b"Service Pack 1"); }
            if size > basic { bytes[basic..].copy_from_slice(&[1, 0, 0, 0, 0, 1, 1, 0]); }
            if !image.memory.write_virt(a[0], &bytes) { return Err(ApiError::MemoryFault); }
            1
        }
        "SetDllDirectoryW" => {
            // The isolated resolver searches the application and system trees,
            // never the current directory. The empty-string operation retains
            // exactly that policy. Custom directories and restoring the full
            // Win32 default search require a resolver with those search paths.
            if a[0] == 0 || !string(&mut image.memory, a[0], true)?.is_empty() {
                return Err(ApiError::Unsupported);
            }
            1
        }
        "GetSystemDirectoryW" | "GetWindowsDirectoryW" => {
            // This mounted-volume runtime installs Windows at C:\Windows.
            // WOW64 redirects filesystem access to System32 separately.
            let path = if name == "GetSystemDirectoryW" { "C:\\Windows\\System32" } else { "C:\\Windows" };
            let units: Vec<u16> = path.encode_utf16().chain(core::iter::once(0)).collect();
            if (a[1] as u32 as usize) < units.len() { units.len() as u64 }
            else {
                let bytes: Vec<u8> = units.iter().flat_map(|v| v.to_le_bytes()).collect();
                if !image.memory.write_virt(a[0], &bytes) { return Err(ApiError::MemoryFault); }
                (units.len() - 1) as u64
            }
        }
        "CreateThread" => return super::threads::create(image, a).map_err(ApiError::Windows),
        "OpenThread" => return super::threads::open(image, a[0] as u32, a[1] as u32 != 0, a[2] as u32).map_err(ApiError::Windows),
        "GetThreadId" | "GetExitCodeThread" | "GetProcessIdOfThread" if services.is_child_handle(a[0]) =>
            return process::child_info(image, a, services, name),
        "ResumeThread" | "SuspendThread" | "GetThreadId" | "GetProcessIdOfThread" | "GetExitCodeThread" | "SetThreadPriority" | "GetThreadPriority" =>
            return super::threads::invoke(image, name, a).map_err(ApiError::Windows),
        "GetThreadPriorityBoost" => {
            let disabled = super::threads::priority_boost_disabled(image, a[0])?;
            if !image.memory.write_virt(a[1], &u32::from(disabled).to_le_bytes()) {
                return Err(ApiError::MemoryFault);
            }
            1
        }
        "SetThreadPriorityBoost" => return super::threads::set_priority_boost_disabled(image, a[0], a[1] as u32 != 0).map_err(ApiError::Windows),
        "QueueUserAPC" => return super::threads::queue_apc(image, a[0], a[1], a[2]).map_err(ApiError::Windows),
        "Sleep" | "SleepEx" if image.has_other_threads() ||
            (name == "SleepEx" && a[1] as u32 != 0 && image.has_pending_apc()) => {
            let alertable = name == "SleepEx" && a[1] as u32 != 0;
            let wait = super::threads::sleep(image, a[0] as u32, alertable, services)?;
            return Err(ApiError::Pending(wait));
        }
        "Sleep" | "SleepEx" => {
            let milliseconds = a[0] as u32;
            let mut interval = if milliseconds == u32::MAX {
                i64::MIN
            } else {
                -i64::from(milliseconds) * 10_000
            };
            let scratch = allocate(image, 0, 16, READ | WRITE)?;
            let result = (|| -> Result<u64, ApiError> {
                let start = if milliseconds != u32::MAX {
                    Some(sleep_counter(abi, services, &mut image.memory, scratch)?)
                } else {
                    None
                };
                let alertable = name == "SleepEx" && a[1] as u32 != 0;
                loop {
                    put(&mut image.memory, scratch, interval as u64, 8)?;
                    match services.call(
                        abi, Service::DelayExecution,
                        &[u64::from(alertable), scratch], &mut image.memory,
                    ) {
                        ServiceResult::Return(0) => return Ok(0),
                        ServiceResult::Return(0xc0) if alertable => return Ok(0xc0),
                        ServiceResult::Return(0x101) => {
                            if let Some((start, frequency)) = start {
                                let (now, current_frequency) = sleep_counter(
                                    abi, services, &mut image.memory, scratch,
                                )?;
                                if frequency != current_frequency || now < start {
                                    return Err(ApiError::Unsupported);
                                }
                                let elapsed = u128::from(now - start) * 10_000_000
                                    / u128::from(frequency);
                                let duration = u128::from(milliseconds) * 10_000;
                                if elapsed >= duration {
                                    return Ok(0);
                                }
                                interval = -((duration - elapsed) as i64);
                            }
                        }
                        // Sleep has no Win32 failure result. Stop execution
                        // when its backend is unavailable rather than return
                        // as if the requested delay completed.
                        _ => return Err(ApiError::Unsupported),
                    }
                }
            })();
            free(image, scratch)?;
            result?
        }
        "GlobalAlloc" => {
            if a[0]&!0x42!=0 || a[1]==0 {return Err(INVALID_PARAMETER.into());}
            heap_allocate(image,a[1])?
        }
        "GlobalFree" => {heap_free(image,a[0])?;0}
        "GlobalLock" => {
            if !image.win32.heap_allocations.iter().any(|v|v.0==a[0]){return Err(6u32.into());}a[0]
        }
        "GlobalUnlock" => {if !image.win32.heap_allocations.iter().any(|v|v.0==a[0]){return Err(6u32.into());}error(image,0);0}
        "LocalAlloc" => {
            // Movable local-memory handles need a separate lock/handle model.
            if a[0] & !0x40 != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            heap_allocate(image, a[1])?
        }
        "LocalReAlloc" => local_reallocate(image, a[0], a[1], a[2] as u32)?,
        "LocalLock" | "LocalUnlock" => {
            let handle = a[0];
            let movable_bit = if abi == GuestAbi::X86_64 { 8 } else { 4 };
            if handle & movable_bit != 0 {
                return Err(6u32.into());
            }
            if name == "LocalUnlock" {
                return Err(158u32.into());
            }
            if image.win32.heap_allocations.iter().any(|entry| entry.0 == handle) { handle } else { 0 }
        }
        "LocalFree" => {
            heap_free(image, a[0])?;
            0
        }
        "LocalSize" => image
            .win32
            .heap_allocations
            .iter()
            .find(|entry| entry.0 == a[0])
            .map(|entry| entry.1 as u64)
            .ok_or(INVALID_PARAMETER)?,
        "QueryPerformanceCounter" | "RtlQueryPerformanceCounter"
        | "QueryPerformanceFrequency"
        | "GetTickCount"
        | "GetTickCount64"
        | "GetSystemTimeAsFileTime" => {
            let scratch = allocate(image, 0, 16, READ | WRITE)?;
            let result = (|| -> Result<u64, ApiError> {
                let system = name == "GetSystemTimeAsFileTime";
                let args = if system {
                    [scratch, 0]
                } else {
                    [scratch, scratch + 8]
                };
                let status = services.call(
                    abi,
                    if system {
                        Service::QuerySystemTime
                    } else {
                        Service::QueryPerformanceCounter
                    },
                    &args[..if system { 1 } else { 2 }],
                    &mut image.memory,
                );
                match status {
                    ServiceResult::Return(0) => {}
                    ServiceResult::Return(s) => return Err(status_error(s).into()),
                    _ => return Err(ApiError::Unsupported),
                }
                let value = word(&mut image.memory, scratch, 8)?;
                let frequency = word(&mut image.memory, scratch + 8, 8)?;
                if name == "GetTickCount" || name == "GetTickCount64" {
                    if frequency == 0 {
                        return Err(INVALID_PARAMETER.into());
                    }
                    let ticks = (value as u128 * 1000 / frequency as u128) as u64;
                    return Ok(if name == "GetTickCount" {
                        ticks as u32 as u64
                    } else {
                        ticks
                    });
                }
                put(
                    &mut image.memory,
                    a[0],
                    if name == "QueryPerformanceFrequency" {
                        frequency
                    } else {
                        value
                    },
                    8,
                )?;
                Ok(if system { 0 } else { 1 })
            })();
            free(image, scratch)?;
            result?
        }
        "QueryUnbiasedInterruptTime" => {
            if a[0] == 0 { return Err(INVALID_PARAMETER.into()); }
            if !image.memory.probe_write(a[0], 8) { return Err(ApiError::MemoryFault); }
            let value = services.unbiased_interrupt_time().map_err(status_error)?;
            put(&mut image.memory, a[0], value, 8)?;
            1
        }
        "GetSystemTimeAdjustment" => {
            let snapshot = services.time_adjustment().map_err(status_error)?;
            for &address in &a[..3] {
                if !image.memory.probe_write(address, 4) {
                    return Err(ApiError::MemoryFault);
                }
            }
            for (&address, value) in a[..3].iter().zip([
                snapshot.adjustment, snapshot.increment, u32::from(snapshot.disabled),
            ]) {
                put(&mut image.memory, address, u64::from(value), 4)?;
            }
            1
        }
        "GetProcessTimes" => {
            // KERNEL_USER_TIMES has four 64-bit fields in both guest ABIs.
            // The native provider owns handle validation and accounting; an
            // unavailable provider must fail instead of inventing zero times.
            let scratch = allocate(image, 0, 32, READ | WRITE)?;
            let result = (|| -> Result<u64, ApiError> {
                match services.call(abi, Service::QueryInformationProcess,
                    &[a[0], 4, scratch, 32, 0], &mut image.memory) {
                    ServiceResult::Return(0) => {}
                    ServiceResult::Return(status) => return Err(status_error(status).into()),
                    _ => return Err(ApiError::Unsupported),
                }
                let snapshot = read(&mut image.memory, scratch, 32)?;
                for &address in &a[1..5] {
                    if !image.memory.probe_write(address, 8) { return Err(ApiError::MemoryFault); }
                }
                for (index, &address) in a[1..5].iter().enumerate() {
                    if !image.memory.write_virt(address, &snapshot[index * 8..index * 8 + 8]) {
                        return Err(ApiError::MemoryFault);
                    }
                }
                Ok(1)
            })();
            free(image, scratch)?;
            result?
        }
        "GetLastError" | "RtlGetLastWin32Error" => word(
            &mut image.memory,
            image.teb + if x86 { 0x34 } else { 0x68 },
            4,
        )?,
        "SetLastError" | "RtlSetLastWin32Error" => {
            error(image, a[0] as u32);
            0
        }
        "GetCurrentProcess" => {
            if x86 {
                u32::MAX as u64
            } else {
                u64::MAX
            }
        }
        "GetCurrentThread" => {
            if x86 {
                u32::MAX as u64 - 1
            } else {
                u64::MAX - 1
            }
        }
        "GetCurrentProcessId" => u64::from(image.process_id()),
        "CreateProcessA" | "CreateProcessW" => return process::create(image, a, services, name.ends_with('W')),
        "GetProcessId" | "GetExitCodeProcess" => return process::child_info(image, a, services, name),
        "GetPriorityClass" => image.process_priority_class(a[0])?,
        "SetPriorityClass" => image.set_process_priority_class(a[0], a[1] as u32)?,
        "SetProcessShutdownParameters" => {
            let flags = a[1] as u32;
            if flags & !1 == 0 {
                image.win32.shutdown_parameters = (a[0] as u32, flags);
            }
            1
        }
        "GetProcessShutdownParameters" => {
            if !image.memory.probe_write(a[0], 4) || !image.memory.probe_write(a[1], 4) {
                return Err(NOACCESS.into());
            }
            let (level, flags) = image.win32.shutdown_parameters;
            put(&mut image.memory, a[0], u64::from(level), 4)?;
            put(&mut image.memory, a[1], u64::from(flags), 4)?;
            1
        }
        "GetCurrentThreadId" => u64::from(image.thread_id()),
        "IsProcessorFeaturePresent" => {
            // Report the isolated interpreter's guarantees, never host CPUID.
            // Instruction fetch checks EXECUTE permissions (PF_NX_ENABLED=12).
            // SIMD, FPU and atomic instruction families are only partial; do
            // not advertise a family until its complete contract is supported.
            u64::from(a[0] as u32 == 12)
        }
        "IsDebuggerPresent" => 0,
        "GetVersion" => 0x1db10106,
        "GetProcessHeap" => HEAP,
        "GetProcessHeaps" => {
            let count = image.win32.private_heaps.len() + 1;
            if a[0] as u32 as usize >= count {
                let width = if abi == GuestAbi::X86_64 { 8 } else { 4 };
                if !image.memory.probe_write(a[1], count * width) {
                    return Err(NOACCESS.into());
                }
                for (index, handle) in core::iter::once(HEAP)
                    .chain(image.win32.private_heaps.iter().map(|heap| heap.handle)).enumerate()
                {
                    put(&mut image.memory, a[1] + (index * width) as u64, handle, width)?;
                }
            }
            count as u64
        }
        "GetFileType" => services.file_type(a[0]).map_err(|status| ApiError::Windows(status_error(status)))? as u64,
        "GetFileSizeEx" => {
            if !image.memory.probe_write(a[1], 8) { return Err(NOACCESS.into()); }
            let size = services.file_size(a[0]).map_err(status_error)?;
            put(&mut image.memory, a[1], size, 8)?;
            1
        }
        "SetFilePointer" => {
            let high = a[2];
            let origin = a[3] as u32;
            let low = a[1] as u32;
            let (distance, limit) = if high == 0 {
                (i64::from(low as i32), u64::from(u32::MAX))
            } else {
                if !image.memory.probe_write(high, 4) { return Err(NOACCESS.into()); }
                let upper = word(&mut image.memory, high, 4)?;
                (((upper << 32) | u64::from(low)) as i64, i64::MAX as u64)
            };
            let position = match services.seek_file(a[0], distance, origin, limit) {
                Ok(position) => position,
                Err(nt61::FileSeekError::SetStatus(status)) => {
                    if high != 0 { put(&mut image.memory, high, u64::from(u32::MAX), 4)?; }
                    return Err(status_error(status).into());
                }
                Err(failure) => return Err(file_seek_error(failure).into()),
            };
            if high != 0 { put(&mut image.memory, high, position >> 32, 4)?; }
            if position as u32 == u32::MAX { error(image, 0); }
            u64::from(position as u32)
        }
        "SetFilePointerEx" => {
            let (distance, output, origin) = if x86 {
                (a[1] as u32 as u64 | (a[2] << 32), a[3], a[4] as u32)
            } else {
                (a[1], a[2], a[3] as u32)
            };
            if origin > 2 { return Err(INVALID_PARAMETER.into()); }
            if output != 0 && !image.memory.probe_write(output, 8) { return Err(NOACCESS.into()); }
            let position = services.seek_file(a[0], distance as i64, origin, i64::MAX as u64)
                .map_err(file_seek_error)?;
            if output != 0 { put(&mut image.memory, output, position, 8)?; }
            1
        }
        "GetStdHandle" => match a[0] as u32 {
            0xfffffff6 => crate::arch::common::btl::io::STD_INPUT,
            0xfffffff5 => crate::arch::common::btl::io::STD_OUTPUT,
            0xfffffff4 => crate::arch::common::btl::io::STD_ERROR,
            _ => return Err(6.into()),
        },
        "GetStartupInfoA" | "GetStartupInfoW" => {
            let size = if x86 { 68 } else { 104 };
            if !image.memory.probe_write(a[0], size) { return Err(ApiError::MemoryFault); }
            let params = word(&mut image.memory, image.peb + if x86 {0x10} else {0x20}, width)?;
            if params == 0 { return Err(ApiError::MemoryFault); }
            let mut bytes = vec![0; size];
            bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
            // STARTUPINFO describes this process's actual RTL parameters.
            // Optional strings are NULL unless supplied by its creator.
            for (source, destination) in [(if x86 {0x78} else {0xc0}, 2 * width),
                                          (if x86 {0x70} else {0xb0}, 3 * width)] {
                let mut ptr = word(&mut image.memory, params + source + width as u64, width)?;
                if ptr != 0 && name.ends_with('A') {
                    let text = string(&mut image.memory, ptr, true)?;
                    ptr = allocate(image, 0, (text.len() + 1) as u64, READ | WRITE)?;
                    write_string(&mut image.memory, ptr, &text, false)?;
                }
                bytes[destination..destination + width].copy_from_slice(&ptr.to_le_bytes()[..width]);
            }
            let geometry = 4 * width;
            let source = read(&mut image.memory, params + if x86 {0x4c} else {0x88}, 36)?;
            bytes[geometry..geometry + 32].copy_from_slice(&source[..32]);
            bytes[geometry + 32..geometry + 34].copy_from_slice(&source[32..34]);
            let runtime = params + if x86 {0x88} else {0xe0};
            let count = read(&mut image.memory, runtime, 2)?;
            let pointer = word(&mut image.memory, runtime + width as u64, width)?;
            bytes[geometry + 34..geometry + 36].copy_from_slice(&count);
            let reserved2 = if x86 {52} else {72};
            bytes[reserved2..reserved2 + width].copy_from_slice(&pointer.to_le_bytes()[..width]);
            let handles = read(&mut image.memory, params + if x86 {0x18} else {0x20}, 3 * width)?;
            bytes[reserved2 + width..reserved2 + 4 * width].copy_from_slice(&handles);
            if !image.memory.write_virt(a[0], &bytes) { return Err(ApiError::MemoryFault); }
            0
        }
        "InitializeSRWLock" => {
            // SRWLOCK_INIT is a zero pointer in both guest ABIs. This only
            // initializes storage; contended locking needs a guest scheduler.
            if a[0] == 0 || !image.memory.write_virt(a[0], &[0; 8][..width]) {
                return Err(ApiError::MemoryFault);
            }
            0
        }
        "InitializeSListHead" => {
            let size = if x86 { 8 } else { 16 };
            if a[0] == 0 || a[0] % size as u64 != 0 { return Err(ApiError::MemoryFault); }
            let mut header = [0; 16];
            if !x86 {
                // Win7's 8-byte compare/exchange header encodes the address
                // region in its upper three bits; all supported user VAs are
                // in region zero. Depth, sequence, next and type start clear.
                header[8..16].copy_from_slice(&(a[0] & 0xe000_0000_0000_0000).to_le_bytes());
            }
            if !image.memory.write_virt(a[0], &header[..size]) { return Err(ApiError::MemoryFault); }
            0
        }
        "InterlockedFlushSList" => flush_slist(&mut image.memory, a[0], x86)?,
        "GetEnvironmentStrings" | "GetEnvironmentStringsW" => environment::get(image, name.ends_with('W'))?,
        "FreeEnvironmentStringsA" | "FreeEnvironmentStringsW" => environment::release(image, a[0], name.ends_with('W'))?,
        "GetEnvironmentVariableA" | "GetEnvironmentVariableW" => environment::variable(image, a, name.ends_with('W'))?,
        "GetTempPathA" | "GetTempPathW" => environment::temp_path(image, a, name.ends_with('W'))?,
        "GetCommandLineA" | "GetCommandLineW" => {
            let p = word(
                &mut image.memory,
                image.peb + if x86 { 0x10 } else { 0x20 },
                width,
            )?;
            let ptr = word(&mut image.memory, p + if x86 { 0x44 } else { 0x78 }, width)?;
            if name.ends_with('W') {
                ptr
            } else {
                let s = string(&mut image.memory, ptr, true)?;
                let ptr = allocate(image, 0, (s.len() + 1) as u64, READ | WRITE)?;
                write_string(&mut image.memory, ptr, &s, false)?;
                ptr
            }
        }
        "RtlPcToFileHeader" => {
            let base = image.module_metadata.iter()
                .find(|m| a[0] >= m.base && a[0] - m.base < m.size as u64)
                .map_or(0, |m| m.base);
            if a[1] == 0 || !image.memory.write_virt(a[1], &base.to_le_bytes()[..width]) {
                return Err(ApiError::MemoryFault);
            }
            base
        }
        "GetModuleFileNameA" | "GetModuleFileNameW" => {
            let capacity = a[2] as u32 as usize;
            let source = if a[0] != 0 && a[0] != image.image_base {
                let Some((_, path)) = image.win32.module_paths.iter().find(|(base, _)| *base == a[0]) else {
                    return if image.module_metadata.iter().any(|module| module.base == a[0]) {
                        Err(ApiError::Unsupported)
                    } else { Err(126.into()) };
                };
                if capacity == 0 { return Err(122.into()); }
                path.encode_utf16().flat_map(u16::to_le_bytes).collect()
            } else {
                if capacity == 0 { return Err(122.into()); }
                let params = word(&mut image.memory, image.peb + if x86 {0x10} else {0x20}, width)?;
                if params == 0 { return Err(ApiError::MemoryFault); }
                let descriptor = params + if x86 {0x38} else {0x60};
                let length = word(&mut image.memory, descriptor, 2)? as usize;
                if length % 2 != 0 { return Err(ApiError::MemoryFault); }
                let pointer = word(&mut image.memory, descriptor + width as u64, width)?;
                read(&mut image.memory, pointer, length)?
            };
            let wide = name.ends_with('W');
            let source = if wide { source } else {
                let mut ansi = Vec::with_capacity(source.len() / 2);
                for unit in source.chunks_exact(2) {
                    let unit = u16::from_le_bytes([unit[0], unit[1]]);
                    if unit > 127 { return Err(1113.into()); }
                    ansi.push(unit as u8);
                }
                ansi
            };
            let unit = if wide {2} else {1};
            let count = source.len() / unit;
            let copied = count.min(capacity - 1);
            let mut output = source[..copied * unit].to_vec();
            output.resize(output.len() + unit, 0);
            if !image.memory.write_virt(a[1], &output) { return Err(NOACCESS.into()); }
            if count >= capacity { error(image, 122); capacity as u64 } else { count as u64 }
        }
        "GetModuleHandleExW" => {
            let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
            if !image.memory.probe_write(a[2], width) {
                return Err(NOACCESS.into());
            }
            put(&mut image.memory, a[2], 0, width)?;
            let flags = a[0] as u32;
            if flags & !7 != 0 || flags & 3 == 3 {
                return Err(INVALID_PARAMETER.into());
            }
            if image.pending_library.is_some() && flags & 2 == 0 {
                return Err(120u32.into());
            }
            let base = if flags & 4 != 0 {
                // Use immutable loader extents, never guest-writable PE
                // SizeOfImage fields or arbitrary private/stack mappings.
                image.module_metadata.iter().find(|m| {
                    a[1] >= m.base && a[1] - m.base < m.size as u64
                }).map(|m| m.base).ok_or(126u32)?
            } else if a[1] == 0 {
                image.image_base
            } else {
                let name = string(&mut image.memory, a[1], true)?;
                module(image, &name).ok_or(126u32)?
            };
            if flags & 1 != 0 {
                if !image.win32.pinned_modules.contains(&base) {
                    image.win32.pinned_modules.try_reserve(1).map_err(|_| 8u32)?;
                    image.win32.pinned_modules.push(base);
                }
            } else if flags & 2 == 0 {
                if let Some((_, count)) = image.win32.library_loads.iter_mut().find(|m| m.0 == base) {
                    *count = count.checked_add(1).ok_or(8u32)?;
                } else {
                    image.win32.library_loads.try_reserve(1).map_err(|_| 8u32)?;
                    image.win32.library_loads.push((base, 1));
                }
            }
            put(&mut image.memory, a[2], base, width)?;
            1
        }
        "GetModuleHandleA" | "GetModuleHandleW" | "LoadLibraryA" | "LoadLibraryW"
        | "LoadLibraryExA" | "LoadLibraryExW" => {
            const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;
            const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;
            let loading = name.starts_with("LoadLibrary");
            image.win32.last_library_error = None;
            let extended = name.starts_with("LoadLibraryEx");
            let flags = if extended { a[2] as u32 } else { 0 };
            if extended && (a[1] != 0 || !matches!(flags, 0 | LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_WITH_ALTERED_SEARCH_PATH)) {
                return Err(120.into());
            }
            if a[0] == 0 {
                if loading {
                    return Err(INVALID_PARAMETER.into());
                }
                image.image_base
            } else {
                let mut n = string(&mut image.memory, a[0], name.ends_with('W'))?;
                let search = match flags {
                    LOAD_LIBRARY_SEARCH_SYSTEM32 => {
                        if n.contains(['\\', '/', ':']) { return Err(120.into()); }
                        LibrarySearch::System32
                    }
                    LOAD_WITH_ALTERED_SEARCH_PATH => {
                        if matches!(n.rsplit(['\\', '/']).next(), Some("" | "." | "..")) {
                            return Err(INVALID_PARAMETER.into());
                        }
                        n = guest_file_path(GuestAbi::X86_64, &n.replace('/', "\\"), false)
                            .map_err(|_| INVALID_PARAMETER)?;
                        let (directory, leaf) = n.rsplit_once('\\').ok_or(INVALID_PARAMETER)?;
                        if leaf.is_empty() || n[2..].contains(':') { return Err(INVALID_PARAMETER.into()); }
                        LibrarySearch::AlteredDirectory(directory)
                    }
                    _ => LibrarySearch::Default,
                };
                if loading {
                    if image.pending_library.is_some() { return Err(120u32.into()); }
                    image.win32.library_loads.try_reserve(1).map_err(|_| 8u32)?;
                }
                let base = if let Some(base) = module(image, &n) {
                    base
                } else if loading {
                    load_library(image, &n, services, search, true)?
                } else {
                    return Err(126.into());
                };
                if loading {
                    if let Some((_, count)) = image.win32.library_loads.iter_mut().find(|m| m.0 == base) {
                        *count = count.checked_add(1).ok_or(8u32)?;
                    } else {
                        image.win32.library_loads.push((base, 1));
                    }
                }
                base
            }
        }
        "GlobalMemoryStatus" | "GlobalMemoryStatusEx" => {
            let extended = name.ends_with("Ex");
            let width = if extended || image.thread.context.mode != DecodeMode::X86 {
                8
            } else {
                4
            };
            let size = if extended { 64 } else { 8 + 6 * width };
            if !image.memory.probe_write(a[0], size) {
                return Err(NOACCESS.into());
            }
            if extended && word(&mut image.memory, a[0], 4)? != 64 {
                return Err(INVALID_PARAMETER.into());
            }
            let stats = services.memory_status().map_err(|_| 120u32)?;
            if stats.physical_total == 0
                || stats.physical_available > stats.physical_total
                || stats.paging_available > stats.paging_total
            {
                return Err(120.into());
            }
            let total_commit = stats.physical_total.saturating_add(stats.paging_total);
            let available_commit = stats
                .physical_available
                .saturating_add(stats.paging_available);
            let virtual_total = MAX_PROCESS_MEMORY as u64;
            let mapped = image.memory.mapped_bytes() as u64;
            let virtual_available = virtual_total.saturating_sub(mapped);
            let load = (((stats.physical_total - stats.physical_available) as u128 * 100)
                / stats.physical_total as u128) as u32;
            let mut bytes = vec![0u8; size];
            bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
            bytes[4..8].copy_from_slice(&load.to_le_bytes());
            let values = [
                stats.physical_total,
                stats.physical_available,
                total_commit,
                available_commit,
                virtual_total,
                virtual_available,
            ];
            for (i, value) in values.iter().enumerate() {
                let value = if width == 4 {
                    (*value).min(u32::MAX as u64)
                } else {
                    *value
                };
                bytes[8 + i * width..8 + (i + 1) * width]
                    .copy_from_slice(&value.to_le_bytes()[..width]);
            }
            if !image.memory.write_virt(a[0], &bytes) {
                return Err(NOACCESS.into());
            }
            if extended {
                1
            } else {
                0
            }
        }
        "GetSystemInfo" | "GetNativeSystemInfo" => {
            let width = if x86 {4usize} else {8};
            let size = 8 + width * 3 + 16;
            if !image.memory.probe_write(a[0], size) { return Err(ApiError::MemoryFault); }
            let processor = if name == "GetNativeSystemInfo" {
                services.native_processor_info().map_err(|_| ApiError::Unsupported)?
            } else {
                nt61::NativeProcessorInfo {
                    architecture: if x86 {0} else {9},
                    processor_type: if x86 {586} else {8664},
                    level: 6,
                    revision: 0,
                }
            };
            let raw = services.processor_mask().map_err(|_| ApiError::Unsupported)?;
            let mask = if x86 {raw & u64::from(u32::MAX)} else {raw};
            if mask == 0 { return Err(ApiError::Unsupported); }
            let mut bytes = alloc::vec![0u8; size];
            bytes[..2].copy_from_slice(&processor.architecture.to_le_bytes());
            bytes[4..8].copy_from_slice(&4096u32.to_le_bytes());
            // Use the executable's SP1 application address model, including
            // WOW64 LARGEADDRESSAWARE. Backing capacity is reported separately.
            let maximum = if x86 && name == "GetNativeSystemInfo" { 0xfffe_ffff }
                else { image.user_address_end - 1 };
            for (index, value) in [0x10000u64, maximum, mask].into_iter().enumerate() {
                bytes[8+index*width..8+(index+1)*width].copy_from_slice(&value.to_le_bytes()[..width]);
            }
            let tail = 8 + width * 3;
            for (index, value) in [mask.count_ones(), processor.processor_type, 65536].into_iter().enumerate() {
                bytes[tail+index*4..tail+(index+1)*4].copy_from_slice(&value.to_le_bytes());
            }
            bytes[tail+12..tail+14].copy_from_slice(&processor.level.to_le_bytes());
            bytes[tail+14..tail+16].copy_from_slice(&processor.revision.to_le_bytes());
            if !image.memory.write_virt(a[0], &bytes) { return Err(ApiError::MemoryFault); }
            0
        }
        "GetProcessAffinityMask" => {
            let width = if image.thread.context.mode == DecodeMode::X86 {
                4
            } else {
                8
            };
            let pseudo = if width == 4 {
                u32::MAX as u64
            } else {
                u64::MAX
            };
            if a[0] != pseudo {
                return Err(6.into());
            }
            if !image.memory.probe_write(a[1], width) || !image.memory.probe_write(a[2], width) {
                return Err(NOACCESS.into());
            }
            let raw = services.processor_mask().map_err(|_| 120u32)?;
            let mask = if width == 4 {
                raw & u32::MAX as u64
            } else {
                raw
            };
            if mask == 0 {
                return Err(120.into());
            }
            put(&mut image.memory, a[1], mask, width)?;
            put(&mut image.memory, a[2], mask, width)?;
            1
        }
        "DisableThreadLibraryCalls" => {
            // Use the validated loader identity, not mutable PE headers or
            // an arbitrary address within a module. Static TLS needs thread
            // notifications; LdrDisableThreadCalloutsForDll rejects it with
            // STATUS_DLL_NOT_FOUND (Win32 ERROR_MOD_NOT_FOUND).
            let module = image.module_metadata.iter_mut().find(|m| m.base == a[0]).ok_or(126u32)?;
            if module.directories[9] != (0, 0) {
                return Err(126u32.into());
            }
            module.thread_notifications_disabled = true;
            1
        }
        "FreeLibrary" => return library::free(image, a[0], None),
        "GetProcAddress" => {
            let by_ordinal = a[1] <= 0xffff;
            let ordinal_failure = by_ordinal && image.win32.modules.iter().any(|m| m.1 == a[0]);
            let result = if by_ordinal {
                proc_address(image, a[0], None, a[1] as u16, 0, services)
            } else {
                let n = string(&mut image.memory, a[1], false)?;
                proc_address(image, a[0], Some(&n), 0, 0, services)
            };
            // SP1 reports lookup failure according to the original request,
            // including unavailable forwarded modules/symbols and ordinal
            // holes. Preserve other backend/resource failures as such.
            result.map_err(|code| match code {
                126 | 127 | 182 => if ordinal_failure { 182 } else { 127 },
                code => code,
            })?
        }
        "FlushInstructionCache" => {
            // SP1 skips handle validation for an empty, non-null range. A
            // null base requests the whole cache even when length is zero.
            if a[1] != 0 && a[2] == 0 { return Ok(1); }
            let pseudo = if image.thread.context.mode == DecodeMode::X86 {
                u32::MAX as u64
            } else { u64::MAX };
            if a[0] != pseudo { return Err(6u32.into()); }
            // Guest code is interpreted, not executed from a host I-cache.
            // Invalidate every thread's decoded instructions; no host pointer
            // is derived from the caller's optional address/length range.
            image.thread.invalidate_code();
            image.threads.invalidate_code();
            1
        }
        "VirtualAlloc" => {
            let kind = a[2] as u32;
            // Validate the requested length before allocation-granularity
            // padding is added. An unaligned address cannot make zero nonzero.
            if a[1] == 0 || kind & !0x3000 != 0 || kind == 0 {
                return Err(INVALID_PARAMETER.into());
            }
            let bits = protection(a[3] as u32)?;
            if a[0] != 0 && kind == 0x1000 {
                change_commit(image, a[0], a[1], bits, true)?;
                a[0] & !0xfff
            } else {
                let p = allocate(image, a[0], a[1], if kind & 0x1000 == 0 { 0 } else { bits })?;
                let length = image
                    .win32
                    .allocations
                    .iter()
                    .find(|a| a.0 == p)
                    .ok_or(INVALID_PARAMETER)?
                    .1;
                image.win32.virtual_allocations.push(VirtualAllocation {
                    base: p,
                    length,
                    committed: vec![kind & 0x1000 != 0; length / PAGE],
                });
                let region = image.memory.regions.iter_mut().find(|r| r.base == p)
                    .ok_or(INVALID_PARAMETER)?;
                region.allocation_protection = a[3] as u32;
                if kind & 0x1000 == 0 { region.protections.fill(0); }
                p
            }
        }
        "VirtualFree" => {
            match a[2] {
                0x8000 if a[1] == 0 => {
                    let i = image
                        .win32
                        .virtual_allocations
                        .iter()
                        .position(|v| v.base == a[0])
                        .ok_or(INVALID_PARAMETER)?;
                    free(image, a[0])?;
                    image.win32.virtual_allocations.remove(i);
                }
                0x4000 => {
                    let n = if a[1] == 0 {
                        image
                            .win32
                            .virtual_allocations
                            .iter()
                            .find(|v| v.base == a[0])
                            .ok_or(INVALID_PARAMETER)?
                            .length as u64
                    } else {
                        a[1]
                    };
                    change_commit(image, a[0], n, 0, false)?;
                }
                _ => return Err(INVALID_PARAMETER.into()),
            }
            1
        }
        "VirtualProtect" => {
            if !image.memory.probe_write(a[3], 4) {
                return Err(NOACCESS.into());
            }
            // lpflOldProtect may alias the changed pages. If publishing it
            // fails after permissions change, restore permissions atomically.
            let snapshot: Vec<(Vec<u8>, Vec<u32>)> = image
                .memory
                .regions
                .iter()
                .map(|r| (r.permissions.clone(), r.protections.clone()))
                .collect();
            let old = protect(image, a[0], a[1], a[2] as u32)?;
            if let Err(e) = put(&mut image.memory, a[3], old as u64, 4) {
                for (region, (permissions, protections)) in image.memory.regions.iter_mut().zip(snapshot) {
                    region.permissions = permissions;
                    region.protections = protections;
                }
                return Err(e.into());
            }
            image.thread.invalidate_code();
            1
        }
        "HeapSetInformation" => {
            if a[0] != 0 || a[1] as u32 != 1 || a[2] != 0 || a[3] != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            image.win32.terminate_on_heap_corruption = true;
            1
        }
        "HeapCreate" => {
            if a[0] as u32 & !1 != 0 || a[2] != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            image.win32.private_heaps.try_reserve(1).map_err(|_| 8u32)?;
            let handle = allocate(image, 0, a[1].max(PAGE as u64), READ | WRITE)?;
            image.win32.private_heaps.push(PrivateHeap { handle, blocks: Vec::new() });
            handle
        }
        "HeapDestroy" => {
            let index = image.win32.private_heaps.iter().position(|heap| heap.handle == a[0])
                .ok_or(INVALID_PARAMETER)?;
            while let Some(&(address, _)) = image.win32.private_heaps[index].blocks.last() {
                heap_free_in(image, a[0], address)?;
            }
            free(image, a[0])?;
            image.win32.private_heaps.remove(index);
            1
        }
        "HeapAlloc" | "HeapReAlloc" => {
            if a[1] & !9 != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            if name == "HeapReAlloc" {
                let (old, length) = image.win32.heap_blocks(a[0])
                    .and_then(|blocks| blocks.iter().find(|block| block.0 == a[2]))
                    .copied().ok_or(INVALID_PARAMETER)?;
                heap_move_in(image, a[0], old, length, a[3])?
            } else {
                heap_allocate_in(image, a[0], a[2])?
            }
        }
        "HeapFree" => {
            if a[1] & !1 != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            let tracked = image.win32.terminate_on_heap_corruption
                && image.win32.heap_blocks(a[0]).is_some_and(|blocks| blocks.iter().any(|block| block.0 == a[2]));
            heap_free_in(image, a[0], a[2]).map_err(|error| {
                if tracked { ApiError::CorruptHeap } else { error.into() }
            })?;
            1
        }
        "HeapSize" => {
            if a[1] & !1 != 0 {
                return Err(INVALID_PARAMETER.into());
            }
            image.win32.heap_blocks(a[0])
                .and_then(|blocks| blocks.iter().find(|block| block.0 == a[2]))
                .ok_or(INVALID_PARAMETER)?
                .1 as u64
        }
        "TlsAlloc" => {
            let slot = if let Some(i) = image.win32.tls.iter().position(|used| !used) { i }
                else {
                    if image.win32.tls.len() >= 1088 { return Err(259.into()); }
                    image.win32.tls.push(false);
                    image.win32.tls.len() - 1
                };
            image.clear_tls_slot(slot);
            image.win32.tls[slot] = true;
            slot as u64
        }
        "TlsFree" => {
            let slot = image.win32.tls.get_mut(a[0] as usize).filter(|s| **s).ok_or(INVALID_PARAMETER)?;
            *slot = false;
            image.clear_tls_slot(a[0] as usize);
            1
        }
        "TlsSetValue" | "TlsGetValue" => {
            let slot = a[0] as u32 as usize;
            if !image.win32.tls.get(slot).copied().unwrap_or(false) { return Err(INVALID_PARAMETER.into()); }
            image.win32.local.tls.resize(image.win32.tls.len(), 0);
            if name == "TlsSetValue" { image.win32.local.tls[slot] = a[1]; 1 }
            else { let value = image.win32.local.tls[slot]; error(image, 0); value }
        }
        "EncodePointer" | "DecodePointer" => a[0],
        "lstrlenA" | "lstrlenW" => {
            if a[0] == 0 {
                0
            } else {
                let s = string(&mut image.memory, a[0], name.ends_with('W'))?;
                if name.ends_with('W') {
                    s.encode_utf16().count() as u64
                } else {
                    s.len() as u64
                }
            }
        }
        "lstrcmpA" | "lstrcmpW" | "lstrcmpiA" | "lstrcmpiW" => {
            let wide = name.ends_with('W');
            let mut x = string(&mut image.memory, a[0], wide)?;
            let mut y = string(&mut image.memory, a[1], wide)?;
            if name.contains("cmpi") {
                x.make_ascii_lowercase();
                y.make_ascii_lowercase()
            }
            match x.cmp(&y) {
                core::cmp::Ordering::Less => u32::MAX as u64,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            }
        }
        "RtlInitUnicodeString" => {
            let (n, p) = if a[1] == 0 {
                (0, 0)
            } else {
                (
                    string(&mut image.memory, a[1], true)?
                        .encode_utf16()
                        .count()
                        * 2,
                    a[1],
                )
            };
            if n > 65532 {
                return Err(INVALID_PARAMETER.into());
            }
            put(&mut image.memory, a[0], n as u64, 2)?;
            put(
                &mut image.memory,
                a[0] + 2,
                if p == 0 { 0 } else { n as u64 + 2 },
                2,
            )?;
            put(&mut image.memory, a[0] + width as u64, p, width)?;
            0
        }
        "CreateFileA" | "CreateFileW" => {
            const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
            let access = a[1] as u32;
            let share = a[2] as u32;
            let disposition = a[4] as u32;
            let flags = a[5] as u32;
            let native_disposition = match disposition {
                1 => 2,
                2 => 5,
                3 => 1,
                4 => 3,
                5 => 4,
                _ => return Err(INVALID_PARAMETER.into()),
            };
            if a[6] != 0 {
                return Err(120.into());
            }
            if share & !7 != 0 {
                return Err(5.into());
            }
            if flags & !(0x0000_00a0 | FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED) != 0
                || flags & (FILE_FLAG_OVERLAPPED | FILE_FLAG_BACKUP_SEMANTICS) == (FILE_FLAG_OVERLAPPED | FILE_FLAG_BACKUP_SEMANTICS) {
                return Err(120.into());
            }
            let options = if flags & FILE_FLAG_BACKUP_SEMANTICS != 0 { 0x4020 }
                else if flags & FILE_FLAG_OVERLAPPED != 0 { 0x40 } else { 0x60 }
                | if flags & FILE_FLAG_SEQUENTIAL_SCAN != 0 { 4 } else { 0 };
            let mut object_attributes = 0x40;
            if a[3] != 0 {
                let size = width * 3;
                let mut attributes = [0; 24];
                if image.memory.read_virt(a[3], &mut attributes[..size]) != size {
                    return Err(NOACCESS.into());
                }
                if attributes[..4] != (size as u32).to_le_bytes() {
                    return Err(INVALID_PARAMETER.into());
                }
                if attributes[width..width * 2].iter().any(|byte| *byte != 0) {
                    return Err(120.into());
                }
                if attributes[width * 2..width * 2 + 4].iter().any(|byte| *byte != 0) {
                    object_attributes |= 2;
                }
            }
            let input = string(&mut image.memory, a[0], name.ends_with('W'))?;
            if input.is_empty() { return Err(123.into()); }
            let path = directory::path(image, &input)?;
            let utf16: Vec<u16> = path.encode_utf16().collect();
            if utf16.len() > 16000 {
                return Err(INVALID_PARAMETER.into());
            }
            let scratch = allocate(image, 0, (utf16.len() * 2 + 130) as u64, READ | WRITE)?;
            let result = (|| -> Result<(u64, u64), u32> {
                let us = scratch + 64;
                let buffer = scratch + 128;
                let iosb = scratch + 48;
                put(&mut image.memory, scratch, if x86 { 24 } else { 48 }, 4)?;
                put(
                    &mut image.memory,
                    scratch + if x86 { 8 } else { 16 },
                    us,
                    width,
                )?;
                put(
                    &mut image.memory,
                    scratch + if x86 { 12 } else { 24 },
                    object_attributes,
                    4,
                )?;
                put(&mut image.memory, us, (utf16.len() * 2) as u64, 2)?;
                put(&mut image.memory, us + 2, (utf16.len() * 2 + 2) as u64, 2)?;
                put(&mut image.memory, us + width as u64, buffer, width)?;
                write_string(&mut image.memory, buffer, &path, true)?;
                let out = scratch + 96;
                let args = [out, u64::from(access), scratch, iosb, 0, u64::from(flags & 0xffff), u64::from(share), native_disposition, options, 0, 0];
                match services.call(abi, Service::CreateFile, &args, &mut image.memory) {
                    ServiceResult::Return(0) => Ok((word(&mut image.memory, out, width)?, word(&mut image.memory, iosb + width as u64, width)?)),
                    ServiceResult::Return(0xc000_0035) if disposition == 1 => Err(80),
                    ServiceResult::Return(s) => Err(status_error(s)),
                    _ => Err(120),
                }
            })();
            free(image, scratch)?;
            let (handle, information) = result?;
            if matches!(disposition, 2 | 4) {
                error(image, if information == 2 { 0 } else { 183 });
            }
            handle
        }
        "SetFileTime" => {
            let mut times = [None; 3];
            for (index, (time, address)) in times.iter_mut().zip(&a[1..4]).enumerate() {
                if *address == 0 {
                    continue;
                }
                let value = word(&mut image.memory, *address, 8)?;
                match value {
                    0 => {},
                    u64::MAX if index != 0 => {},
                    value if value <= i64::MAX as u64 => *time = Some(value),
                    _ => return Err(INVALID_PARAMETER.into()),
                }
            }
            services.set_file_times(a[0], times).map_err(status_error)?;
            1
        }
        "CancelIo" => {
            services.cancel_file_io(a[0]).map_err(status_error)?;
            1
        }
        "GetOverlappedResult" => {
            services.validate_disk_file(a[0]).map_err(status_error)?;
            let completion = read(&mut image.memory, a[1], width * 2)?;
            let status = u32::from_le_bytes(completion[..4].try_into().unwrap());
            if status == 0x103 {
                return Err(if a[3] as u32 == 0 { 996u32 } else { 120u32 }.into());
            }
            let mut transferred = [0; 8];
            transferred[..width].copy_from_slice(&completion[width..]);
            put(&mut image.memory, a[2], u64::from_le_bytes(transferred), 4)?;
            if (status as i32) < 0 { return Err(status_error(status).into()); }
            1
        }
        "ReadFile" | "WriteFile" => {
            let length = a[2] as u32;
            if name == "WriteFile" && length > 65536 {
                return Err(120.into());
            }
            let service = if name == "ReadFile" { Service::ReadFile } else { Service::WriteFile };
            if a[4] != 0 {
                overlapped_file_io(image, services, abi, width, service, a, length)?;
                return Ok(1);
            }
            if a[3] == 0 || !image.memory.probe_write(a[3], 4) {
                return Err(NOACCESS.into());
            }
            put(&mut image.memory, a[3], 0, 4)?;
            if name == "ReadFile" && length != 0 && !image.memory.probe_write(a[1], length as usize) {
                return Err(NOACCESS.into());
            }
            let scratch = allocate(image, 0, 16, READ | WRITE)?;
            let args = [a[0], 0, 0, 0, scratch, a[1], u64::from(length), 0, 0];
            let result = (|| -> Result<u64, u32> {
                match services.call(
                    abi,
                    service,
                    &args,
                    &mut image.memory,
                ) {
                    ServiceResult::Return(s) if s == 0 || s == 0xc0000011 => {
                        let n = word(&mut image.memory, scratch + width as u64, width)?;
                        put(&mut image.memory, a[3], n, 4)?;
                        Ok(1)
                    }
                    ServiceResult::Return(s) => Err(status_error(s)),
                    _ => Err(120),
                }
            })();
            free(image, scratch)?;
            result?
        }
        "memcpy" | "memmove" => {
            if a[2] == 0 {
                return Ok(a[0]);
            }
            let n = usize::try_from(a[2]).map_err(|_| ApiError::Unsupported)?;
            if n > MAX_BUFFER {
                return Err(ApiError::Unsupported);
            }
            let b = read(&mut image.memory, a[1], n).map_err(|_| ApiError::MemoryFault)?;
            if !image.memory.write_virt(a[0], &b) {
                return Err(ApiError::MemoryFault);
            }
            a[0]
        }
        "memset" => {
            if a[2] == 0 {
                return Ok(a[0]);
            }
            let n = usize::try_from(a[2]).map_err(|_| ApiError::Unsupported)?;
            if n > MAX_BUFFER {
                return Err(ApiError::Unsupported);
            }
            if !image.memory.write_virt(a[0], &vec![a[1] as u8; n]) {
                return Err(ApiError::MemoryFault);
            }
            a[0]
        }
        "memcmp" => {
            if a[2] == 0 {
                return Ok(0);
            }
            let n = usize::try_from(a[2]).map_err(|_| ApiError::Unsupported)?;
            if n > MAX_BUFFER {
                return Err(ApiError::Unsupported);
            }
            let x = read(&mut image.memory, a[0], n).map_err(|_| ApiError::MemoryFault)?;
            let y = read(&mut image.memory, a[1], n).map_err(|_| ApiError::MemoryFault)?;
            x.iter()
                .zip(y.iter())
                .find(|(x, y)| x != y)
                // Both SP1 NTDLL and MSVCRT return a sign for memcmp,
                // independently of the magnitude of the byte difference.
                .map_or(0, |(x, y)| (i32::from(*x) - i32::from(*y)).signum() as u32 as u64)
        }
        "DuplicateHandle" => super::threads::duplicate(image, a)?,
        "GetHandleInformation" => {
            let flags = if super::threads::is_handle(a[0]) {
                super::threads::handle_flags(image, a[0])?
            } else {
                services.file_handle_flags(a[0]).map_err(status_error)?
            };
            put(&mut image.memory, a[1], u64::from(flags), 4)?;
            1
        }
        "SetHandleInformation" if super::threads::is_handle(a[0]) => super::threads::set_handle_information(image, a)?,
        "CloseHandle" if sync::is_wait_handle(a[0]) => {
            sync::close_wait_object(image, a[0])?;
            1
        }
        "CloseHandle" if super::threads::is_handle(a[0]) => {
            super::threads::close(image, a[0])?;
            1
        }
        "CloseHandle" => {
            if mapping::close(image, services, a[0]) { return Ok(1); }
            let status = match close_token(image, a[0]) {
                Some(status) => status,
                None => match services.call(abi, Service::Close, &a[..1], &mut image.memory) {
                    ServiceResult::Return(status) => status,
                    _ => return Err(ApiError::Unsupported),
                },
            };
            if status != 0 {
                return Err(status_error(status).into());
            }
            1
        }
        "CryptBinaryToStringA" | "CryptBinaryToStringW" => {
            binary_to_string(&mut image.memory, a, name.ends_with('W'))?
        }
        "CryptStringToBinaryA" | "CryptStringToBinaryW" => {
            string_to_binary(&mut image.memory, a, name.ends_with('W'))?
        }
        _ => return Err(ApiError::Unsupported),
    })
}

pub(super) fn sleep_counter(
    abi: GuestAbi,
    services: &mut dyn GuestServices,
    memory: &mut dyn GuestMemory,
    scratch: u64,
) -> Result<(u64, u64), ApiError> {
    match services.call(abi, Service::QueryPerformanceCounter, &[scratch, scratch + 8], memory) {
        ServiceResult::Return(0) => {}
        _ => return Err(ApiError::Unsupported),
    }
    let counter = word(memory, scratch, 8)?;
    let frequency = word(memory, scratch + 8, 8)?;
    if frequency == 0 {
        return Err(ApiError::Unsupported);
    }
    Ok((counter, frequency))
}
fn flush_slist(memory: &mut super::ImageMemory, address: u64, x86: bool) -> Result<u64, ApiError> {
    let size = if x86 { 8 } else { 16 };
    if address == 0 || address % size as u64 != 0 {
        return Err(ApiError::MemoryFault);
    }
    let mut header = [0; 16];
    if memory.read_virt(address, &mut header[..size]) != size {
        return Err(ApiError::MemoryFault);
    }
    let low = u64::from_le_bytes(header[..8].try_into().unwrap());
    let high = u64::from_le_bytes(header[8..].try_into().unwrap());
    let (entry, replacement, changed) = if x86 {
        (u64::from(low as u32), 0xffff_0000_0000_0000, 8)
    } else if high & 1 != 0 {
        (high & !0xf, low & !0xffff, 16)
    } else {
        (low & !0x01ff_ffff, low & 0x01ff_0000, 8)
    };
    if entry == 0 {
        return Ok(0);
    }
    header[..8].copy_from_slice(&replacement.to_le_bytes());
    if changed == 16 {
        header[8..].copy_from_slice(&1u64.to_le_bytes());
    }
    if !memory.write_virt(address, &header[..changed]) {
        return Err(ApiError::MemoryFault);
    }
    Ok(if !x86 && changed == 8 { entry >> 21 } else { entry })
}

fn wow64_fs_redirection(image: &mut GuestImage, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    let rtl = name.starts_with("Rtl");
    let status = if image.thread.context.mode != DecodeMode::X86 {
        nt61::STATUS_NOT_IMPLEMENTED
    } else {
        let (next, previous) = match name {
            "Wow64DisableWow64FsRedirection" => (1, Some(args[0])),
            "Wow64RevertWow64FsRedirection" => (args[0], None),
            "RtlWow64EnableFsRedirectionEx" => (args[0], Some(args[1])),
            _ => (u64::from(args[0] as u8 == 0), None),
        };
        if next > 1 {
            0xc000000d
        } else if previous.is_some_and(|address| !image.memory.probe_write(address, 4)) {
            nt61::STATUS_ACCESS_VIOLATION
        } else {
            if let Some(address) = previous {
                put(&mut image.memory, address, u64::from(image.win32.local.fs_redirection_disabled), 4)?;
            }
            image.win32.local.fs_redirection_disabled = next != 0;
            0
        }
    };
    if rtl {
        return Ok(u64::from(status));
    }
    if status == 0 {
        return Ok(1);
    }
    let last_status = if image.thread.context.mode == DecodeMode::X86 { 0xbf4 } else { 0x1250 };
    put(&mut image.memory, image.teb + last_status, u64::from(status), 4)?;
    Err(ApiError::Windows(if status == nt61::STATUS_NOT_IMPLEMENTED { 1 } else { status_error(status) }))
}

fn file_seek_error(error: nt61::FileSeekError) -> u32 {
    match error {
        nt61::FileSeekError::Status(status) | nt61::FileSeekError::SetStatus(status) => status_error(status),
        nt61::FileSeekError::NegativePosition => 131,
        nt61::FileSeekError::PositionTooLarge => INVALID_PARAMETER,
    }
}

fn overlapped_file_io(image: &mut GuestImage, services: &mut dyn GuestServices, abi: GuestAbi,
    width: usize, service: Service, args: &[u64], length: u32) -> Result<(), ApiError> {
    let file = args[0];
    let buffer = args[1];
    let count = args[3];
    let overlapped = args[4];
    services.validate_disk_file(file).map_err(status_error)?;
    let offset = overlapped.checked_add(2 * width as u64).ok_or(NOACCESS)?;
    let event = word(&mut image.memory, offset.checked_add(8).ok_or(NOACCESS)?, width)? & !1;
    word(&mut image.memory, offset, 8)?;
    if !image.memory.probe_write(overlapped, width * 2)
        || (count != 0 && !image.memory.probe_write(count, 4)) {
        return Err(NOACCESS.into());
    }
    if count != 0 { put(&mut image.memory, count, 0, 4)?; }
    if event != 0 { services.change_event(event, false).map_err(status_error)?; }
    let transferred_address = overlapped + width as u64;
    put(&mut image.memory, transferred_address, 0, width)?;
    let request = [file, 0, 0, 0, overlapped, buffer, u64::from(length), offset, 0];
    let ServiceResult::Return(status) = services.call(abi, service, &request, &mut image.memory) else {
        return Err(ApiError::Unsupported);
    };
    put(&mut image.memory, overlapped, u64::from(status), width)?;
    if status == 0x103 { return Err(997u32.into()); }
    let transferred = word(&mut image.memory, transferred_address, width)?;
    if count != 0 { put(&mut image.memory, count, transferred, 4)?; }
    if event != 0 { services.change_event(event, true).map_err(status_error)?; }
    if status == 0 { Ok(()) } else { Err(status_error(status).into()) }
}

pub(super) fn status_error(s: u32) -> u32 {
    match s {
        0xc0000005 => NOACCESS,
        0xc0000008 => 6,
        0xc0000017 => 8,
        0xc000011f => 4,
        0xc000000d => 87,
        0xc0000022 => 5,
        0xc0000034 | 0xc000000f => 2,
        0xc0000035 => 183,
        0xc000003a => 3,
        0xc0000043 => 32,
        0xc0000023 => 122,
        0xc0000054 | 0xc0000055 => 33,
        0xc0000061 => 1314,
        0xc0000079 => 1338,
        0xc000007e => 158,
        0xc0000101 => 145,
        0xc0000103 => 267,
        0xc00000ba => 5,
        0xc00000a2 => 19,
        0xc00000a5 => 1346,
        0xc0000011 => 38,
        0xc0000185 => 1117,
        0xc0000002 | 0xc00000bb => 120,
        _ => 1,
    }
}

fn volume_space(image: &mut GuestImage, services: &mut dyn GuestServices, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    let extended = matches!(name, "GetDiskFreeSpaceExA" | "GetDiskFreeSpaceExW");
    let requested = process::process_string(&mut image.memory, args[0], name.ends_with('W'))?;
    let directory = if extended {
        directory::path(image, requested.as_deref().unwrap_or(""))?
    } else if let Some(root) = requested.as_deref().filter(|root| *root != "\\") {
        match root.as_bytes() {
            [drive, b':', b'\\' | b'/'] if drive.is_ascii_alphabetic() => root.replace('/', "\\"),
            _ => return Err(123u32.into()),
        }
    } else {
        let cwd = directory::path(image, "")?;
        cwd.get(..3).filter(|root| root.as_bytes().get(1..) == Some(b":\\"))
            .ok_or(15u32)?.into()
    };
    let width = if extended { 8 } else { 4 };
    let outputs = &args[1..if extended { 4 } else { 5 }];
    if outputs.iter().any(|&out| !(extended && out == 0) && !image.memory.probe_write(out, width)) {
        return Err(NOACCESS.into());
    }
    let abi = if image.thread.context.mode == DecodeMode::X86 { GuestAbi::X86_32 } else { GuestAbi::X86_64 };
    let allocation = services.volume_allocation(abi, &directory).map_err(|status| match status {
        0xc000_0034 => 15,
        other => status_error(other),
    })?;
    let values = if extended {
        let cluster = u64::from(allocation.bytes_per_sector) * u64::from(allocation.sectors_per_cluster);
        let free = allocation.free_clusters.checked_mul(cluster).ok_or(534u32)?;
        [free, allocation.total_clusters.checked_mul(cluster).ok_or(534u32)?, free, 0]
    } else {
        let total = u32::try_from(allocation.total_clusters).map_err(|_| 534u32)?;
        let free = u32::try_from(allocation.free_clusters).map_err(|_| 534u32)?;
        [allocation.sectors_per_cluster.into(), allocation.bytes_per_sector.into(), free.into(), total.into()]
    };
    for (&out, value) in outputs.iter().zip(values) {
        if out != 0 { put(&mut image.memory, out, value, width)?; }
    }
    Ok(1)
}
const BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn encode64(b: &[u8]) -> String {
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | c.get(2).copied().unwrap_or(0) as u32;
        s.push(BASE64[(n >> 18) as usize] as char);
        s.push(BASE64[((n >> 12) & 63) as usize] as char);
        s.push(if c.len() > 1 {
            BASE64[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            BASE64[(n & 63) as usize] as char
        } else {
            '='
        })
    }
    s
}
fn binary_to_string(m: &mut dyn GuestMemory, a: &[u64], wide: bool) -> Result<u64, u32> {
    let flags = a[2] as u32;
    let kind = flags & 0x0fff_ffff;
    if flags & !0xcfff_ffff != 0 || !matches!(kind, 0 | 1 | 3 | 12) {
        return Err(INVALID_PARAMETER);
    }
    let b = read(
        m,
        a[0],
        usize::try_from(a[1]).map_err(|_| INVALID_PARAMETER)?,
    )?;
    let capacity = word(m, a[4], 4)? as usize;
    let mut s = String::new();
    if kind == 12 {
        for v in &b {
            use core::fmt::Write;
            let _ = write!(s, "{v:02x}");
        }
    } else if kind == 3 {
        return Err(120);
    } else {
        let encoded = encode64(&b);
        let newline = if flags & 0x4000_0000 != 0 {
            ""
        } else if flags & 0x8000_0000 != 0 {
            "\n"
        } else {
            "\r\n"
        };
        if kind == 0 {
            s.push_str("-----BEGIN CERTIFICATE-----");
            s.push_str(newline)
        }
        for c in encoded.as_bytes().chunks(64) {
            s.push_str(core::str::from_utf8(c).map_err(|_| 13u32)?);
            s.push_str(newline)
        }
        if kind == 0 {
            s.push_str("-----END CERTIFICATE-----");
            s.push_str(newline)
        }
    }
    let required = if wide {
        s.encode_utf16().count()
    } else {
        s.len()
    };
    if a[3] == 0 {
        put(m, a[4], (required + 1) as u64, 4)?;
        return Ok(1);
    }
    if capacity < required + 1 {
        put(m, a[4], (required + 1) as u64, 4)?;
        return Err(234);
    }
    if !m.probe_write(a[3], (required + 1) * if wide { 2 } else { 1 }) {
        return Err(NOACCESS);
    }
    write_string(m, a[3], &s, wide)?;
    put(m, a[4], required as u64, 4)?;
    Ok(1)
}
fn decode64(s: &str) -> Result<Vec<u8>, u32> {
    let b: Vec<u8> = s
        .bytes()
        .filter(|c| c.is_ascii_whitespace() == false)
        .collect();
    if b.len() % 4 != 0 {
        return Err(13);
    }
    let mut out = Vec::new();
    for (i, c) in b.chunks(4).enumerate() {
        let last = i * 4 + 4 == b.len();
        let pad = if c[2] == b'=' {
            2
        } else if c[3] == b'=' {
            1
        } else {
            0
        };
        if pad > 0 && !last || pad == 2 && c[3] != b'=' {
            return Err(13);
        }
        let mut v = [0u32; 4];
        for j in 0..4 - pad {
            v[j] = BASE64.iter().position(|x| *x == c[j]).ok_or(13u32)? as u32
        }
        if pad == 2 && v[1] & 15 != 0 || pad == 1 && v[2] & 3 != 0 {
            return Err(13);
        }
        let n = v[0] << 18 | v[1] << 12 | v[2] << 6 | v[3];
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8)
        }
        if pad < 1 {
            out.push(n as u8)
        }
    }
    Ok(out)
}
fn string_to_binary(m: &mut dyn GuestMemory, a: &[u64], wide: bool) -> Result<u64, u32> {
    let kind = a[2] as u32;
    if !matches!(kind, 1 | 12) {
        return Err(INVALID_PARAMETER);
    }
    let s = if a[1] == 0 {
        string(m, a[0], wide)?
    } else {
        let n = usize::try_from(a[1]).map_err(|_| INVALID_PARAMETER)?;
        let b = read(
            m,
            a[0],
            n.checked_mul(if wide { 2 } else { 1 })
                .ok_or(INVALID_PARAMETER)?,
        )?;
        if wide {
            String::from_utf16(
                &b.chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| 13u32)?
        } else {
            String::from_utf8(b).map_err(|_| 13u32)?
        }
    };
    let b = if kind == 1 {
        decode64(&s)?
    } else {
        let hex: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if hex.len() % 2 != 0 {
            return Err(13);
        }
        let mut b = Vec::new();
        for c in hex.chunks_exact(2) {
            let h = (c[0] as char).to_digit(16).ok_or(13u32)?;
            let l = (c[1] as char).to_digit(16).ok_or(13u32)?;
            b.push((h * 16 + l) as u8)
        }
        b
    };
    let capacity = word(m, a[4], 4)?;
    for p in [a[5], a[6]] {
        if p != 0 && !m.probe_write(p, 4) {
            return Err(NOACCESS);
        }
    }
    if !m.probe_write(a[4], 4) {
        return Err(NOACCESS);
    }
    if a[3] != 0 && capacity < b.len() as u64 {
        put(m, a[4], b.len() as u64, 4)?;
        return Err(234);
    }
    if a[3] != 0 && !m.write_virt(a[3], &b) {
        return Err(NOACCESS);
    }
    put(m, a[4], b.len() as u64, 4)?;
    if a[5] != 0 {
        put(m, a[5], 0, 4)?
    }
    if a[6] != 0 {
        put(m, a[6], kind as u64, 4)?
    }
    Ok(1)
}

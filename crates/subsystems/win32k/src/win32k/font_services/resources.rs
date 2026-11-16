//! Process-private memory fonts. Logical font realizations and file-query IDs
//! retain an Arc, so removing a resource cannot invalidate outstanding readers.
use super::objects;
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{copy_from_user, probe_user_write, write_user_object};
use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use nt61_fonts::{FontFace, SfntFont};

const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
static RETAINED_BYTES: AtomicUsize = AtomicUsize::new(0);

struct Reservation(usize);
impl Reservation {
    fn new(size: usize) -> Option<Self> {
        RETAINED_BYTES
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(size)
                    .filter(|&sum| sum <= MAX_RETAINED_BYTES)
            })
            .ok()?;
        Some(Self(size))
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        RETAINED_BYTES.fetch_sub(self.0, Ordering::Relaxed);
    }
}

pub(super) struct MemoryFont {
    process: u64,
    data: Vec<u8>,
    family: Vec<u16>,
    weight: u16,
    italic: bool,
    pub(super) rasterizer: Spinlock<Option<FontFace>>,
    _reservation: Reservation,
}

#[derive(Clone)]
pub(super) enum FontSource {
    BuiltIn(bool),
    Memory(Arc<MemoryFont>),
}

impl FontSource {
    pub(super) fn belongs_to(&self, process: u64) -> bool {
        matches!(self, Self::Memory(font) if font.process == process)
    }

    pub(super) fn accessible(&self) -> bool {
        match self {
            Self::BuiltIn(_) => true,
            Self::Memory(font) => {
                font.process == super::super::window::input_context::caller_identity().0
            }
        }
    }

    pub(super) fn data(&self) -> &[u8] {
        match self {
            Self::BuiltIn(true) => nt61_fonts::TERMINAL_FONT,
            Self::BuiltIn(false) => nt61_fonts::UI_FONT,
            Self::Memory(font) => &font.data,
        }
    }

    pub(super) fn family(&self) -> Vec<u16> {
        match self {
            Self::BuiltIn(mono) => if *mono {
                "DejaVu Sans Mono"
            } else {
                "DejaVu Sans"
            }
            .encode_utf16()
            .collect(),
            Self::Memory(font) => font.family.clone(),
        }
    }
}

struct Resource {
    handle: u64,
    process: u64,
    font: Arc<MemoryFont>,
}
struct Binding {
    handle: u64,
    process: u64,
    source: FontSource,
}
struct Registry {
    next_handle: u64,
    resources: Vec<Resource>,
    bindings: Vec<Binding>,
}
static REGISTRY: Spinlock<Registry> = Spinlock::new(Registry {
    next_handle: 0x7000_0000,
    resources: Vec::new(),
    bindings: Vec::new(),
});

fn same_family(left: &[u16], right: &[u16]) -> bool {
    // SFNT family names are UTF-16. Matching includes non-ASCII case pairs.
    match (String::from_utf16(left), String::from_utf16(right)) {
        (Ok(left), Ok(right)) => left.to_lowercase() == right.to_lowercase(),
        _ => false,
    }
}

/// Called with the GDI table locked, preventing deletion/reuse of the logical
/// font handle until its binding has been installed.
pub(super) fn select(requested: &objects::GdiFont, handle: u64) -> Option<FontSource> {
    let process = super::super::window::input_context::caller_identity().0;
    let name: Vec<u16> = requested
        .face_name
        .iter()
        .copied()
        .take_while(|&unit| unit != 0)
        .collect();
    let mut registry = REGISTRY.lock();
    if handle != 0 {
        if let Some(binding) = registry
            .bindings
            .iter()
            .find(|entry| entry.handle == handle)
        {
            return Some(binding.source.clone());
        }
    }
    let selected = registry
        .resources
        .iter()
        .rev()
        .filter(|entry| entry.process == process && same_family(&name, &entry.font.family))
        .min_by_key(|entry| {
            i64::from(requested.weight).abs_diff(i64::from(entry.font.weight))
                + if (requested.italic != 0) == entry.font.italic {
                    0
                } else {
                    1000
                }
        })
        .map(|entry| FontSource::Memory(Arc::clone(&entry.font)));
    let source = selected.unwrap_or_else(|| {
        let mono = requested.pitch_and_family & 3 == 1
            || [
                "DejaVu Sans Mono",
                "Courier New",
                "Consolas",
                "Fixedsys",
                "Terminal",
            ]
            .iter()
            .any(|expected| same_family(&name, &expected.encode_utf16().collect::<Vec<_>>()));
        FontSource::BuiltIn(mono)
    });
    if handle != 0 {
        registry.bindings.try_reserve(1).ok()?;
        registry.bindings.push(Binding {
            handle,
            process,
            source: source.clone(),
        });
    }
    Some(source)
}

pub(crate) fn forget_font(handle: u64) {
    REGISTRY
        .lock()
        .bindings
        .retain(|entry| entry.handle != handle);
}

pub(super) fn release_process(process: u64) {
    let mut registry = REGISTRY.lock();
    registry.resources.retain(|entry| entry.process != process);
    registry.bindings.retain(|entry| entry.process != process);
}

/// Win7's native call has five slots: bytes, length, design vector, design
/// vector length, and font-count output. The Win32 wrapper has four arguments.
pub extern "C" fn nt_gdi_add_font_mem_resource_ex(frame: &SyscallFrame) -> u64 {
    let result = (|| {
        let size = frame.args[1] as u32 as usize;
        // Multiple-master design vectors and collections need separate face
        // indexing. Reject them until the engine supports those file formats.
        if size == 0 || size > MAX_FONT_BYTES || frame.args[2] != 0 || frame.args[3] != 0 {
            return None;
        }
        probe_user_write(frame.args[4], 4).ok()?;
        let reservation = Reservation::new(size)?;
        let mut data = Vec::new();
        data.try_reserve_exact(size).ok()?;
        data.resize(size, 0);
        copy_from_user(&mut data, frame.args[0]).ok()?;
        if data.starts_with(b"ttcf") {
            return None;
        }
        let parsed = SfntFont::parse(&data)?;
        let description = parsed.description()?;
        let family = parsed.family_name()?;
        // Validate both consumers before publishing a selectable resource.
        let rasterizer = FontFace::from_bytes(&data).ok()?;
        let process = super::super::window::input_context::caller_identity().0;
        let font = Arc::new(MemoryFont {
            process,
            data,
            family,
            weight: description.weight,
            italic: description.italic,
            rasterizer: Spinlock::new(Some(rasterizer)),
            _reservation: reservation,
        });
        let handle = {
            let mut registry = REGISTRY.lock();
            if registry.resources.len() >= 256 {
                return None;
            }
            let handle = registry.next_handle;
            registry.next_handle = handle
                .checked_add(1)
                .filter(|&next| next < u32::MAX as u64)?;
            registry.resources.try_reserve(1).ok()?;
            registry.resources.push(Resource {
                handle,
                process,
                font,
            });
            handle
        };
        if write_user_object(frame.args[4], &1u32).is_err() {
            REGISTRY
                .lock()
                .resources
                .retain(|entry| entry.handle != handle);
            return None;
        }
        Some(handle)
    })();
    result.unwrap_or(0)
}

pub extern "C" fn nt_gdi_remove_font_mem_resource_ex(frame: &SyscallFrame) -> u64 {
    let process = super::super::window::input_context::caller_identity().0;
    let mut registry = REGISTRY.lock();
    let Some(index) = registry
        .resources
        .iter()
        .position(|entry| entry.handle == frame.args[0] && entry.process == process)
    else {
        return 0;
    };
    registry.resources.remove(index);
    1
}

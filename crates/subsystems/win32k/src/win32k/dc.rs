//! Device Context (DC) Management
//
//! Implements DC object allocation, attribute management, and DC state.
//! DC is the fundamental abstraction for GDI rendering operations.
//
//! ## Windows 7 DC Architecture
//
//! A DC encapsulates:
//! - The destination surface (screen, memory, printer)
//! - Current GDI objects (pen, brush, font, palette, region)
//! - Drawing attributes (colors, modes, transformations)
//! - Clipping regions
//
//! ## Magic Numbers and Constants
//
//! | Constant | Value | Description |
//! |----------|-------|-------------|
//! | `DC_TYPE_WINDOW` | 0x00000001 | Window DC type |
//! | `DC_TYPE_MEMORY` | 0x00000002 | Memory DC type |
//! | `DC_TYPE_METAFILE` | 0x00000004 | Metafile DC type |
//! | `DC_TYPE_PRINTER` | 0x00000008 | Printer DC type |
//! | `DC_TYPE_DISPLAY` | 0x00000001 | Display DC (alias of window) |
//
//! | Mapping Mode | Value | Description |
//! |--------------|-------|-------------|
//! | `MM_TEXT` | 1 | 1 logical unit = 1 pixel |
//! | `MM_LOMETRIC` | 2 | 1 logical unit = 0.1 mm |
//! | `MM_HIMETRIC` | 3 | 1 logical unit = 0.01 mm |
//! | `MM_LOENGLISH` | 4 | 1 logical unit = 0.01 inch |
//! | `MM_HIENGLISH` | 5 | 1 logical unit = 0.001 inch |
//! | `MM_TWIPS` | 6 | 1 logical unit = 1/1440 inch |
//! | `MM_ISOTROPIC` | 7 | Isotropic scaling |
//! | `MM_ANISOTROPIC` | 8 | Anisotropic scaling |
//
//! Reference: ReactOS win32ss/gdi/dc, Geoff Chappell

extern crate alloc;

use super::region_geometry::Region;
use alloc::vec::Vec;

#[path = "dc/mapping.rs"]
pub(crate) mod mapping;

#[path = "dc/services.rs"]
pub mod services;
#[path = "dc/caps.rs"]
pub mod caps;
#[path = "dc/bounds.rs"]
pub mod bounds;
#[path = "dc/info.rs"]
pub mod info;
#[path = "dc/mirror.rs"]
pub mod mirror;

/// DC type flags
const DC_TYPE_WINDOW: u32 = 0x00000001;
const DC_TYPE_MEMORY: u32 = 0x00000002;
pub const DC_TYPE_METAFILE: u32 = 0x00000004;
pub const DC_TYPE_PRINTER: u32 = 0x00000008;
pub const DC_TYPE_INFORMATION: u32 = 0x00000010;
const DC_TYPE_DISPLAY: u32 = DC_TYPE_WINDOW;

/// Mapping modes
pub const MM_TEXT: i32 = 1;
pub const MM_LOMETRIC: i32 = 2;
pub const MM_HIMETRIC: i32 = 3;
pub const MM_LOENGLISH: i32 = 4;
pub const MM_HIENGLISH: i32 = 5;
pub const MM_TWIPS: i32 = 6;
pub const MM_ISOTROPIC: i32 = 7;
pub const MM_ANISOTROPIC: i32 = 8;

/// Background modes
pub const TRANSPARENT: i32 = 1;
pub const OPAQUE: i32 = 2;

/// R2 raster operation modes
pub const R2_BLACK: i32 = 1;
pub const R2_NOTMERGEPEN: i32 = 2;
pub const R2_MASKNOTPEN: i32 = 3;
pub const R2_NOTCOPYPEN: i32 = 4;
pub const R2_MASKPENNOT: i32 = 5;
pub const R2_INVERT: i32 = 6;
pub const R2_XORPEN: i32 = 7;
pub const R2_NOTMASKPEN: i32 = 8;
pub const R2_MASKPEN: i32 = 9;
pub const R2_NOTXORPEN: i32 = 10;
pub const R2_NOP: i32 = 11;
pub const R2_MERGENOTPEN: i32 = 12;
pub const R2_COPYPEN: i32 = 13;
pub const R2_MERGEPENNOT: i32 = 14;
pub const R2_MERGEPEN: i32 = 15;
pub const R2_WHITE: i32 = 16;

/// Text alignment flags
pub const TA_NOUPDATECP: i32 = 0;
pub const TA_UPDATECP: i32 = 1;
pub const TA_LEFT: i32 = 0;
pub const TA_CENTER: i32 = 6;
pub const TA_RIGHT: i32 = 2;
pub const TA_TOP: i32 = 0;
pub const TA_BOTTOM: i32 = 8;
pub const TA_BASELINE: i32 = 24;

/// Point structure (LONG)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PointL {
    pub x: i32,
    pub y: i32,
}

/// 2x3 coordinate transform used by GDI world transforms.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Xform {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Xform {
    pub const IDENTITY: Self = Self {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 0.0,
        dy: 0.0,
    };
}

impl PointL {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Size structure
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SizeL {
    pub cx: i32,
    pub cy: i32,
}

impl SizeL {
    pub fn new(cx: i32, cy: i32) -> Self {
        Self { cx, cy }
    }
}

/// Attributes exchanged by NtGdiGetAndSetDCDword. These are part of the
/// saved drawing state, unlike the device's EPS and print-copy settings.
#[derive(Clone, Copy)]
pub struct DcAttributes {
    pub layout: u32,
    pub mirrored_window_origin_x: Option<i32>,
    pub color_space: u64,
    pub brush_origin: PointL,
    pub relative_mode: u32,
    pub mapper_flags: u32,
    pub arc_direction: u32,
    pub color_adjustment: services::ColorAdjustment,
    pub virtual_resolution: [u32; 4],
    /// Native SetSizeDevice metrics, used by virtual isotropic mapping.
    pub size_device: SizeL,
    pub text_justification_extra: i32,
    pub text_justification_breaks: i32,
}

impl Default for DcAttributes {
    fn default() -> Self {
        Self {
            layout: 0,
            mirrored_window_origin_x: None,
            color_space: 0,
            brush_origin: PointL::default(),
            relative_mode: 1,
            mapper_flags: 0,
            arc_direction: 1,
            color_adjustment: services::ColorAdjustment::default(),
            virtual_resolution: [0; 4],
            size_device: SizeL::default(),
            text_justification_extra: 0,
            text_justification_breaks: 0,
        }
    }
}

/// Saved DC state
#[repr(C)]
pub struct SavedDcState {
    pub attributes: DcAttributes,
    selected_references: [Option<super::objects::ObjectReference>; 7],
    pub path: super::path::GdiPath,
    pub poly_fill_mode: i32,
    pub graphics_mode: i32,
    pub current_position: PointL,
    pub application_clip: Option<Region>,
    pub meta_clip: Option<Region>,
    pub miter_limit: f32,
    pub pen: u64,
    pub brush: u64,
    pub font: u64,
    pub palette_handle: u64,
    pub region: u64,
    pub bitmap: u64,
    pub text_color: u32,
    pub bg_color: u32,
    pub text_align: i32,
    pub bk_mode: i32,
    pub rop_mode: i32,
    pub stretch_mode: i32,
    pub map_mode: i32,
    pub viewport_ext: SizeL,
    pub window_ext: SizeL,
    pub viewport_org: PointL,
    pub window_org: PointL,
    pub world_transform: Xform,
    pub world_transform_valid: bool,
}

impl Default for SavedDcState {
    fn default() -> Self {
        Self {
            attributes: DcAttributes::default(),
            selected_references: core::array::from_fn(|_| None),
            path: super::path::GdiPath::default(),
            poly_fill_mode: 1,
            graphics_mode: 1,
            current_position: PointL::default(),
            application_clip: None,
            meta_clip: None,
            miter_limit: 10.0,
            pen: 0,
            brush: 0,
            font: 0,
            palette_handle: 0,
            region: 0,
            bitmap: 0,
            text_color: 0x00000000,
            bg_color: 0x00FFFFFF,
            text_align: TA_LEFT | TA_TOP | TA_NOUPDATECP,
            bk_mode: OPAQUE,
            rop_mode: R2_COPYPEN,
            stretch_mode: super::raster::stretch::BLACKONWHITE,
            map_mode: MM_TEXT,
            viewport_ext: SizeL::new(1, 1),
            window_ext: SizeL::new(1, 1),
            viewport_org: PointL::new(0, 0),
            window_org: PointL::new(0, 0),
            world_transform: Xform::IDENTITY,
            world_transform_valid: false,
        }
    }
}

/// DC object
#[repr(C)]
pub struct DcObject {
    pub process_id: u64,
    /// A pixel format can be assigned only once during a DC lifetime.
    pub pixel_format: u32,
    pub drawing_bounds: bounds::DrawingBounds,
    pub attributes: DcAttributes,
    pub eps_printing: bool,
    pub print_copies: u32,
    selected_references: [Option<super::objects::ObjectReference>; 7],
    pub path: super::path::GdiPath,
    pub poly_fill_mode: i32,
    pub graphics_mode: i32,
    // handle header
    pub type_: u8,
    pub uses: u8,
    pub flags: u8,
    pub palette: u8,

    // DC type
    pub dc_type: u32,

    // Window association
    pub hwnd: u64,

    // Surface
    pub surface: u64,
    // Device-owned surface detached by MakeInfoDC. This is DC lifetime state,
    // independent of the attributes captured by SaveDC/RestoreDC.
    information: Option<info::DetachedSurface>,

    // Clipping region
    pub clip_region: u64,
    pub application_clip: Option<Region>,
    pub meta_clip: Option<Region>,
    pub miter_limit: f32,

    // Selected GDI objects
    pub pen: u64,
    pub brush: u64,
    pub font: u64,
    pub palette_handle: u64, // renamed to avoid conflict with palette field
    pub bitmap: u64,
    pub region: u64,

    // Colors (BGR format)
    pub text_color: u32,
    pub bg_color: u32,

    // Text alignment
    pub text_align: i32,

    // Drawing modes
    pub bk_mode: i32,
    pub rop_mode: i32,
    pub stretch_mode: i32,

    // Mapping mode and transformations
    pub map_mode: i32,
    // Current pen position (updated by MoveToEx, consumed by LineTo)
    pub cur_x: i32,
    pub cur_y: i32,
    pub viewport_ext: SizeL,
    pub window_ext: SizeL,
    pub viewport_org: PointL,
    pub window_org: PointL,
    pub world_transform: Xform,
    pub world_transform_valid: bool,

    // DC bounds
    pub bounds: crate::libs::win32k::objects::Rect,

    // Saved state stack
    saved_states: Vec<SavedDcState>,
}

impl DcObject {
    pub fn new() -> Self {
        Self {
            process_id: super::window::input_context::caller_identity().0,
            pixel_format: 0,
            drawing_bounds: bounds::DrawingBounds::default(),
            attributes: DcAttributes::default(),
            eps_printing: false,
            print_copies: 1,
            selected_references: core::array::from_fn(|_| None),
            path: super::path::GdiPath::default(),
            poly_fill_mode: 1,
            graphics_mode: 1,
            type_: crate::libs::win32k::objects::GdiObjectType::DC as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            dc_type: DC_TYPE_DISPLAY,
            hwnd: 0,
            surface: 0,
            information: None,
            clip_region: 0,
            application_clip: None,
            meta_clip: None,
            miter_limit: 10.0,
            pen: 0,
            brush: 0,
            font: 0,
            palette_handle: 0,
            bitmap: 0,
            region: 0,
            text_color: 0x00000000, // Black
            bg_color: 0x00FFFFFF,   // White
            text_align: TA_LEFT | TA_TOP | TA_NOUPDATECP,
            cur_x: 0,
            cur_y: 0,
            bk_mode: OPAQUE,
            rop_mode: R2_COPYPEN,
            stretch_mode: super::raster::stretch::BLACKONWHITE,
            map_mode: MM_TEXT,
            viewport_ext: SizeL::new(1, 1),
            window_ext: SizeL::new(1, 1),
            viewport_org: PointL::new(0, 0),
            window_org: PointL::new(0, 0),
            world_transform: Xform::IDENTITY,
            world_transform_valid: false,
            bounds: crate::libs::win32k::objects::Rect::new(0, 0, 0, 0),
            saved_states: Vec::new(),
        }
    }

    /// Save current DC state
    pub fn save_state(&mut self) -> bool {
        let Some(path) = self.path.try_clone() else {
            return false;
        };
        let clone_clip = |clip: &Option<Region>| -> Option<Option<Region>> {
            Some(match clip {
                Some(region) => Some(region.try_clone().ok()?),
                None => None,
            })
        };
        let (Some(application_clip), Some(meta_clip)) = (
            clone_clip(&self.application_clip),
            clone_clip(&self.meta_clip),
        ) else {
            return false;
        };
        if self.saved_states.try_reserve(1).is_err() {
            return false;
        }
        let mut selected_references = core::array::from_fn(|_| None);
        for (index, handle) in [
            self.pen,
            self.brush,
            self.font,
            self.palette_handle,
            self.region,
            self.bitmap,
            self.attributes.color_space,
        ]
        .into_iter()
        .enumerate()
        {
            if handle != 0 {
                let Some(reference) = super::objects::ObjectReference::acquire(handle) else {
                    return false;
                };
                selected_references[index] = Some(reference);
            }
        }
        let state = SavedDcState {
            attributes: self.attributes,
            selected_references,
            path,
            poly_fill_mode: self.poly_fill_mode,
            graphics_mode: self.graphics_mode,
            current_position: PointL::new(self.cur_x, self.cur_y),
            application_clip,
            meta_clip,
            miter_limit: self.miter_limit,
            pen: self.pen,
            brush: self.brush,
            font: self.font,
            palette_handle: self.palette_handle,
            region: self.region,
            bitmap: self.bitmap,
            text_color: self.text_color,
            bg_color: self.bg_color,
            text_align: self.text_align,
            bk_mode: self.bk_mode,
            rop_mode: self.rop_mode,
            stretch_mode: self.stretch_mode,
            map_mode: self.map_mode,
            viewport_ext: self.viewport_ext,
            window_ext: self.window_ext,
            viewport_org: self.viewport_org,
            window_org: self.window_org,
            world_transform: self.world_transform,
            world_transform_valid: self.world_transform_valid,
        };
        let _ = &state;
        self.saved_states.push(state);
        true
    }

    /// Restore previous DC state
    pub fn restore_state(&mut self) -> bool {
        if let Some(state) = self.saved_states.pop() {
            self.attributes = state.attributes;
            self.selected_references = state.selected_references;
            self.path = state.path;
            self.poly_fill_mode = state.poly_fill_mode;
            self.graphics_mode = state.graphics_mode;
            self.cur_x = state.current_position.x;
            self.cur_y = state.current_position.y;
            self.application_clip = state.application_clip;
            self.meta_clip = state.meta_clip;
            self.miter_limit = state.miter_limit;
            self.pen = state.pen;
            self.brush = state.brush;
            self.font = state.font;
            self.palette_handle = state.palette_handle;
            self.region = state.region;
            self.bitmap = state.bitmap;
            if self.dc_type == DC_TYPE_MEMORY {
                self.reset_bitmap_bounds();
            }
            self.text_color = state.text_color;
            self.bg_color = state.bg_color;
            self.text_align = state.text_align;
            self.bk_mode = state.bk_mode;
            self.rop_mode = state.rop_mode;
            self.stretch_mode = state.stretch_mode;
            self.map_mode = state.map_mode;
            self.viewport_ext = state.viewport_ext;
            self.window_ext = state.window_ext;
            self.viewport_org = state.viewport_org;
            self.window_org = state.window_org;
            self.world_transform = state.world_transform;
            self.world_transform_valid = state.world_transform_valid;
            true
        } else {
            false
        }
    }

    fn reset_bitmap_bounds(&mut self) {
        let (size, surface) = bitmap_binding(self.bitmap).unwrap_or((SizeL::new(1, 1), 0));
        self.surface = surface;
        self.bounds = super::objects::Rect::new(0, 0, size.cx, size.cy);
    }
}

// Callers retain a selected ObjectReference while using the returned binding.
fn bitmap_binding(handle: u64) -> Option<(SizeL, u64)> {
    super::objects::with_handle_table(|table| {
        let entry = table.get(handle)?;
        if entry.get_type()? != super::objects::GdiObjectType::Bitmap || entry.obj_ptr == 0 {
            return None;
        }
        // SAFETY: the table guard retains this validated bitmap allocation.
        let bitmap = unsafe { &*(entry.obj_ptr as *const super::objects::GdiBitmap) };
        Some((
            SizeL::new(bitmap.width, bitmap.height.checked_abs()?),
            core::ptr::addr_of!(bitmap.drawing_surface) as u64,
        ))
    })
}

impl Default for DcObject {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// DC handle Table
// =============================================================================

/// DC handle table
pub struct DcHandleTable {
    entries: Vec<Option<*mut DcObject>>,
    next_handle: u64,
}

impl DcHandleTable {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_handle: 0x80000000,
        }
    }

    /// create a new DC
    pub fn create(&mut self, dc_type: u32, surface: u64) -> u64 {
        if self.next_handle > u32::MAX as u64 || self.entries.try_reserve(1).is_err() {
            return 0;
        }
        let dc = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<DcObject>(),
        );
        let _ = &dc;

        if dc.is_null() {
            // kprintln!("[win32k] DcHandleTable::create: failed to allocate DC");
            return 0;
        }

        let mut object = DcObject::new();
        object.dc_type = dc_type;
        object.surface = surface;
        let size = if dc_type == DC_TYPE_MEMORY {
            // The default memory device is the one-pixel monochrome device.
            SizeL::new(1, 1)
        } else if surface != 0 {
            // SAFETY: creation receives a live display-owned surface binding.
            let surface = unsafe { &*(surface as *const super::surface::GdiSurface) };
            SizeL::new(surface.width, surface.height)
        } else {
            mapping::DisplayMetrics::current().pixels
        };
        object.bounds = super::objects::Rect::new(0, 0, size.cx, size.cy);
        for (slot, index) in [(0, 7), (1, 0), (2, 13), (3, 15)] {
            let handle = super::objects::gdi_get_stock_object(index);
            let Some(reference) = super::objects::ObjectReference::acquire(handle) else {
                // SAFETY: no DcObject has been written to this allocation yet.
                unsafe {
                    let _ = crate::mm::pool::free(dc);
                }
                return 0;
            };
            match slot {
                0 => object.pen = handle,
                1 => object.brush = handle,
                2 => object.font = handle,
                _ => object.palette_handle = handle,
            }
            object.selected_references[slot] = Some(reference);
        }
        // SAFETY: initialize a fresh allocation without dropping uninitialized Vecs.
        unsafe {
            dc.cast::<DcObject>().write(object);
        }

        let handle = self.next_handle;

        let _ = &handle;
        let _ = &handle;
        self.next_handle += 1;
        self.entries.push(Some(dc as *mut DcObject));

        // kprintln!("[win32k] DcHandleTable::create: type=0x{:08x}, surface=0x{:x} -> handle=0x{:08x}",
        //                   dc_type, surface, handle);

        handle
    }

    pub fn count_for_process(&self, process: u64) -> usize {
        self.entries.iter().flatten().filter(|&&pointer| {
            // SAFETY: this table guard prevents concurrent destruction.
            unsafe { (*pointer).process_id == process }
        }).count()
    }

    /// Get a DC by handle
    pub fn get(&mut self, handle: u64) -> Option<&mut DcObject> {
        let index = usize::try_from(handle.checked_sub(0x80000000)?).ok()?;
        let _ = &index;
        let _ = &index;
        if index < self.entries.len() {
            self.entries[index].map(|ptr| unsafe { &mut *ptr })
        } else {
            None
        }
    }

    /// Delete a DC
    pub fn delete(&mut self, handle: u64) -> bool {
        let Some(index) = handle
            .checked_sub(0x80000000)
            .and_then(|index| usize::try_from(index).ok())
        else {
            return false;
        };
        let _ = &index;
        let _ = &index;
        if index < self.entries.len() {
            if let Some(ptr) = self.entries[index].take() {
                unsafe {
                    // Release any selected objects
                    let dc = &mut *ptr;
                    let _ = &dc;
                    let _ = &dc;
                    if dc.surface != 0 {
                        // Surface cleanup handled separately
                    }
                    core::ptr::drop_in_place(ptr);
                    let _ = crate::mm::pool::free(ptr as *mut u8);
                }
                // kprintln!("[win32k] DcHandleTable::delete: handle=0x{:08x}", handle);
                return true;
            }
        }
        false
    }
}

impl Default for DcHandleTable {
    fn default() -> Self {
        Self::new()
    }
}

// All references to a DC are tied to this guard. A competing delete or
// state mutation cannot run until the current operation releases the table.
static DC_HANDLE_TABLE: crate::ke::sync::Spinlock<Option<DcHandleTable>> =
    crate::ke::sync::Spinlock::new(None);

pub struct DcTableGuard(crate::ke::sync::SpinlockGuard<'static, Option<DcHandleTable>>);

impl core::ops::Deref for DcTableGuard {
    type Target = DcHandleTable;
    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("DC table initialized under its lock")
    }
}

impl core::ops::DerefMut for DcTableGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
            .as_mut()
            .expect("DC table initialized under its lock")
    }
}

pub fn get_dc_table() -> DcTableGuard {
    let mut guard = DC_HANDLE_TABLE.lock();
    if guard.is_none() {
        *guard = Some(DcHandleTable::new());
    }
    DcTableGuard(guard)
}

pub struct DcGuard {
    table: DcTableGuard,
    index: usize,
}

impl core::ops::Deref for DcGuard {
    type Target = DcObject;
    fn deref(&self) -> &Self::Target {
        // SAFETY: get_dc validated this slot and the guard prevents deletion.
        unsafe { &*self.table.entries[self.index].expect("live guarded DC") }
    }
}

impl core::ops::DerefMut for DcGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.table.entries[self.index].expect("live guarded DC") }
    }
}

// =============================================================================
// DC Creation Functions
// =============================================================================

/// create a display DC (screen DC)
pub fn gre_create_display_dc() -> u64 {
    // Get primary surface
    let surface = crate::libs::win32k::surface::get_primary_surface() as u64;
    let _ = &surface;
    let _ = &surface;
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    table.create(DC_TYPE_DISPLAY | DC_TYPE_WINDOW, surface)
}

/// create a memory DC
pub fn gre_create_compatible_dc(source_dc: u64) -> u64 {
    if source_dc != 0 {
        let mut table = get_dc_table();
        if table.get(source_dc).is_none() {
            return 0;
        }
    }
    // Compatibility selects device characteristics, never the device's pixels.
    // The default monochrome bitmap is not yet realized by this backend.
    get_dc_table().create(DC_TYPE_MEMORY, 0)
}

/// Delete a DC
pub fn gre_delete_dc(dc_handle: u64) -> bool {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    table.delete(dc_handle)
}

/// Get DC for a bitmap (creates a DC with the bitmap selected)
pub fn gre_get_dcfor_bitmap(bitmap_handle: u64) -> u64 {
    let _ = bitmap_handle;
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;

    // create a memory DC compatible with the primary surface
    let mem_dc = table.create(DC_TYPE_MEMORY, 0);
    let _ = &mem_dc;
    let _ = &mem_dc;

    mem_dc
}

/// Get DC by handle
pub fn get_dc(dc_handle: u64) -> Option<DcGuard> {
    let mut table = get_dc_table();
    table.get(dc_handle)?;
    let index = (dc_handle - 0x80000000) as usize;
    Some(DcGuard { table, index })
}

// =============================================================================
// DC Attribute Functions
// =============================================================================

/// Set text color
pub fn gre_set_text_color(dc_handle: u64, color: u32) -> u32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        let old = dc.text_color;
        let _ = &old;
        let _ = &old;
        dc.text_color = color;
        old
    } else {
        0xFFFFFFFF
    }
}

/// Get text color
pub fn gre_get_text_color(dc_handle: u64) -> u32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.text_color
    } else {
        0
    }
}

/// Set background color
pub fn gre_set_bk_color(dc_handle: u64, color: u32) -> u32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        let old = dc.bg_color;
        let _ = &old;
        let _ = &old;
        dc.bg_color = color;
        old
    } else {
        0xFFFFFFFF
    }
}

/// Get background color
pub fn gre_get_bk_color(dc_handle: u64) -> u32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.bg_color
    } else {
        0x00FFFFFF
    }
}

/// Set background mode
pub fn gre_set_bk_mode(dc_handle: u64, mode: i32) -> i32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        let old = dc.bk_mode;
        let _ = &old;
        let _ = &old;
        dc.bk_mode = mode;
        old
    } else {
        OPAQUE
    }
}

/// Get background mode
pub fn gre_get_bk_mode(dc_handle: u64) -> i32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.bk_mode
    } else {
        OPAQUE
    }
}

/// Set ROP mode
pub fn gre_set_rop2(dc_handle: u64, mode: i32) -> i32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        let old = dc.rop_mode;
        let _ = &old;
        let _ = &old;
        dc.rop_mode = mode;
        old
    } else {
        R2_COPYPEN
    }
}

/// Get ROP mode
pub fn gre_get_rop2(dc_handle: u64) -> i32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.rop_mode
    } else {
        R2_COPYPEN
    }
}

/// Set text alignment
pub fn gre_set_text_align(dc_handle: u64, align: i32) -> i32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        let old = dc.text_align;
        let _ = &old;
        let _ = &old;
        dc.text_align = align;
        old
    } else {
        TA_LEFT
    }
}

/// Get text alignment
pub fn gre_get_text_align(dc_handle: u64) -> i32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.text_align
    } else {
        TA_LEFT
    }
}

// =============================================================================
// select_object
// =============================================================================

/// Select an object into a DC
pub fn gre_select_object(dc_handle: u64, object_handle: u64) -> u64 {
    select_typed_object(dc_handle, object_handle, None)
}

/// Acquire the new object before changing the DC; releasing the old reference
/// afterwards makes deletion safe across selection and SaveDC/RestoreDC.
pub fn select_typed_object(
    dc_handle: u64,
    object_handle: u64,
    required: Option<super::objects::GdiObjectType>,
) -> u64 {
    use super::objects::{GdiObjectType, ObjectReference};
    let Some(mut dc) = get_dc(dc_handle) else {
        return 0;
    };
    let Some(reference) = ObjectReference::acquire(object_handle) else {
        return 0;
    };
    let object_type = reference.object_type();
    if required.is_some_and(|kind| {
        kind != object_type
            && !(kind == GdiObjectType::Pen && object_type == GdiObjectType::ExtPen)
    }) {
        return 0;
    }
    // GreSelectBitmap only accepts memory DCs (Win7 x64 RVA 0xc68cd).
    // In particular an information DC keeps its device size while detached.
    if object_type == GdiObjectType::Bitmap && dc.dc_type != DC_TYPE_MEMORY {
        return 0;
    }
    if object_type == GdiObjectType::Font && !super::font_services::bind_logical_font(object_handle) {
        return 0;
    }
    let (slot, old) = match object_type {
        GdiObjectType::Pen | GdiObjectType::ExtPen => {
            (0, core::mem::replace(&mut dc.pen, object_handle))
        }
        GdiObjectType::Brush => (1, core::mem::replace(&mut dc.brush, object_handle)),
        GdiObjectType::Font => (2, core::mem::replace(&mut dc.font, object_handle)),
        GdiObjectType::Palette => (3, core::mem::replace(&mut dc.palette_handle, object_handle)),
        GdiObjectType::Bitmap => {
            let Some((size, surface)) = bitmap_binding(object_handle) else { return 0 };
            dc.surface = surface;
            dc.bounds = super::objects::Rect::new(0, 0, size.cx, size.cy);
            (5, core::mem::replace(&mut dc.bitmap, object_handle))
        }
        _ => return 0,
    };
    dc.selected_references[slot] = Some(reference);
    old
}

/// Get current pen
pub fn gre_get_current_pen(dc_handle: u64) -> u64 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.pen
    } else {
        0
    }
}

/// Get current brush
pub fn gre_get_current_brush(dc_handle: u64) -> u64 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.brush
    } else {
        0
    }
}

/// Get current font
pub fn gre_get_current_font(dc_handle: u64) -> u64 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.font
    } else {
        0
    }
}

// =============================================================================
// save_dc / restore_dc
// =============================================================================

/// Save DC state
pub fn gre_save_dc(dc_handle: u64) -> i32 {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        if !dc.save_state() {
            return 0;
        }
        let count = dc.saved_states.len() as i32;
        let _ = &count;
        let _ = &count;
        // kprintln!("[win32k] gre_save_dc: dc=0x{:08x}, saved_count={}", dc_handle, count);
        count
    } else {
        0
    }
}

/// Restore DC state
pub fn gre_restore_dc(dc_handle: u64, saved_state: i32) -> bool {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        let length = dc.saved_states.len() as i64;
        let target = if saved_state < 0 {
            length + i64::from(saved_state)
        } else {
            i64::from(saved_state) - 1
        };
        if target < 0 || target >= length {
            return false;
        }
        dc.saved_states.truncate(target as usize + 1);
        dc.restore_state()
    } else {
        false
    }
}

// =============================================================================
// Mapping Mode Functions
// =============================================================================

/// Set mapping mode
pub fn gre_set_map_mode(dc_handle: u64, mode: i32) -> i32 {
    let Some(mut dc) = get_dc(dc_handle) else {
        return 0;
    };
    mapping::set_map_mode(&mut dc, mode)
}

/// Get mapping mode
pub fn gre_get_map_mode(dc_handle: u64) -> i32 {
    get_dc(dc_handle).map_or(0, |dc| dc.map_mode)
}

/// Set viewport extent
pub fn gre_set_viewport_ext_ex(
    dc_handle: u64,
    cx: i32,
    cy: i32,
    old_size: Option<&mut SizeL>,
) -> bool {
    let Some(mut dc) = get_dc(dc_handle) else {
        return false;
    };
    if let Some(size) = old_size {
        *size = dc.viewport_ext;
    }
    mapping::set_extent(&mut dc, SizeL::new(cx, cy), true)
}

/// Set window extent
pub fn gre_set_window_ext_ex(
    dc_handle: u64,
    cx: i32,
    cy: i32,
    old_size: Option<&mut SizeL>,
) -> bool {
    let Some(mut dc) = get_dc(dc_handle) else {
        return false;
    };
    if let Some(size) = old_size {
        *size = dc.window_ext;
        if dc.attributes.layout & mirror::LAYOUT_RTL != 0 {
            size.cx = size.cx.wrapping_neg();
        }
    }
    let cx = if dc.attributes.layout & mirror::LAYOUT_RTL != 0 {
        cx.wrapping_neg()
    } else {
        cx
    };
    mapping::set_extent(&mut dc, SizeL::new(cx, cy), false)
}

/// Set viewport origin
pub fn gre_set_viewport_org_ex(
    dc_handle: u64,
    x: i32,
    y: i32,
    old_origin: Option<&mut PointL>,
) -> bool {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        if let Some(origin) = old_origin {
            *origin = dc.viewport_org;
            if dc.attributes.layout & mirror::LAYOUT_RTL != 0 {
                origin.x = origin.x.wrapping_neg();
            }
        }
        let x = if dc.attributes.layout & mirror::LAYOUT_RTL != 0 {
            x.wrapping_neg()
        } else {
            x
        };
        if !(-0x7fffffe..=0x7fffffe).contains(&x)
            || !(-0x7fffffe..=0x7fffffe).contains(&y)
        {
            return false;
        }
        dc.viewport_org = PointL::new(x, y);
        true
    } else {
        false
    }
}

// =============================================================================
// DC Bounds
// =============================================================================

/// Get DC bounds
pub fn gre_get_dcbounds(dc_handle: u64) -> crate::libs::win32k::objects::Rect {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.bounds
    } else {
        crate::libs::win32k::objects::Rect::new(0, 0, 0, 0)
    }
}

/// Set DC bounds
pub fn gre_set_dcbounds(dc_handle: u64, bounds: &crate::libs::win32k::objects::Rect) {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        dc.bounds = *bounds;
    }
}

// =============================================================================
// Debug
// =============================================================================

/// Dump DC info
pub fn dump_dc_info(dc_handle: u64) {
    let mut table = get_dc_table();
    let _ = &table;
    let _ = &table;
    if let Some(dc) = table.get(dc_handle) {
        let _ = dc;
        // kprintln!("[win32k] DC 0x{:08x} info:", dc_handle);
        // kprintln!("  type: 0x{:08x}, surface: 0x{:x}", dc.dc_type, dc.surface);
        // kprintln!("  pen: 0x{:016x}, brush: 0x{:016x}, font: 0x{:016x}",
        //                   dc.pen, dc.brush, dc.font);
        // kprintln!("  text_color: 0x{:08x}, bg_color: 0x{:08x}", dc.text_color, dc.bg_color);
        // kprintln!("  text_align: 0x{:x}, bk_mode: {}, rop_mode: {}",
        //                   dc.text_align, dc.bk_mode, dc.rop_mode);
        // kprintln!("  map_mode: {}, viewport: {}x{}, window: {}x{}",
        //                   dc.map_mode, dc.viewport_ext.cx, dc.viewport_ext.cy,
        //                   dc.window_ext.cx, dc.window_ext.cy);
        // kprintln!("  viewport_org: ({}, {}), window_org: ({}, {})",
        //                   dc.viewport_org.x, dc.viewport_org.y,
        //                   dc.window_org.x, dc.window_org.y);
        // kprintln!("  saved_states: {}", dc.saved_states.len());
    }
}

#[path = "dc/pixel_format.rs"]
pub mod pixel_format;

impl DcObject {
    pub(super) fn set_color_space(&mut self, handle: u64) -> bool {
        let Some(reference) = super::objects::ObjectReference::acquire(handle) else { return false; };
        if reference.object_type() != super::objects::GdiObjectType::ColorSpace { return false; }
        self.selected_references[6] = Some(reference);
        self.attributes.color_space = handle;
        true
    }
}

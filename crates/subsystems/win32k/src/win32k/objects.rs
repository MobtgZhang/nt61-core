//! GDI Object Management
//
//! Implements the GDI handle table and object allocation/freeing.
//! GDI objects include pens, brushes, fonts, bitmaps, regions, palettes, and DCs.
//
//! ## Windows 7 GDI handle Architecture
//
//! GDI handles are 32-bit values with the following structure:
//! - High 16 bits: handle type indicator (0x4000 | base_type)
//! - Low 16 bits: Index into handle table
//
//! ## Magic Numbers and Constants
//
//! | Constant | Value | Description |
//! |----------|-------|-------------|
//! | `HANDLE_TYPE_DC` | 0x4001 | Device Context handle type |
//! | `HANDLE_TYPE_BITMAP` | 0x4002 | Bitmap handle type |
//! | `HANDLE_TYPE_PALETTE` | 0x4003 | Palette handle type |
//! | `HANDLE_TYPE_FONT` | 0x4004 | Font handle type |
//! | `HANDLE_TYPE_PEN` | 0x4005 | Pen handle type |
//! | `HANDLE_TYPE_EXTPEN` | 0x4006 | Extended pen handle type |
//! | `HANDLE_TYPE_BRUSH` | 0x4007 | Brush handle type |
//! | `HANDLE_TYPE_REGION` | 0x4008 | Region handle type |
//! | `HANDLE_TYPE_METAFILE` | 0x4009 | Metafile handle type |
//! | `HANDLE_TYPE_MEMDC` | 0x400A | Memory DC handle type |
//! | `HANDLE_TYPE_ENHANCED_DC` | 0x400B | Enhanced DC handle type |
//! | `HANDLE_INDEX_MASK` | 0xFFFF | Mask to extract index from handle |
//! | `HANDLE_TYPE_MASK` | 0xFFFF0000 | Mask to extract type from handle |
//
//! Reference: ReactOS win32ss/gdi/gdiobj, Geoff Chappell

extern crate alloc;

use super::region_geometry::Region;
use alloc::vec::Vec;

/// GDI object types (matches Windows GDIObjType)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GdiObjectType {
    DC = 1,
    Bitmap = 2,
    Palette = 3,
    Font = 4,
    Pen = 5,
    ExtPen = 6,
    Brush = 7,
    Region = 8,
    Metafile = 9,
    MemDC = 10,
    EnhancedMetafileDC = 11,
    ColorSpace = 12,
}

impl GdiObjectType {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(GdiObjectType::DC),
            2 => Some(GdiObjectType::Bitmap),
            3 => Some(GdiObjectType::Palette),
            4 => Some(GdiObjectType::Font),
            5 => Some(GdiObjectType::Pen),
            6 => Some(GdiObjectType::ExtPen),
            7 => Some(GdiObjectType::Brush),
            8 => Some(GdiObjectType::Region),
            9 => Some(GdiObjectType::Metafile),
            10 => Some(GdiObjectType::MemDC),
            11 => Some(GdiObjectType::EnhancedMetafileDC),
            12 => Some(GdiObjectType::ColorSpace),
            _ => None,
        }
    }
}

/// GDI handle entry
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GdiHandleEntry {
    /// Object type (1-11)
    pub obj_type: u8,
    /// Usage count
    pub uses: u32,
    /// flags
    pub flags: u8,
    /// Palette index
    pub palette: u8,
    /// Kernel address of the object
    pub obj_ptr: u64,
    /// Process ID that owns this handle
    pub process_id: u32,
    /// handle stamp for validation
    pub stamp: u32,
}

impl GdiHandleEntry {
    pub fn new(obj_type: GdiObjectType, obj_ptr: u64, pid: u32) -> Self {
        Self {
            obj_type: obj_type as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            obj_ptr,
            process_id: pid,
            stamp: 0,
        }
    }

    pub fn get_type(&self) -> Option<GdiObjectType> {
        GdiObjectType::from_u8(self.obj_type)
    }
}

/// handle type prefixes (high 16 bits)
const HANDLE_TYPE_DC: u32 = 0x4001; // 0x0001 with high bit
const HANDLE_TYPE_BITMAP: u32 = 0x4002;
const HANDLE_TYPE_PALETTE: u32 = 0x4003;
const HANDLE_TYPE_FONT: u32 = 0x4004;
const HANDLE_TYPE_PEN: u32 = 0x4005;
const HANDLE_TYPE_EXTPEN: u32 = 0x4006;
const HANDLE_TYPE_BRUSH: u32 = 0x4007;
const HANDLE_TYPE_REGION: u32 = 0x4008;
const HANDLE_TYPE_METAFILE: u32 = 0x4009;
const HANDLE_TYPE_MEMDC: u32 = 0x400A;
const HANDLE_TYPE_ENHANCED_DC: u32 = 0x400B;

fn get_handle_prefix(obj_type: GdiObjectType) -> u32 {
    match obj_type {
        GdiObjectType::DC => HANDLE_TYPE_DC,
        GdiObjectType::Bitmap => HANDLE_TYPE_BITMAP,
        GdiObjectType::Palette => HANDLE_TYPE_PALETTE,
        GdiObjectType::Font => HANDLE_TYPE_FONT,
        GdiObjectType::Pen => HANDLE_TYPE_PEN,
        GdiObjectType::ExtPen => HANDLE_TYPE_EXTPEN,
        GdiObjectType::Brush => HANDLE_TYPE_BRUSH,
        GdiObjectType::Region => HANDLE_TYPE_REGION,
        GdiObjectType::Metafile => HANDLE_TYPE_METAFILE,
        GdiObjectType::MemDC => HANDLE_TYPE_MEMDC,
        GdiObjectType::EnhancedMetafileDC => HANDLE_TYPE_ENHANCED_DC,
        GdiObjectType::ColorSpace => 0x400c,
    }
}

/// GDI handle table
pub struct GdiHandleTable {
    entries: Vec<Option<GdiHandleEntry>>,
}

impl GdiHandleTable {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Allocate a new GDI handle
    /// handle format: [31:16] = type prefix, [15:0] = index
    pub fn allocate(&mut self, obj_type: GdiObjectType, obj_ptr: u64, pid: u32) -> u64 {
        // Do not recycle slots until generation bits are part of the ABI. A stale
        // handle must never identify a later allocation of the same type.
        let index = self.entries.len();
        if index > u16::MAX as usize || self.entries.try_reserve(1).is_err() {
            return 0;
        }
        self.entries
            .push(Some(GdiHandleEntry::new(obj_type, obj_ptr, pid)));
        ((get_handle_prefix(obj_type) as u64) << 16) | index as u64
    }

    pub fn count_for_process(&self, process: u32) -> usize {
        self.entries
            .iter()
            .flatten()
            .filter(|entry| entry.process_id == process && entry.flags & HANDLE_FLAG_STOCK == 0)
            .count()
    }

    /// Get a handle entry - validates both index and type prefix
    pub fn get(&self, handle: u64) -> Option<&GdiHandleEntry> {
        if handle > u32::MAX as u64 {
            return None;
        }
        let index = (handle & 0xFFFF) as usize;
        let _ = &index;
        if index < self.entries.len() {
            let entry = self.entries[index].as_ref()?;
            let _ = &entry;
            // Validate type prefix matches
            let expected_prefix = get_handle_prefix(entry.get_type()?);
            let _ = &expected_prefix;
            let actual_prefix = (handle >> 16) as u32;
            let _ = &actual_prefix;
            if actual_prefix
                == (expected_prefix
                    | if entry.flags & attributes::CONVERTED_STOCK != 0 {
                        (attributes::STOCK_HANDLE_BIT >> 16) as u32
                    } else {
                        0
                    })
            {
                Some(entry)
            } else {
                // kprintln!("[win32k] handle type mismatch: expected=0x{:04x}, got=0x{:04x}",
                //                     expected_prefix, actual_prefix);
                None
            }
        } else {
            None
        }
    }

    /// Get a mutable handle entry with validation
    pub fn get_mut(&mut self, handle: u64) -> Option<&mut GdiHandleEntry> {
        if handle > u32::MAX as u64 {
            return None;
        }
        let index = (handle & 0xFFFF) as usize;
        let _ = &index;
        if index < self.entries.len() {
            let entry = self.entries[index].as_ref()?;
            let _ = &entry;
            // Validate type prefix matches
            let expected_prefix = get_handle_prefix(entry.get_type()?);
            let _ = &expected_prefix;
            let actual_prefix = (handle >> 16) as u32;
            let _ = &actual_prefix;
            if actual_prefix
                == (expected_prefix
                    | if entry.flags & attributes::CONVERTED_STOCK != 0 {
                        (attributes::STOCK_HANDLE_BIT >> 16) as u32
                    } else {
                        0
                    })
            {
                self.entries[index].as_mut()
            } else {
                // kprintln!("[win32k] handle type mismatch: expected=0x{:04x}, got=0x{:04x}",
                //                     expected_prefix, actual_prefix);
                None
            }
        } else {
            None
        }
    }

    /// Free a GDI handle
    pub fn free(&mut self, handle: u64) -> Option<GdiHandleEntry> {
        self.get(handle)?;
        self.entries[(handle & 0xFFFF) as usize].take()
    }

    /// Increment usage count
    pub fn add_ref(&mut self, handle: u64) -> bool {
        if let Some(entry) = self.get_mut(handle) {
            let Some(uses) = entry.uses.checked_add(1) else {
                return false;
            };
            entry.uses = uses;
            true
        } else {
            false
        }
    }

    /// Decrement usage count
    pub fn release(&mut self, handle: u64) -> bool {
        if let Some(entry) = self.get_mut(handle) {
            entry.uses = entry.uses.saturating_sub(1);
            true
        } else {
            false
        }
    }
}

impl Default for GdiHandleTable {
    fn default() -> Self {
        Self::new()
    }
}

// Keep the guard alive throughout each table operation. Region operations also
// hold it while accessing the allocation, so deletion cannot invalidate a region
// between handle validation and use.
static GDI_HANDLE_TABLE: crate::ke::sync::Spinlock<Option<GdiHandleTable>> =
    crate::ke::sync::Spinlock::new(None);

struct HandleTableGuard(crate::ke::sync::SpinlockGuard<'static, Option<GdiHandleTable>>);

impl core::ops::Deref for HandleTableGuard {
    type Target = GdiHandleTable;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("GDI table initialized under its lock")
    }
}

impl core::ops::DerefMut for HandleTableGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
            .as_mut()
            .expect("GDI table initialized under its lock")
    }
}

fn get_handle_table() -> HandleTableGuard {
    let mut guard = GDI_HANDLE_TABLE.lock();
    if guard.is_none() {
        *guard = Some(GdiHandleTable::new());
    }
    HandleTableGuard(guard)
}

pub(super) fn with_handle_table<R>(f: impl FnOnce(&mut GdiHandleTable) -> R) -> R {
    f(&mut get_handle_table())
}

// =============================================================================
// GDI Object Structures
// =============================================================================

/// Pen style flags
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum PenStyle {
    Solid = 0x00000000,
    Dash = 0x00000001,
    Dot = 0x00000002,
    DashDot = 0x00000003,
    DashDotDot = 0x00000004,
    Null = 0x00000005,
    InsideFrame = 0x00000006,
}

impl PenStyle {
    pub fn from_raw(value: i32) -> Self {
        match value as u32 {
            0x00000000 => PenStyle::Solid,
            0x00000001 => PenStyle::Dash,
            0x00000002 => PenStyle::Dot,
            0x00000003 => PenStyle::DashDot,
            0x00000004 => PenStyle::DashDotDot,
            0x00000005 => PenStyle::Null,
            0x00000006 => PenStyle::InsideFrame,
            _ => PenStyle::Solid,
        }
    }
}

/// Pen object
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GdiPen {
    pub type_: u8,
    pub uses: u8,
    pub flags: u8,
    pub palette: u8,
    pub color: u32,
    pub style: u32,
    pub width: i32,
    pub brush_lined: u64,
    /// Original brush description, distinct from any realized brush handle.
    pub brush_style: u32,
    pub client_hatch: u64,
    pub hatch: u64,
    pub legacy: bool,
    pub user_style_count: u8,
    pub realized_style_count: u8,
    pub user_styles: [u32; 16],
    pub realized_styles: [f32; 16],
}

impl GdiPen {
    pub fn new(style: PenStyle, width: i32, color: u32) -> Self {
        Self {
            type_: GdiObjectType::Pen as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            color,
            style: style as u32,
            width,
            brush_lined: 0,
            brush_style: 0,
            client_hatch: 0,
            hatch: 0,
            legacy: true,
            user_style_count: 0,
            realized_style_count: 0,
            user_styles: [0; 16],
            realized_styles: [0.0; 16],
        }
    }
}

/// Brush style flags
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum BrushStyle {
    Solid = 0x00000000,
    Null = 0x00000001,
    Hatched = 0x00000002,
    Pattern = 0x00000003,
    Indexed = 0x00000004,
    DIBPattern = 0x00000005,
    DIBPatternPt = 0x00000006,
    Pattern8x8 = 0x00000007,
    MonochromeBitmap = 0x00000008,
}

/// Hatch style
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum HatchStyle {
    Horizontal = 0x00000000,
    Vertical = 0x00000001,
    FDiagonal = 0x00000002,
    BDiagonal = 0x00000003,
    Cross = 0x00000004,
    DiagonalCross = 0x00000005,
}

/// Brush object
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GdiBrush {
    pub type_: u8,
    pub uses: u8,
    pub flags: u8,
    pub palette: u8,
    pub color: u32,
    pub style: u32,
    pub hatch: i32,
    pub bitmap: u64,
}

impl GdiBrush {
    pub fn new_solid(color: u32) -> Self {
        Self {
            type_: GdiObjectType::Brush as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            color,
            style: BrushStyle::Solid as u32,
            hatch: 0,
            bitmap: 0,
        }
    }

    pub fn new_hatched(style: HatchStyle, color: u32) -> Self {
        Self {
            type_: GdiObjectType::Brush as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            color,
            style: BrushStyle::Hatched as u32,
            hatch: style as i32,
            bitmap: 0,
        }
    }
}

/// Font quality
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum FontQuality {
    Default = 0,
    Draft = 1,
    Proof = 2,
    NonAntialiased = 3,
    Antialiased = 4,
    ClearType = 5,
    ClearTypeNatural = 6,
}

/// Font object (LOGFONTW - simplified)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GdiFont {
    pub type_: u8,
    pub uses: u8,
    pub flags: u8,
    pub palette: u8,
    pub height: i32,
    pub width: i32,
    pub escapement: i32,
    pub orientation: i32,
    pub weight: i32,
    pub italic: u8,
    pub underline: u8,
    pub strike_out: u8,
    pub charset: u8,
    pub out_precision: u8,
    pub clip_precision: u8,
    pub quality: u8,
    pub pitch_and_family: u8,
    pub face_name: [u16; 32],
    /// Captured ENUMLOGFONTEXW + zero-axis DESIGNVECTOR header. Keep all
    /// names/style/script/reserved bytes; LOGFONT alone cannot answer GetObject.
    pub(super) captured_description: Option<[u8; 356]>,
}

impl GdiFont {
    pub fn new() -> Self {
        Self {
            type_: GdiObjectType::Font as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            height: 16,
            width: 0,
            escapement: 0,
            orientation: 0,
            weight: 400,
            italic: 0,
            underline: 0,
            strike_out: 0,
            charset: 0,
            out_precision: 0,
            clip_precision: 0,
            quality: FontQuality::Default as u8,
            pitch_and_family: 0,
            face_name: [0; 32],
            captured_description: None,
        }
    }

    /// Preserve the native logical description independently of font realization.
    pub(super) fn logical_description(&self) -> [u8; 356] {
        if let Some(bytes) = self.captured_description { return bytes; }
        let mut bytes = [0; 356];
        for (slot, value) in bytes[..20].chunks_exact_mut(4).zip([
            self.height, self.width, self.escapement, self.orientation, self.weight,
        ]) { slot.copy_from_slice(&value.to_le_bytes()); }
        bytes[20..28].copy_from_slice(&[
            self.italic, self.underline, self.strike_out, self.charset,
            self.out_precision, self.clip_precision, self.quality, self.pitch_and_family,
        ]);
        for (slot, unit) in bytes[28..92].chunks_exact_mut(2).zip(self.face_name) {
            slot.copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    pub fn set_face_name(&mut self, name: &[u16]) {
        let len = name.len().min(31);
        let _ = &len;
        self.face_name[..len].copy_from_slice(&name[..len]);
        self.face_name[len] = 0;
    }
}

impl Default for GdiFont {
    fn default() -> Self {
        Self::new()
    }
}

/// Bitmap object
#[repr(C)]
pub struct GdiBitmap {
    pub type_: u8,
    pub uses: u8,
    pub flags: u8,
    pub palette: u8,
    pub size: u64,
    pub width: i32,
    pub height: i32,
    pub width_bytes: i32,
    pub planes: u16,
    pub bit_count: u16,
    pub format: u32,
    pub bits: *mut u8,
    /// Application metadata, independent of the allocated pixel dimensions.
    pub logical_dimensions: super::dc::SizeL,
}

impl GdiBitmap {
    pub fn new(width: i32, height: i32, bit_count: u16) -> Option<Self> {
        if width <= 0 || height == 0 || !matches!(bit_count, 1 | 4 | 8 | 16 | 24 | 32) {
            return None;
        }
        let rows = height.checked_abs()? as usize;
        let row_bits = (width as usize).checked_mul(usize::from(bit_count))?;
        let pitch = row_bits.checked_add(31)?.checked_div(32)?.checked_mul(4)?;
        let width_bytes = i32::try_from(pitch).ok()?;
        let allocation_size = pitch.checked_mul(rows)?;
        // Pointer offsets must fit isize even when the dimensions fit LONG.
        isize::try_from(allocation_size).ok()?;
        let bits = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged, allocation_size,
        );
        if bits.is_null() {
            return None;
        }
        // SAFETY: the fresh, exclusively owned allocation has allocation_size
        // bytes. Initialize padding too, before any bitmap reader can see it.
        unsafe { bits.write_bytes(0, allocation_size); }

        Some(Self {
            type_: GdiObjectType::Bitmap as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            size: allocation_size as u64,
            width,
            height,
            width_bytes,
            planes: 1,
            bit_count,
            format: 0,
            bits,
            logical_dimensions: super::dc::SizeL::default(),
        })
    }
}

impl Drop for GdiBitmap {
    fn drop(&mut self) {
        // SAFETY: GdiBitmap exclusively owns this allocation and is not Clone.
        unsafe {
            if !self.bits.is_null() {
                let _ = crate::mm::pool::free(self.bits);
            }
        }
    }
}

/// Palette entry
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PaletteEntry {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub flags: u8,
}

/// Palette object
#[repr(C)]
pub struct GdiPalette {
    pub type_: u8,
    pub uses: u8,
    pub flags: u8,
    pub palette_flags: u8,
    pub version: u16,
    pub num_entries: u32,
    pub entries: Vec<PaletteEntry>,
}

impl GdiPalette {
    pub fn try_new() -> Option<Self> {
        let mut entries = Vec::new();
        entries.try_reserve_exact(20).ok()?;
        let colors = [
            (0, 0, 0),
            (128, 0, 0),
            (0, 128, 0),
            (128, 128, 0),
            (0, 0, 128),
            (128, 0, 128),
            (0, 128, 128),
            (192, 192, 192),
            (192, 220, 192),
            (166, 202, 240),
            (255, 251, 240),
            (160, 160, 164),
            (128, 128, 128),
            (255, 0, 0),
            (0, 255, 0),
            (255, 255, 0),
            (0, 0, 255),
            (255, 0, 255),
            (0, 255, 255),
            (255, 255, 255),
        ];
        for (red, green, blue) in colors {
            entries.push(PaletteEntry {
                red,
                green,
                blue,
                flags: 0,
            });
        }

        Some(Self {
            type_: GdiObjectType::Palette as u8,
            uses: 0,
            flags: 0,
            palette_flags: 0,
            version: 0x0300,
            num_entries: 20,
            entries,
        })
    }
}

/// RECT structure - unified definition for all win32k modules
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// Check if a point is inside the rectangle
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Compute intersection with another rectangle
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let left = core::cmp::max(self.left, other.left);
        let _ = &left;
        let top = core::cmp::max(self.top, other.top);
        let _ = &top;
        let right = core::cmp::min(self.right, other.right);
        let _ = &right;
        let bottom = core::cmp::min(self.bottom, other.bottom);
        let _ = &bottom;

        if right > left && bottom > top {
            Some(Rect {
                left,
                top,
                right,
                bottom,
            })
        } else {
            None
        }
    }

    /// Check if rectangle is empty (zero or negative dimensions)
    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    /// Normalize rectangle so left <= right and top <= bottom
    pub fn normalize(&mut self) {
        if self.left > self.right {
            core::mem::swap(&mut self.left, &mut self.right);
        }
        if self.top > self.bottom {
            core::mem::swap(&mut self.top, &mut self.bottom);
        }
    }

    /// Convert to a 4-element array [left, top, right, bottom]
    pub fn to_array(&self) -> [i32; 4] {
        [self.left, self.top, self.right, self.bottom]
    }

    /// create from a 4-element array [left, top, right, bottom]
    pub fn from_array(arr: [i32; 4]) -> Self {
        Self {
            left: arr[0],
            top: arr[1],
            right: arr[2],
            bottom: arr[3],
        }
    }
}

/// Owned region geometry. The legacy header remains internal to win32k;
/// RGNDATA serialization is explicit and never copies this allocation to users.
#[repr(C)]
pub struct GdiRegion {
    pub type_: u8,
    pub uses: u8,
    pub flags: u8,
    pub palette: u8,
    pub size: u64,
    pub num_rects: i32,
    pub extents: Rect,
    pub geometry: Region,
}

impl GdiRegion {
    pub fn new(geometry: Region) -> Self {
        Self {
            type_: GdiObjectType::Region as u8,
            uses: 0,
            flags: 0,
            palette: 0,
            size: core::mem::size_of::<Self>() as u64,
            num_rects: geometry.rectangles().len() as i32,
            extents: geometry.bounds(),
            geometry,
        }
    }

    pub fn replace(&mut self, geometry: Region) {
        self.num_rects = geometry.rectangles().len() as i32;
        self.extents = geometry.bounds();
        self.geometry = geometry;
    }
}

// =============================================================================
// GDI Object Creation Functions
// =============================================================================

/// create a pen
pub fn gdi_create_pen(style: PenStyle, width: i32, color: u32) -> u64 {
    let input = super::pen_services::PenParameters {
        style: style as u32, width: width as u32, brush_style: 0, color,
        client_hatch: 0, hatch: 0, style_count: 0, old_style: true,
    };
    match super::pen_services::validate_pen(input, None) {
        Ok(Some(pen)) => gdi_publish_pen(pen),
        // Stock initialization calls this constructor for NULL_PEN itself;
        // requesting the stock handle here would recurse through its lock.
        Ok(None) => gdi_publish_pen(GdiPen::new(PenStyle::Null, 0, 0)),
        Err(_) => 0,
    }
}

/// Publish a fully captured pen. All subordinate state is embedded, so failed
/// publication and deletion require a single pool release.
pub(super) fn gdi_publish_pen(pen: GdiPen) -> u64 {
    let memory = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<GdiPen>(),
    );
    if memory.is_null() { return 0; }
    let kind = if pen.legacy { GdiObjectType::Pen } else { GdiObjectType::ExtPen };
    // SAFETY: fresh aligned pool allocation, not yet exposed through a handle.
    unsafe { memory.cast::<GdiPen>().write(pen); }
    let handle = get_handle_table().allocate(
        kind, memory as u64,
        super::window::input_context::caller_identity().0 as u32,
    );
    if handle == 0 {
        // SAFETY: no handle was published; GdiPen owns only embedded values.
        unsafe { let _ = crate::mm::pool::free(memory); }
    }
    handle
}

/// Copy pen state while its handle is locked, avoiding a pointer surviving
/// concurrent deletion or handle reuse.
pub fn gdi_get_pen(handle: u64) -> Option<GdiPen> {
    let table = get_handle_table();
    let entry = table.get(handle)?;
    if !matches!(entry.get_type(), Some(GdiObjectType::Pen | GdiObjectType::ExtPen))
        || entry.obj_ptr == 0
        || entry.process_id != 0
            && entry.process_id != super::window::input_context::caller_identity().0 as u32
    { return None; }
    // SAFETY: the validated pen-family entry remains protected by table guard.
    Some(unsafe { *(entry.obj_ptr as *const GdiPen) })
}

/// create a solid brush
pub fn gdi_create_solid_brush(color: u32) -> u64 {
    let brush = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<GdiBrush>(),
    );

    if brush.is_null() {
        // kprintln!("[win32k] gdi_create_solid_brush: failed to allocate memory");
        return 0;
    }

    unsafe {
        brush.cast::<GdiBrush>().write(GdiBrush::new_solid(color));
    }

    let mut handle_table = get_handle_table();
    let _ = &handle_table;
    let handle = handle_table.allocate(
        GdiObjectType::Brush,
        brush as u64,
        super::window::input_context::caller_identity().0 as u32,
    );
    let _ = &handle;

    // kprintln!("[win32k] gdi_create_solid_brush: color=0x{:08x} -> handle=0x{:016x}", color, handle);

    if handle == 0 {
        unsafe {
            let _ = crate::mm::pool::free(brush);
        }
    }
    handle
}

/// create a hatched brush
pub fn gdi_create_hatch_brush(style: HatchStyle, color: u32) -> u64 {
    let brush = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<GdiBrush>(),
    );

    if brush.is_null() {
        // kprintln!("[win32k] gdi_create_hatch_brush: failed to allocate memory");
        return 0;
    }

    unsafe {
        // SAFETY: this fresh pool allocation is aligned and not yet published.
        brush
            .cast::<GdiBrush>()
            .write(GdiBrush::new_hatched(style, color));
    }

    let mut handle_table = get_handle_table();
    let _ = &handle_table;
    let handle = handle_table.allocate(
        GdiObjectType::Brush,
        brush as u64,
        super::window::input_context::caller_identity().0 as u32,
    );
    if handle == 0 {
        // SAFETY: the fresh allocation was never published as a GDI object.
        unsafe {
            let _ = crate::mm::pool::free(brush);
        }
    }
    let _ = &handle;

    // kprintln!("[win32k] gdi_create_hatch_brush: style={:?}, color=0x{:08x} -> handle=0x{:016x}",
    //               style, color, handle);

    handle
}

pub extern "C" fn nt_gdi_create_hatch_brush_internal(
    frame: &crate::arch::common::syscall_frame::SyscallFrame,
) -> u64 {
    let style = match frame.args[0] as i32 {
        0 => HatchStyle::Horizontal,
        1 => HatchStyle::Vertical,
        2 => HatchStyle::FDiagonal,
        3 => HatchStyle::BDiagonal,
        4 => HatchStyle::Cross,
        5 => HatchStyle::DiagonalCross,
        _ => return 0,
    };
    gdi_create_hatch_brush(style, frame.args[1] as u32)
}

/// create a font
pub fn gdi_create_font(
    height: i32,
    width: i32,
    weight: i32,
    italic: bool,
    face_name: &[u16],
) -> u64 {
    let mut description = GdiFont::new();
    description.height = height;
    description.width = width;
    description.weight = weight;
    description.italic = u8::from(italic);
    description.set_face_name(face_name);
    gdi_create_logical_font(description)
}

/// Publish a captured logical font with the caller's process ownership.
pub(super) fn gdi_create_logical_font(description: GdiFont) -> u64 {
    let font = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<GdiFont>(),
    );

    if font.is_null() {
        // kprintln!("[win32k] gdi_create_font: failed to allocate memory");
        return 0;
    }

    // SAFETY: this fresh pool allocation has space and alignment for GdiFont;
    // no handle can observe it until the initialized object is published.
    unsafe { (font as *mut GdiFont).write(description); }

    let mut handle_table = get_handle_table();
    let _ = &handle_table;
    let handle = handle_table.allocate(
        GdiObjectType::Font,
        font as u64,
        super::window::input_context::caller_identity().0 as u32,
    );
    if handle == 0 {
        // SAFETY: the fresh allocation was never published as a GDI object.
        unsafe {
            let _ = crate::mm::pool::free(font);
        }
    }
    let _ = &handle;

    // kprintln!("[win32k] gdi_create_font: height={}, weight={} -> handle=0x{:016x}",
    //               height, weight, handle);

    handle
}

/// create a bitmap
pub fn gdi_create_bitmap(width: i32, height: i32, bit_count: u16) -> u64 {
    let bitmap = match GdiBitmap::new(width, height, bit_count) {
        Some(b) => b,
        None => {
            // kprintln!("[win32k] gdi_create_bitmap: failed to create bitmap");
            return 0;
        }
    };

    let bitmap_mem = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<GdiBitmap>(),
    );

    if bitmap_mem.is_null() {
        // kprintln!("[win32k] gdi_create_bitmap: failed to allocate memory");
        return 0;
    }

    // SAFETY: initialize the fresh allocation without dropping uninitialized
    // memory. Ownership of the pixels moves into the published bitmap.
    unsafe { bitmap_mem.cast::<GdiBitmap>().write(bitmap); }

    let mut handle_table = get_handle_table();
    let _ = &handle_table;
    let handle = handle_table.allocate(
        GdiObjectType::Bitmap,
        bitmap_mem as u64,
        super::window::input_context::caller_identity().0 as u32,
    );
    drop(handle_table);
    if handle == 0 {
        // SAFETY: the failed registration never published the allocation.
        unsafe {
            core::ptr::drop_in_place(bitmap_mem.cast::<GdiBitmap>());
            let _ = crate::mm::pool::free(bitmap_mem);
        }
    }

    // kprintln!("[win32k] gdi_create_bitmap: {}x{}x{} -> handle=0x{:016x}",
    //               width, height, bit_count, handle);

    handle
}

/// create a region from a rectangle
pub fn gdi_create_rect_rgn(left: i32, top: i32, right: i32, bottom: i32) -> u64 {
    match Region::rectangle(Rect::new(left, top, right, bottom)) {
        Ok(geometry) => gdi_create_region(geometry),
        Err(_) => 0,
    }
}

pub(super) fn gdi_create_region(geometry: Region) -> u64 {
    let allocation = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<GdiRegion>(),
    );
    if allocation.is_null() {
        return 0;
    }
    // SAFETY: this is a fresh allocation of sufficient size and alignment.
    unsafe {
        allocation
            .cast::<GdiRegion>()
            .write(GdiRegion::new(geometry));
    }
    let handle = with_handle_table(|table| {
        table.allocate(
            GdiObjectType::Region,
            allocation as u64,
            super::window::input_context::caller_identity().0 as u32,
        )
    });
    if handle == 0 {
        unsafe {
            core::ptr::drop_in_place(allocation.cast::<GdiRegion>());
            let _ = crate::mm::pool::free(allocation);
        }
    }
    handle
}

/// create a palette
pub fn gdi_create_palette() -> u64 {
    let Some(object) = GdiPalette::try_new() else {
        return 0;
    };
    let palette = crate::mm::pool::allocate(
        crate::mm::pool::PoolType::NonPaged,
        core::mem::size_of::<GdiPalette>(),
    );

    if palette.is_null() {
        // kprintln!("[win32k] gdi_create_palette: failed to allocate memory");
        return 0;
    }

    unsafe {
        // SAFETY: initialize fresh pool storage without dropping an uninitialized Vec.
        palette.cast::<GdiPalette>().write(object);
    }

    let mut handle_table = get_handle_table();
    let _ = &handle_table;
    let handle = handle_table.allocate(
        GdiObjectType::Palette,
        palette as u64,
        super::window::input_context::caller_identity().0 as u32,
    );
    if handle == 0 {
        // SAFETY: the fresh allocation was never published as a GDI object.
        unsafe {
            core::ptr::drop_in_place(palette.cast::<GdiPalette>());
            let _ = crate::mm::pool::free(palette);
        }
    }
    let _ = &handle;

    // kprintln!("[win32k] gdi_create_palette: -> handle=0x{:016x}", handle);

    handle
}

// =============================================================================
// GDI Object Deletion Functions
// =============================================================================

/// Delete a GDI object
pub fn gdi_delete_object(handle: u64) -> bool {
    // Win7 NtGdiDeleteObjectApp dispatches DCs to bDeleteDCInternal as
    // well as deleting brush/pen/font objects. Our DC table currently has
    // its own monotonically allocated 32-bit handle namespace.
    if (0x8000_0000..=u32::MAX as u64).contains(&handle) {
        return super::dc::gre_delete_dc(handle);
    }
    delete_object_checked(handle, None)
}

/// Validate the expected type and ownership while holding the deletion lock.
/// Keeping these checks together prevents handle reuse between validation and free.
pub(super) fn delete_object_checked(handle: u64, expected: Option<GdiObjectType>) -> bool {
    unsafe {
        if handle == 0 {
            return false;
        }

        let mut handle_table = get_handle_table();
        let _ = &handle_table;
        let entry = match handle_table.get(handle) {
            Some(e) => e,
            None => {
                // kprintln!("[win32k] gdi_delete_object: invalid handle 0x{:016x}", handle);
                return false;
            }
        };

        if expected.is_some_and(|kind| entry.get_type() != Some(kind))
            || entry.process_id != 0
                && entry.process_id != super::window::input_context::caller_identity().0 as u32
        {
            return false;
        }
        if entry.flags & HANDLE_FLAG_STOCK != 0 {
            return true;
        }
        if entry.uses != 0 {
            return false;
        }
        let obj_ptr = entry.obj_ptr;
        let _ = &obj_ptr;
        let obj_type = entry.get_type();
        let _ = &obj_type;

        // Free the object memory
        if obj_ptr != 0 {
            match obj_type {
                Some(GdiObjectType::ColorSpace | GdiObjectType::Pen | GdiObjectType::ExtPen) => {
                    let _ = crate::mm::pool::free(obj_ptr as *mut u8);
                }
                Some(GdiObjectType::Brush) => {
                    let _ = crate::mm::pool::free(obj_ptr as *mut u8);
                }
                Some(GdiObjectType::Font) => {
                    super::font_services::resources::forget_font(handle);
                    let _ = crate::mm::pool::free(obj_ptr as *mut u8);
                }
                Some(GdiObjectType::Bitmap) => {
                    // The table guard and zero selection count give exclusive
                    // ownership; Drop releases the bitmap's pixel allocation.
                    core::ptr::drop_in_place(obj_ptr as *mut GdiBitmap);
                    let _ = crate::mm::pool::free(obj_ptr as *mut u8);
                }
                Some(GdiObjectType::Region) => {
                    core::ptr::drop_in_place(obj_ptr as *mut GdiRegion);
                    let _ = crate::mm::pool::free(obj_ptr as *mut u8);
                }
                Some(GdiObjectType::Palette) => {
                    core::ptr::drop_in_place(obj_ptr as *mut GdiPalette);
                    let _ = crate::mm::pool::free(obj_ptr as *mut u8);
                }
                _ => {
                    let _ = crate::mm::pool::free(obj_ptr as *mut u8);
                }
            }
        }

        // Remove from handle table
        handle_table.free(handle);

        // kprintln!("[win32k] gdi_delete_object: handle=0x{:016x}, type={:?} deleted", handle, obj_type);

        true
    }
}

/// Get object pointer from handle
pub fn gdi_get_object_ptr(handle: u64) -> Option<u64> {
    let handle_table = get_handle_table();
    let _ = &handle_table;
    handle_table.get(handle).map(|e| e.obj_ptr)
}

// =============================================================================
// GDI Object Validation
// =============================================================================

/// Validate a GDI handle and return the entry if valid.
/// Checks:
/// 1. handle index is within bounds
/// 2. handle type prefix matches the stored object type
/// 3. Object pointer is non-null
pub fn gdi_validate_handle(handle: u64) -> Option<u64> {
    let handle_table = get_handle_table();
    let _ = &handle_table;

    // First check basic validity through the table
    if let Some(entry) = handle_table.get(handle) {
        if entry.obj_ptr != 0 {
            Some(entry.obj_ptr)
        } else {
            // kprintln!("[win32k] gdi_validate_handle: handle=0x{:016x} has null obj_ptr", handle);
            None
        }
    } else {
        // kprintln!("[win32k] gdi_validate_handle: handle=0x{:016x} not found or type mismatch", handle);
        None
    }
}

/// Validate a GDI handle for a specific process.
/// Windows GDI handles are process-specific - a handle created by one process
/// cannot be used by another process (without proper sharing).
pub fn gdi_validate_handle_for_process(handle: u64, process_id: u32) -> Option<u64> {
    let handle_table = get_handle_table();
    let _ = &handle_table;

    if let Some(entry) = handle_table.get(handle) {
        // Check process ownership (0 means kernel/system handle, valid for all)
        if (entry.process_id == process_id || entry.process_id == 0) && entry.obj_ptr != 0 {
            return Some(entry.obj_ptr);
        }
        // kprintln!("[win32k] gdi_validate_handle_for_process: handle=0x{:016x}, pid={}, owner={}",
        //             handle, process_id, entry.process_id);
        None
    } else {
        None
    }
}

/// Check if a handle is valid (exists and type matches)
pub fn gdi_is_handle_valid(handle: u64) -> bool {
    let handle_table = get_handle_table();
    let _ = &handle_table;
    handle_table.get(handle).is_some()
}

/// Get object type from handle
pub fn gdi_get_object_type(handle: u64) -> Option<GdiObjectType> {
    let handle_table = get_handle_table();
    let _ = &handle_table;
    handle_table.get(handle).and_then(|e| e.get_type())
}

/// Get handle entry for debugging
pub fn gdi_get_handle_entry(handle: u64) -> Option<GdiHandleEntry> {
    let handle_table = get_handle_table();
    let _ = &handle_table;
    handle_table.get(handle).copied()
}

/// Get pen color
pub fn gdi_get_pen_color(handle: u64) -> u32 {
    gdi_get_pen(handle).map_or(0, |pen| pen.color)
}

/// Get normalized pen width for either native pen family.
pub fn gdi_get_pen_width(handle: u64) -> i32 {
    gdi_get_pen(handle).map_or(1, |pen| pen.width)
}

/// Get brush color
pub fn gdi_get_brush_color(handle: u64) -> u32 {
    let handle_table = get_handle_table();
    let _ = &handle_table;
    if let Some(entry) = handle_table.get(handle) {
        if let Some(GdiObjectType::Brush) = entry.get_type() {
            let brush = unsafe { &*(entry.obj_ptr as *const GdiBrush) };
            let _ = &brush;
            return brush.color;
        }
    }
    0
}

// =============================================================================
// Stock Objects (Predefined GDI Objects)
// =============================================================================

#[path = "objects/stock.rs"]
mod stock;
pub use stock::{gdi_get_stock_object, init_stock_objects, nt_gdi_get_stock_object};
pub(crate) const HANDLE_FLAG_STOCK: u8 = stock::STOCK_FLAG;

// =============================================================================
// GDI Object Reference Counting
// =============================================================================

/// Select an object into a DC (increment usage)
pub fn gdi_select_object(handle: u64) -> bool {
    let mut handle_table = get_handle_table();
    let _ = &handle_table;
    handle_table.add_ref(handle)
}

/// Retains a selected object until its DC or saved state releases it. The
/// reference is acquired while validating the handle under the same lock.
pub struct ObjectReference {
    handle: u64,
    object_type: GdiObjectType,
}

impl ObjectReference {
    pub fn acquire(handle: u64) -> Option<Self> {
        with_handle_table(|table| {
            let entry = table.get(handle)?;
            if entry.process_id != 0
                && entry.process_id != super::window::input_context::caller_identity().0 as u32
            {
                return None;
            }
            let object_type = entry.get_type()?;
            if !table.add_ref(handle) {
                return None;
            }
            Some(Self {
                handle,
                object_type,
            })
        })
    }

    pub fn object_type(&self) -> GdiObjectType {
        self.object_type
    }
}

impl Drop for ObjectReference {
    fn drop(&mut self) {
        gdi_unselect_object(self.handle);
    }
}

/// Unselect an object from a DC (decrement usage)
pub fn gdi_unselect_object(handle: u64) {
    let mut handle_table = get_handle_table();
    let _ = &handle_table;
    handle_table.release(handle);
}

// =============================================================================
// Debug and Diagnostics
// =============================================================================

/// Dump handle table statistics
pub fn dump_handle_stats() {
    let handle_table = get_handle_table();
    let _ = &handle_table;
    let total = handle_table.entries.len();
    let _ = &total;
    let used = handle_table.entries.iter().filter(|e| e.is_some()).count();
    let _ = &used;

    // kprintln!("[win32k] handle table: {}/{} slots used", used, total);
}

#[path = "objects/attributes.rs"]
pub mod attributes;

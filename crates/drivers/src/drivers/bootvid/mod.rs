//! Boot Video (bootvid.dll)
//
//! Implements the kernel's text-mode framebuffer used during
//! early boot. The real `bootvid.dll` in Windows NT 6.1 has two
//! responsibilities:
//
//! 1. Drive the legacy VGA text buffer (0xB8000) so kernel
//!    progress messages are visible to the user while the
//!    kernel-mode VDM is being set up.
//! 2. Provide a small bitmap interface for `Inbv`-style boot
//!    screens (the four-corner progress bar, the rotating
//!    spinner, the "Setup is loading..." banner).
//
//!    In our environment we have two backends:
//
//! * The UEFI GOP linear framebuffer (preferred). winload
//!   publishes the LFB address, width, height, pitch, and
//!   format in `BootInfo.framebuffer_*`. We render into this
//!   memory once the kernel has mapped it (the bootloader
//!   already has it mapped).
//! * The legacy VGA text buffer at 0xB8000 (fallback). Used
//!   only when no GOP framebuffer is available.
//
//!   BOOTVID itself is hardware-agnostic: the public API is the
//!   same (`inbv_display_string`, `inbv_set_progress_bar_subset`,
//!   `inbv_rotate_waiting_spinner`, `vid_clean_up`, ...) and only the
//!   pixel-write layer changes when the backend switches.
//
//! # Windows 7 Boot Display Modes
//
//! - Normal Boot: Static Windows logo, progress bar only (no scrolling logs)
//! - BootLog Mode: Minimal/no screen output, logs to ntbtlog.txt
//! - SOS Mode: Shows driver loading messages like "Loading \SystemRoot\..."

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};

pub mod win7;

/// Boot display mode (Windows 7 behavior)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BootDisplayMode {
    /// Normal boot - Windows logo, progress bar only (no scrolling logs)
    #[default]
    Normal,
    /// BootLog mode - minimal screen output, logging to ntbtlog.txt
    BootLog,
    /// SOS/Verbose mode - shows driver loading messages
    Sos,
}

const VGA_TEXT_BASE: u64 = 0xB8000;
const VGA_COLS: u32 = 80;
const VGA_ROWS: u32 = 50;
const VGA_CELLS: usize = (VGA_COLS * VGA_ROWS) as usize;

/// Default text attributes used by the legacy VGA text buffer.
pub const ATTR_DEFAULT: u8 = 0x07; // light grey on black
pub const ATTR_HEADER: u8 = 0x1F; // white on blue
pub const ATTR_BSOD: u8 = 0x1F; // white on blue (Windows BSOD)

/// Cached description of the active framebuffer. Once
/// `init_from_framebuffer()` has been called we never go back
/// to the VGA path until `init()` is called explicitly.
static MODE_LFB: AtomicBool = AtomicBool::new(false);
static FB_BASE: AtomicU64 = AtomicU64::new(0);
static FB_WIDTH: AtomicU32 = AtomicU32::new(0);
static FB_HEIGHT: AtomicU32 = AtomicU32::new(0);
static FB_PITCH: AtomicU32 = AtomicU32::new(0);

/// Optional off-screen render target used by the boot animation.
/// When set, pixel blending goes to this buffer instead of the
/// visible framebuffer; the animation then copies the finished frame
/// back once per refresh to avoid clearing-induced flicker.
static RENDER_BASE: AtomicU64 = AtomicU64::new(0);
static RENDER_PITCH: AtomicU32 = AtomicU32::new(0);

fn render_base() -> u64 {
    let base = RENDER_BASE.load(Ordering::Acquire);
    if base != 0 {
        base
    } else {
        FB_BASE.load(Ordering::Acquire)
    }
}

fn render_pitch() -> u32 {
    let pitch = RENDER_PITCH.load(Ordering::Acquire);
    if pitch != 0 {
        pitch
    } else {
        FB_PITCH.load(Ordering::Acquire)
    }
}

pub fn set_render_target(base: u64, pitch: u32) {
    RENDER_BASE.store(base, Ordering::Release);
    RENDER_PITCH.store(pitch, Ordering::Release);
}

pub fn clear_render_target() {
    RENDER_BASE.store(0, Ordering::Release);
    RENDER_PITCH.store(0, Ordering::Release);
}

/// Boot display mode - controls what is shown on screen
static BOOT_DISPLAY_MODE: AtomicI32 = AtomicI32::new(0);

/// Set only after the kernel selects Ring-3 desktop mode. While this
/// flag is active, Ring-3 debug output must remain on the serial port
/// and must not paint over LogonUI/Explorer graphics.
static GUI_CONSOLE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Cursor and SPINNER state.
static INITIALISED: AtomicBool = AtomicBool::new(false);
static CURSOR_X: AtomicU32 = AtomicU32::new(0);
static CURSOR_Y: AtomicU32 = AtomicU32::new(0);
static SAVED_X: AtomicU32 = AtomicU32::new(0);
static SAVED_Y: AtomicU32 = AtomicU32::new(0);
static SPINNER_TICK: AtomicU32 = AtomicU32::new(0);
static ATTR: AtomicU8 = AtomicU8::new(ATTR_DEFAULT);

const SPINNER: [u8; 4] = *b"|/-\\";

/// Height of one BOOTVID glyph in pixels.
const FONT_HEIGHT: u32 = 16;

/// Number of scan lines advanced after each BOOTVID character.
const FONT_LINE_HEIGHT: u32 = FONT_HEIGHT;

/// Vertical distance from the top of a text cell to the baseline.
const FONT_BASELINE_OFFSET: u32 = FONT_HEIGHT - 4;

/// Number of character widths used by a tab stop.
const TAB_COLUMNS: u32 = 8;

/// Fallback cell width used when the embedded face cannot be parsed.
const FALLBACK_CELL_WIDTH: u32 = FONT_HEIGHT * 3 / 4;

/// Parsed monospaced face shared with the boot manager. Initialised
/// lazily on the first text draw because `fontdue` requires heap
/// allocations that are not available during early `#[no_mangle]`
/// entry.
#[cfg(not(target_arch = "riscv64"))]
static FONT_FACE: spin::Once<Option<nt61_fonts::FontFace>> = spin::Once::new();

/// Set while the embedded face is being parsed inside `FONT_FACE`.
///
/// Parsing allocates, and allocation paths can emit boot text, which
/// re-enters `font_face` while the `Once` is still in its RUNNING
/// state. With `panic = "abort"` the `Once`'s poison guard never
/// runs, so a blocking re-entry would deadlock the whole kernel (the
/// panic handler itself draws through bootvid). The guard converts
/// any re-entrant call into the fallback metrics instead.
#[cfg(not(target_arch = "riscv64"))]
static FONT_PARSE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Resolve the embedded terminal face, parsing it at most once.
/// The parse allocates, so it must wait until the kernel heap is
/// up; before that the fallback metrics below are used and the
/// real face is picked up automatically once heap::init ran.
#[cfg(target_arch = "riscv64")]
fn font_face_parsed() -> Option<&'static nt61_fonts::FontFace> {
    // RISC-V bring-up currently traps inside ttf-parser's CFF path
    // before reaching the shell. The boot console falls back to
    // conservative cell metrics, and serial output remains active.
    None
}

#[cfg(not(target_arch = "riscv64"))]
fn font_face_parsed() -> Option<&'static nt61_fonts::FontFace> {
    if let Some(face) = FONT_FACE.get() {
        return face.as_ref();
    }
    // The global allocator is the kernel heap, so its configuration
    // (not the mm init-state machine, which no production path
    // advances) is the authoritative signal that fontdue can allocate.
    crate::mm::heap::KERNEL_HEAP.configured_region()?;
    if FONT_PARSE_IN_PROGRESS.swap(true, Ordering::AcqRel) {
        return None;
    }
    let face =
        FONT_FACE.call_once(|| nt61_fonts::FontFace::from_bytes(nt61_fonts::TERMINAL_FONT).ok());
    FONT_PARSE_IN_PROGRESS.store(false, Ordering::Release);
    face.as_ref()
}

fn font_face() -> Option<&'static nt61_fonts::FontFace> {
    font_face_parsed()
}

/// Horizontal advance of one monospaced glyph cell, derived from
/// the embedded face metrics so the layout tracks the font rather
/// than a hard-coded pixel count.
fn glyph_cell_width() -> u32 {
    // Cache the metric: the fontdue glyph lookup must run exactly
    // once per boot (repeated lookups on the same face hit a hash
    // path that can spin forever when the heap is fragmented).
    static CACHED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    let cached = CACHED.load(core::sync::atomic::Ordering::Acquire);
    if cached != 0 {
        return cached;
    }
    let r = font_face()
        .and_then(|face| face.metrics('M', FONT_HEIGHT))
        .map(|metrics| ((metrics.advance_width + 0.5) as u32).max(1))
        .unwrap_or(FALLBACK_CELL_WIDTH);
    CACHED.store(r, core::sync::atomic::Ordering::Release);
    r
}

/// Initialise bootvid. If `init_from_framebuffer` was called
/// prior to this (the winload path) we render to the LFB.
/// Otherwise we fall back to the VGA text buffer on x86_64 or
/// become a no-op on the architectures that don't have a legacy
/// VGA text buffer (aarch64 / riscv64 / loongarch64).
pub fn init() {
    if INITIALISED.load(Ordering::Acquire) {
        return;
    }
    if MODE_LFB.load(Ordering::Acquire) {
        let base = FB_BASE.load(Ordering::Acquire);
        let width = FB_WIDTH.load(Ordering::Acquire);
        let height = FB_HEIGHT.load(Ordering::Acquire);
        let pitch = FB_PITCH.load(Ordering::Acquire);
        if base != 0 && width != 0 && height != 0 && pitch != 0 {
            // Paint the WHOLE framebuffer solid black so the OVMF
            // "Loading..." pixels / boot-manager UI that the
            // firmware left on the LFB are wiped. Without this
            // step the kernel's text overlays on top of the stale
            // OVMF splash which makes the boot log unreadable.
            // Then park the cursor at (0, 0) with the standard
            // light-grey-on-black attribute (0x07) so subsequent
            // `boot_println!` writes paint readable text.
            unsafe {
                let bg = colour_for_attr(0); // black BGRA
                let bytes_per_row = pitch as usize;
                let total_bytes = (height as usize) * bytes_per_row;
                let dst = base as *mut u8;
                for off in (0..total_bytes).step_by(4) {
                    let p = dst.add(off);
                    core::ptr::write_volatile(p, bg[0]); // B
                    core::ptr::write_volatile(p.add(1), bg[1]); // G
                    core::ptr::write_volatile(p.add(2), bg[2]); // R
                                                                // Alpha stays at 0xFF.
                }
            }
            CURSOR_X.store(0, Ordering::Release);
            CURSOR_Y.store(0, Ordering::Release);
            ATTR.store(0x07, Ordering::Release); // light grey on black
            INITIALISED.store(true, Ordering::Release);
            return;
        }
    }
    // Legacy VGA path. Only x86_64 actually maps 0xB8000; the
    // other architectures have no legacy text buffer and bootvid
    // becomes a no-op (boot output goes to the serial port or,
    // when the LFB is wired later, the `force_lfb_console()`
    // helper switches `MODE_LFB` on).
    if crate::drivers::arch::has_legacy_vga_text_console() {
        vid_initialize();
    } else {
        INITIALISED.store(true, Ordering::Release);
    }
}

/// Wire bootvid onto the winload-provided LFB. Call this from
/// `kernel_main` after the BOOTVID subsystem is selected as the
/// active display, but before any `inbv_display_string` /
/// `paint_progress_bar` / `bugcheck_screen` calls.
pub fn init_from_framebuffer(base: u64, width: u32, height: u32, pitch: u32) {
    if MODE_LFB.load(Ordering::Acquire) && FB_BASE.load(Ordering::Acquire) != 0 {
        return;
    }
    if base == 0 || width == 0 || height == 0 || pitch == 0 {
        crate::boot_println!("[BOOTVID] framebuffer zero");
        return;
    }
    FB_BASE.store(base, Ordering::Release);
    FB_WIDTH.store(width, Ordering::Release);
    FB_HEIGHT.store(height, Ordering::Release);
    FB_PITCH.store(pitch, Ordering::Release);
    MODE_LFB.store(true, Ordering::Release);
    crate::boot_debug_println!(
        "[BOOTVID] framebuffer VA 0x{:x} {}x{} pitch={}",
        base,
        width,
        height,
        pitch
    );
    if font_face().is_none() {
        crate::boot_println!("[BOOTVID] font face missing");
    } else {
        crate::boot_debug_println!("[BOOTVID] font face ok");
    }
    // We do NOT mark INITIALISED here so the public `init()`
    // call from `kernel_main` performs the cursor-home step.
}

/// One text-mode cell on the VGA path.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct TextCell {
    char: u8,
    attr: u8,
}

fn write_cell_vga(idx: usize, ch: u8, attr: u8) {
    let ptr = (VGA_TEXT_BASE as *mut u8).wrapping_add(idx * 2);
    unsafe {
        core::ptr::write_volatile(ptr, ch);
        core::ptr::write_volatile(ptr.add(1), attr);
    }
}

fn read_cell_vga(idx: usize) -> TextCell {
    let ptr = (VGA_TEXT_BASE as *const u8).wrapping_add(idx * 2);
    unsafe {
        TextCell {
            char: core::ptr::read_volatile(ptr),
            attr: core::ptr::read_volatile(ptr.add(1)),
        }
    }
}

fn scroll_up_vga() {
    for r in 1..VGA_ROWS as usize {
        for c in 0..VGA_COLS as usize {
            let src = read_cell_vga(r * VGA_COLS as usize + c);
            write_cell_vga((r - 1) * VGA_COLS as usize + c, src.char, src.attr);
        }
    }
    for c in 0..VGA_COLS as usize {
        write_cell_vga(
            (VGA_ROWS as usize - 1) * VGA_COLS as usize + c,
            b' ',
            ATTR_DEFAULT,
        );
    }
}

/// Scroll the linear framebuffer up by one line of glyphs (12 px).
/// The framebuffer is treated as 32-bit BGRA, pitch bytes per scan
/// line. We `memmove` the bytes that contain rows [12 .. height)
/// up to [0 .. height - 12) and then blank the newly exposed band
/// at the bottom with the current background colour.
///
/// This is the LFB counterpart of `scroll_up_vga`. Without it, once
/// the cursor walks off the bottom of the framebuffer the kernel
/// logs that were emitted earlier (boot phases, driver load lines,
/// SMSS subsystem bring-up, cmd.exe banner) sit at the bottom of
/// the screen forever and any new writes just stomp on top of them.
///
/// Scrolling moves every row up by exactly one glyph row. The glyph
/// row height used by `newline()` is 14 px (12 px glyph + 2 px gap)
/// and `scroll_up_lfb` must match that — earlier versions used 12 px
/// and left a 2-px black band at the top of every line plus left
/// stale characters in the bottom gap area.
fn scroll_up_lfb() {
    let base = FB_BASE.load(Ordering::Acquire);
    let pitch = FB_PITCH.load(Ordering::Acquire);
    let height = FB_HEIGHT.load(Ordering::Acquire);
    if base == 0 || pitch == 0 || height <= FONT_LINE_HEIGHT {
        return;
    }
    let bytes_per_row = pitch as usize;
    let glyph_row_bytes = FONT_LINE_HEIGHT as usize * bytes_per_row;
    let move_bytes = (height as usize - FONT_LINE_HEIGHT as usize) * bytes_per_row;
    unsafe {
        let dst = base as *mut u8;
        let src = dst.add(glyph_row_bytes);
        // memmove(dst, src, move_bytes) — regions overlap.
        core::ptr::copy(src, dst, move_bytes);
        // Blank the bottom band with the current background colour.
        let bg = colour_for_attr(ATTR.load(Ordering::Acquire) >> 4);
        let band_start = (height as usize - FONT_LINE_HEIGHT as usize) * bytes_per_row;
        let band_bytes = FONT_LINE_HEIGHT as usize * bytes_per_row;
        for off in (band_start..band_start + band_bytes).step_by(4) {
            let p = dst.add(off);
            core::ptr::write_volatile(p, bg[0]);
            core::ptr::write_volatile(p.add(1), bg[1]);
            core::ptr::write_volatile(p.add(2), bg[2]);
            // Alpha stays at 0xFF — the LFB is fully opaque.
        }
        // Also blank the previous top 14 rows so we don't leave
        // a stale smear at the very top of the screen after the
        // scroll completes.
        for off in (0..glyph_row_bytes).step_by(4) {
            let p = dst.add(off);
            core::ptr::write_volatile(p, bg[0]);
            core::ptr::write_volatile(p.add(1), bg[1]);
            core::ptr::write_volatile(p.add(2), bg[2]);
        }
    }
}

fn newline() {
    CURSOR_X.store(0, Ordering::Release);
    if MODE_LFB.load(Ordering::Acquire) {
        let y = CURSOR_Y.load(Ordering::Acquire) + FONT_LINE_HEIGHT;
        let h = FB_HEIGHT.load(Ordering::Acquire);
        if y + FONT_LINE_HEIGHT > h {
            // Walked off the bottom of the framebuffer — push
            // everything up by one glyph row and reuse the
            // bottom band for the next line. This is the LFB
            // analogue of `scroll_up_vga` and is what keeps
            // the boot log scrolling on the QEMU GUI window.
            scroll_up_lfb();
            CURSOR_Y.store(h.saturating_sub(FONT_LINE_HEIGHT), Ordering::Release);
        } else {
            CURSOR_Y.store(y, Ordering::Release);
        }
        return;
    }
    // Legacy VGA path.
    let y = CURSOR_Y.load(Ordering::Acquire) + 1;
    if y >= VGA_ROWS {
        scroll_up_vga();
        CURSOR_Y.store(VGA_ROWS - 1, Ordering::Release);
    } else {
        CURSOR_Y.store(y, Ordering::Release);
    }
}

/// write a single ASCII character to the active console. The LFB
/// path uses the embedded TrueType face and updates `CURSOR_X`/
/// `CURSOR_Y`. The legacy path uses the 0xB8000 text buffer.
/// Always call this through the public surface (`inbv_display_string`
/// or `put_byte_to_active_console`) so all callers see a single
/// code path.
fn putchar(ch: u8) {
    if MODE_LFB.load(Ordering::Acquire) {
        let x = CURSOR_X.load(Ordering::Acquire);
        let y = CURSOR_Y.load(Ordering::Acquire);
        putchar_lfb(ch);
        // VirtIO scanout mirrors only damaged pixels. Text bypasses the
        // desktop drawing API, so it must publish its own damage as well.
        // A line feed or wrap can scroll the whole framebuffer.
        let width = FB_WIDTH.load(Ordering::Acquire);
        let height = FB_HEIGHT.load(Ordering::Acquire);
        if ch == b'\n' || CURSOR_Y.load(Ordering::Acquire) != y {
            crate::hal::common::framebuffer_impl::mark_damage(0, 0, width, height);
        } else if ch >= b' ' {
            crate::hal::common::framebuffer_impl::mark_damage(x, y, glyph_cell_width(), FONT_HEIGHT);
        }
        return;
    }
    putchar_vga(ch);
}

/// Public wrapper for the per-byte console writer. Used by the
/// kernel's text-only paths (e.g. `gui_log` in `kernel_main` and
/// `rtl::klog::write_serial`) which already handle their own
/// `\n` / `\r` / `\t` semantics and just want to dump one byte
/// to whichever display is currently active. On the LFB path it
/// stays within the current `ATTR` colour pair.
///
/// This is an LFB-first API: in the winload → kernel boot path
/// the bootloader has already programmed the VGA controller to
/// scan out the linear framebuffer at the address supplied via
/// `BootInfo`, so writes to 0xB8000 are NOT visible. The bootvid
/// LFB backend is the only path that actually paints pixels
/// during that window.
pub fn put_byte_to_active_console(b: u8) {
    // Always publish what we render to the LFB path. Even when
    // the LFB hasn't been wired in yet, `MODE_LFB` is false and
    // we fall through to the legacy buffer.
    putchar(b);
}

/// True when the LFB backend is currently driving the display.
pub fn lfb_active() -> bool {
    MODE_LFB.load(Ordering::Acquire)
}

/// Force the bootvid putchar / cursor logic onto the LFB
/// backend. Used by `gui_log::init()` once the kernel has
/// transitioned from UEFI GOP to the VGA text-mode console:
/// QEMU's `-display gtk` window keeps scanning the GOP LFB
/// until it is told otherwise, so the only way to make the
/// user's GUI window show the kernel's CMD prompt is to
/// render every cell into the LFB as well.
///
/// Safe to call before `init_from_framebuffer`; if the LFB
/// wasn't wired in (no `BootInfo.framebuffer_*` reported by
/// winload) the putchar path will silently no-op because
/// `FB_BASE` / `FB_PITCH` stay zero.
pub fn force_lfb_console() {
    let base = FB_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    MODE_LFB.store(true, Ordering::Release);
}

fn putchar_vga(ch: u8) {
    match ch {
        b'\n' => {
            newline();
            return;
        }
        b'\r' => {
            CURSOR_X.store(0, Ordering::Release);
            return;
        }
        b'\t' => {
            let x = CURSOR_X.load(Ordering::Acquire);
            let next = (x / 8 + 1) * 8;
            CURSOR_X.store(next, Ordering::Release);
            if CURSOR_X.load(Ordering::Acquire) >= VGA_COLS {
                newline();
            }
            return;
        }
        0x08 => {
            let x = CURSOR_X.load(Ordering::Acquire);
            if x > 0 {
                CURSOR_X.store(x - 1, Ordering::Release);
            }
            let y = CURSOR_Y.load(Ordering::Acquire);
            write_cell_vga(
                y as usize * VGA_COLS as usize + CURSOR_X.load(Ordering::Acquire) as usize,
                b' ',
                ATTR_DEFAULT,
            );
            return;
        }
        _ => {}
    }
    let x = CURSOR_X.load(Ordering::Acquire);
    let y = CURSOR_Y.load(Ordering::Acquire);
    let attr = ATTR.load(Ordering::Acquire);
    write_cell_vga(y as usize * VGA_COLS as usize + x as usize, ch, attr);
    let nx = x + 1;
    if nx >= VGA_COLS {
        newline();
    } else {
        CURSOR_X.store(nx, Ordering::Release);
    }
}

/// Draw the glyph for `ch` at the current cursor position on the
/// LFB backend using the embedded TrueType face. The cell is first
/// filled with the background colour so stale pixels never bleed
/// through, then the antialiased coverage is alpha-blended on top.
fn putchar_lfb(ch: u8) {
    let cell_width = glyph_cell_width();
    match ch {
        b'\n' => {
            newline();
            return;
        }
        b'\r' => {
            CURSOR_X.store(0, Ordering::Release);
            return;
        }
        b'\t' => {
            let tab_width = cell_width * TAB_COLUMNS;
            let x = CURSOR_X.load(Ordering::Acquire);
            let next = ((x / tab_width) + 1) * tab_width;
            CURSOR_X.store(next, Ordering::Release);
            return;
        }
        0x08 => {
            let x = CURSOR_X.load(Ordering::Acquire);
            if x >= cell_width {
                CURSOR_X.store(x - cell_width, Ordering::Release);
            }
            return;
        }
        _ => {}
    }
    let base = FB_BASE.load(Ordering::Acquire);
    let pitch = FB_PITCH.load(Ordering::Acquire);
    let height = FB_HEIGHT.load(Ordering::Acquire);
    let width = FB_WIDTH.load(Ordering::Acquire);
    let px = CURSOR_X.load(Ordering::Acquire);
    let py = CURSOR_Y.load(Ordering::Acquire);
    if base == 0 || pitch == 0 {
        return;
    }
    let attr = ATTR.load(Ordering::Acquire);
    let fg = colour_for_attr(attr);
    let bg = colour_for_attr(attr >> 4);

    // Paint the glyph cell with the background colour first.
    for row in 0..FONT_HEIGHT {
        for col in 0..cell_width {
            let x = px + col;
            let y = py + row;
            if x >= width || y >= height {
                continue;
            }
            let off = (y * pitch + x * 4) as usize;
            unsafe {
                let p = (base as *mut u8).add(off);
                core::ptr::write_volatile(p, bg[0]); // B
                core::ptr::write_volatile(p.add(1), bg[1]); // G
                core::ptr::write_volatile(p.add(2), bg[2]); // R
                core::ptr::write_volatile(p.add(3), bg[3]); // A
            }
        }
    }

    let Some(face) = font_face() else {
        // The font parser is unavailable before heap setup and on the
        // RISC-V bring-up path. Still paint real glyphs, not empty cells.
        for row in 0..FONT_HEIGHT {
            let bits = crate::hal::common::framebuffer_impl::bitmap_glyph_row(
                ch, (row * 16 / FONT_HEIGHT) as usize,
            );
            for col in 0..cell_width {
                if bits & (0x80 >> (col * 8 / cell_width)) == 0 { continue; }
                let (x, y) = (px + col, py + row);
                if x < width && y < height {
                    unsafe {
                        ((base + u64::from(y * pitch + x * 4)) as *mut u32)
                            .write_volatile(u32::from_le_bytes(fg));
                    }
                }
            }
        }
        advance_cursor_lfb(px, cell_width, width);
        return;
    };
    let draw_char = if (0x20..0x7F).contains(&ch) {
        ch as char
    } else {
        ' '
    };
    let Some(glyph) = face.rasterize(draw_char, FONT_HEIGHT) else {
        advance_cursor_lfb(px, cell_width, width);
        return;
    };
    let metrics = &glyph.metrics;
    // The coverage bitmap is laid out with row 0 at the top of the
    // glyph. `ymin` is the offset of the bitmap's bottom edge below
    // the baseline (negative = extends below), so the top edge sits
    // at `baseline - (height + ymin)`.
    let baseline_y = (py + FONT_BASELINE_OFFSET) as i32;
    let top_y = baseline_y - (metrics.height as i32 + metrics.ymin);
    let origin_x = px as i32 + metrics.xmin;
    for row in 0..metrics.height {
        for col in 0..metrics.width {
            let Some(coverage) = glyph.coverage_at(col, row) else {
                continue;
            };
            if coverage == 0 {
                continue;
            }
            let x = origin_x + col as i32;
            let y = top_y + row as i32;
            if x < 0 || y < 0 {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            if x >= width || y >= height {
                continue;
            }
            let off = (y * pitch + x * 4) as usize;
            let alpha = u32::from(coverage);
            let inverse = 255 - alpha;
            let mut pix = [0u8; 4];
            for channel in 0..4 {
                let background = u32::from(bg[channel]);
                let foreground = u32::from(fg[channel]);
                pix[channel] = ((background * inverse + foreground * alpha) / 255) as u8;
            }
            unsafe {
                let p = (base as *mut u8).add(off);
                core::ptr::write_volatile(p, pix[0]); // B
                core::ptr::write_volatile(p.add(1), pix[1]); // G
                core::ptr::write_volatile(p.add(2), pix[2]); // R
                core::ptr::write_volatile(p.add(3), pix[3]); // A
            }
        }
    }
    advance_cursor_lfb(px, cell_width, width);
}

/// Advance the LFB cursor by one cell, wrapping to the next line
/// when the cell would run off the right edge.
fn advance_cursor_lfb(px: u32, cell_width: u32, width: u32) {
    let next = px + cell_width;
    if next + cell_width > width {
        newline();
    } else {
        CURSOR_X.store(next, Ordering::Release);
    }
}

/// Map a 4-bit VGA attribute (low nibble = fg) to a 4-byte BGRA
/// pixel. We use the canonical Windows 7 console palette. Each entry
/// is `[B, G, R, A]` because the LFB is scanned out in BGRA order
/// (matches the OVMF GOP `PixelFormat::Bgr`). Note in particular:
///   * index 1 (dark blue)  → R=0x00, G=0x00, B=0xAA — a TRUE dark blue,
///     NOT dark red (which was the bug in earlier builds, where the
///     table stored `[0xAA,0,0]` which QEMU scanned out as B=0xAA so
///     the blue console title bar came out red).
///   * index 4 (dark red)   → R=0xAA, G=0x00, B=0x00 — a TRUE dark red.
fn colour_for_attr(idx: u8) -> [u8; 4] {
    match idx & 0x0F {
        0 => [0x00, 0x00, 0x00, 0xFF],  // black
        1 => [0xAA, 0x00, 0x00, 0xFF],  // dark blue   (B=0xAA, R=0x00)
        2 => [0x00, 0xAA, 0x00, 0xFF],  // dark green  (G=0xAA)
        3 => [0xAA, 0xAA, 0x00, 0xFF],  // dark cyan   (B=0xAA, G=0xAA)
        4 => [0x00, 0x00, 0xAA, 0xFF],  // dark red    (R=0xAA)
        5 => [0xAA, 0x00, 0xAA, 0xFF],  // dark magenta
        6 => [0x00, 0x55, 0xAA, 0xFF],  // dark yellow / brown
        7 => [0xAA, 0xAA, 0xAA, 0xFF],  // light grey
        8 => [0x55, 0x55, 0x55, 0xFF],  // dark grey
        9 => [0xFF, 0x55, 0x55, 0xFF],  // light blue
        10 => [0x55, 0xFF, 0x55, 0xFF], // light green
        11 => [0xFF, 0xFF, 0x55, 0xFF], // light cyan
        12 => [0x55, 0x55, 0xFF, 0xFF], // light red
        13 => [0xFF, 0x55, 0xFF, 0xFF], // light magenta
        14 => [0x55, 0xFF, 0xFF, 0xFF], // yellow
        15 => [0xFF, 0xFF, 0xFF, 0xFF], // white
        _ => [0xFF, 0xFF, 0xFF, 0xFF],
    }
}

/// `inbv_display_string` — write a NUL-terminated ASCII string.
pub fn inbv_display_string(s: &[u8]) {
    for &b in s {
        if b == 0 {
            break;
        }
        putchar(b);
    }
}

/// NUL-terminated display, attribute can be supplied by the caller.
pub fn vid_display_string(s: &[u8], attr: u8) {
    for &b in s {
        if b == 0 {
            break;
        }
        if b == b'\n' {
            newline();
            continue;
        }
        ATTR.store(attr, Ordering::Release);
        putchar(b);
    }
    ATTR.store(ATTR_DEFAULT, Ordering::Release);
}

/// Same as above but also flushes any pending display operations
/// (no-op in our environment because the serial line is the same
/// one the kernel prints to).
pub fn inbv_display_string_blocking(s: &[u8]) {
    inbv_display_string(s);
}

/// `inbv_set_progress_bar_subset` — display `count` filled blocks.
pub fn inbv_set_progress_bar_subset(count: u32) {
    if !INITIALISED.load(Ordering::Acquire) {
        return;
    }
    if MODE_LFB.load(Ordering::Acquire) {
        // On the LFB we paint a 24-pixel-tall horizontal bar near
        // the bottom of the framebuffer. Colours are BGRA byte
        // triplets: byte 0 = B, byte 1 = G, byte 2 = R.
        let bar_h = 24u32;
        let bar_y0 = FB_HEIGHT.load(Ordering::Acquire).saturating_sub(bar_h + 8);
        let width = FB_WIDTH.load(Ordering::Acquire);
        let base = FB_BASE.load(Ordering::Acquire);
        let pitch = FB_PITCH.load(Ordering::Acquire);
        let cell = glyph_cell_width();
        let total = (width / cell).max(1);
        let filled = count.min(total);
        // Empty bar (dark grey on black).
        for row in 0..bar_h {
            for col in 0..width {
                let off = ((bar_y0 + row) * pitch + col * 4) as usize;
                unsafe {
                    let p = (base as *mut u8).add(off);
                    core::ptr::write_volatile(p, 0x40); // B
                    core::ptr::write_volatile(p.add(1), 0x40); // G
                    core::ptr::write_volatile(p.add(2), 0x40); // R
                    core::ptr::write_volatile(p.add(3), 0xFF);
                }
            }
        }
        // Filled segment (bright yellow: high R, high G, low B).
        let fill_w = filled * cell;
        for row in 0..bar_h {
            for col in 0..fill_w {
                let off = ((bar_y0 + row) * pitch + col * 4) as usize;
                unsafe {
                    let p = (base as *mut u8).add(off);
                    core::ptr::write_volatile(p, 0x00); // B
                    core::ptr::write_volatile(p.add(1), 0xFF); // G
                    core::ptr::write_volatile(p.add(2), 0xFF); // R
                    core::ptr::write_volatile(p.add(3), 0xFF);
                }
            }
        }
        return;
    }
    // VGA path.
    if !crate::drivers::arch::has_legacy_vga_text_console() {
        return;
    }
    SAVED_X.store(CURSOR_X.load(Ordering::Acquire), Ordering::Release);
    SAVED_Y.store(CURSOR_Y.load(Ordering::Acquire), Ordering::Release);
    CURSOR_X.store(0, Ordering::Release);
    CURSOR_Y.store(VGA_ROWS - 1, Ordering::Release);
    let blocks = count.min(VGA_COLS);
    for c in 0..VGA_COLS as usize {
        let ch = if (c as u32) < blocks { 0xDB } else { 0xB0 };
        write_cell_vga(
            (VGA_ROWS as usize - 1) * VGA_COLS as usize + c,
            ch,
            ATTR_HEADER,
        );
    }
    CURSOR_X.store(SAVED_X.load(Ordering::Acquire), Ordering::Release);
    CURSOR_Y.store(SAVED_Y.load(Ordering::Acquire), Ordering::Release);
}

/// `inbv_rotate_waiting_spinner` — show the four-frame spinner.
pub fn inbv_rotate_waiting_spinner() {
    if !INITIALISED.load(Ordering::Acquire) {
        return;
    }
    let tick = SPINNER_TICK.fetch_add(1, Ordering::Relaxed);
    let frame = SPINNER[(tick as usize) & 3];
    putchar(frame);
}

/// `vid_reset_display` — clear and rehome.
pub fn vid_reset_display(bios: bool) {
    let _ = bios;
    if MODE_LFB.load(Ordering::Acquire) {
        // Paint the entire framebuffer solid BLACK. We deliberately
        // do NOT use the loader-blue (`0xD8, 0x98, 0x70`) that bootmgr
        // and winload leave on screen — that colour was chosen by the
        // Windows 7 boot-manager UI team and only makes sense against
        // a black wallpaper with white text. The kernel's own boot
        // screen uses white-on-black, so we wipe the framebuffer to
        // black here and let `paint_phase_banner` (if called) draw
        // whatever top stripe the caller wants. Black (`0x00, 0x00,
        // 0x00, 0xFF` in BGRA) keeps the LFB in sync with the legacy
        // 0xB8000 text console that `gui_log::init()` also clears to
        // attribute 0x07 (light grey on black).
        let base = FB_BASE.load(Ordering::Acquire);
        let pitch = FB_PITCH.load(Ordering::Acquire);
        let width = FB_WIDTH.load(Ordering::Acquire);
        let height = FB_HEIGHT.load(Ordering::Acquire);
        if base != 0 && pitch != 0 && width != 0 && height != 0 {
            let total_bytes = (pitch as usize) * (height as usize);
            unsafe {
                let p = base as *mut u8;
                let mut off = 0usize;
                while off + 4 <= total_bytes {
                    core::ptr::write_volatile(p.add(off), 0x00);
                    core::ptr::write_volatile(p.add(off + 1), 0x00);
                    core::ptr::write_volatile(p.add(off + 2), 0x00);
                    core::ptr::write_volatile(p.add(off + 3), 0xFF);
                    off += 4;
                }
            }
        }
        CURSOR_X.store(0, Ordering::Release);
        CURSOR_Y.store(0, Ordering::Release);
        crate::hal::common::framebuffer_impl::mark_damage(0, 0, width, height);
        return;
    }
    for i in 0..VGA_CELLS {
        write_cell_vga(i, b' ', ATTR_DEFAULT);
    }
    CURSOR_X.store(0, Ordering::Release);
    CURSOR_Y.store(0, Ordering::Release);
}

/// `vid_clean_up` — disable the cursor and leave the screen alone.
pub fn vid_clean_up() {
    if !INITIALISED.load(Ordering::Acquire) {}
    // No-op for the LFB path. On the VGA path this would disable
    // the cursor via the CRTC index port, which is unsafe in
    // long mode.
}

/// `vid_clear_black` — paint the entire LFB solid black. Used
/// when transitioning from the UEFI GOP loader screen (which
/// leaves a blue/grey panel) to the kernel's VGA text mode
/// (which renders black-on-white). QEMU's `-display gtk`
/// keeps scanning out the GOP LFB until the user re-opens
/// the window, so we have to overwrite the stale pixels
/// before we start drawing text on top of them.
pub fn vid_clear_black() {
    if !MODE_LFB.load(Ordering::Acquire) {
        return;
    }
    let base = FB_BASE.load(Ordering::Acquire);
    let pitch = FB_PITCH.load(Ordering::Acquire);
    let width = FB_WIDTH.load(Ordering::Acquire);
    let height = FB_HEIGHT.load(Ordering::Acquire);
    if base == 0 || pitch == 0 || width == 0 || height == 0 {
        return;
    }
    let total_bytes = (pitch as usize) * (height as usize);
    unsafe {
        let p = base as *mut u8;
        let mut off = 0usize;
        while off + 4 <= total_bytes {
            core::ptr::write_volatile(p.add(off), 0x00);
            core::ptr::write_volatile(p.add(off + 1), 0x00);
            core::ptr::write_volatile(p.add(off + 2), 0x00);
            core::ptr::write_volatile(p.add(off + 3), 0xFF);
            off += 4;
        }
    }
    CURSOR_X.store(0, Ordering::Release);
    CURSOR_Y.store(0, Ordering::Release);
    crate::hal::common::framebuffer_impl::mark_damage(0, 0, width, height);
}

/// `vid_set_cursor_position` — programmatic cursor placement.
pub fn vid_set_cursor_position(x: u16, y: u16) {
    CURSOR_X.store(x as u32, Ordering::Release);
    CURSOR_Y.store(y as u32, Ordering::Release);
}

/// `vid_scroll_up` — push the framebuffer up by one glyph row and
/// reuse the bottom band for fresh text. Public counterpart of
/// `scroll_up_lfb`, exposed so user-mode code (cmd.exe, the bugcheck
/// screen painter) can request an explicit scroll when it has
/// decided the current row should be retired.
pub fn vid_scroll_up() {
    if MODE_LFB.load(Ordering::Acquire) {
        scroll_up_lfb();
        crate::hal::common::framebuffer_impl::mark_damage(
            0, 0, FB_WIDTH.load(Ordering::Acquire), FB_HEIGHT.load(Ordering::Acquire),
        );
    } else {
        scroll_up_vga();
    }
}

/// read the current bootvid cursor coordinates. Used by
/// callers that want to position a glyph without disturbing
/// the natural cursor flow (e.g. the Safe-Mode console
/// layout paint routine in `hal::x86_64::text_console`).
pub fn cursor_position() -> (u16, u16) {
    (
        CURSOR_X.load(Ordering::Acquire) as u16,
        CURSOR_Y.load(Ordering::Acquire) as u16,
    )
}

/// Snapshot of the wired-in linear framebuffer (base, width,
/// height, pitch). Useful for diagnostics — `show_safe_mode_console`
/// logs these so an operator can confirm the LFB is alive when
/// the Safe-Mode console is painted.
pub fn lfb_info() -> (u64, u32, u32, u32) {
    (
        FB_BASE.load(Ordering::Acquire),
        FB_WIDTH.load(Ordering::Acquire),
        FB_HEIGHT.load(Ordering::Acquire),
        FB_PITCH.load(Ordering::Acquire),
    )
}

#[inline]
pub fn fb_base() -> u64 {
    FB_BASE.load(Ordering::Acquire)
}
#[inline]
pub fn fb_width() -> u32 {
    FB_WIDTH.load(Ordering::Acquire)
}
#[inline]
pub fn fb_height() -> u32 {
    FB_HEIGHT.load(Ordering::Acquire)
}

pub fn fb_pitch() -> u32 {
    FB_PITCH.load(Ordering::Acquire)
}

pub fn is_lfb_active() -> bool {
    MODE_LFB.load(Ordering::Acquire)
}

/// Pick the text attribute used by the next characters. Standard
/// 16-colour VGA palette (see `colour_for_attr`).
pub fn set_attr(attr: u8) {
    ATTR.store(attr, Ordering::Release);
}

pub fn get_attr() -> u8 {
    ATTR.load(Ordering::Acquire)
}

/// `vid_initialize` — clear the screen, home the cursor. VGA path only.
pub fn vid_initialize() {
    if INITIALISED.load(Ordering::Acquire) {
        return;
    }
    // Skip VGA clear — 0xB8000 may be RO after ExitBootServices
    // (UEFI page tables don't identity-map it).
    CURSOR_X.store(0, Ordering::Release);
    CURSOR_Y.store(0, Ordering::Release);
    INITIALISED.store(true, Ordering::Release);
    ATTR.store(ATTR_DEFAULT, Ordering::Release);
}

/// Phase banner: paint a coloured top strip with the supplied title,
/// then clear the rest of the screen. Used by the BOOTVID glue
/// in `kernel_main` to draw a Windows-style progress banner.
///
/// NOTE: We previously painted the top 48 rows with the loader blue
/// (`0xD8, 0x98, 0x70`) — that was the original Win7 bootmgr screen
/// but it does not match the kernel's white-on-black console that
/// `gui_log` brings up afterwards. We now keep the entire framebuffer
/// black (the `vid_reset_display` call above already wiped it) and
/// only paint the title text. The banner therefore reads as white
/// "Windows 7" centred in the top quarter of a black screen, which
/// is what the kernel user sees during the normal boot sequence.
pub fn paint_phase_banner(title: &str) {
    if !MODE_LFB.load(Ordering::Acquire) {
        return;
    }
    vid_reset_display(true);
    let base = FB_BASE.load(Ordering::Acquire);
    let pitch = FB_PITCH.load(Ordering::Acquire);
    let width = FB_WIDTH.load(Ordering::Acquire);
    let height = FB_HEIGHT.load(Ordering::Acquire);
    if base == 0 || pitch == 0 {
        return;
    }
    let _ = base;
    let _ = pitch;
    let _ = height; // silence unused
                    // Title text in WHITE on BLACK. We use the same TTF face. The
                    // default attribute `ATTR_DEFAULT` (0x07 = light grey on black)
                    // was previously swapped to 0x1F (white on blue) so the title
                    // would be visible on the loader-blue top stripe; with the
                    // stripe gone we want white text on the existing black BGRA
                    // pixels and ATTR 0x0F (white on black) is the right value.
    let title_w_px = (title.len() as u32) * glyph_cell_width();
    let x0 = if width > title_w_px {
        (width - title_w_px) / 2
    } else {
        0
    };
    let y0 = (height / 4).saturating_sub(6);
    CURSOR_X.store(x0, Ordering::Release);
    CURSOR_Y.store(y0, Ordering::Release);
    let prev_attr = ATTR.swap(0x0F, Ordering::AcqRel);
    inbv_display_string(title.as_bytes());
    ATTR.store(prev_attr, Ordering::Release);
    CURSOR_X.store(0, Ordering::Release);
    CURSOR_Y.store(y0 + FONT_LINE_HEIGHT * 2, Ordering::Release);
}

/// Paint the Windows 7 normal-boot screen: black background, glowing
/// four-pane logo, "Starting Windows" text and a centered progress bar.
pub fn paint_boot_screen(percent: u32) {
    win7::paint_boot_screen(percent);
}

pub fn paint_boot_screen_time(t_ms: u32) {
    win7::paint_boot_screen_time(t_ms);
}

pub fn poll_animation_strip(cpu: u32) -> bool {
    win7::poll_animation_strip(cpu)
}

pub fn present_desktop_frame() -> bool {
    win7::present_desktop_frame()
}

pub fn initialize_basic_display() {
    win7::initialize_basic_display();
}

/// Draw four colored orbs moving toward the flag center with soft trails.
fn boot_draw_converging_orbs(screen_width: u32, screen_height: u32, logo_size: u32, progress: u32) {
    let progress = progress.clamp(0, 100);
    let center_x = screen_width / 2;
    let center_y = screen_height / 3;
    let spread_x = screen_width * 2 / 5;
    let spread_y = screen_height * 2 / 5;
    let travel = (progress * 73804 / 100) as i32;
    const ORBS: [(i32, i32, u32, u32, u32); 4] = [
        (-1, -1, 0x1C, 0x5A, 0xF7),
        (1, -1, 0x08, 0xAF, 0x38),
        (-1, 1, 0xF7, 0xA1, 0x00),
        (1, 1, 0x00, 0xB4, 0xFF),
    ];
    for (direction_x, direction_y, blue, green, red) in ORBS {
        let end_x = spread_x as i32 * direction_x;
        let end_y = spread_y as i32 * direction_y;
        let position_x = center_x as i32 + end_x * (100 - progress as i32) / 100;
        let position_y = center_y as i32 + end_y * (100 - progress as i32) / 100;
        let core_size = logo_size / 4 + logo_size / 8;
        let _ = travel;
        boot_orb(
            position_x.max(0) as u32,
            position_y.max(0) as u32,
            core_size,
            blue,
            green,
            red,
            0xE8,
        );
        let trail_length = 4 + progress / 12;
        for trail in 1..=trail_length {
            let trail_x = position_x + (end_x * trail as i32 / 16);
            let trail_y = position_y + (end_y * trail as i32 / 16);
            let alpha = 0xD8u32.saturating_sub(trail * 0x30);
            if alpha == 0 || trail_x < 0 || trail_y < 0 {
                continue;
            }
            boot_orb(
                trail_x as u32,
                trail_y as u32,
                core_size * 3 / 4,
                blue,
                green,
                red,
                alpha,
            );
        }
    }
}

/// Blend the Windows flag in as the four colored lights settle.
fn boot_flag_forming(x: u32, y: u32, size: u32, progress: u32) {
    let progress = progress.clamp(0, 255);
    if progress == 0 {
        return;
    }
    let screen_size = size as i32;
    let half = screen_size / 2;
    let wave = (size / 10).max(2) as i32;
    let gap = 2_i32;
    for row in 0..size {
        let fy = row as i32;
        for column in 0..size {
            let fx = column as i32;
            let edge_y = wave * boot_sine_u16((fx * 64 / screen_size.max(1)) as u32) / 73804;
            let edge_x = wave * boot_sine_u16((fy * 64 / screen_size.max(1)) as u32) / 73804;
            let vertical_division = half + edge_y;
            let horizontal_division = half + edge_x;
            if fx < edge_y
                || fx >= screen_size - edge_y
                || fy < edge_x
                || fy >= screen_size - edge_x
                || (fx - vertical_division).abs() <= gap
                || (fy - horizontal_division).abs() <= gap
            {
                continue;
            }
            let color = if fx < vertical_division && fy < horizontal_division {
                (0x1C, 0x5A, 0xF7)
            } else if fx >= vertical_division && fy < horizontal_division {
                (0x08, 0xAF, 0x38)
            } else if fx < vertical_division && fy >= horizontal_division {
                (0xF7, 0xA1, 0x00)
            } else {
                (0x00, 0xB4, 0xFF)
            };
            boot_blend_pixel(x + column, y + row, color.0, color.1, color.2, progress);
        }
    }
}

/// Draw one soft radial light with a bright core.
fn boot_orb(center_x: u32, center_y: u32, size: u32, blue: u32, green: u32, red: u32, alpha: u32) {
    let origin_x = center_x.saturating_sub(size / 2);
    let origin_y = center_y.saturating_sub(size / 2);
    let radius = size.max(1) / 2;
    let radius_sq = (radius * radius).max(1);
    for row in 0..size {
        let dy = row as i32 - radius as i32;
        for column in 0..size {
            let dx = column as i32 - radius as i32;
            let distance_sq = (dx * dx + dy * dy) as u32;
            if distance_sq > radius_sq {
                continue;
            }
            let falloff = 255 - distance_sq * 190 / radius_sq;
            let pixel_alpha = (alpha * falloff / 255).clamp(0, 255);
            if pixel_alpha == 0 {
                continue;
            }
            boot_blend_pixel(
                origin_x + column,
                origin_y + row,
                blue,
                green,
                red,
                pixel_alpha,
            );
        }
    }
}

/// Bright central flash when the four lights meet.
fn boot_flash(center_x: u32, center_y: u32, size: u32, alpha: u32) {
    let flash_size = size * 4;
    boot_orb(
        center_x,
        center_y,
        flash_size,
        0xFF,
        0xFF,
        0xFF,
        alpha.max(32),
    );
    boot_orb(center_x, center_y, flash_size / 3, 0xFF, 0xFF, 0xFF, alpha);
}

/// Paint the four Windows flag panes with curved, waving edges. The
/// curve model is intentionally deterministic and integer-friendly;
/// it reproduces the Win7 flag's 2x2 quadrants without shipping a
/// bitmap asset.
fn boot_flag(x: u32, y: u32, size: u32) {
    let screen_size = size as i32;
    let half = screen_size / 2;
    let wave = (size / 10).max(2) as i32;
    let gap = 2_i32;
    for row in 0..size {
        let fy = row as i32;
        for column in 0..size {
            let fx = column as i32;
            let edge_y = wave * boot_sine_u16((fx * 64 / screen_size.max(1)) as u32) / 73804;
            let edge_x = wave * boot_sine_u16((fy * 64 / screen_size.max(1)) as u32) / 73804;
            let vertical_division = half + edge_y;
            let horizontal_division = half + edge_x;
            if fx < edge_y
                || fx >= screen_size - edge_y
                || fy < edge_x
                || fy >= screen_size - edge_x
            {
                continue;
            }
            if (fx - vertical_division).abs() <= gap || (fy - horizontal_division).abs() <= gap {
                continue;
            }
            let color = if fx < vertical_division && fy < horizontal_division {
                (0x1C, 0x5A, 0xF7)
            } else if fx >= vertical_division && fy < horizontal_division {
                (0x08, 0xAF, 0x38)
            } else if fx < vertical_division && fy >= horizontal_division {
                (0xF7, 0xA1, 0x00)
            } else {
                (0x00, 0xB4, 0xFF)
            };
            boot_fill_lfb(x + column, y + row, 1, 1, color.0, color.1, color.2);
        }
    }
}

fn boot_sine_u16(angle: u32) -> i32 {
    const SINE: [u32; 66] = [
        0, 1608, 3216, 4823, 6428, 8031, 9632, 11230, 12825, 14416, 16002, 17583, 19157, 20725,
        22285, 23837, 25380, 26912, 28433, 29943, 31439, 32923, 34393, 35849, 37289, 38713, 40120,
        41510, 42883, 44236, 45570, 46885, 48178, 49450, 50700, 51927, 53131, 54311, 55466, 56596,
        57699, 58774, 59821, 60838, 61825, 62780, 63702, 64591, 65445, 66264, 67046, 67790, 68496,
        69163, 69790, 70377, 70923, 71427, 71889, 72308, 72684, 73016, 73304, 73548, 73748, 73804,
    ];
    SINE[angle.min(64) as usize] as i32
}

/// Draw a soft, animated glow behind the flag.
fn boot_flag_glow(center_x: u32, center_y: u32, size: u32, alpha: u32) {
    let glow_size = size * 9 / 4;
    let origin_x = center_x.saturating_sub(glow_size / 2);
    let origin_y = center_y.saturating_sub(glow_size / 2);
    let radius = glow_size as i32 / 2;
    let radius_sq = (radius * radius).max(1) as u32;
    let pulse_step = (alpha * 24 / 255) as i32;
    let base_alpha = 22 + pulse_step;
    for row in 0..glow_size {
        let dy = row as i32 - radius;
        for column in 0..glow_size {
            let dx = column as i32 - radius;
            let distance_sq = (dx * dx + dy * dy) as u32;
            if distance_sq > radius_sq {
                continue;
            }
            let falloff = 255 - distance_sq * 210 / radius_sq;
            let pixel_alpha = ((base_alpha as u32) * falloff / 255).min(255);
            if pixel_alpha == 0 {
                continue;
            }
            boot_blend_pixel(
                origin_x + column,
                origin_y + row,
                0xD4,
                0xB4,
                0x68,
                pixel_alpha,
            );
        }
    }
}

/// Integer pulse in `[0, 255]`; the logo's glow breathes during boot.
fn boot_pulse(percent: u32) -> u32 {
    let cycle = (percent * 24 / 100 + 12) % 48;
    const SINE: [u32; 49] = [
        0, 33, 65, 96, 126, 155, 182, 207, 230, 249, 264, 275, 282, 286, 286, 282, 275, 264, 249,
        230, 207, 182, 155, 126, 96, 65, 33, 0, 33, 65, 96, 126, 155, 182, 207, 230, 249, 264, 275,
        282, 286, 286, 282, 275, 264, 249, 230, 207, 182,
    ];
    SINE[cycle as usize]
}

/// Draw the centered boot label with a sans-serif face, cached as a
/// coverage bitmap to avoid repeated font allocations per frame.
fn boot_draw_label(text: &str, screen_width: u32, y: u32) {
    const LABEL_HEIGHT: u32 = 18;
    static LABEL: spin::Once<Option<BootLabel>> = spin::Once::new();
    let label = LABEL.get().and_then(|label| label.as_ref()).or_else(|| {
        let label = BootLabel::new(text, LABEL_HEIGHT);
        LABEL.call_once(|| label).as_ref()
    });
    let Some(label) = label else { return };
    let x = screen_width.saturating_sub(label.width) / 2;
    for row in 0..label.height {
        for column in 0..label.width {
            let Some(alpha) = label
                .coverage
                .get(row as usize * label.width as usize + column as usize)
                .copied()
            else {
                continue;
            };
            if alpha == 0 {
                continue;
            }
            boot_blend_pixel(x + column, y + row, 0xFF, 0xFF, 0xFF, u32::from(alpha));
        }
    }
}

fn boot_draw_label_at(
    cache: &'static spin::Once<Option<BootLabel>>,
    text: &str,
    screen_width: u32,
    y: u32,
    alpha: u32,
    blue: u32,
    green: u32,
    red: u32,
    pixel_height: u32,
) {
    let label = cache.get().and_then(|label| label.as_ref()).or_else(|| {
        let label = BootLabel::new(text, pixel_height);
        cache.call_once(|| label).as_ref()
    });
    let Some(label) = label else { return };
    let x = screen_width.saturating_sub(label.width) / 2;
    for row in 0..label.height {
        for column in 0..label.width {
            let Some(coverage) = label
                .coverage
                .get(row as usize * label.width as usize + column as usize)
                .copied()
            else {
                continue;
            };
            if coverage == 0 {
                continue;
            }
            let blended = (u32::from(coverage) * alpha) / 255;
            if blended > 0 {
                boot_blend_pixel(x + column, y + row, blue, green, red, blended);
            }
        }
    }
}

#[derive(Debug)]
struct BootLabel {
    width: u32,
    height: u32,
    coverage: alloc::vec::Vec<u8>,
}

impl BootLabel {
    fn new(text: &str, pixel_height: u32) -> Option<Self> {
        let face = nt61_fonts::FontFace::from_bytes(nt61_fonts::UI_FONT).ok()?;
        let mut width = 0_u32;
        let mut top = i32::MAX;
        let mut bottom = i32::MIN;
        let mut glyphs = alloc::vec::Vec::new();
        for character in text.chars() {
            let glyph = face.rasterize(character, pixel_height)?;
            let top_y = -(glyph.metrics.height as i32 + glyph.metrics.ymin);
            let bottom_y = top_y + glyph.metrics.height as i32;
            top = top.min(top_y);
            bottom = bottom.max(bottom_y);
            width += (glyph.metrics.advance_width + 0.5) as u32;
            glyphs.push(glyph);
        }
        if glyphs.is_empty() || top > bottom {
            return None;
        }
        let height = (bottom - top) as u32;
        let mut coverage = alloc::vec![0_u8; width as usize * height as usize];
        let mut pen_x = 0_i32;
        for glyph in glyphs {
            let x0 = pen_x + glyph.metrics.xmin;
            let top_y = -(glyph.metrics.height as i32 + glyph.metrics.ymin) - top;
            for row in 0..glyph.metrics.height {
                for column in 0..glyph.metrics.width {
                    let Some(source) = glyph.coverage_at(column, row) else {
                        continue;
                    };
                    if source == 0 {
                        continue;
                    }
                    let x = x0 + column as i32;
                    let y = top_y + row as i32;
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let index = y as usize * width as usize + x as usize;
                    coverage[index] = source.max(coverage[index]);
                }
            }
            pen_x += (glyph.metrics.advance_width + 0.5) as i32;
        }
        Some(Self {
            width,
            height,
            coverage,
        })
    }
}

fn boot_fill_rounded_lfb(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    radius: u32,
    blue: u32,
    green: u32,
    red: u32,
) {
    let radius = radius.min(width / 2).min(height / 2);
    for row in 0..height {
        for column in 0..width {
            if !boot_rounded_inside(column, row, width, height, radius) {
                continue;
            }
            boot_fill_lfb(x + column, y + row, 1, 1, blue, green, red);
        }
    }
}

fn boot_fill_rounded_blend(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    radius: u32,
    blue: u32,
    green: u32,
    red: u32,
    alpha: u32,
) {
    let radius = radius.min(width / 2).min(height / 2);
    for row in 0..height {
        for column in 0..width {
            if !boot_rounded_inside(column, row, width, height, radius) {
                continue;
            }
            boot_blend_pixel(x + column, y + row, blue, green, red, alpha);
        }
    }
}

fn boot_rounded_inside(column: u32, row: u32, width: u32, height: u32, radius: u32) -> bool {
    if radius == 0 {
        return true;
    }
    let right = width.saturating_sub(radius);
    let bottom = height.saturating_sub(radius);
    let dx = if column < radius {
        radius as i32 - column as i32
    } else if column >= right {
        (column - right + 1) as i32
    } else {
        0
    };
    let dy = if row < radius {
        radius as i32 - row as i32
    } else if row >= bottom {
        (row - bottom + 1) as i32
    } else {
        0
    };
    (dx * dx + dy * dy) <= (radius * radius) as i32
}

fn boot_blend_pixel(x: u32, y: u32, blue: u32, green: u32, red: u32, alpha: u32) {
    let base = render_base();
    let pitch = render_pitch();
    let fb_width = FB_WIDTH.load(Ordering::Acquire);
    let fb_height = FB_HEIGHT.load(Ordering::Acquire);
    if base == 0 || pitch == 0 || x >= fb_width || y >= fb_height {
        return;
    }
    let offset = ((y * pitch) + (x * 4)) as usize;
    let pixel = unsafe { (base as *mut u8).add(offset) };
    unsafe {
        for (channel_index, source) in [blue, green, red].iter().enumerate() {
            let destination = core::ptr::read_volatile(pixel.add(channel_index)) as u32;
            let mixed = (source * alpha + destination * (255 - alpha) + 128) / 255;
            core::ptr::write_volatile(pixel.add(channel_index), mixed as u8);
        }
    }
}

fn boot_fill_lfb(x: u32, y: u32, width: u32, height: u32, blue: u32, green: u32, red: u32) {
    let base = FB_BASE.load(Ordering::Acquire);
    let pitch = FB_PITCH.load(Ordering::Acquire);
    let fb_width = FB_WIDTH.load(Ordering::Acquire);
    let fb_height = FB_HEIGHT.load(Ordering::Acquire);
    if base == 0 || pitch == 0 || x >= fb_width || y >= fb_height {
        return;
    }
    let width = width.min(fb_width - x);
    let height = height.min(fb_height - y);
    unsafe {
        for row in 0..height {
            for column in 0..width {
                let offset = ((y + row) * pitch + (x + column) * 4) as usize;
                let pixel = (base as *mut u8).add(offset);
                core::ptr::write_volatile(pixel, blue as u8);
                core::ptr::write_volatile(pixel.add(1), green as u8);
                core::ptr::write_volatile(pixel.add(2), red as u8);
                core::ptr::write_volatile(pixel.add(3), 0xFF);
            }
        }
    }
}

fn boot_fill_blend(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    blue: u32,
    green: u32,
    red: u32,
    alpha: u32,
) {
    let base = FB_BASE.load(Ordering::Acquire);
    let pitch = FB_PITCH.load(Ordering::Acquire);
    let fb_width = FB_WIDTH.load(Ordering::Acquire);
    let fb_height = FB_HEIGHT.load(Ordering::Acquire);
    if base == 0 || pitch == 0 {
        return;
    }
    let x0 = x.min(fb_width.saturating_sub(1));
    let y0 = y.min(fb_height.saturating_sub(1));
    let width = width.min(fb_width - x0);
    let height = height.min(fb_height - y0);
    unsafe {
        for row in 0..height {
            for column in 0..width {
                let offset = ((y0 + row) * pitch + (x0 + column) * 4) as usize;
                let pixel = (base as *mut u8).add(offset);
                for (channel_index, source) in [blue, green, red].iter().enumerate() {
                    let destination = core::ptr::read_volatile(pixel.add(channel_index)) as u32;
                    let mixed = (source * alpha + destination * (255 - alpha) + 128) / 255;
                    core::ptr::write_volatile(pixel.add(channel_index), mixed as u8);
                }
            }
        }
    }
}

/// Paint a bottom progress-bar segment representing `percent`% of
/// `100` (clamped to `[0, 100]`). Used by `kernel_main` between
/// phases.
pub fn paint_progress_bar(percent: u32) {
    inbv_set_progress_bar_subset(percent);
}

/// Paint the Windows-style BSOD. `code` is the bugcheck code
/// formatted as 8 hex digits, `secondary` is the optional
/// secondary status line.
pub fn bugcheck_screen(code: u32, secondary: &str) {
    if MODE_LFB.load(Ordering::Acquire) {
        vid_reset_display(true);
        // Cover the entire screen in 0x0012_90_78 (BGRA) — the
        // canonical Win7 BSOD background.
        let base = FB_BASE.load(Ordering::Acquire);
        let pitch = FB_PITCH.load(Ordering::Acquire);
        let width = FB_WIDTH.load(Ordering::Acquire);
        let height = FB_HEIGHT.load(Ordering::Acquire);
        if base == 0 || pitch == 0 {
            return;
        }
        for y in 0..height {
            for x in 0..width {
                let off = (y * pitch + x * 4) as usize;
                unsafe {
                    let p = (base as *mut u8).add(off);
                    core::ptr::write_volatile(p, 0x90);
                    core::ptr::write_volatile(p.add(1), 0x12);
                    core::ptr::write_volatile(p.add(2), 0x00);
                    core::ptr::write_volatile(p.add(3), 0xFF);
                }
            }
        }
        // Title in 0x1F (white on blue attribute semantically, but
        // we render pure white on the blue background).
        CURSOR_X.store((width / 4).max(8), Ordering::Release);
        CURSOR_Y.store(height / 4, Ordering::Release);
        ATTR.store(0x1F, Ordering::Release);
        // Use a small scratch buffer to format a diagnostic header so the
        // `hex_buf` value is actually consumed by the call site below.
        let mut hex_buf = [0u8; 16];
        let hex_prefix = b"*** STOP: 0x";
        hex_buf[..hex_prefix.len()].copy_from_slice(hex_prefix);
        inbv_display_string(&hex_buf[..hex_prefix.len()]);
        let code_str = format_hex(code);
        inbv_display_string(&code_str);
        inbv_display_string(b"\n\n");
        ATTR.store(0x0E, Ordering::Release);
        inbv_display_string(secondary.as_bytes());
        return;
    }

    // VGA fallback: write to the text buffer. Only the legacy VGA
    // console actually maps 0xB8000 — on other architectures the
    // bugcheck text has already gone to the serial port, and writing
    // here would fault a second time inside the panic handler and
    // bury the real panic message.
    if !crate::drivers::arch::has_legacy_vga_text_console() {
        return;
    }
    vid_initialize();
    inbv_display_string(b"*** STOP: 0x");
    let code_str = format_hex(code);
    inbv_display_string(&code_str);
    inbv_display_string(b"\n\n");
    inbv_display_string(secondary.as_bytes());
}

fn format_hex(v: u32) -> [u8; 10] {
    let hex = b"0123456789ABCDEF";
    let mut out = [0u8; 10];
    for i in 0..8 {
        let nib = ((v >> ((7 - i) * 4)) & 0xF) as usize;
        out[i] = hex[nib];
    }
    out[8] = b' ';
    out[9] = b' ';
    out
}

// ============================================================================
// Windows 7 Boot Display Mode control
// ============================================================================

impl BootDisplayMode {
    /// Get boot display mode from atomic value
    pub fn from_atomic() -> Self {
        match BOOT_DISPLAY_MODE.load(Ordering::Acquire) {
            0 => BootDisplayMode::Normal,
            1 => BootDisplayMode::BootLog,
            2 => BootDisplayMode::Sos,
            _ => BootDisplayMode::Normal,
        }
    }

    /// Set boot display mode
    pub fn set(mode: BootDisplayMode) {
        match mode {
            BootDisplayMode::Normal => BOOT_DISPLAY_MODE.store(0, Ordering::Release),
            BootDisplayMode::BootLog => BOOT_DISPLAY_MODE.store(1, Ordering::Release),
            BootDisplayMode::Sos => BOOT_DISPLAY_MODE.store(2, Ordering::Release),
        }
    }
}

/// Set the boot display mode (called from kernel_main based on boot options)
pub fn set_boot_mode(mode: BootDisplayMode) {
    BootDisplayMode::set(mode);
}

/// Get current boot display mode
pub fn get_boot_mode() -> BootDisplayMode {
    BootDisplayMode::from_atomic()
}

/// Mark the framebuffer as owned by the Ring-3 desktop session.
pub fn set_gui_console_active(active: bool) {
    GUI_CONSOLE_ACTIVE.store(active, Ordering::Release);
}

/// Returns `true` when LogonUI/Explorer owns the framebuffer. Ring-3
/// debug strings should only use serial output in this state.
pub fn gui_console_active() -> bool {
    GUI_CONSOLE_ACTIVE.load(Ordering::Acquire)
}

/// Check if we should display scrolling text (SOS mode only)
fn should_display_scrolling() -> bool {
    BootDisplayMode::from_atomic() == BootDisplayMode::Sos
}

/// Display a message on screen (respects boot mode)
/// - Normal: Only progress bar updates
/// - BootLog: Minimal/no output
/// - SOS: Show driver loading messages
pub fn display_boot_message(msg: &[u8]) {
    let mode = BootDisplayMode::from_atomic();

    match mode {
        BootDisplayMode::Normal => {
            // Normal mode: Don't display scrolling text, only update progress bar
            // This matches Windows 7 behavior where you see the logo and progress bar
        }
        BootDisplayMode::BootLog => {
            // BootLog mode: Minimal output, logging goes to ntbtlog.txt
        }
        BootDisplayMode::Sos => {
            // SOS mode: Display driver loading messages
            inbv_display_string(msg);
        }
    }
}

/// Display SOS-style driver loading message
/// Format: "Loading \SystemRoot\system32\file.sys"
pub fn display_sos_loading(driver_path: &[u8]) {
    if !should_display_scrolling() {
        return;
    }

    // Format: "Loading \SystemRoot\..."
    let prefix = b"Loading ";
    for &b in prefix {
        putchar(b);
    }
    inbv_display_string(driver_path);
}

/// Display Windows 7 logo screen (normal boot)
pub fn display_windows_logo() {
    // In normal boot mode, display the Windows 7 loading screen
    // The logo is displayed during boot, not scrolling text
    paint_phase_banner("Windows 7");

    // Set up progress bar area
    paint_progress_bar(0);
}

/// Update boot progress (0-100)
pub fn update_progress(percent: u32) {
    paint_progress_bar(percent);
}

/// Smoke test for bootvid
pub fn smoke_test() -> bool {
    init();
    inbv_display_string(b"BootVid Smoke Test:\n");
    inbv_display_string(b"  Hello, kernel!\n");
    for _ in 0..4 {
        inbv_rotate_waiting_spinner();
    }
    inbv_set_progress_bar_subset(40);
    inbv_display_string(b"\n");
    vid_clean_up();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_includes_printable_glyphs() {
        let face = nt61_fonts::FontFace::from_bytes(nt61_fonts::TERMINAL_FONT)
            .expect("embedded terminal font must parse");
        for ch in ['A', 'Z', 'a', 'z', '0', '9', ' ', '-', '.'] {
            assert!(
                face.has_glyph(ch),
                "terminal font is missing glyph for {ch}"
            );
        }
    }

    #[test]
    fn lowercase_glyphs_are_distinct_from_uppercase() {
        let face = nt61_fonts::FontFace::from_bytes(nt61_fonts::TERMINAL_FONT)
            .expect("embedded terminal font must parse");
        for index in 0..26u8 {
            let lowercase = (b'a' + index) as char;
            let uppercase = (b'A' + index) as char;
            let lowercase_glyph = face
                .rasterize(lowercase, FONT_HEIGHT)
                .expect("lowercase glyph must rasterize");
            let uppercase_glyph = face
                .rasterize(uppercase, FONT_HEIGHT)
                .expect("uppercase glyph must rasterize");
            assert_ne!(
                lowercase_glyph.coverage, uppercase_glyph.coverage,
                "coverage for {lowercase} is identical to {uppercase}"
            );
        }
    }

    #[test]
    fn boot_display_mode_default() {
        assert_eq!(get_boot_mode(), BootDisplayMode::Normal);
    }
}

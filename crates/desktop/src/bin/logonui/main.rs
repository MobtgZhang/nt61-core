#![no_std]
#![no_main]
#![deny(warnings)]

mod arch;

#[path = "../../font_view.rs"]
mod font_view;
#[path = "../explorer/png.rs"]
mod png;
mod resources;

use core::panic::PanicInfo;

extern crate alloc;

struct LogonUiAllocator;

#[global_allocator]
static LOGONUI_ALLOCATOR: LogonUiAllocator = LogonUiAllocator;

// Room for the decoded 1024x768 background (~3 MiB) plus both glyph
// atlases (~1 MiB each) and the panel bitmaps.
const LOGONUI_HEAP_SIZE: usize = 32 * 1024 * 1024;
// Interior mutability is required: a plain immutable `static` lands in
// the read-only `.rodata` PE section, and every bump allocation would
// then page-fault in ring 3 (the section's PTE is read-only for user
// pages). `UnsafeCell` forces the linker to emit this in writable
// `.data`.
struct LogonUiHeap(core::cell::UnsafeCell<[u8; LOGONUI_HEAP_SIZE]>);
unsafe impl Sync for LogonUiHeap {}
static LOGONUI_HEAP: LogonUiHeap =
    LogonUiHeap(core::cell::UnsafeCell::new([0; LOGONUI_HEAP_SIZE]));
static LOGONUI_HEAP_CURSOR: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

unsafe impl core::alloc::GlobalAlloc for LogonUiAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let alignment = layout.align().max(1);
        let cursor = LOGONUI_HEAP_CURSOR.load(core::sync::atomic::Ordering::Acquire);
        let aligned = cursor.div_ceil(alignment) * alignment;
        let end = aligned.checked_add(layout.size()).unwrap_or(usize::MAX);
        if end > LOGONUI_HEAP_SIZE {
            return core::ptr::null_mut();
        }
        LOGONUI_HEAP_CURSOR.store(end, core::sync::atomic::Ordering::Release);
        (LOGONUI_HEAP.0.get() as *mut u8).add(aligned)
    }

    unsafe fn dealloc(&self, _pointer: *mut u8, _layout: core::alloc::Layout) {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write as _;
    struct SerialWriter;
    impl core::fmt::Write for SerialWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            #[cfg(target_arch = "x86_64")]
            arch::debug_str(s);
            let _ = s;
            Ok(())
        }
    }
    let _ = write!(SerialWriter {}, "[LOGONUI] panic: {}\r\n", info);
    arch::exit_process(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry() -> ! {
    logonui_main()
}

const PANEL_WIDTH: u32 = 320;
const PANEL_HEIGHT: u32 = 300;
const PANEL_LIFT: u32 = 60;
const BRAND_FLAG_WIDTH: u32 = 32;
const BRAND_GAP: u32 = 12;

struct LogonAssets {
    background: png::Image,
    panel: png::Image,
    brand_flag: png::Image,
    btn_ease: png::Image,
    btn_shutdown: png::Image,
    fonts: font_view::FontPack,
}

fn logonui_main() -> ! {
    let mut framebuffer = arch::DesktopFramebufferInfo::default();
    arch::desktop_info(&mut framebuffer);
    if framebuffer.width == 0 || framebuffer.height == 0 {
        loop {
            let mut key = 0u8;
            unsafe {
                arch::wait_key(&mut key);
            }
        }
    }

    match LogonAssets::load() {
        Some(assets) => {
            assets.draw_logon_screen(&framebuffer);
        }
        None => {
            draw_fallback_screen(&framebuffer);
        }
    }
    #[cfg(target_arch = "x86_64")]
    arch::debug_str("[LOGONUI] screen drawn, waiting for enter\r\n");

    // Wait for the user to press Enter, then hand off to the desktop:
    // exiting dispatches the ready explorer.exe thread via the
    // scheduler (same path cmd.exe uses after autoexec).
    loop {
        let mut key = 0u8;
        unsafe {
            let pending = arch::poll_key(&mut key) == 1;
            // When a key is already pending, use it directly - do NOT
            // block on a second key via wait_key (the original Enter
            // would be swallowed and the user would have to press it
            // twice). Fall back to the blocking read only when idle.
            let got = if pending {
                key as u64
            } else {
                arch::wait_key(&mut key)
            };
            if got == b'\r' as u64 || got == b'\n' as u64 {
                arch::exit_process(0);
            }
        }
    }
}

impl LogonAssets {
    fn load() -> Option<Self> {
        Some(Self {
            background: png::decode_png(resources::BACKGROUND).ok()?,
            panel: png::decode_png(resources::PANEL).ok()?,
            brand_flag: png::decode_png(resources::BRAND_FLAG).ok()?,
            btn_ease: png::decode_png(resources::BTN_EASE).ok()?,
            btn_shutdown: png::decode_png(resources::BTN_SHUTDOWN).ok()?,
            fonts: font_view::FontPack::decode()?,
        })
    }

    fn draw_logon_screen(&self, framebuffer: &arch::DesktopFramebufferInfo) {
        let width = framebuffer.width;
        let height = framebuffer.height;

        // Base wash for viewports larger than the artwork, then the
        // pre-rendered Aero background.
        arch::fill_rect(0, 0, width, height, 0x0c2450);
        unsafe {
            arch::draw_rgba(
                self.background.pixels.as_ptr(),
                0,
                0,
                self.background.width,
                self.background.height,
                self.background.stride(),
            );
        }

        // Centered welcome panel (avatar frame + password field are
        // baked into the glass bitmap).
        let panel_x = (width.saturating_sub(PANEL_WIDTH)) / 2;
        let panel_y = (height.saturating_sub(PANEL_HEIGHT)) / 2 - PANEL_LIFT;
        unsafe {
            arch::draw_rgba(
                self.panel.pixels.as_ptr(),
                panel_x,
                panel_y,
                self.panel.width,
                self.panel.height,
                self.panel.stride(),
            );
        }

        // User tile label.
        let name = "User";
        let name_width = self.fonts.measure(self.fonts.ui(), name);
        self.fonts.draw_labelled(
            self.fonts.ui(),
            name,
            panel_x + (PANEL_WIDTH - name_width) / 2,
            panel_y + 132,
        );

        // Bottom brand: four-pane flag + product name, like the real
        // welcome screen.
        let brand = "Windows 7";
        let brand_width = self.fonts.measure(self.fonts.brand(), brand);
        let combo_width = BRAND_FLAG_WIDTH + BRAND_GAP + brand_width;
        let combo_x = (width.saturating_sub(combo_width)) / 2;
        let brand_y = height.saturating_sub(96);
        unsafe {
            arch::draw_rgba(
                self.brand_flag.pixels.as_ptr(),
                combo_x,
                brand_y + 4,
                self.brand_flag.width,
                self.brand_flag.height,
                self.brand_flag.stride(),
            );
        }
        self.fonts.draw(
            self.fonts.brand(),
            brand,
            combo_x + BRAND_FLAG_WIDTH + BRAND_GAP,
            brand_y,
            font_view::Ink::White,
        );

        // Subtle sign-in hint.
        let hint = "Press ENTER to log on";
        let hint_width = self.fonts.measure(self.fonts.ui(), hint);
        self.fonts.draw_labelled(
            self.fonts.ui(),
            hint,
            (width.saturating_sub(hint_width)) / 2,
            height.saturating_sub(40),
        );

        // Ease of access (bottom left) and shutdown (bottom right).
        unsafe {
            arch::draw_rgba(
                self.btn_ease.pixels.as_ptr(),
                12,
                height.saturating_sub(36),
                self.btn_ease.width,
                self.btn_ease.height,
                self.btn_ease.stride(),
            );
            arch::draw_rgba(
                self.btn_shutdown.pixels.as_ptr(),
                width.saturating_sub(36),
                height.saturating_sub(36),
                self.btn_shutdown.width,
                self.btn_shutdown.height,
                self.btn_shutdown.stride(),
            );
        }
    }
}

fn draw_fallback_screen(framebuffer: &arch::DesktopFramebufferInfo) {
    const TOP: u32 = 0x3a6ea5;
    const BOTTOM: u32 = 0x0b2a4a;
    let height = framebuffer.height;
    let mut row = 0;
    while row < height {
        let scale = (row as u64 * 255) / (height as u64).max(1);
        let rgb = lerp(TOP, BOTTOM, scale);
        arch::fill_rect(0, row as u32, framebuffer.width, 1, rgb);
        row += 1;
    }
    let _ = height;
}

fn lerp(top: u32, bottom: u32, scale: u64) -> u32 {
    let top_r = (top >> 16) as i64;
    let top_g = (top >> 8) as i64 & 0xFF;
    let top_b = (top & 0xFF) as i64;
    let bottom_r = (bottom >> 16) as i64;
    let bottom_g = (bottom >> 8) as i64 & 0xFF;
    let bottom_b = (bottom & 0xFF) as i64;
    let red = top_r + ((bottom_r - top_r) * scale as i64 / 255);
    let green = top_g + ((bottom_g - top_g) * scale as i64 / 255);
    let blue = top_b + ((bottom_b - top_b) * scale as i64 / 255);
    ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
}

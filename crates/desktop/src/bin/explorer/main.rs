#![no_std]
#![no_main]
#![deny(warnings)]

mod arch;
#[path = "../../font_view.rs"]
mod font_view;
mod png;
mod resources_data;

use core::panic::PanicInfo;

extern crate alloc;

struct ExplorerAllocator;

#[global_allocator]
static EXPLORER_ALLOCATOR: ExplorerAllocator = ExplorerAllocator;

// Room for the decoded 1024x768 wallpaper (~3 MiB), both glyph
// atlases (~1 MiB each) and the shell bitmaps. The bump allocator
// never frees, so this must cover the worst case.
const EXPLORER_HEAP_SIZE: usize = 48 * 1024 * 1024;
// Interior mutability is required: a plain immutable `static` lands in
// the read-only `.rodata` PE section, and every bump allocation would
// then page-fault in ring 3 (the section's PTE is read-only for user
// pages). `UnsafeCell` forces the linker to emit this in writable
// `.data`.
struct ExplorerHeap(core::cell::UnsafeCell<[u8; EXPLORER_HEAP_SIZE]>);
unsafe impl Sync for ExplorerHeap {}
static EXPLORER_HEAP: ExplorerHeap =
    ExplorerHeap(core::cell::UnsafeCell::new([0; EXPLORER_HEAP_SIZE]));
static EXPLORER_HEAP_CURSOR: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

unsafe impl core::alloc::GlobalAlloc for ExplorerAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let alignment = layout.align().max(1);
        let cursor = EXPLORER_HEAP_CURSOR.load(core::sync::atomic::Ordering::Acquire);
        let aligned = cursor.div_ceil(alignment) * alignment;
        let end = aligned.checked_add(layout.size()).unwrap_or(usize::MAX);
        if end > EXPLORER_HEAP_SIZE {
            return core::ptr::null_mut();
        }
        EXPLORER_HEAP_CURSOR.store(end, core::sync::atomic::Ordering::Release);
        (EXPLORER_HEAP.0.get() as *mut u8).add(aligned)
    }

    unsafe fn dealloc(&self, _pointer: *mut u8, _layout: core::alloc::Layout) {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Render the panic message through the kernel serial syscall so
    // the death reason shows up in the boot log (the desktop has no
    // other console).
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
    let _ = write!(SerialWriter {}, "[EXPLORER] panic: {}\r\n", info);
    arch::exit_process(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry() -> ! {
    desktop_main()
}

const TASKBAR_HEIGHT: u32 = 40;
const ICON_CELL: u32 = 96;

struct Shell {
    wallpaper: png::Image,
    taskbar: png::Image,
    orb: png::Image,
    computer: png::Image,
    network: png::Image,
    recycle_bin: png::Image,
    control_panel: png::Image,
    fonts: font_view::FontPack,
}

fn desktop_main() -> ! {
    #[cfg(target_arch = "x86_64")]
    arch::debug_str("[EXPLORER] enter\r\n");
    let mut framebuffer = arch::DesktopFramebufferInfo::default();
    arch::desktop_info(&mut framebuffer);
    #[cfg(target_arch = "x86_64")]
    arch::debug_str("[EXPLORER] fb ok\r\n");
    if framebuffer.width == 0 || framebuffer.height == 0 {
        loop {
            let mut key = 0u8;
            unsafe {
                arch::wait_key(&mut key);
            }
        }
    }

    let shell = match Shell::load() {
        Some(shell) => shell,
        None => {
            #[cfg(target_arch = "x86_64")]
            arch::debug_str("[EXPLORER] resources decode failed, fallback\r\n");
            // Resource decode failure: keep the old flat fallback so the
            // desktop is still visible and the user can still log on.
            draw_fallback_desktop(&framebuffer);
            loop {
                let mut key = 0u8;
                unsafe {
                    let pending = arch::poll_key(&mut key) == 1;
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
    };

    shell.draw_desktop(&framebuffer);
    // Event pump: Enter re-renders the icon layer (mirrors the
    // Progman refresh message the real shell receives).
    loop {
        let mut key = 0u8;
        unsafe {
            let pending = arch::poll_key(&mut key) == 1;
            let got = if pending {
                key as u64
            } else {
                arch::wait_key(&mut key)
            };
            if got == b'\r' as u64 || got == b'\n' as u64 {
                shell.draw_icons(&framebuffer);
            }
        }
    }
}

impl Shell {
    fn load() -> Option<Self> {
        Some(Self {
            wallpaper: png::decode_png(resources_data::WALLPAPER).ok()?,
            taskbar: png::decode_png(resources_data::TASKBAR).ok()?,
            orb: png::decode_png(resources_data::ORB).ok()?,
            computer: png::decode_png(resources_data::COMPUTER).ok()?,
            network: png::decode_png(resources_data::NETWORK).ok()?,
            recycle_bin: png::decode_png(resources_data::RECYCLE_BIN).ok()?,
            control_panel: png::decode_png(resources_data::CONTROL_PANEL).ok()?,
            fonts: font_view::FontPack::decode()?,
        })
    }

    fn draw_desktop(&self, framebuffer: &arch::DesktopFramebufferInfo) {
        self.draw_wallpaper(framebuffer);
        self.draw_icons(framebuffer);
        self.draw_taskbar(framebuffer);
    }

    fn draw_wallpaper(&self, framebuffer: &arch::DesktopFramebufferInfo) {
        // Base wash for viewports larger than the artwork.
        arch::fill_rect(0, 0, framebuffer.width, framebuffer.height, 0x12305e);
        unsafe {
            arch::draw_rgba(
                self.wallpaper.pixels.as_ptr(),
                0,
                0,
                self.wallpaper.width,
                self.wallpaper.height,
                self.wallpaper.stride(),
            );
        }
    }

    fn draw_icons(&self, framebuffer: &arch::DesktopFramebufferInfo) {
        let _ = framebuffer;
        for (index, (icon, label)) in [
            (&self.computer, "Computer"),
            (&self.network, "Network"),
            (&self.recycle_bin, "Recycle Bin"),
            (&self.control_panel, "Control Panel"),
        ]
        .into_iter()
        .enumerate()
        {
            let x = 20u32;
            let y = 20u32 + index as u32 * ICON_CELL;
            unsafe {
                arch::draw_rgba(
                    icon.pixels.as_ptr(),
                    x,
                    y,
                    icon.width,
                    icon.height,
                    icon.stride(),
                );
            }
            let width = self.fonts.measure(self.fonts.ui(), label);
            let label_x = x + (56u32.saturating_sub(width)) / 2;
            self.fonts
                .draw_labelled(self.fonts.ui(), label, label_x, y + 52);
        }
    }

    fn draw_taskbar(&self, framebuffer: &arch::DesktopFramebufferInfo) {
        let top = framebuffer.height.saturating_sub(TASKBAR_HEIGHT);
        unsafe {
            arch::draw_rgba(
                self.taskbar.pixels.as_ptr(),
                0,
                top,
                self.taskbar.width,
                self.taskbar.height,
                self.taskbar.stride(),
            );
            arch::draw_rgba(
                self.orb.pixels.as_ptr(),
                6,
                framebuffer.height - 38,
                self.orb.width,
                self.orb.height,
                self.orb.stride(),
            );
        }
    }
}

fn draw_fallback_desktop(framebuffer: &arch::DesktopFramebufferInfo) {
    const TOP: u32 = 0x003c8f;
    const BOTTOM: u32 = 0x00224a;
    let height = framebuffer.height.saturating_sub(TASKBAR_HEIGHT);
    let mut row = 0;
    while row < height {
        let scale = (row as u64 * 255) / (height as u64).max(1);
        let rgb = lerp(TOP, BOTTOM, scale);
        arch::fill_rect(0, row as u32, framebuffer.width, 1, rgb);
        row += 1;
    }
    arch::fill_rect(
        0,
        height,
        framebuffer.width,
        TASKBAR_HEIGHT,
        0x1b2739,
    );
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

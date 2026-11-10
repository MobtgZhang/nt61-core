#![no_std]
#![no_main]
#![deny(warnings)]
#![allow(dead_code)]

#[path = "../explorer/arch/mod.rs"]
mod arch;
#[path = "../../font_view.rs"]
mod font_view;

use alloc::string::String;
use core::panic::PanicInfo;
use nt61_desktop::compositor::aero;
use nt61_desktop::graphics::Surface;
use nt61_desktop::widgets::textbox::TextBox;

extern crate alloc;

struct NotepadAllocator;

#[global_allocator]
static NOTEPAD_ALLOCATOR: NotepadAllocator = NotepadAllocator;

const NOTEPAD_HEAP_SIZE: usize = 12 * 1024 * 1024;
const CLIENT_EDGE: u32 = 8;
const CAPTION_HEIGHT: u32 = 30;

struct NotepadHeap(core::cell::UnsafeCell<[u8; NOTEPAD_HEAP_SIZE]>);

unsafe impl Sync for NotepadHeap {}

static NOTEPAD_HEAP: NotepadHeap = NotepadHeap(core::cell::UnsafeCell::new([0; NOTEPAD_HEAP_SIZE]));

static NOTEPAD_HEAP_CURSOR: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

unsafe impl core::alloc::GlobalAlloc for NotepadAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let alignment = layout.align().max(1);
        let cursor = NOTEPAD_HEAP_CURSOR.load(core::sync::atomic::Ordering::Acquire);
        let aligned = cursor.div_ceil(alignment) * alignment;
        let end = aligned.checked_add(layout.size()).unwrap_or(usize::MAX);
        if end > NOTEPAD_HEAP_SIZE {
            return core::ptr::null_mut();
        }
        NOTEPAD_HEAP_CURSOR.store(end, core::sync::atomic::Ordering::Release);
        (NOTEPAD_HEAP.0.get() as *mut u8).add(aligned)
    }

    unsafe fn dealloc(&self, _pointer: *mut u8, _layout: core::alloc::Layout) {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write as _;
    struct SerialWriter;

    impl core::fmt::Write for SerialWriter {
        fn write_str(&mut self, string: &str) -> core::fmt::Result {
            #[cfg(target_arch = "x86_64")]
            arch::debug_str(string);
            let _ = string;
            Ok(())
        }
    }

    let _ = write!(SerialWriter {}, "[NOTEPAD] panic: {}\r\n", info);
    arch::exit_process(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry() -> ! {
    notepad_main()
}

struct Notepad {
    framebuffer: arch::DesktopFramebufferInfo,
    frame: Surface,
    fonts: font_view::FontPack,
    text: TextBox,
    bounds: nt61_desktop::window::Rect,
}

impl Notepad {
    fn new() -> Option<Self> {
        let mut framebuffer = arch::DesktopFramebufferInfo::default();
        arch::desktop_info(&mut framebuffer);
        if framebuffer.width == 0 || framebuffer.height == 0 {
            return None;
        }
        let frame = Surface::new(framebuffer.width, framebuffer.height);
        let fonts = font_view::FontPack::new(16)?;
        let margin = 80_u32;
        let bounds = nt61_desktop::window::Rect::new(
            margin as i32,
            margin as i32,
            framebuffer.width.saturating_sub(margin * 2),
            framebuffer.height.saturating_sub(200),
        );
        let mut text = TextBox::new(1);
        text.focused = true;
        Some(Self {
            framebuffer,
            frame,
            fonts,
            text,
            bounds,
        })
    }

    fn draw(&mut self) {
        self.frame.fill_rect(
            0,
            0,
            self.framebuffer.width,
            self.framebuffer.height,
            0xFF12_406F,
        );
        let bounds = nt61_desktop::graphics::Rect::new(
            self.bounds.x,
            self.bounds.y,
            self.bounds.width,
            self.bounds.height,
        );
        aero::shadow(&mut self.frame, bounds, 6);
        aero::rounded_window(&mut self.frame, bounds, 5, 0xF2F4_F9FD);
        let caption =
            nt61_desktop::graphics::Rect::new(bounds.x, bounds.y, bounds.width, CAPTION_HEIGHT);
        let backdrop = self.frame.clone();
        aero::glass_bar(&mut self.frame, caption, &backdrop);
        self.frame.blend_rect(caption, 0x6030_50A0);
        self.fonts.draw_labelled_onto(
            &mut self.frame,
            "Untitled - Notepad",
            bounds.x + 12,
            bounds.y + 8,
        );
        let client = nt61_desktop::graphics::Rect::new(
            bounds.x + CLIENT_EDGE as i32,
            bounds.y + CAPTION_HEIGHT as i32 + 8,
            bounds.width.saturating_sub(CLIENT_EDGE * 2),
            bounds
                .height
                .saturating_sub(CAPTION_HEIGHT + CLIENT_EDGE * 2),
        );
        self.frame.blend_rounded_rect(client, 2, 0xF8F4_F9FD);
        self.draw_text(client);
        present_surface(&self.frame);
    }

    fn draw_text(&mut self, client: nt61_desktop::graphics::Rect) {
        let prefix = self
            .text
            .text
            .char_indices()
            .nth(self.text.cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.text.text.len());
        let before = String::from(&self.text.text[..prefix]);
        let after = String::from(&self.text.text[prefix..]);
        self.fonts.draw_onto(
            &mut self.frame,
            &before,
            client.x + 8,
            client.y + 8,
            font_view::Ink::Black,
        );
        let width = self.fonts.measure(&before);
        let cursor_x = client.x + 8 + width as i32;
        let cursor_y = client.y + 6;
        self.frame.blend_rect(
            nt61_desktop::graphics::Rect::new(cursor_x, cursor_y, 1, 18),
            0xFF10_1010,
        );
        self.fonts.draw_onto(
            &mut self.frame,
            &after,
            cursor_x + 1,
            client.y + 8,
            font_view::Ink::Black,
        );
    }

    fn route_key(&mut self, key: u8) -> bool {
        match key {
            0x08 => self.text.backspace(),
            0x13 => {
                self.text.insert('\r');
                self.text.insert('\n');
                true
            }
            _ => {
                let byte = key;
                if byte >= 0x20 && byte <= 0x7E {
                    self.text.insert(byte as char);
                    true
                } else {
                    false
                }
            }
        }
    }
}

fn notepad_main() -> ! {
    #[cfg(target_arch = "x86_64")]
    arch::debug_str("[NOTEPAD] enter\r\n");
    let Some(mut notepad) = Notepad::new() else {
        arch::exit_process(1);
    };
    notepad.draw();
    loop {
        let mut key = 0_u8;
        if unsafe { arch::poll_key(&mut key) } == 1 {
            if notepad.route_key(key) {
                notepad.draw();
            }
        } else {
            core::hint::spin_loop();
        }
    }
}

fn present_surface(surface: &Surface) {
    let mut pixels = alloc::vec::Vec::with_capacity(surface.pixels().len() * 4);
    for pixel in surface.pixels() {
        pixels.push((pixel >> 16) as u8);
        pixels.push((pixel >> 8) as u8);
        pixels.push(*pixel as u8);
        pixels.push((pixel >> 24) as u8);
    }
    unsafe {
        arch::draw_rgba(
            pixels.as_ptr(),
            0,
            0,
            surface.width(),
            surface.height(),
            surface.width() as usize * 4,
        );
    }
}

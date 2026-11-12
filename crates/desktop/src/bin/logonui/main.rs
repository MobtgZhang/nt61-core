#![no_std]
#![no_main]
#![deny(warnings)]

mod arch;
#[path = "../explorer/ico.rs"]
mod ico;

#[path = "../../font_view.rs"]
mod font_view;
#[path = "../explorer/png.rs"]
mod png;

use core::panic::PanicInfo;
use core::str;
use nt61_desktop::bmp;
use nt61_desktop::compositor::aero;
use nt61_desktop::controls::{ControlEvent, ControlKind, ControlTree, PointerAction};
use nt61_desktop::graphics::blit::blit_rgba;
use nt61_desktop::graphics::Surface;
use nt61_desktop::login::AuthenticationProvider;
use nt61_desktop::user_account_pictures::USER;
use png::Image;

extern crate alloc;

const CURSOR_CUR: &[u8] = include_bytes!("../../../assets/windows7/cursors/aero_arrow_l.cur");
const FLAG_ICO: &[u8] = include_bytes!("../../../assets/windows7/icons/shell32/ICON34_1.ico");
const EASE_ICO: &[u8] = include_bytes!("../../../assets/windows7/icons/shell32/ICON29_1.ico");
const POWER_ICO: &[u8] = include_bytes!("../../../assets/windows7/icons/shell32/ICON28_1.ico");
const WALLPAPER_PNG: &[u8] = include_bytes!("../../../assets/windows7/wallpapers/login-1024.png");

const MAX_PASSWORD: usize = 32;
const USER_NAME: &str = "User";
const CONTROL_PASSWORD: u32 = 1001;
const CONTROL_SUBMIT: u32 = 1002;
const CONTROL_EASE: u32 = 1003;
const CONTROL_POWER: u32 = 1004;

use nt61_desktop::user_allocator::StaticHeapAllocator;

#[global_allocator]
static LOGONUI_ALLOCATOR: StaticHeapAllocator<{ 64 * 1024 * 1024 }> = StaticHeapAllocator::new();

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write as _;
    struct SerialWriter;
    impl core::fmt::Write for SerialWriter {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            arch::debug_str(text);
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

struct Layout {
    avatar: u32,
    tile: u32,
    tile_y: u32,
    user_y: u32,
    padding: u32,
    password_width: u32,
    password_height: u32,
    password_y: u32,
    bottom_y: u32,
}

impl Layout {
    fn new(_width: u32, height: u32) -> Self {
        let avatar = (height * 118 / 696).max(80);
        let padding = (avatar / 13).max(5);
        let tile = avatar + padding * 2;
        let password_height = (height * 26 / 768).max(24);
        Self {
            avatar,
            tile,
            tile_y: height * 31 / 100,
            user_y: height * 55 / 100,
            padding,
            password_width: (height * 34 / 100).max(210),
            password_height,
            password_y: height * 61 / 100,
            bottom_y: height.saturating_sub(password_height + height / 28),
        }
    }
}

struct LogonAssets {
    fonts: font_view::FontPack,
    avatar: Image,
    cursor: Image,
    flag: Image,
    ease: Image,
    power: Image,
    wallpaper: Image,
}

struct Credentials {
    password: [u8; MAX_PASSWORD],
    length: usize,
}

fn decoded_wallpaper() -> Option<Image> {
    png::decode_png(WALLPAPER_PNG).ok()
}

impl Credentials {
    fn new() -> Self {
        Self {
            password: [0; MAX_PASSWORD],
            length: 0,
        }
    }

    fn append(&mut self, byte: u8) {
        if self.length < MAX_PASSWORD {
            self.password[self.length] = byte;
            self.length += 1;
        }
    }

    fn backspace(&mut self) {
        self.length = self.length.saturating_sub(1);
    }

    fn clear(&mut self) {
        self.length = 0;
    }

    fn as_str(&self) -> Option<&str> {
        str::from_utf8(&self.password[..self.length]).ok()
    }
}

fn logonui_main() -> ! {
    let mut framebuffer = arch::DesktopFramebufferInfo::default();
    arch::desktop_info(&mut framebuffer);
    if framebuffer.width == 0 || framebuffer.height == 0 {
        loop {
            core::hint::spin_loop();
        }
    }

    let assets = LogonAssets::load(framebuffer.width, framebuffer.height);
    let mut credentials = Credentials::new();
    let mut authentication = AuthenticationProvider::new();
    let layout = Layout::new(framebuffer.width, framebuffer.height);
    let mut controls = build_login_controls(framebuffer.width, framebuffer.height, &layout);
    let mut frame = Surface::new(framebuffer.width, framebuffer.height);
    let mut frame_rgba = alloc::vec![0u8; frame.pixels().len() * 4];
    if let Some(screen) = assets.as_ref() {
        screen.draw(&framebuffer, &mut frame, &credentials, false, &controls);
        draw_login_cursor(
            screen,
            &mut frame,
            (framebuffer.width / 2) as i32,
            (framebuffer.height / 2) as i32,
        );
        present_surface_with(&frame, &mut frame_rgba);
    } else {
        arch::debug_str("[LOGONUI] assets failed\r\n");
        draw_fallback_screen(&framebuffer);
    }
    arch::debug_str("[LOGONUI] assets loaded\r\n");

    let mut previous_buttons = 0u32;
    let mut cursor_x = (framebuffer.width / 2) as i32;
    let mut cursor_y = (framebuffer.height / 2) as i32;
    loop {
        let mut key = 0u8;
        let mut mouse = arch::DesktopMouseState::default();
        let mut failed = false;
        let mut redraw = false;
        let mouse_pending = unsafe { arch::poll_mouse(&mut mouse) != 0 };
        if mouse_pending {
            cursor_x = mouse.x.clamp(0, framebuffer.width.saturating_sub(1) as i32);
            cursor_y = mouse
                .y
                .clamp(0, framebuffer.height.saturating_sub(1) as i32);
            let action = if mouse.buttons & 1 == 0 && previous_buttons & 1 != 0 {
                Some(PointerAction::Up)
            } else if mouse.buttons & 1 != 0 && previous_buttons & 1 == 0 {
                Some(PointerAction::Down)
            } else {
                Some(PointerAction::Move)
            };
            if let Some(action) = action {
                if let Some(event) = controls.route_pointer(action, mouse.x, mouse.y) {
                    if let ControlEvent::Activated(_) = event {
                        let password = credentials.as_str().unwrap_or(" ");
                        if authentication
                            .authenticate(USER_NAME, password)
                            .map(|authentication| authentication.user == USER_NAME)
                            .unwrap_or(false)
                        {
                            arch::exit_process(0);
                        }
                        credentials.clear();
                        failed = true;
                    }
                }
            }
            redraw = true;
            previous_buttons = mouse.buttons;
        }
        unsafe {
            if arch::poll_key(&mut key) == 0 {
                key = arch::wait_key(&mut key) as u8;
            }
        }

        let key_redraw = match controls.route_key(key) {
            Some(ControlEvent::Submit) => {
                let password = credentials.as_str().unwrap_or(" ");
                if authentication
                    .authenticate(USER_NAME, password)
                    .map(|authentication| authentication.user == USER_NAME)
                    .unwrap_or(false)
                {
                    arch::exit_process(0);
                }
                credentials.clear();
                failed = true;
                true
            }
            Some(ControlEvent::TextChanged(CONTROL_PASSWORD)) => {
                match key {
                    8 | 127 => credentials.backspace(),
                    byte if (0x20..=0x7e).contains(&byte) => credentials.append(byte),
                    _ => {}
                }
                true
            }
            Some(ControlEvent::FocusChanged(_)) => true,
            _ => false,
        };

        if redraw || key_redraw {
            if let Some(screen) = assets.as_ref() {
                screen.draw(&framebuffer, &mut frame, &credentials, failed, &controls);
                draw_login_cursor(screen, &mut frame, cursor_x, cursor_y);
                present_surface_with(&frame, &mut frame_rgba);
            } else {
                draw_fallback_screen(&framebuffer);
            }
        }
    }
}

fn draw_login_cursor(screen: &LogonAssets, frame: &mut Surface, cursor_x: i32, cursor_y: i32) {
    blit_rgba(
        frame,
        &screen.cursor.pixels,
        screen.cursor.stride(),
        nt61_desktop::graphics::Rect::new(
            cursor_x,
            cursor_y,
            screen.cursor.width,
            screen.cursor.height,
        ),
    );
}

fn build_login_controls(width: u32, _height: u32, layout: &Layout) -> ControlTree {
    let mut tree = ControlTree::new();
    let field_x = width.saturating_sub(layout.password_width) / 2;
    let button_x = field_x + layout.password_width + layout.padding;
    tree.add(
        CONTROL_PASSWORD,
        ControlKind::Edit,
        nt61_desktop::graphics::Rect::new(
            field_x as i32,
            layout.password_y as i32,
            layout.password_width,
            layout.password_height,
        ),
    );
    tree.add(
        CONTROL_SUBMIT,
        ControlKind::Button,
        nt61_desktop::graphics::Rect::new(
            button_x as i32,
            layout.password_y as i32,
            layout.password_height,
            layout.password_height,
        ),
    );
    let ease_width = layout.password_height * 4 / 3;
    tree.add(
        CONTROL_EASE,
        ControlKind::Button,
        nt61_desktop::graphics::Rect::new(
            32,
            layout.bottom_y as i32,
            ease_width,
            layout.password_height,
        ),
    );
    let power_width = layout.password_height * 8 / 3;
    tree.add(
        CONTROL_POWER,
        ControlKind::Button,
        nt61_desktop::graphics::Rect::new(
            width.saturating_sub(power_width + 24) as i32,
            layout.bottom_y as i32,
            power_width,
            layout.password_height,
        ),
    );
    tree
}

impl LogonAssets {
    fn load(_width: u32, _height: u32) -> Option<Self> {
        let fonts = font_view::FontPack::new(16);
        let fonts = fonts?;
        arch::debug_str("[LOGONUI] asset fonts ok\r\n");
        let decoded_avatar = bmp::decode_bmp(USER.bytes).ok()?;
        let avatar = png::Image {
            width: decoded_avatar.width,
            height: decoded_avatar.height,
            pixels: decoded_avatar.pixels,
        };
        arch::debug_str("[LOGONUI] asset avatar ok\r\n");
        let cursor = ico::decode_ico_preferred(CURSOR_CUR, 32).ok();
        let cursor = cursor?;
        arch::debug_str("[LOGONUI] asset cursor ok\r\n");
        let flag = ico::decode_ico(FLAG_ICO).ok();
        let flag = flag?;
        arch::debug_str("[LOGONUI] asset flag ok\r\n");
        let ease = ico::decode_ico(EASE_ICO).ok();
        let ease = ease?;
        arch::debug_str("[LOGONUI] asset ease ok\r\n");
        let power = ico::decode_ico(POWER_ICO).ok();
        let power = power?;
        arch::debug_str("[LOGONUI] asset power ok\r\n");
        let wallpaper = decoded_wallpaper();
        let wallpaper = wallpaper?;
        arch::debug_str("[LOGONUI] asset wallpaper ok\r\n");
        Some(Self {
            fonts,
            avatar,
            cursor,
            flag,
            ease,
            power,
            wallpaper,
        })
    }

    fn draw(
        &self,
        framebuffer: &arch::DesktopFramebufferInfo,
        frame: &mut Surface,
        credentials: &Credentials,
        authentication_failed: bool,
        controls: &ControlTree,
    ) {
        let width = framebuffer.width;
        let height = framebuffer.height;
        let layout = Layout::new(width, height);

        let wallpaper = cover_image(&self.wallpaper, width, height);
        blit_rgba(
            frame,
            &wallpaper,
            width as usize * 4,
            nt61_desktop::graphics::Rect::new(0, 0, width, height),
        );

        let tile_x = width.saturating_sub(layout.tile) / 2;
        aero::shadow(
            frame,
            nt61_desktop::graphics::Rect::new(
                tile_x.saturating_sub(2) as i32,
                layout.tile_y.saturating_sub(2) as i32,
                layout.tile + 4,
                layout.tile + 4,
            ),
            4,
        );
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                tile_x as i32,
                layout.tile_y as i32,
                layout.tile,
                layout.tile,
            ),
            layout.avatar / 4,
            0xE6EAF0FF,
        );
        let scaled_avatar = scale_rgba_bilinear(&self.avatar, layout.avatar, layout.avatar);
        blit_rgba(
            frame,
            &scaled_avatar,
            layout.avatar as usize * 4,
            nt61_desktop::graphics::Rect::new(
                (tile_x + layout.padding) as i32,
                (layout.tile_y + layout.padding) as i32,
                layout.avatar,
                layout.avatar,
            ),
        );

        self.draw_centered_labelled(frame, USER_NAME, width, layout.user_y);

        let password_layout = controls
            .control(CONTROL_PASSWORD)
            .copied()
            .or_else(|| controls.controls().first().copied())
            .unwrap();
        let submit_layout = controls
            .control(CONTROL_SUBMIT)
            .copied()
            .unwrap_or(password_layout);
        let field_x = password_layout.rect.x as u32;
        let field_y = password_layout.rect.y as u32;
        let password_width = password_layout.rect.width;
        let password_height = password_layout.rect.height;
        let field_bounds = nt61_desktop::graphics::Rect::new(
            field_x as i32,
            field_y as i32,
            password_width,
            password_height,
        );
        aero::blend_rounded_rect(frame, field_bounds, 3, 0xF8FFFFFF);
        aero::blend_rect(frame, field_bounds, 0xFF6C7A86);
        fill_gradient(
            frame,
            nt61_desktop::graphics::Rect::new(
                (field_x + 2) as i32,
                (field_y + 2) as i32,
                password_width.saturating_sub(4),
                password_height.saturating_sub(4),
            ),
            0xFFFCFDFF,
            0xFFEDF2F7,
        );

        let button = submit_layout.rect.width.min(submit_layout.rect.height);
        let button_x = submit_layout.rect.x as u32;
        let button_bounds =
            nt61_desktop::graphics::Rect::new(button_x as i32, field_y as i32, button, button);
        aero::blend_rounded_rect(frame, button_bounds, button / 2, 0xF04B7AAB);
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                (button_x + 1) as i32,
                (field_y + 1) as i32,
                button - 2,
                button - 2,
            ),
            button / 2 - 1,
            0xF06EB7EA,
        );
        aero::arrow_right(
            frame,
            nt61_desktop::graphics::Rect::new(
                (button_x + button / 4) as i32,
                (field_y + button / 4) as i32,
                button / 2,
                button / 2,
            ),
            0xFFFFFFFF,
        );

        let placeholder = "Password";
        let empty = credentials.length == 0;
        let star_width = self.fonts.measure("*");
        if empty {
            self.fonts.draw_onto(
                frame,
                placeholder,
                (field_x + 8) as i32,
                (field_y + password_height / 4) as i32,
                font_view::Ink::Black,
            );
        } else {
            for index in 0..credentials.length as u32 {
                self.fonts.draw_onto(
                    frame,
                    "*",
                    (field_x + 8 + index * star_width) as i32,
                    (field_y + password_height / 4) as i32,
                    font_view::Ink::Black,
                );
            }
        }
        let caret_x = field_x + 8 + star_width * credentials.length as u32;
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                caret_x as i32,
                (field_y + 4) as i32,
                2,
                password_height.saturating_sub(8),
            ),
            0xC0202020,
        );

        if authentication_failed {
            self.draw_centered(
                frame,
                "The user name or password is incorrect.",
                width,
                (field_y + password_height + layout.padding + 2) as i32,
                font_view::Ink::White,
            );
        }

        let ease_layout = controls
            .control(CONTROL_EASE)
            .copied()
            .unwrap_or(password_layout);
        let power_layout = controls
            .control(CONTROL_POWER)
            .copied()
            .unwrap_or(password_layout);
        self.draw_image(
            frame,
            &self.ease,
            ease_layout.rect.x as u32,
            ease_layout.rect.y as u32,
            ease_layout.rect.width,
        );
        self.draw_image(
            frame,
            &self.power,
            power_layout.rect.x as u32,
            power_layout.rect.y as u32,
            power_layout.rect.width,
        );

        let flag_size = layout.password_height;
        let flag = scale_rgba_bilinear(&self.flag, flag_size, flag_size);
        let brand_width = self.fonts.measure("Windows");
        let suffix_width = self.fonts.measure(" 7 Ultimate");
        let brand_x = width.saturating_sub(brand_width + suffix_width + flag_size + 4) / 2;
        blit_rgba(
            frame,
            &flag,
            flag_size as usize * 4,
            nt61_desktop::graphics::Rect::new(
                brand_x as i32,
                layout.bottom_y as i32,
                flag_size,
                flag_size,
            ),
        );
        self.fonts.draw_onto(
            frame,
            "Windows",
            (brand_x + flag_size + 4) as i32,
            (layout.bottom_y + flag_size / 4) as i32,
            font_view::Ink::White,
        );
        self.fonts.draw_onto(
            frame,
            " 7 Ultimate",
            (brand_x + flag_size + 4 + brand_width) as i32,
            (layout.bottom_y + flag_size / 3) as i32,
            font_view::Ink::White,
        );

        let cursor_x = width.saturating_sub(self.cursor.width as u32 + 8);
        let cursor_y = height.saturating_sub(self.cursor.height as u32 + 8);
        blit_rgba(
            frame,
            &self.cursor.pixels,
            self.cursor.stride(),
            nt61_desktop::graphics::Rect::new(
                cursor_x as i32,
                cursor_y as i32,
                self.cursor.width as u32,
                self.cursor.height as u32,
            ),
        );
    }

    fn draw_image(&self, frame: &mut Surface, image: &Image, x: u32, y: u32, target_width: u32) {
        let target_height = target_width * image.height as u32 / image.width as u32;
        let scaled = scale_rgba_bilinear(image, target_width, target_height);
        blit_rgba(
            frame,
            &scaled,
            target_width as usize * 4,
            nt61_desktop::graphics::Rect::new(x as i32, y as i32, target_width, target_height),
        );
    }

    fn draw_centered(
        &self,
        frame: &mut Surface,
        text: &str,
        width: u32,
        y: i32,
        ink: font_view::Ink,
    ) {
        let text_width = self.fonts.measure(text);
        self.fonts.draw_onto(
            frame,
            text,
            (width.saturating_sub(text_width) / 2) as i32,
            y as i32,
            ink,
        );
    }

    fn draw_centered_labelled(&self, frame: &mut Surface, text: &str, width: u32, y: u32) {
        let text_width = self.fonts.measure(text);
        let x = width.saturating_sub(text_width) / 2;
        self.fonts
            .draw_onto(frame, text, x as i32, y as i32 + 1, font_view::Ink::Black);
        self.fonts
            .draw_onto(frame, text, x as i32, y as i32, font_view::Ink::White);
    }
}

fn scale_rgba_bilinear(
    source: &Image,
    target_width: u32,
    target_height: u32,
) -> alloc::vec::Vec<u8> {
    if source.width == 0 || source.height == 0 || target_width == 0 || target_height == 0 {
        return alloc::vec![0u8; (target_width * target_height * 4) as usize];
    }

    let x_step = ((source.width as u64) << 8) / target_width as u64;
    let y_step = ((source.height as u64) << 8) / target_height as u64;
    sample_bilinear(
        source,
        0,
        0,
        (source.width as u64) << 8,
        (source.height as u64) << 8,
        x_step,
        y_step,
        target_width,
        target_height,
    )
}

fn cover_image(source: &Image, target_width: u32, target_height: u32) -> alloc::vec::Vec<u8> {
    if source.width == 0 || source.height == 0 || target_width == 0 || target_height == 0 {
        return alloc::vec![0u8; (target_width * target_height * 4) as usize];
    }

    let target_aspect = (target_width as u64) * 256 / target_height as u64;
    let source_aspect = (source.width as u64) * 256 / source.height as u64;
    let (sample_width, sample_height) = if source_aspect > target_aspect {
        (
            (source.height as u64) * target_width as u64 * 256 / target_height as u64,
            (source.height as u64) << 8,
        )
    } else {
        (
            (source.width as u64) << 8,
            (source.width as u64) * target_height as u64 * 256 / target_width as u64,
        )
    };
    let sample_x = (((source.width as u64) << 8).saturating_sub(sample_width)) / 2;
    let sample_y = (((source.height as u64) << 8).saturating_sub(sample_height)) / 2;
    let x_step = sample_width / target_width as u64;
    let y_step = sample_height / target_height as u64;
    sample_bilinear(
        source,
        sample_x,
        sample_y,
        sample_width,
        sample_height,
        x_step,
        y_step,
        target_width,
        target_height,
    )
}

fn sample_bilinear(
    source: &Image,
    sample_x: u64,
    sample_y: u64,
    _sample_width: u64,
    _sample_height: u64,
    x_step: u64,
    y_step: u64,
    target_width: u32,
    target_height: u32,
) -> alloc::vec::Vec<u8> {
    let mut output = alloc::vec![0u8; (target_width * target_height * 4) as usize];
    let source_width = source.width as usize;
    let source_height = source.height as usize;
    let x_limit = ((source_width - 1) as u64) << 8;
    let y_limit = ((source_height - 1) as u64) << 8;

    for row in 0..target_height as usize {
        let y_fixed = (sample_y + (row as u64 * y_step)).min(y_limit);
        let y0 = (y_fixed >> 8) as usize;
        let y1 = (y0 + 1).min(source_height - 1);
        let y_fraction = (y_fixed & 0xFF) as u16;
        for column in 0..target_width as usize {
            let x_fixed = (sample_x + (column as u64 * x_step)).min(x_limit);
            let x0 = (x_fixed >> 8) as usize;
            let x1 = (x0 + 1).min(source_width - 1);
            let x_fraction = (x_fixed & 0xFF) as u16;

            let top_left = source_pixel(&source.pixels, source_width, x0, y0);
            let top_right = source_pixel(&source.pixels, source_width, x1, y0);
            let bottom_left = source_pixel(&source.pixels, source_width, x0, y1);
            let bottom_right = source_pixel(&source.pixels, source_width, x1, y1);
            let blended = blend_pixel(
                top_left,
                top_right,
                bottom_left,
                bottom_right,
                x_fraction,
                y_fraction,
            );

            let offset = (row * target_width as usize + column) * 4;
            output[offset..offset + 4].copy_from_slice(&blended);
        }
    }
    output
}

fn source_pixel(pixels: &[u8], source_width: usize, x: usize, y: usize) -> [u8; 4] {
    let offset = (y * source_width + x) * 4;
    let mut pixel = [0u8; 4];
    pixel.copy_from_slice(&pixels[offset..offset + 4]);
    pixel
}

fn blend_pixel(
    top_left: [u8; 4],
    top_right: [u8; 4],
    bottom_left: [u8; 4],
    bottom_right: [u8; 4],
    x_fraction: u16,
    y_fraction: u16,
) -> [u8; 4] {
    let mut blended = [0u8; 4];
    for channel in 0..4 {
        let top = lerp(top_left[channel], top_right[channel], x_fraction);
        let bottom = lerp(bottom_left[channel], bottom_right[channel], x_fraction);
        blended[channel] = lerp(top, bottom, y_fraction);
    }
    blended
}

fn lerp(left: u8, right: u8, scale: u16) -> u8 {
    ((left as u32 * (255 - scale as u32) + right as u32 * scale as u32) / 255) as u8
}

fn draw_fallback_screen(framebuffer: &arch::DesktopFramebufferInfo) {
    let mut surface = Surface::new(framebuffer.width, framebuffer.height);
    surface.vertical_gradient(0x396996, 0x08243f);
    let mut rgba = alloc::vec![0u8; surface.pixels().len() * 4];
    present_surface_with(&surface, &mut rgba);
}

fn fill_gradient(
    surface: &mut Surface,
    bounds: nt61_desktop::graphics::Rect,
    top: u32,
    bottom: u32,
) {
    for row in 0..bounds.height {
        let scale = if bounds.height <= 1 {
            0
        } else {
            row * 255 / bounds.height.saturating_sub(1)
        };
        let color = nt61_desktop::graphics::lerp_color(top, bottom, scale);
        surface.fill_rect(bounds.x, bounds.y + row as i32, bounds.width, 1, color);
    }
}

fn present_surface_with(surface: &Surface, rgba: &mut [u8]) {
    let pixels = surface.pixels();
    if rgba.len() < pixels.len() * 4 {
        return;
    }
    for (target, color) in rgba.chunks_exact_mut(4).zip(pixels) {
        target.copy_from_slice(&[
            ((color >> 16) & 0xFF) as u8,
            ((color >> 8) & 0xFF) as u8,
            (color & 0xFF) as u8,
            ((color >> 24) & 0xFF) as u8,
        ]);
    }
    unsafe {
        arch::draw_rgba(
            rgba.as_ptr() as *const u8,
            0,
            0,
            surface.width(),
            surface.height(),
            surface.width() as usize * 4,
        );
    }
}

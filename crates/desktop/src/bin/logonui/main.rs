#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![deny(warnings)]

mod arch;
#[path = "../explorer/ico.rs"]
mod ico;

#[path = "../../font_view.rs"]
mod font_view;
#[path = "../explorer/png.rs"]
mod png;

#[cfg(not(test))]
use core::panic::PanicInfo;
use nt61_desktop::bmp;
use nt61_desktop::compositor::aero;
use nt61_desktop::controls::{ControlEvent, ControlKind, ControlTree, PointerAction};
use nt61_desktop::graphics::blit::blit_rgba;
use nt61_desktop::graphics::Surface;

use nt61_desktop::user_account_pictures::USER;
use png::Image;

extern crate alloc;

const CURSOR_CUR: &[u8] = include_bytes!("../../../assets/windows7/cursors/aero_arrow_l.cur");
const BRAND_BMP: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/brand.bmp");
const TILE_BMP: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/user-tile.bmp");
const EDIT_BMP: [&[u8];3] = [
    include_bytes!("../../../assets/windows7/logon-buttons/edit-normal.bmp"),
    include_bytes!("../../../assets/windows7/logon-buttons/edit-hover.bmp"),
    include_bytes!("../../../assets/windows7/logon-buttons/edit-focus.bmp"),
];
const SUBMIT_BMP: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/submit.bmp");
const SUBMIT_HOVER_BMP: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/submit-hover.bmp");
const SUBMIT_PRESSED_BMP: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/submit-pressed.bmp");
const EASE_BMP: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/ease-of-access.bmp");
const POWER_BMP: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/power.bmp");
const WALLPAPER_PNG: &[u8] = include_bytes!("../../../assets/windows7/wallpapers/login-1024.png");

const USER_NAME: &str = "Administrator";
const CONTROL_PASSWORD: u32 = 1001;
const CONTROL_SUBMIT: u32 = 1002;
const CONTROL_EASE: u32 = 1003;
const CONTROL_POWER: u32 = 1004;

use nt61_desktop::user_allocator::StaticHeapAllocator;

#[global_allocator]
static LOGONUI_ALLOCATOR: StaticHeapAllocator<{ 64 * 1024 * 1024 }> = StaticHeapAllocator::new();

#[cfg(not(test))]
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
        // AuthUX's selected credential uses a 190px frame with a 126px
        // picture aperture at 96 DPI, rather than the 80px account-list tile.
        let tile = 190.min(height / 3);
        let padding = 4;
        let password_height = (height * 30 / 768).max(26);
        Self {
            tile,
            tile_y: height * 31 / 100,
            user_y: height * 55 / 100,
            padding,
            password_width: (height * 225 / 768).max(200),
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
    brand: Image,
    tile: Image,
    edit: [Image;3],
    submit: [Image;3],
    ease: Image,
    power: Image,
    wallpaper: Surface,
}

use nt61_desktop::login::PasswordEntry as Credentials;

fn decoded_wallpaper() -> Option<Image> {
    png::decode_png(WALLPAPER_PNG).ok()
}

fn decoded_button_bitmap(bytes: &[u8]) -> Option<Image> {
    let decoded = bmp::decode_bmp(bytes).ok()?;
    Some(Image {
        width: decoded.width,
        height: decoded.height,
        pixels: decoded.pixels,
    })
}


fn logonui_main() -> ! {
    arch::debug_str("[LOGONUI] startup\r\n");
    let mut framebuffer = arch::DesktopFramebufferInfo::default();
    arch::desktop_info(&mut framebuffer);
    if framebuffer.width == 0 || framebuffer.height == 0 {
        loop {
            core::hint::spin_loop();
        }
    }

    arch::debug_str("[LOGONUI] framebuffer ready\r\n");
    let assets = LogonAssets::load(framebuffer.width, framebuffer.height);
    let mut credentials = Credentials::new();
    let layout = Layout::new(framebuffer.width, framebuffer.height);
    let mut controls = build_login_controls(framebuffer.width, framebuffer.height, &layout);
    let mut frame = Surface::new(framebuffer.width, framebuffer.height);
    let mut frame_rgba = alloc::vec![0u8; frame.pixels().len() * 4];
    let mut cursor_frame = frame.clone();
    let mut authentication_failed = false;
    let mut notice = None;
    if let Some(screen) = assets.as_ref() {
        screen.draw(&framebuffer, &mut frame, &credentials, false, &controls);
        cursor_frame.copy_from(&frame);
        draw_login_cursor(
            screen,
            &mut cursor_frame,
            (framebuffer.width / 2) as i32,
            (framebuffer.height / 2) as i32,
        );
        present_surface_with(&cursor_frame, &mut frame_rgba);
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
        let mut redraw = false;
        let mut submit = false;
        let old_cursor = (cursor_x, cursor_y);
        let key_pending = unsafe { arch::poll_key(&mut key) == 1 };
        let mouse_pending = unsafe { arch::poll_mouse(&mut mouse) != 0 };
        if !key_pending && !mouse_pending {
            core::hint::spin_loop();
            continue;
        }
        if mouse_pending {
            let old_controls = (controls.hot(), controls.focus(), controls.pressed());
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
                if let Some(event) = controls.route_pointer(action, cursor_x, cursor_y) {
                    match event {
                        ControlEvent::Activated(CONTROL_SUBMIT) => submit = true,
                        ControlEvent::Activated(CONTROL_EASE) => {
                            notice =
                                Some("Accessibility services are not available in this session.");
                            redraw = true;
                        }
                        ControlEvent::Activated(CONTROL_POWER) => {
                            notice = Some("Session power services are not available.");
                            redraw = true;
                        }
                        _ => {}
                    }
                }
            }
            redraw |= old_controls != (controls.hot(), controls.focus(), controls.pressed());
            previous_buttons = mouse.buttons;
        }
        let mut key_redraw = false;
        if key_pending {
            key_redraw = match controls.route_key(key) {
                Some(ControlEvent::Submit | ControlEvent::Activated(CONTROL_SUBMIT)) => {
                    submit = true;
                    true
                }
                Some(ControlEvent::Activated(CONTROL_EASE)) => {
                    notice = Some("Accessibility services are not available in this session.");
                    true
                }
                Some(ControlEvent::Activated(CONTROL_POWER)) => {
                    notice = Some("Session power services are not available.");
                    true
                }
                Some(ControlEvent::Cancel) => {
                    notice = None;
                    true
                }
                Some(ControlEvent::TextChanged(CONTROL_PASSWORD)) => {
                    authentication_failed = false;
                    notice = None;
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
        }

        if submit {
            let password = credentials.as_str().unwrap_or(" ");
            let authenticated = nt61_desktop::platform::services::logon(USER_NAME, password).is_ok();
            credentials.clear();
            if authenticated { arch::exit_process(0); }
            authentication_failed = true;
            notice = None;
            redraw = true;
        }

        if redraw || key_redraw {
            if let Some(screen) = assets.as_ref() {
                screen.draw(
                    &framebuffer,
                    &mut frame,
                    &credentials,
                    authentication_failed,
                    &controls,
                );
                if let Some(message) = notice {
                    screen.draw_centered(
                        &mut frame,
                        message,
                        framebuffer.width,
                        (layout.password_y + layout.password_height + layout.padding + 28) as i32,
                        font_view::Ink::White,
                    );
                }
                cursor_frame.copy_from(&frame);
                draw_login_cursor(screen, &mut cursor_frame, cursor_x, cursor_y);
                present_surface_with(&cursor_frame, &mut frame_rgba);
            } else {
                draw_fallback_screen(&framebuffer);
            }
        } else if mouse_pending && old_cursor != (cursor_x, cursor_y) {
            if let Some(screen) = assets.as_ref() {
                let old_bounds = nt61_desktop::graphics::Rect::new(
                    old_cursor.0,
                    old_cursor.1,
                    screen.cursor.width,
                    screen.cursor.height,
                );
                cursor_frame.copy_region(&frame, old_bounds, old_cursor.0, old_cursor.1);
                draw_login_cursor(screen, &mut cursor_frame, cursor_x, cursor_y);
                // Convert and present just the old and new cursor rectangles.
                present_surface_region(&cursor_frame, &mut frame_rgba, old_bounds);
                present_surface_region(
                    &cursor_frame,
                    &mut frame_rgba,
                    nt61_desktop::graphics::Rect::new(
                        cursor_x,
                        cursor_y,
                        screen.cursor.width,
                        screen.cursor.height,
                    ),
                );
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
    let field_x = width.saturating_sub(layout.password_width + layout.padding + layout.password_height) / 2;
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
    let ease_width = layout.password_height;
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
    let power_width = layout.password_height;
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
    fn load(width: u32, height: u32) -> Option<Self> {
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
        let brand = decoded_button_bitmap(BRAND_BMP)?;
        let tile = decoded_button_bitmap(TILE_BMP)?;
        let edit = [decoded_button_bitmap(EDIT_BMP[0])?,decoded_button_bitmap(EDIT_BMP[1])?,decoded_button_bitmap(EDIT_BMP[2])?];
        let submit = [decoded_button_bitmap(SUBMIT_BMP)?,decoded_button_bitmap(SUBMIT_HOVER_BMP)?,decoded_button_bitmap(SUBMIT_PRESSED_BMP)?];
        arch::debug_str("[LOGONUI] asset flag ok\r\n");
        let ease = decoded_button_bitmap(EASE_BMP);
        let ease = ease?;
        arch::debug_str("[LOGONUI] asset ease ok\r\n");
        let power = decoded_button_bitmap(POWER_BMP);
        let power = power?;
        arch::debug_str("[LOGONUI] asset power ok\r\n");
        let wallpaper = decoded_wallpaper();
        let wallpaper = wallpaper?;
        let pixels = cover_image(&wallpaper, width, height);
        let mut background = Surface::try_new(width, height)?;
        blit_rgba(
            &mut background,
            &pixels,
            width as usize * 4,
            nt61_desktop::graphics::Rect::new(0, 0, width, height),
        );
        arch::debug_str("[LOGONUI] asset wallpaper ok\r\n");
        Some(Self {
            fonts,
            avatar,
            cursor,
            brand,
            tile,
            edit,
            submit,
            ease,
            power,
            wallpaper: background,
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

        frame.copy_from(&self.wallpaper);

        let tile_x = width.saturating_sub(layout.tile) / 2;
        // AuthUX's tile has a glass frame and a transparent picture aperture.
        // Bitmap 12223's picture aperture occupies the central 126/190 pixels.
        let picture = layout.tile * 126 / 190;
        let inset = (layout.tile - picture) / 2;
        self.draw_image(frame,&self.avatar,tile_x+inset,layout.tile_y+inset,picture);
        self.draw_image(frame,&self.tile,tile_x,layout.tile_y,layout.tile);

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
        let edit_state=if controls.focus()==CONTROL_PASSWORD {2}
            else if controls.hot()==CONTROL_PASSWORD {1}else{0};
        let image=&self.edit[edit_state];
        nt61_desktop::graphics::blit::blit_nine_slice(frame,&image.pixels,
            (image.width,image.height),field_bounds,4);

        let button = submit_layout.rect.width.min(submit_layout.rect.height);
        let button_x = submit_layout.rect.x as u32;
        let button_state=if controls.pressed()==CONTROL_SUBMIT {2}else if controls.hot()==CONTROL_SUBMIT || controls.focus()==CONTROL_SUBMIT {1}else{0};
        self.draw_image(frame,&self.submit[button_state],button_x,field_y,button);

        let placeholder = "Password";
        let empty = credentials.is_empty();
        let star_width = self.fonts.measure("*");
        let visible = password_width.saturating_sub(20) / star_width.max(1);
        let visible_length = (credentials.len() as u32).min(visible);
        if empty {
            self.fonts.draw_onto(
                frame,
                placeholder,
                (field_x + 8) as i32,
                (field_y + password_height / 4) as i32,
                font_view::Ink::Black,
            );
        } else {
            for index in 0..visible_length {
                self.fonts.draw_onto(
                    frame,
                    "*",
                    (field_x + 8 + index * star_width) as i32,
                    (field_y + password_height / 4) as i32,
                    font_view::Ink::Black,
                );
            }
        }
        let caret_x = field_x + 8 + star_width * visible_length;
        if controls.focus()==CONTROL_PASSWORD {aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                caret_x as i32,
                (field_y + 4) as i32,
                2,
                password_height.saturating_sub(8),
            ),
            0xC0202020,
        );}

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
        for (id, bounds) in [
            (CONTROL_EASE, ease_layout.rect),
            (CONTROL_POWER, power_layout.rect),
        ] {
            if controls.hot() == id || controls.focus() == id {
                aero::blend_rounded_rect(
                    frame,
                    bounds,
                    4,
                    if controls.pressed() == id {
                        0x705994BE
                    } else {
                        0x60B7E4FF
                    },
                );
            }
        }
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

        let brand_width=(350*height/768).clamp(280,438).min(width);
        self.draw_image(frame,&self.brand,width.saturating_sub(brand_width)/2,layout.bottom_y,brand_width);
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

fn present_surface_with(surface: &Surface, rgba: &mut [u8]) {
    if let Some(damage) = nt61_desktop::graphics::present::update_rgba(surface, rgba) {
        present_surface_region(surface, rgba, damage);
    }
}

fn present_surface_region(
    surface: &Surface,
    rgba: &mut [u8],
    bounds: nt61_desktop::graphics::Rect,
) {
    let stride = surface.width() as usize * 4;
    if rgba.len() != surface.pixels().len() * 4 {
        return;
    }
    let Some(clip) = nt61_desktop::graphics::Rect::new(0, 0, surface.width(), surface.height())
        .intersection(&bounds)
    else {
        return;
    };
    for y in clip.y as usize..clip.y as usize + clip.height as usize {
        for x in clip.x as usize..clip.x as usize + clip.width as usize {
            let index = y * surface.width() as usize + x;
            let color = surface.pixels()[index];
            rgba[index * 4..index * 4 + 4].copy_from_slice(&[
                (color >> 16) as u8,
                (color >> 8) as u8,
                color as u8,
                (color >> 24) as u8,
            ]);
        }
    }
    // SAFETY: the clipped rectangle and every row are inside the staging buffer.
    unsafe {
        arch::draw_rgba(
            rgba.as_ptr()
                .add(clip.y as usize * stride + clip.x as usize * 4),
            clip.x as u32,
            clip.y as u32,
            clip.width,
            clip.height,
            stride,
        );
    }
}

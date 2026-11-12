#![no_std]
#![no_main]
#![deny(warnings)]

mod arch;
#[path = "../../font_view.rs"]
mod font_view;
mod ico;
mod png;
mod resources_data;

const CURSOR_CUR: &[u8] = include_bytes!("../../../assets/windows7/cursors/aero_arrow_l.cur");
const WALLPAPER_PNG: &[u8] =
    include_bytes!("../../../assets/windows7/wallpapers/Desktop/Windows/img0-1024.png");
const START_BUTTON_NORMAL_PNG: &[u8] =
    include_bytes!("../../../assets/windows7/start_button_normal.png");
const START_BUTTON_HOVER_PNG: &[u8] =
    include_bytes!("../../../assets/windows7/start_button_hover.png");
const START_BUTTON_PRESSED_PNG: &[u8] =
    include_bytes!("../../../assets/windows7/start_button_pressed.png");

use core::panic::PanicInfo;
use nt61_desktop::bmp;
use nt61_desktop::compositor::aero;
use nt61_desktop::controls::PointerAction;
use nt61_desktop::graphics::blit::blit_rgba;
use nt61_desktop::shell::{
    notification_area_left, taskbar_program_x, ShellModel, CONTROL_DESKTOP_BASE, CONTROL_START,
    CONTROL_TASKBAR_BASE, PROGRAM_BUTTON_WIDTH, START_BUTTON_WIDTH, START_MENU_HEIGHT,
    START_MENU_WIDTH, TASKBAR_HEIGHT,
};
use nt61_desktop::theme::Theme;
use nt61_desktop::user_account_pictures::USER;

extern crate alloc;

use nt61_desktop::user_allocator::StaticHeapAllocator;

#[global_allocator]
static EXPLORER_ALLOCATOR: StaticHeapAllocator<{ 64 * 1024 * 1024 }> = StaticHeapAllocator::new();

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

const ICON_CELL: u32 = 96;
const DESKTOP_MARGIN: u32 = 12;
const WALLPAPER_TOP: u32 = 0x2f5f8f;
const WALLPAPER_BOTTOM: u32 = 0x12406f;

struct Shell {
    computer: png::Image,
    network: png::Image,
    recycle_bin: png::Image,
    control_panel: png::Image,
    avatar: png::Image,
    cursor: png::Image,
    start_button_normal: png::Image,
    start_button_hover: png::Image,
    start_button_pressed: png::Image,
    wallpaper_asset: png::Image,
    fonts: font_view::FontPack,
    model: ShellModel,
    wallpaper: nt61_desktop::graphics::Surface,
    glass: nt61_desktop::graphics::Surface,
}

fn desktop_main() -> ! {
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

    let mut shell = match Shell::load() {
        Some(shell) => shell,
        None => {
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
                    match got {
                        byte if byte == b'\r' as u64 || byte == b'\n' as u64 => {
                            arch::exit_process(0)
                        }
                        _ => {}
                    }
                }
            }
        }
    };

    let mut frame = nt61_desktop::graphics::Surface::new(framebuffer.width, framebuffer.height);
    shell.prepare_aero(framebuffer.width, framebuffer.height);
    let mut mouse = arch::DesktopMouseState::default();
    let mut previous_buttons = 0u32;
    let mut cursor_x = (framebuffer.width / 2) as i32;
    let mut cursor_y = (framebuffer.height / 2) as i32;
    let mut frame_rgba = alloc::vec![0u8; frame.pixels().len() * 4];
    shell.draw_desktop(
        &framebuffer,
        &mut frame,
        cursor_x,
        cursor_y,
        &mut frame_rgba,
    );
    shell
        .model
        .refresh_controls(framebuffer.width, framebuffer.height);
    loop {
        let mut redraw = false;
        let mut key = 0u8;
        let key_pending = unsafe { arch::poll_key(&mut key) == 1 };
        let mut mouse_pending = false;
        while unsafe { arch::poll_mouse(&mut mouse) != 0 } {
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
                if action == PointerAction::Down {
                    if let Some(window_id) = shell.model.window_hit_test(cursor_x, cursor_y) {
                        if !shell.model.handle_window_control_click(
                            window_id,
                            cursor_x,
                            cursor_y,
                            framebuffer.width,
                            framebuffer.height.saturating_sub(TASKBAR_HEIGHT),
                        ) {
                            shell.model.select_window(window_id);
                            shell.model.open_file_at(window_id, cursor_y);
                            if shell.model.begin_window_drag(window_id, cursor_x, cursor_y) {
                                shell.model.select_window(window_id);
                            }
                        }
                    }
                }
                if action == PointerAction::Up {
                    shell.model.release_window_drag();
                }
                if action == PointerAction::Move && shell.model.drag_window.is_some() {
                    shell.model.drag_window_to(cursor_x, cursor_y);
                }
                let changed = match shell
                    .model
                    .controls
                    .route_pointer(action, cursor_x, cursor_y)
                {
                    Some(event) => shell.model.handle_control_event(event),
                    None => false,
                };
                if changed {
                    shell
                        .model
                        .refresh_controls(framebuffer.width, framebuffer.height);
                }
            }
            previous_buttons = mouse.buttons;
            mouse_pending = true;
        }
        if mouse_pending {
            redraw = true;
        }
        if key_pending {
            let changed = if shell.model.start_menu.open {
                if let Some(event) = shell.model.controls.route_key(key) {
                    shell.model.handle_control_event(event)
                } else {
                    shell.model.handle_start_menu_key(key)
                }
            } else {
                shell.model.handle_window_key(key)
            };
            if changed {
                shell
                    .model
                    .refresh_controls(framebuffer.width, framebuffer.height);
            }
            redraw = true;
        }
        if redraw {
            shell.draw_desktop(
                &framebuffer,
                &mut frame,
                cursor_x,
                cursor_y,
                &mut frame_rgba,
            );
        }
        if !key_pending && !mouse_pending {
            core::hint::spin_loop();
        }
    }
}

fn decoded_wallpaper() -> Option<png::Image> {
    png::decode_png(WALLPAPER_PNG).ok()
}

fn scaled_icon(bytes: &[u8]) -> Option<png::Image> {
    let decoded = ico::decode_ico(bytes).ok()?;
    Some(png::Image {
        width: 56,
        height: 56,
        pixels: scale_rgba(&decoded, 56, 56),
    })
}

impl Shell {
    fn load() -> Option<Self> {
        let mut model = ShellModel::new(Theme::aero());
        let root = model.files.root_id();
        model.files.mkdir(root, "Windows");
        model.files.mkdir(root, "Users");
        model.files.mkdir(root, "Program Files");
        model.files.touch(root, "readme.txt", b"NT61 desktop");
        model.desktop.add(
            "Computer",
            nt61_desktop::resources::ImageResource::Computer,
            12,
            12,
        );
        model.desktop.add(
            "Network",
            nt61_desktop::resources::ImageResource::Network,
            12,
            108,
        );
        model.desktop.add(
            "Recycle Bin",
            nt61_desktop::resources::ImageResource::RecycleBin,
            12,
            204,
        );
        model.desktop.add(
            "Control Panel",
            nt61_desktop::resources::ImageResource::ControlPanel,
            12,
            300,
        );
        Some(Self {
            computer: scaled_icon(resources_data::COMPUTER)?,
            network: scaled_icon(resources_data::NETWORK)?,
            recycle_bin: scaled_icon(resources_data::RECYCLE_BIN)?,
            control_panel: scaled_icon(resources_data::CONTROL_PANEL)?,
            avatar: {
                let decoded_avatar = bmp::decode_bmp(USER.bytes).ok()?;
                png::Image {
                    width: decoded_avatar.width,
                    height: decoded_avatar.height,
                    pixels: decoded_avatar.pixels,
                }
            },
            cursor: ico::decode_ico_preferred(CURSOR_CUR, 32).ok()?,
            start_button_normal: png::decode_png(START_BUTTON_NORMAL_PNG).ok()?,
            start_button_hover: png::decode_png(START_BUTTON_HOVER_PNG).ok()?,
            start_button_pressed: png::decode_png(START_BUTTON_PRESSED_PNG).ok()?,
            wallpaper_asset: decoded_wallpaper()?,
            fonts: font_view::FontPack::new(16)?,
            model,
            wallpaper: nt61_desktop::graphics::Surface::new(1, 1),
            glass: nt61_desktop::graphics::Surface::new(1, 1),
        })
    }

    fn prepare_aero(&mut self, width: u32, height: u32) {
        let Some(mut wallpaper) = nt61_desktop::graphics::Surface::try_new(width, height) else {
            return;
        };
        let Some(mut glass) = nt61_desktop::graphics::Surface::try_new(width, height) else {
            return;
        };
        if !cover_image(&self.wallpaper_asset, &mut wallpaper) {
            draw_wallpaper_into(&mut wallpaper, width, height);
        }
        glass.copy_from(&wallpaper);
        glass.box_blur(aero::GLASS_BLUR_RADIUS);
        glass.saturate(96);
        glass.tint(0x10_1B2B40, aero::GLASS_TINT_STRENGTH);
        self.wallpaper = wallpaper;
        self.glass = glass;
    }

    fn draw_desktop(
        &self,
        framebuffer: &arch::DesktopFramebufferInfo,
        frame: &mut nt61_desktop::graphics::Surface,
        cursor_x: i32,
        cursor_y: i32,
        output_pixels: &mut [u8],
    ) {
        frame.copy_from(&self.wallpaper);
        self.draw_icons(frame, framebuffer);
        self.draw_windows(frame);
        self.draw_taskbar(frame, framebuffer);
        if self.model.start_menu.open {
            self.draw_start_menu(frame, framebuffer);
        }
        blit_rgba(
            frame,
            &self.cursor.pixels,
            self.cursor.stride(),
            nt61_desktop::graphics::Rect::new(
                cursor_x,
                cursor_y,
                self.cursor.width,
                self.cursor.height,
            ),
        );
        present_surface_with(frame, output_pixels);
    }

    fn draw_icons(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        framebuffer: &arch::DesktopFramebufferInfo,
    ) {
        let desktop_height = framebuffer
            .height
            .saturating_sub(TASKBAR_HEIGHT)
            .max(ICON_CELL);
        let rows = (desktop_height / ICON_CELL).max(1);
        let icons = [
            (&self.computer, "Computer"),
            (&self.network, "Network"),
            (&self.recycle_bin, "Recycle Bin"),
            (&self.control_panel, "Control Panel"),
        ];
        for (index, (icon, label)) in icons.iter().enumerate() {
            let column = (index as u32) / rows;
            let row = (index as u32) % rows;
            let x = DESKTOP_MARGIN + column * ICON_CELL;
            let y = DESKTOP_MARGIN + row * ICON_CELL + 4;
            let control_id = CONTROL_DESKTOP_BASE + index as u32;
            blit_rgba(
                frame,
                &icon.pixels,
                56 * 4,
                nt61_desktop::graphics::Rect::new(
                    (x + (ICON_CELL - 56) / 2) as i32,
                    y as i32,
                    56,
                    56,
                ),
            );
            let width = self.fonts.measure(label);
            let label_x = x + (ICON_CELL.saturating_sub(width)) / 2;
            if self.model.controls.hot() == control_id {
                aero::blend_rounded_rect(
                    frame,
                    nt61_desktop::graphics::Rect::new(
                        x.saturating_sub(6) as i32,
                        y.saturating_sub(8) as i32,
                        ICON_CELL,
                        ICON_CELL,
                    ),
                    3,
                    0x30FF_FFFF,
                );
            }
            self.fonts
                .draw_labelled_onto(frame, label, label_x as i32, y as i32 + 64);
        }
    }

    fn draw_windows(&self, frame: &mut nt61_desktop::graphics::Surface) {
        for window in &self.model.taskbar.windows {
            if window.minimized {
                continue;
            }
            let x = window.x.max(0) as u32;
            let y = window.y.max(0) as u32;
            let width = window.width.max(1) as u32;
            let height = window.height.max(1) as u32;
            let bounds = nt61_desktop::graphics::Rect::new(x as i32, y as i32, width, height);
            aero::shadow(frame, bounds, 4);
            aero::rounded_window(frame, bounds, 6, aero::WINDOW_GLASS);
            aero::window_border(frame, bounds, 6);
            let caption = nt61_desktop::graphics::Rect::new(
                x as i32 + 2,
                y as i32 + 2,
                width.saturating_sub(4),
                28,
            );
            aero::title_bar(frame, caption, &self.glass);
            aero::blend_rect(
                frame,
                nt61_desktop::graphics::Rect::new(caption.x, caption.y + 27, caption.width, 1),
                0x60FF_FFFF,
            );
            let body = nt61_desktop::graphics::Rect::new(
                x as i32 + 2,
                y as i32 + 30,
                width.saturating_sub(4),
                height.saturating_sub(32),
            );
            aero::window_body(frame, body);
            match window.kind {
                nt61_desktop::shell::WindowKind::Notepad => {
                    let client = nt61_desktop::graphics::Rect::new(
                        body.x + 12,
                        body.y + 12,
                        body.width.saturating_sub(24),
                        body.height.saturating_sub(24),
                    );
                    if let Some(textbox) = self.model.textbox(window.id) {
                        self.draw_notepad_content(frame, client, textbox);
                    }
                }
                nt61_desktop::shell::WindowKind::Explorer
                | nt61_desktop::shell::WindowKind::Computer => {
                    let client = nt61_desktop::graphics::Rect::new(
                        body.x + 12,
                        body.y + 12,
                        body.width.saturating_sub(24),
                        body.height.saturating_sub(24),
                    );
                    self.draw_file_explorer(frame, client);
                }
                nt61_desktop::shell::WindowKind::CommandPrompt => {
                    self.draw_command_prompt(frame, body);
                }
                nt61_desktop::shell::WindowKind::Generic => {
                    let client = nt61_desktop::graphics::Rect::new(
                        body.x + 12,
                        body.y + 12,
                        body.width.saturating_sub(24),
                        28,
                    );
                    if let Some(textbox) = self.model.textbox(window.id) {
                        self.draw_textbox(frame, client, textbox);
                    }
                }
            }
            aero::blend_rect(
                frame,
                nt61_desktop::graphics::Rect::new(body.x, body.y, body.width, 1),
                0xD0FF_FFFF,
            );
            let title_x = (window.x + 8).max(0) as u32;
            let title_y = (window.y + 5).max(0) as u32;
            self.fonts.draw_onto(
                frame,
                &window.title,
                title_x as i32,
                title_y as i32,
                font_view::Ink::Black,
            );
            let group_bounds = nt61_desktop::graphics::Rect::new(
                (x + width.saturating_sub(102)) as i32,
                (y + 7) as i32,
                102,
                20,
            );
            aero::window_control_group(
                frame,
                group_bounds,
                Some(window.id) == self.model.selected_window,
            );
            let button_width = 29u32;
            aero::window_control_button(
                frame,
                nt61_desktop::graphics::Rect::new(
                    group_bounds.x,
                    group_bounds.y,
                    button_width,
                    group_bounds.height,
                ),
                aero::WindowControlButtonKind::Minimize,
                false,
            );
            aero::window_control_button(
                frame,
                nt61_desktop::graphics::Rect::new(
                    group_bounds.x + 29,
                    group_bounds.y,
                    button_width,
                    group_bounds.height,
                ),
                aero::WindowControlButtonKind::Maximize,
                false,
            );
            aero::window_control_button(
                frame,
                nt61_desktop::graphics::Rect::new(
                    group_bounds.x + 58,
                    group_bounds.y,
                    44,
                    group_bounds.height,
                ),
                aero::WindowControlButtonKind::Close,
                false,
            );
        }
    }

    fn draw_notepad_content(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        client: nt61_desktop::graphics::Rect,
        textbox: &nt61_desktop::widgets::textbox::TextBox,
    ) {
        frame.fill_rect(client.x, client.y, client.width, client.height, 0xFFFF_FFFF);
        frame.fill_rect(
            client.x,
            client.y,
            client.width,
            1,
            if textbox.focused {
                0xFF2A_87D8
            } else {
                0xFF9C_A3AF
            },
        );
        let line_height = 18_u32;
        let mut line_start = 0_usize;
        let mut y = client.y + 8;
        for (index, character) in textbox.text.char_indices() {
            if character == '\n' {
                let line = &textbox.text[line_start..index];
                self.draw_wrapped_line(frame, line, client, y);
                y += line_height as i32;
                line_start = index + 1;
            }
        }
        let last_line = &textbox.text[line_start..];
        self.draw_wrapped_line(frame, last_line, client, y);
        let cursor_byte = textbox
            .text
            .char_indices()
            .nth(textbox.cursor)
            .map(|(index, _)| index)
            .unwrap_or(textbox.text.len());
        let before = &textbox.text[..cursor_byte];
        let cursor_line = before.matches('\n').count() as u32;
        let current_line_start = before.rfind('\n').map_or(0, |index| index + 1);
        let current_line = before[current_line_start..]
            .strip_suffix('\r')
            .unwrap_or(&before[current_line_start..]);
        let cursor_column = self.fonts.measure(current_line);
        let cursor_x = client.x + 8 + cursor_column as i32;
        let cursor_y = client.y + 8 + (cursor_line * line_height) as i32;
        if textbox.focused {
            frame.fill_rect(cursor_x, cursor_y, 1, 16, 0xFF1B_1B1B);
        }
    }

    fn draw_wrapped_line(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        line: &str,
        client: nt61_desktop::graphics::Rect,
        y: i32,
    ) {
        let mut offset = 0_i32;
        let mut y = y;
        let mut x = client.x + 8;
        for word in line.split_inclusive(' ') {
            let width = self.fonts.measure(word);
            if offset + width as i32 > client.width.saturating_sub(16) as i32 && offset > 0 {
                x = client.x + 8;
                y += 18;
                offset = 0;
            }
            self.fonts
                .draw_onto(frame, word, x, y, font_view::Ink::Black);
            x += width as i32;
            offset += width as i32;
        }
    }

    fn draw_file_explorer(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        client: nt61_desktop::graphics::Rect,
    ) {
        frame.fill_rect(client.x, client.y, client.width, client.height, 0xFFF7_FAFD);
        let current = self.model.files.current_directory();
        let path = self
            .model
            .files
            .path(current)
            .unwrap_or_else(|| alloc::string::String::from("C:"));
        self.fonts.draw_onto(
            frame,
            &path,
            client.x + 8,
            client.y + 8,
            font_view::Ink::Black,
        );
        frame.fill_rect(client.x, client.y + 24, client.width, 1, 0xFFCC_D3DC);
        for (index, node) in self.model.files.children(current).into_iter().enumerate() {
            let y = client.y + 34 + index as i32 * 22;
            if y + 22 > client.y + client.height as i32 {
                break;
            }
            let label = alloc::format!(
                "{}  {}",
                if node.kind == nt61_desktop::file::NodeKind::Directory {
                    "[DIR]"
                } else {
                    "[FILE]"
                },
                node.name
            );
            self.fonts
                .draw_onto(frame, &label, client.x + 12, y, font_view::Ink::Black);
        }
    }

    fn draw_command_prompt(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        body: nt61_desktop::graphics::Rect,
    ) {
        frame.fill_rect(body.x, body.y, body.width, body.height, 0xFF0C_0C0C);
        self.fonts.draw_onto(
            frame,
            "Microsoft Windows [Version 6.1.7601]\r\nC:\\Users\\User>",
            body.x + 10,
            body.y + 10,
            font_view::Ink::White,
        );
    }

    fn draw_textbox(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        bounds: nt61_desktop::graphics::Rect,
        textbox: &nt61_desktop::widgets::textbox::TextBox,
    ) {
        frame.fill_rect(bounds.x, bounds.y, bounds.width, bounds.height, 0xFFFF_FFFF);
        frame.fill_rect(
            bounds.x,
            bounds.y,
            bounds.width,
            1,
            if textbox.focused {
                0xFF2A_87D8
            } else {
                0xFF9C_A3AF
            },
        );
        frame.fill_rect(
            bounds.x,
            bounds.y + bounds.height.saturating_sub(1) as i32,
            bounds.width,
            1,
            0xFF9C_A3AF,
        );
        let prefix_len = textbox
            .text
            .char_indices()
            .nth(textbox.cursor)
            .map(|(index, _)| index)
            .unwrap_or_else(|| textbox.text.len());
        let before = alloc::string::String::from(&textbox.text[..prefix_len]);
        let after = alloc::string::String::from(&textbox.text[prefix_len..]);
        let before_width = self.fonts.measure(&before);
        self.fonts.draw_onto(
            frame,
            &before,
            bounds.x + 6,
            bounds.y + 7,
            font_view::Ink::Black,
        );
        if textbox.focused {
            frame.fill_rect(
                bounds.x + 6 + before_width as i32,
                bounds.y + 6,
                1,
                bounds.height.saturating_sub(12),
                0xFF1B_1B1B,
            );
        }
        self.fonts.draw_onto(
            frame,
            &after,
            bounds.x + 7 + before_width as i32,
            bounds.y + 7,
            font_view::Ink::Black,
        );
    }

    fn draw_taskbar(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        framebuffer: &arch::DesktopFramebufferInfo,
    ) {
        let top = framebuffer.height.saturating_sub(TASKBAR_HEIGHT);
        let taskbar =
            nt61_desktop::graphics::Rect::new(0, top as i32, framebuffer.width, TASKBAR_HEIGHT);
        aero::taskbar_chrome(frame, taskbar, &self.glass);
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(0, top as i32, framebuffer.width, 1),
            0x90FF_FFFF,
        );
        self.draw_start_button(frame, top);
        for (index, window) in self.model.taskbar.windows.iter().enumerate() {
            let button_id = CONTROL_TASKBAR_BASE + index as u32;
            let x = taskbar_program_x(index as u32);
            if x + PROGRAM_BUTTON_WIDTH > notification_area_left(framebuffer.width) {
                break;
            }
            let button = nt61_desktop::graphics::Rect::new(
                x as i32,
                top as i32 + 4,
                PROGRAM_BUTTON_WIDTH,
                TASKBAR_HEIGHT.saturating_sub(9),
            );
            let selected = Some(window.id) == self.model.selected_window && !window.minimized;
            aero::superbar_button(frame, button, selected);
            if !selected && self.model.controls.hot() == button_id {
                aero::blend_rounded_rect(frame, button, 3, 0x70FF_FFFF);
            }
            if self.model.controls.pressed() == button_id {
                aero::blend_rounded_rect(frame, button, 3, 0x8000_0000);
            }
            self.fonts
                .draw_labelled_onto(frame, &window.title, x as i32 + 8, top as i32 + 15);
        }
        self.draw_notification_area(frame, framebuffer);
    }

    fn draw_start_button(&self, frame: &mut nt61_desktop::graphics::Surface, top: u32) {
        let active = self.model.controls.pressed() == CONTROL_START;
        let hovered = self.model.controls.hot() == CONTROL_START;
        let image = if active {
            &self.start_button_pressed
        } else if hovered {
            &self.start_button_hover
        } else {
            &self.start_button_normal
        };
        blit_rgba(
            frame,
            &image.pixels,
            image.stride(),
            nt61_desktop::graphics::Rect::new(
                0,
                top.saturating_sub(START_BUTTON_WIDTH.saturating_sub(TASKBAR_HEIGHT) / 2) as i32,
                START_BUTTON_WIDTH,
                START_BUTTON_WIDTH,
            ),
        );
    }

    fn draw_notification_area(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        framebuffer: &arch::DesktopFramebufferInfo,
    ) {
        let top = framebuffer.height.saturating_sub(TASKBAR_HEIGHT);
        let right = framebuffer.width;
        let left = notification_area_left(right);
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(left as i32, top as i32 + 5, 1, TASKBAR_HEIGHT - 10),
            0x50FF_FFFF,
        );

        let volume_x = (left + 16) as i32;
        for row in 0..3 {
            let width = 8 - row * 2;
            let y = top + 14 + row * 5;
            frame.fill_rect(volume_x, y as i32, width, 3, 0xFFF0_F6FF);
        }

        let network_x = (left + 40) as i32;
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(network_x, top as i32 + 14, 16, 12),
            2,
            0xE8F0_F6FF,
        );
        frame.fill_rect(network_x + 4, top as i32 + 17, 8, 3, 0xFF18_4A7C);

        let action_x = (left + 64) as i32;
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(action_x, top as i32 + 13, 14, 14),
            2,
            0xE8F0_F6FF,
        );
        frame.fill_rect(action_x + 6, top as i32 + 15, 2, 6, 0xFF18_4A7C);
        frame.fill_rect(action_x + 6, top as i32 + 23, 2, 2, 0xFF18_4A7C);

        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                right.saturating_sub(88) as i32,
                top as i32 + 5,
                1,
                TASKBAR_HEIGHT.saturating_sub(10),
            ),
            0x90FF_FFFF,
        );
        self.fonts.draw_labelled_onto(
            frame,
            "12:00 PM",
            right.saturating_sub(82) as i32,
            top as i32 + 14,
        );

        let show_desktop_x = right.saturating_sub(16);
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                show_desktop_x as i32,
                top as i32 + 5,
                1,
                TASKBAR_HEIGHT.saturating_sub(10),
            ),
            0x50FF_FFFF,
        );
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                (show_desktop_x + 5) as i32,
                top as i32 + 11,
                5,
                TASKBAR_HEIGHT.saturating_sub(22),
            ),
            0x70FF_FFFF,
        );
    }

    fn draw_start_menu(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        framebuffer: &arch::DesktopFramebufferInfo,
    ) {
        let width = START_MENU_WIDTH;
        let height = START_MENU_HEIGHT;
        let x = 2;
        let y = framebuffer.height.saturating_sub(TASKBAR_HEIGHT + height);
        let bounds = nt61_desktop::graphics::Rect::new(x as i32, y as i32, width, height);
        aero::shadow(frame, bounds, 4);
        aero::blend_rounded_rect(frame, bounds, 4, 0xEE15_1F2C);
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                x as i32 + 1,
                y as i32 + 1,
                width.saturating_sub(2),
                height.saturating_sub(2),
            ),
            3,
            0x50FF_FFFF,
        );

        let pane = width * 62 / 100;
        frame.fill_rect(
            x as i32 + 5,
            y as i32 + 5,
            pane.saturating_sub(10),
            height.saturating_sub(10),
            0xFFF7_FAFF,
        );
        frame.fill_rect(
            (x + pane + 1) as i32,
            y as i32 + 5,
            width.saturating_sub(pane + 6),
            height.saturating_sub(10),
            0xFF15_2538,
        );

        let programs = self.model.start_menu.all_programs();
        for (index, program) in programs.iter().enumerate() {
            let item_id = nt61_desktop::shell::CONTROL_MENU_BASE + index as u32;
            let item_y = y + 24 + index as u32 * 38;
            let item = nt61_desktop::graphics::Rect::new(
                (x + 14) as i32,
                item_y as i32,
                pane.saturating_sub(28),
                32,
            );
            let highlighted =
                self.model.start_menu.highlighted == index || self.model.controls.hot() == item_id;
            if highlighted {
                aero::blend_rounded_rect(frame, item, 3, 0xD86B_B2E6);
                aero::blend_rect(
                    frame,
                    nt61_desktop::graphics::Rect::new(item.x, item.y, item.width, 1),
                    0x80FF_FFFF,
                );
            }
            self.fonts.draw_onto(
                frame,
                program.title(),
                item.x + 12,
                item_y as i32 + 9,
                font_view::Ink::Black,
            );
        }
        frame.fill_rect(
            (x + 8) as i32,
            (y + height.saturating_sub(48)) as i32,
            pane.saturating_sub(20),
            26,
            0xFFFF_FFFF,
        );
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                (x + 8) as i32,
                (y + height.saturating_sub(48)) as i32,
                pane.saturating_sub(20),
                26,
            ),
            0xFF66_6666,
        );
        self.fonts.draw_onto(
            frame,
            if self.model.start_menu.query.is_empty() {
                "Search programs and files"
            } else {
                &self.model.start_menu.query
            },
            (x + 14) as i32,
            (y + height.saturating_sub(42)) as i32,
            font_view::Ink::Black,
        );

        let links = [
            "Documents",
            "Pictures",
            "Music",
            "Computer",
            "Control Panel",
            "Devices",
        ];
        for (index, link) in links.iter().enumerate() {
            let item_y = y + 88 + index as u32 * 40;
            aero::menu_item(
                frame,
                nt61_desktop::graphics::Rect::new(
                    (x + pane + 3) as i32,
                    item_y as i32,
                    width.saturating_sub(pane + 12),
                    38,
                ),
                false,
            );
            self.fonts
                .draw_labelled_onto(frame, link, (x + pane + 10) as i32, item_y as i32 + 12);
        }

        let avatar_size = 44;
        let avatar_x = x + pane + (width - pane - avatar_size) / 2;
        let avatar_y = y + 20;
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                (avatar_x - 2) as i32,
                (avatar_y - 2) as i32,
                avatar_size + 4,
                avatar_size + 4,
            ),
            4,
            0xE8FF_FFFF,
        );
        blit_rgba(
            frame,
            &self.avatar.pixels,
            self.avatar.stride(),
            nt61_desktop::graphics::Rect::new(
                avatar_x as i32,
                avatar_y as i32,
                avatar_size,
                avatar_size,
            ),
        );
        self.fonts.draw_labelled_onto(
            frame,
            "User",
            (avatar_x + avatar_size + 8) as i32,
            (avatar_y + 16) as i32,
        );
    }
}

fn scale_rgba(source: &png::Image, target_width: u32, target_height: u32) -> alloc::vec::Vec<u8> {
    let mut output = alloc::vec![0u8; (target_width * target_height * 4) as usize];
    if source.width == 0 || source.height == 0 {
        return output;
    }
    scale_rgba_bilinear(source, target_width, target_height, &mut output);
    output
}

fn scale_rgba_bilinear(
    source: &png::Image,
    target_width: u32,
    target_height: u32,
    output: &mut [u8],
) {
    let source_width = source.width as usize;
    let source_height = source.height as usize;
    let target_width = target_width as usize;
    let target_height = target_height as usize;
    if target_width == 0 || target_height == 0 {
        return;
    }
    let x_ratio: u64 = if target_width > 1 {
        (((source_width - 1) as u64) << 16) / (target_width - 1) as u64
    } else {
        0
    };
    let y_ratio: u64 = if target_height > 1 {
        (((source_height - 1) as u64) << 16) / (target_height - 1) as u64
    } else {
        0
    };
    for target_row in 0..target_height {
        let source_y = (target_row as u64 * y_ratio) >> 16;
        let y_frac = (((target_row as u64 * y_ratio) & 0xFFFF) >> 8) as u32;
        let y0 = source_y as usize;
        let y1 = (y0 + 1).min(source_height - 1);
        for target_column in 0..target_width {
            let source_x = (target_column as u64 * x_ratio) >> 16;
            let x_frac = (((target_column as u64 * x_ratio) & 0xFFFF) >> 8) as u32;
            let x0 = source_x as usize;
            let x1 = (x0 + 1).min(source_width - 1);

            let top_left = (y0 * source_width + x0) * 4;
            let top_right = (y0 * source_width + x1) * 4;
            let bottom_left = (y1 * source_width + x0) * 4;
            let bottom_right = (y1 * source_width + x1) * 4;

            let target_offset = (target_row * target_width + target_column) * 4;
            for channel in 0..4 {
                let top = lerp(
                    source.pixels[top_left + channel],
                    source.pixels[top_right + channel],
                    x_frac,
                );
                let bottom = lerp(
                    source.pixels[bottom_left + channel],
                    source.pixels[bottom_right + channel],
                    x_frac,
                );
                let result = lerp_u32(top, bottom, y_frac);
                output[target_offset + channel] = ((result >> 8) & 0xFF).min(255) as u8;
            }
        }
    }
}

fn lerp(a: u8, b: u8, t: u32) -> u32 {
    let af = a as u32;
    let bf = b as u32;
    ((af << 8) * (255 - t) / 255) + ((bf << 8) * t / 255)
}

fn lerp_u32(a: u32, b: u32, t: u32) -> u32 {
    (a * (255 - t) / 255) + (b * t / 255)
}

fn cover_image(source: &png::Image, target: &mut nt61_desktop::graphics::Surface) -> bool {
    if source.width == 0 || source.height == 0 || target.width() == 0 || target.height() == 0 {
        return false;
    }
    let target_width = target.width();
    let target_height = target.height();
    let source_ratio = source.width as u64 * target_height as u64;
    let target_ratio = target_width as u64 * source.height as u64;
    let (visible_width, visible_height) = if source_ratio > target_ratio {
        (
            source.height as u64 * target_width as u64 / target_height as u64,
            source.height as u64,
        )
    } else {
        (
            source.width as u64,
            source.width as u64 * target_height as u64 / target_width as u64,
        )
    };
    let offset_x = (source.width as u64).saturating_sub(visible_width) / 2;
    let offset_y = (source.height as u64).saturating_sub(visible_height) / 2;
    let x_ratio: u64 = if target_width > 1 {
        (((visible_width - 1) as u64) << 16) / (target_width - 1) as u64
    } else {
        0
    };
    let y_ratio: u64 = if target_height > 1 {
        (((visible_height - 1) as u64) << 16) / (target_height - 1) as u64
    } else {
        0
    };
    for row in 0..target_height as usize {
        let source_y = offset_y + (row as u64 * y_ratio >> 16);
        let y_frac = (((row as u64 * y_ratio) & 0xFFFF) >> 8) as u32;
        let y0 = source_y as usize;
        let y1 = (y0 + 1).min(source.height as usize - 1);
        for column in 0..target_width as usize {
            let source_x = offset_x + (column as u64 * x_ratio >> 16);
            let x_frac = (((column as u64 * x_ratio) & 0xFFFF) >> 8) as u32;
            let x0 = source_x as usize;
            let x1 = (x0 + 1).min(source.width as usize - 1);
            let top_left = (y0 * source.width as usize + x0) * 4;
            let top_right = (y0 * source.width as usize + x1) * 4;
            let bottom_left = (y1 * source.width as usize + x0) * 4;
            let bottom_right = (y1 * source.width as usize + x1) * 4;
            let mut pixel = [0u8; 4];
            for channel in 0..4 {
                let top = lerp(
                    source.pixels[top_left + channel],
                    source.pixels[top_right + channel],
                    x_frac,
                );
                let bottom = lerp(
                    source.pixels[bottom_left + channel],
                    source.pixels[bottom_right + channel],
                    x_frac,
                );
                let result = lerp_u32(top, bottom, y_frac);
                pixel[channel] = ((result >> 8) & 0xFF).min(255) as u8;
            }
            target.set_pixel(
                column as u32,
                row as u32,
                u32::from_le_bytes([pixel[2], pixel[1], pixel[0], pixel[3]]),
            );
        }
    }
    true
}

fn draw_wallpaper_into(surface: &mut nt61_desktop::graphics::Surface, width: u32, height: u32) {
    let desktop_height = height.saturating_sub(TASKBAR_HEIGHT).max(1);
    surface.vertical_gradient(WALLPAPER_TOP, WALLPAPER_BOTTOM);
    aero::radial_blend(
        surface,
        width / 3,
        desktop_height / 4,
        width.min(desktop_height * 2),
        0xFFB6_E2FF,
        0xFF1F_5FA6,
        0x50,
        0x00,
    );
}

fn present_surface_with(surface: &nt61_desktop::graphics::Surface, pixels: &mut [u8]) {
    if pixels.len() < surface.pixels().len() * 4 {
        return;
    }
    for (target, color) in pixels.chunks_exact_mut(4).zip(surface.pixels()) {
        target.copy_from_slice(&[
            ((color >> 16) & 0xFF) as u8,
            ((color >> 8) & 0xFF) as u8,
            (color & 0xFF) as u8,
            ((color >> 24) & 0xFF) as u8,
        ]);
    }
    unsafe {
        arch::draw_rgba(
            pixels.as_ptr() as *const u8,
            0,
            0,
            surface.width(),
            surface.height(),
            surface.width() as usize * 4,
        );
    }
}

fn draw_fallback_desktop(framebuffer: &arch::DesktopFramebufferInfo) {
    let mut surface = nt61_desktop::graphics::Surface::new(framebuffer.width, framebuffer.height);
    draw_wallpaper_into(&mut surface, framebuffer.width, framebuffer.height);
    let mut glass = surface.clone();
    glass.box_blur(aero::GLASS_BLUR_RADIUS);
    glass.saturate(96);
    glass.tint(0x10_1B2B40, aero::GLASS_TINT_STRENGTH);
    aero::glass_bar_precomputed(
        &mut surface,
        nt61_desktop::graphics::Rect::new(
            0,
            framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32,
            framebuffer.width,
            TASKBAR_HEIGHT,
        ),
        &glass,
    );
    let mut pixels = alloc::vec![0u8; surface.pixels().len() * 4];
    present_surface_with(&surface, &mut pixels);
}

#![no_std]
#![no_main]
#![deny(warnings)]

mod arch;
#[path = "../../font_view.rs"]
mod font_view;
mod png;
mod resources_data;

const AVATAR_PNG: &[u8] = include_bytes!("../../../assets/avatar.png");
const CLOSE_BUTTON: &[u8] = include_bytes!("../../../assets/7css/gui/icon/close.png");
const MAXIMIZE_BUTTON: &[u8] = include_bytes!("../../../assets/7css/gui/icon/maximize.png");
const MINIMIZE_BUTTON: &[u8] = include_bytes!("../../../assets/7css/gui/icon/minimize.png");
const RESTORE_BUTTON: &[u8] = include_bytes!("../../../assets/7css/gui/icon/restore.png");
const CURSOR_PNG: &[u8] = include_bytes!("../../../assets/cursor.png");
const START_ORB_PNG: &[u8] = include_bytes!("../../../assets/theme/orb.png");
const TASKBAR_PNG: &[u8] = include_bytes!("../../../assets/theme/taskbar.png");
const DESKTOP_WALLPAPER_PNG: &[u8] = include_bytes!("../../../assets/desktop-wallpaper.png");

use core::panic::PanicInfo;
use nt61_desktop::compositor::aero;
use nt61_desktop::controls::PointerAction;
use nt61_desktop::graphics::blit::blit_rgba;
use nt61_desktop::shell::ShellModel;
use nt61_desktop::theme::Theme;

extern crate alloc;

use nt61_desktop::user_allocator::StaticHeapAllocator;

#[global_allocator]
static EXPLORER_ALLOCATOR: StaticHeapAllocator<{ 48 * 1024 * 1024 }> = StaticHeapAllocator::new();

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
    arch::debug_str("[EXPLORER] kernel_entry\r\n");
    desktop_main()
}

const TASKBAR_HEIGHT: u32 = 40;
const ICON_CELL: u32 = 96;
const DESKTOP_MARGIN: u32 = 12;
const WALLPAPER_TOP: u32 = 0x2f5f8f;
const WALLPAPER_BOTTOM: u32 = 0x12406f;

struct Shell {
    computer: png::Image,
    network: png::Image,
    recycle_bin: png::Image,
    control_panel: png::Image,
    close: png::Image,
    maximize: png::Image,
    minimize: png::Image,
    restore: png::Image,
    avatar: png::Image,
    cursor: png::Image,
    start_orb: png::Image,
    taskbar_asset: png::Image,
    wallpaper_asset: png::Image,
    fonts: font_view::FontPack,
    model: ShellModel,
    wallpaper: nt61_desktop::graphics::Surface,
    glass: nt61_desktop::graphics::Surface,
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

    let mut shell = match Shell::load() {
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
    shell.draw_desktop(&framebuffer, &mut frame, cursor_x, cursor_y);
    shell
        .model
        .refresh_controls(framebuffer.width, framebuffer.height);
    #[cfg(target_arch = "x86_64")]
    arch::debug_str("[EXPLORER] desktop drawn\r\n");
    loop {
        let mut redraw = false;
        let mut key = 0u8;
        let key_pending = unsafe { arch::poll_key(&mut key) == 1 };
        let mouse_pending = unsafe { arch::poll_mouse(&mut mouse) == 1 };
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
            redraw = true;
        }
        if key_pending {
            let changed = if shell.model.start_menu.open {
                match shell.model.controls.route_key(key) {
                    Some(event) => shell.model.handle_control_event(event),
                    None => false,
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
            shell.draw_desktop(&framebuffer, &mut frame, cursor_x, cursor_y);
        }
        if !key_pending && !mouse_pending {
            core::hint::spin_loop();
        }
    }
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
            computer: png::decode_png(resources_data::COMPUTER).ok()?,
            network: png::decode_png(resources_data::NETWORK).ok()?,
            recycle_bin: png::decode_png(resources_data::RECYCLE_BIN).ok()?,
            control_panel: png::decode_png(resources_data::CONTROL_PANEL).ok()?,
            close: png::decode_png(CLOSE_BUTTON).ok()?,
            maximize: png::decode_png(MAXIMIZE_BUTTON).ok()?,
            minimize: png::decode_png(MINIMIZE_BUTTON).ok()?,
            restore: png::decode_png(RESTORE_BUTTON).ok()?,
            avatar: png::decode_png(AVATAR_PNG).ok()?,
            cursor: png::decode_png(CURSOR_PNG).ok()?,
            start_orb: png::decode_png(START_ORB_PNG).ok()?,
            taskbar_asset: png::decode_png(TASKBAR_PNG).ok()?,
            wallpaper_asset: png::decode_png(DESKTOP_WALLPAPER_PNG).ok()?,
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
        present_surface(frame);
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
            let y = DESKTOP_MARGIN + row * ICON_CELL;
            let scaled = scale_rgba(icon, 56, 56);
            blit_rgba(
                frame,
                &scaled,
                56 * 4,
                nt61_desktop::graphics::Rect::new(x as i32, y as i32, 56, 56),
            );
            let width = self.fonts.measure(label);
            let label_x = x + (56u32.saturating_sub(width)) / 2;
            aero::blend_rounded_rect(
                frame,
                nt61_desktop::graphics::Rect::new(
                    x.saturating_sub(8) as i32,
                    y.saturating_sub(4) as i32,
                    72,
                    84,
                ),
                3,
                0x30FF_FFFF,
            );
            self.fonts
                .draw_labelled_onto(frame, label, label_x as i32, y as i32 + 52);
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
            let mut blit_icon = |icon: &png::Image, x: u32| {
                blit_rgba(
                    frame,
                    &icon.pixels,
                    icon.stride(),
                    nt61_desktop::graphics::Rect::new(
                        x as i32,
                        group_bounds.y + (20 - icon.height) as i32 / 2,
                        icon.width,
                        icon.height,
                    ),
                );
            };
            blit_icon(
                &self.minimize,
                group_bounds.x as u32 + (29 - self.minimize.width) / 2,
            );
            let middle_icon = if window.maximized {
                &self.restore
            } else {
                &self.maximize
            };
            blit_icon(
                middle_icon,
                group_bounds.x as u32 + 29 + (29 - middle_icon.width) / 2,
            );
            blit_icon(
                &self.close,
                group_bounds.x as u32 + 58 + (44 - self.close.width) / 2,
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
        let taskbar = scale_rgba(&self.taskbar_asset, framebuffer.width, TASKBAR_HEIGHT);
        blit_rgba(
            frame,
            &taskbar,
            (framebuffer.width * 4) as usize,
            nt61_desktop::graphics::Rect::new(0, top as i32, framebuffer.width, TASKBAR_HEIGHT),
        );
        self.draw_start_button(frame, top);
        let notification_width = 118;
        for (index, window) in self.model.taskbar.windows.iter().enumerate() {
            let x = 44 + index as u32 * 92;
            if x + 86 > framebuffer.width.saturating_sub(notification_width + 12) {
                break;
            }
            aero::superbar_button(
                frame,
                nt61_desktop::graphics::Rect::new(
                    x as i32,
                    top as i32 + 4,
                    86,
                    TASKBAR_HEIGHT.saturating_sub(9),
                ),
                Some(window.id) == self.model.selected_window && !window.minimized,
            );
            self.fonts
                .draw_labelled_onto(frame, &window.title, x as i32 + 4, top as i32 + 14);
        }
        self.draw_notification_area(frame, framebuffer);
    }

    fn draw_start_button(&self, frame: &mut nt61_desktop::graphics::Surface, top: u32) {
        blit_rgba(
            frame,
            &self.start_orb.pixels,
            self.start_orb.stride(),
            nt61_desktop::graphics::Rect::new(
                4,
                top as i32 + 2,
                self.start_orb.width,
                self.start_orb.height,
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
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                right.saturating_sub(10) as i32,
                top as i32 + 6,
                5,
                TASKBAR_HEIGHT.saturating_sub(12),
            ),
            1,
            0x50FF_FFFF,
        );
        let icon_center = right.saturating_sub(46);
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(icon_center as i32 - 5, top as i32 + 14, 10, 8),
            2,
            0xF0FF_FFFF,
        );
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(icon_center as i32 - 2, top as i32 + 20, 4, 3),
            1,
            0xF0FF_FFFF,
        );
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                right.saturating_sub(74) as i32,
                top as i32 + 14,
                10,
                8,
            ),
            3,
            0xE8FF_FFFF,
        );
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                right.saturating_sub(71) as i32,
                top as i32 + 16,
                4,
                4,
            ),
            0xE80B4C9B,
        );
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                right.saturating_sub(98) as i32,
                top as i32 + 8,
                1,
                TASKBAR_HEIGHT.saturating_sub(16),
            ),
            0x90FF_FFFF,
        );
        self.fonts.draw_labelled_onto(
            frame,
            "12:00 PM",
            right.saturating_sub(92) as i32,
            top as i32 + 14,
        );
    }

    fn draw_start_menu(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        framebuffer: &arch::DesktopFramebufferInfo,
    ) {
        let width = 374;
        let height = 374;
        let x = 2;
        let y = framebuffer.height.saturating_sub(TASKBAR_HEIGHT + height);
        let bounds = nt61_desktop::graphics::Rect::new(x as i32, y as i32, width, height);
        aero::shadow(frame, bounds, 4);
        aero::blend_rounded_rect(frame, bounds, 4, 0xE820_3850);
        aero::blend_rounded_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                x as i32 + 1,
                y as i32 + 1,
                width.saturating_sub(2),
                height.saturating_sub(2),
            ),
            3,
            0x28FF_FFFF,
        );

        let pane = width * 55 / 100;
        frame.fill_rect(
            x as i32 + 5,
            y as i32 + 5,
            pane.saturating_sub(10),
            height.saturating_sub(10),
            0xFFF6_FAFF,
        );
        frame.fill_rect(
            (x + pane + 1) as i32,
            y as i32 + 5,
            width.saturating_sub(pane + 6),
            height.saturating_sub(10),
            0xFF20_3850,
        );

        let programs = [
            "Notepad",
            "Command Prompt",
            "Windows Explorer",
            "Calculator",
        ];
        for (index, program) in programs.iter().enumerate() {
            let item_y = y + 12 + index as u32 * 38;
            aero::blend_rounded_rect(
                frame,
                nt61_desktop::graphics::Rect::new(
                    (x + 8) as i32,
                    item_y as i32,
                    pane.saturating_sub(20),
                    32,
                ),
                2,
                0x22CC_E2EF,
            );
            self.fonts.draw_onto(
                frame,
                program,
                (x + 16) as i32,
                item_y as i32 + 9,
                font_view::Ink::Black,
            );
        }
        frame.fill_rect(
            (x + 8) as i32,
            (y + height.saturating_sub(48)) as i32,
            pane.saturating_sub(20),
            28,
            0xFFFF_FFFF,
        );
        aero::blend_rect(
            frame,
            nt61_desktop::graphics::Rect::new(
                (x + 8) as i32,
                (y + height.saturating_sub(48)) as i32,
                pane.saturating_sub(20),
                28,
            ),
            0xFF66_6666,
        );
        self.fonts.draw_onto(
            frame,
            "Search programs and files",
            (x + 14) as i32,
            (y + height.saturating_sub(40)) as i32,
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
            let item_y = y + 12 + index as u32 * 44;
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
        let avatar_x = x + pane + 10;
        let avatar_y = y + height.saturating_sub(54);
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
    for row in 0..target_height as usize {
        let source_row = row * source.height as usize / target_height as usize;
        for column in 0..target_width as usize {
            let source_column = column * source.width as usize / target_width as usize;
            let source_offset = (source_row * source.width as usize + source_column) * 4;
            let target_offset = (row * target_width as usize + column) * 4;
            output[target_offset..target_offset + 4]
                .copy_from_slice(&source.pixels[source_offset..source_offset + 4]);
        }
    }
    output
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
    for row in 0..target_height as usize {
        let source_y = offset_y + (row as u64 * visible_height) / target_height as u64;
        for column in 0..target_width as usize {
            let source_x = offset_x + (column as u64 * visible_width) / target_width as u64;
            let source_offset = (source_y as usize * source.width as usize + source_x as usize) * 4;
            let Some(chunk) = source.pixels.get(source_offset..source_offset + 4) else {
                return false;
            };
            target.set_pixel(
                column as u32,
                row as u32,
                u32::from_le_bytes([chunk[2], chunk[1], chunk[0], chunk[3]]),
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

fn present_surface(surface: &nt61_desktop::graphics::Surface) {
    let mut pixels = alloc::vec![0u8; surface.pixels().len() * 4];
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
    present_surface(&surface);
}

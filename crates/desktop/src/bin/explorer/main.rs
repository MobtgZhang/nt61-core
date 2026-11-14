#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![deny(warnings)]

mod arch;
mod chrome;
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

#[cfg(not(test))]
#[cfg(not(test))]
use core::panic::PanicInfo;
use nt61_desktop::bmp;
use nt61_desktop::compositor::aero;
use nt61_desktop::controls::PointerAction;
use nt61_desktop::graphics::blit::blit_rgba;
use nt61_desktop::shell::{WindowCursor, ShellModel, CONTROL_DESKTOP_BASE, CONTROL_SUBMENU_BASE, TASKBAR_HEIGHT};
use nt61_desktop::theme::Theme;
use nt61_desktop::user_account_pictures::USER;

extern crate alloc;

use nt61_desktop::user_allocator::StaticHeapAllocator;

#[global_allocator]
static EXPLORER_ALLOCATOR: StaticHeapAllocator<{ 64 * 1024 * 1024 }> = StaticHeapAllocator::new();

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Render the panic message through the kernel serial syscall so
    // the death reason shows up in the boot log (the desktop has no
    // other console).
    use core::fmt::Write as _;
    struct SerialWriter;
    impl core::fmt::Write for SerialWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            arch::debug_str(s);
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
const WALLPAPER_TOP: u32 = 0x2f5f8f;
const WALLPAPER_BOTTOM: u32 = 0x12406f;

struct Shell {
    computer: png::Image,
    network: png::Image,
    recycle_bin: png::Image,
    control_panel: png::Image,
    document: png::Image,
    folder: png::Image,
    avatar: png::Image,
    cursor: CursorImage,
    resize_cursors: [CursorImage; 4],
    cursor_kind: WindowCursor,
    start_button_normal: png::Image,
    start_button_hover: png::Image,
    start_button_pressed: png::Image,
    wallpaper_asset: png::Image,
    fonts: font_view::FontPack,
    model: ShellModel,
    wallpaper: nt61_desktop::graphics::Surface,
    glass: nt61_desktop::graphics::Surface,
    hovered_caption: Option<(u32, u8)>,
    hovered_application: Option<u32>,
    scene: core::cell::RefCell<nt61_desktop::compositor::scene::WindowScene>,
    window_dirty: core::cell::Cell<Option<u32>>,
    scene_dirty: core::cell::Cell<bool>,
}

fn desktop_main() -> ! {
    arch::debug_str("[EXPLORER] startup\r\n");
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
            arch::debug_str("[EXPLORER] fallback frame ready\r\n");
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
    shell.model.update_clock(arch::shell_time());
    let devices = arch::shell_devices();
    shell.model.network_adapters = (devices >> 8) as u32;
    shell.model.audio_available = devices & 1 != 0;
    let mut next_status = 0u64;
    let mut mouse = arch::DesktopMouseState::default();
    let mut previous_buttons = 0u32;
    let mut cursor_x = (framebuffer.width / 2) as i32;
    let mut cursor_y = (framebuffer.height / 2) as i32;
    let mut previous_cursor_x = cursor_x;
    let mut previous_cursor_y = cursor_y;
    let mut cursor_rgba = alloc::vec![0u8; 32 * 32 * 4];
    let mut previous_cursor_bounds = shell.cursor_bounds(cursor_x, cursor_y);
    let mut last_desktop_click: Option<(usize, u64)> = None;
    let mut next_explorer_job=0usize;
    let mut next_directory_poll=0u64;
    let mut next_process_poll=0u64;
    let mut directory_poll_index=0usize;
    let mut frame_rgba = alloc::vec![0u8; frame.pixels().len() * 4];
    shell.draw_desktop(&framebuffer, &mut frame, &mut frame_rgba);
    shell.draw_cursor(&frame, cursor_x, cursor_y, &mut cursor_rgba);
    shell
        .model
        .refresh_controls(framebuffer.width, framebuffer.height);
    arch::debug_str("[EXPLORER] desktop frame ready\r\n");
    loop {
        let mut redraw = false;
        let ticks = arch::shell_ticks();
        for app in &mut shell.model.applications{app.input_time=ticks;}
        for explorer in &mut shell.model.explorers { explorer.input_time=Some(ticks); }
        if ticks >= next_status {
            redraw |= shell.model.update_clock(arch::shell_time());
            next_status = ticks.saturating_add(1000);
        }
        let old_audio = (shell.model.volume, shell.model.muted);
        let mut key = 0u8;
        let mut key_pending = unsafe { arch::poll_key(&mut key) == 1 };
        let mut pointer_moved = false;
        let mut pointer_full = false;
        // A busy input device must leave time to submit the composed frame
        // and process keyboard input. Unread button edges stay in the queue.
        for _ in 0..64 {
            if unsafe { arch::poll_mouse(&mut mouse) == 0 } { break; }
            let previous_scene = shell.window_geometry();
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
            // Retain the composed scene for pointer motion. Invalidate below
            // only when a window, application or desktop icon actually changes.
            if mouse.buttons & 2 != 0 && previous_buttons & 2 == 0
                && shell.model.window_hit_test(cursor_x,cursor_y).is_some_and(|id|shell.model.application(id).is_some() || shell.model.explorers.iter().any(|e|e.window_id==id)) {
                if let Some(id)=shell.model.window_hit_test(cursor_x,cursor_y) {shell.model.application_pointer(id,cursor_x,cursor_y,true,false,true);}
                shell.scene_dirty.set(true);pointer_full=true;previous_buttons=mouse.buttons;continue;
            }
            if mouse.buttons & 2 != 0 && previous_buttons & 2 == 0 {
                {
                    shell.model.open_context_menu(
                        cursor_x,
                        cursor_y,
                        framebuffer.width,
                        framebuffer.height,
                    );
                }
                shell.scene_dirty.set(true);
                shell
                    .model
                    .refresh_controls(framebuffer.width, framebuffer.height);
                pointer_full = true;
                previous_buttons = mouse.buttons;
                continue;
            }
            if action == Some(PointerAction::Down)
                && shell.model.close_popups_if_outside(
                    cursor_x,
                    cursor_y,
                    framebuffer.width,
                    framebuffer.height,
                )
            {
                pointer_full = true;
                shell
                    .model
                    .refresh_controls(framebuffer.width, framebuffer.height);
            }
            if let Some(action) = action {
                let popup_over = shell.model.popup_contains(
                    cursor_x,
                    cursor_y,
                    framebuffer.width,
                    framebuffer.height,
                );
                let over_window = !popup_over
                    && cursor_y < framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32
                    && shell.model.window_hit_test(cursor_x, cursor_y).is_some();
                let hovered_application = if over_window && shell.model.drag_window.is_none() {
                    shell.model.window_hit_test(cursor_x,cursor_y)
                } else {None};
                if shell.hovered_application != hovered_application {
                    if let Some(id)=shell.hovered_application {
                        if shell.model.clear_application_hover(id) {
                            shell.mark_window_dirty(id);pointer_full=true;
                        }
                    }
                    shell.hovered_application=hovered_application;
                }
                let hovered_caption = if over_window && shell.model.drag_window.is_none() {
                    shell.model.window_hit_test(cursor_x, cursor_y)
                        .and_then(|id| shell.model.taskbar.windows.iter().find(|w| w.id == id))
                        .and_then(|w| w.caption_control_at(cursor_x, cursor_y).map(|button| (w.id, button)))
                } else { None };
                if shell.hovered_caption != hovered_caption {
                    if let Some((id,_))=shell.hovered_caption {shell.mark_window_dirty(id);}
                    if let Some((id,_))=hovered_caption {shell.mark_window_dirty(id);}
                    shell.hovered_caption = hovered_caption;
                    pointer_full = true;
                }
                if action == PointerAction::Down && over_window {
                    shell.scene_dirty.set(true);
                    if let Some(window_id) = shell.model.window_hit_test(cursor_x, cursor_y) {
                        if !shell.model.handle_window_control_click(
                            window_id,
                            cursor_x,
                            cursor_y,
                            framebuffer.width,
                            framebuffer.height.saturating_sub(TASKBAR_HEIGHT),
                        ) {
                            shell.model.select_window(window_id);
                            if !shell.model.begin_window_drag(window_id, cursor_x, cursor_y) {
                                shell.model.application_pointer(window_id,cursor_x,cursor_y,true,true,false);
                            }
                        }
                    }
                }
                if action==PointerAction::Down && over_window && shell.model.drag_window.is_none() {
                    if let Some(id)=shell.model.window_hit_test(cursor_x,cursor_y) {
                        if shell.model.needs_capture(id,cursor_x,cursor_y) {
                            let hidden=shell.model.taskbar.windows.iter().position(|w|w.id==id);
                            if let Some(index)=hidden {
                                shell.model.taskbar.windows[index].minimized=true;
                                shell.scene_dirty.set(true);
                                shell.draw_desktop(&framebuffer,&mut frame,&mut frame_rgba);
                                shell.model.capture_application(id,frame.clone());
                                shell.model.taskbar.windows[index].minimized=false;
                                shell.scene_dirty.set(true);
                            }
                        }
                    }
                }
                if action==PointerAction::Move && shell.model.drag_window.is_none() && over_window {
                    if let Some(id)=shell.model.window_hit_test(cursor_x,cursor_y) {
                        if shell.model.application_pointer(id,cursor_x,cursor_y,false,mouse.buttons&1!=0,false){
                            shell.mark_window_dirty(id);pointer_full=true;
                        }
                    }
                }
                if matches!(action, PointerAction::Move | PointerAction::Up)
                    && shell.model.drag_window.is_some()
                    && shell.model.drag_window_to(cursor_x, cursor_y) {
                    shell.scene_dirty.set(true);
                    pointer_full = true;
                }
                if action == PointerAction::Up {
                    if shell.model.drag_window.is_none() {
                        if let Some(id) = shell.model.selected_window {
                            shell.model.application_pointer(id,cursor_x,cursor_y,false,false,false);
                        }
                    }
                    shell.model.release_window_drag();
                }
                let previous_hot=shell.model.controls.hot();
                let changed = match shell.model.controls.route_pointer(
                    action,
                    if over_window || shell.model.drag_window.is_some() { -1 } else { cursor_x },
                    cursor_y,
                ) {
                    Some(event) => {
                        if let nt61_desktop::controls::ControlEvent::Activated(id) = event {
                            if (CONTROL_DESKTOP_BASE..nt61_desktop::shell::CONTROL_MENU_BASE).contains(&id) {
                                let index = (id - CONTROL_DESKTOP_BASE) as usize;
                                if last_desktop_click.is_some_and(|(previous, time)| previous == index && ticks.saturating_sub(time) <= 500) {
                                    shell.model.open_desktop_item(index);
                                    last_desktop_click = None;
                                } else {
                                    last_desktop_click = Some((index, ticks));
                                }
                            }
                        }
                        shell.model.handle_control_event(event)
                    },
                    None => false,
                };
                // Popup and taskbar hover changes only their overlay. Repaint
                // the retained desktop when an icon changes its highlight.
                let icon_hot_changed = [previous_hot,shell.model.controls.hot()].iter()
                    .any(|id|(CONTROL_DESKTOP_BASE..nt61_desktop::shell::CONTROL_MENU_BASE).contains(id));
                if changed && (icon_hot_changed || (action != PointerAction::Move && !popup_over
                    && cursor_y < framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32)) {
                    shell.scene_dirty.set(true);
                }
                let hot = shell.model.controls.hot();
                if (action == PointerAction::Move || action == PointerAction::Down)
                    && shell
                        .model
                        .update_menu_hover(hot, framebuffer.width, framebuffer.height)
                {
                    pointer_full = true;
                    shell
                        .model
                        .refresh_controls(framebuffer.width, framebuffer.height);
                }
                if mouse.buttons & 1 != 0
                    && shell.model.controls.pressed() == nt61_desktop::shell::CONTROL_VOLUME_TRACK
                {
                    pointer_full |=
                        shell
                            .model
                            .set_volume_at(cursor_y, framebuffer.width, framebuffer.height);
                }
                if changed || action != PointerAction::Move || shell.model.drag_window.is_some() {
                    pointer_full = true;
                } else {
                    pointer_moved = cursor_x != previous_cursor_x || cursor_y != previous_cursor_y;
                }
                if changed {
                    shell
                        .model
                        .refresh_controls(framebuffer.width, framebuffer.height);
                }
            }
            if shell.window_geometry() != previous_scene { shell.scene_dirty.set(true); }
            previous_buttons = mouse.buttons;
        }
        let cursor_kind = if shell.model.drag_window.is_some()
            || (cursor_y < framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32
                && !shell.model.popup_contains(cursor_x, cursor_y, framebuffer.width, framebuffer.height)) {
            shell.model.window_cursor_at(cursor_x, cursor_y)
        } else { WindowCursor::Arrow };
        pointer_moved |= shell.cursor_kind != cursor_kind;
        shell.cursor_kind = cursor_kind;
        if pointer_full {
            redraw = true;
        }
        // Consume queued typing before repainting. Software rendering can take
        // longer than a key repeat interval on the emulated non-x86 CPUs.
        for index in 0..64 {
            if !key_pending {
                break;
            }
            let changed = if shell.model.context_menu.is_some()
                || shell.model.start_menu.open
                || shell.model.flyout.is_some()
            {
                if key == 13 || key == 10 { shell.scene_dirty.set(true); }
                shell.model.handle_popup_key(key)
            } else {
                shell.scene_dirty.set(true);
                if (key == 13 || key == 10) && shell.model.selected_window.is_none() {
                    shell.model.desktop.selected.first().copied().is_some_and(|index| shell.model.open_desktop_item(index))
                } else {
                    shell.model.handle_window_key(key)
                }
            };
            if changed {
                shell
                    .model
                    .refresh_controls(framebuffer.width, framebuffer.height);
            }
            redraw = true;
            if index < 63 {
                key_pending = unsafe { arch::poll_key(&mut key) == 1 };
            }
        }
        if shell.model.flush_application_input(ticks){shell.scene_dirty.set(true);redraw=true;}
        for explorer in &mut shell.model.explorers {
            if explorer.flush_input(&mut shell.model.files){shell.scene_dirty.set(true);redraw=true;}
        }
        // Give input priority, then service one cancellable directory-search batch.
        let count=shell.model.explorers.len();
        for step in 0..count {
            let index=(next_explorer_job+step)%count;
            if shell.model.explorers[index].work_pending() {
                shell.model.explorers[index].work(&mut shell.model.files);
                next_explorer_job=(index+1)%count;
                shell.scene_dirty.set(true);redraw=true;break;
            }
        }
        // Round-robin volume polling also observes changes made by CMD and
        // other applications. At most one directory is read per interval.
        if ticks>=next_directory_poll && count!=0 {
            next_directory_poll=ticks.saturating_add(250);
            let index=directory_poll_index%count;
            directory_poll_index=(index+1)%count;
            if shell.model.explorers[index].poll_directory(&mut shell.model.files){
                shell.scene_dirty.set(true);redraw=true;
            }
        }
        if ticks >= next_process_poll {
            next_process_poll = ticks.saturating_add(1000);
            let observers: alloc::vec::Vec<_> = shell.model.applications.iter().filter(|app|
                matches!(app.id, nt61_desktop::applications::AppId::TaskManager
                    | nt61_desktop::applications::AppId::SystemInformation
                    | nt61_desktop::applications::AppId::ResourceMonitor))
                .filter(|app| shell.model.taskbar.windows.iter().any(|w|
                    w.id == app.window_id && !w.minimized)).map(|app| app.window_id).collect();
            if !observers.is_empty() {
                if let Ok(snapshot) = nt61_desktop::platform::services::processes() {
                    if shell.model.process_snapshot.as_ref() != Some(&snapshot) {
                        shell.model.process_snapshot = Some(snapshot);
                        for id in observers { shell.mark_window_dirty(id); }
                        redraw = true;
                    }
                }
            }
        }
        if old_audio != (shell.model.volume, shell.model.muted) {
            let _ = arch::shell_audio(shell.model.volume, shell.model.muted);
        }
        if shell.model.power_action.is_some() {
            shell.model.show_power_unavailable();
            shell
                .model
                .refresh_controls(framebuffer.width, framebuffer.height);
            redraw = true;
        }
        if redraw {
            shell.draw_desktop(&framebuffer, &mut frame, &mut frame_rgba);
            present_surface_region(&frame, &mut frame_rgba, previous_cursor_bounds);
            shell.draw_cursor(&frame, cursor_x, cursor_y, &mut cursor_rgba);
        } else if pointer_moved {
            shell.update_cursor(
                &frame,
                previous_cursor_bounds,
                cursor_x,
                cursor_y,
                &mut frame_rgba,
                &mut cursor_rgba,
            );
        }
        if !key_pending && !pointer_moved {
            core::hint::spin_loop();
        }
        previous_cursor_bounds = shell.cursor_bounds(cursor_x, cursor_y);
        previous_cursor_x = cursor_x;
        previous_cursor_y = cursor_y;
    }
}

fn decoded_wallpaper() -> Option<png::Image> {
    png::decode_png(WALLPAPER_PNG).ok()
}

fn scaled_icon(bytes: &[u8]) -> Option<png::Image> {
    let source = ico::decode_ico_preferred(bytes, TARGET_SIZE).ok()?;
    const TARGET_SIZE: u32 = 48;
    Some(png::Image {
        width: TARGET_SIZE,
        height: TARGET_SIZE,
        pixels: scale_rgba(&source, TARGET_SIZE, TARGET_SIZE),
    })
}

struct CursorImage {
    image: png::Image,
    hotspot: (i32, i32),
}

impl CursorImage {
    fn load(bytes: &[u8]) -> Option<Self> {
        let (image, hotspot) = ico::decode_cur_preferred(bytes, 32).ok()?;
        Some(Self { image, hotspot: (i32::from(hotspot.0), i32::from(hotspot.1)) })
    }
}

impl Shell {
    fn load() -> Option<Self> {
        let mut model = ShellModel::new(Theme::aero());
        model.files = match nt61_desktop::file::FileSystem::mounted(
            nt61_desktop::file::Backend(nt61_desktop::platform::services::transfer),
        ) {
            Ok(files) => files,
            Err(_) => {
                arch::debug_str("[EXPLORER] system volume mount failed\r\n");
                return None;
            }
        };
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
            hovered_caption: None,
            hovered_application: None,
            scene: core::cell::RefCell::new(nt61_desktop::compositor::scene::WindowScene::new(1, 1)),
            window_dirty: core::cell::Cell::new(None),
            scene_dirty: core::cell::Cell::new(true),
            computer: scaled_icon(resources_data::COMPUTER)?,
            network: scaled_icon(resources_data::NETWORK)?,
            recycle_bin: scaled_icon(resources_data::RECYCLE_BIN)?,
            control_panel: scaled_icon(resources_data::CONTROL_PANEL)?,
            document: scaled_icon(resources_data::DOCUMENT)?,
            folder: scaled_icon(resources_data::FOLDER)?,
            avatar: {
                let decoded_avatar = bmp::decode_bmp(USER.bytes).ok()?;
                png::Image {
                    width: decoded_avatar.width,
                    height: decoded_avatar.height,
                    pixels: decoded_avatar.pixels,
                }
            },
            cursor: CursorImage::load(CURSOR_CUR)?,
            resize_cursors: [
                CursorImage::load(include_bytes!("../../../assets/windows7/cursors/aero_ew.cur"))?,
                CursorImage::load(include_bytes!("../../../assets/windows7/cursors/aero_ns.cur"))?,
                CursorImage::load(include_bytes!("../../../assets/windows7/cursors/aero_nwse.cur"))?,
                CursorImage::load(include_bytes!("../../../assets/windows7/cursors/aero_nesw.cur"))?,
            ],
            cursor_kind: WindowCursor::Arrow,
            start_button_normal: png::decode_png(START_BUTTON_NORMAL_PNG).ok()?,
            start_button_hover: png::decode_png(START_BUTTON_HOVER_PNG).ok()?,
            start_button_pressed: png::decode_png(START_BUTTON_PRESSED_PNG).ok()?,
            wallpaper_asset: decoded_wallpaper()?,
            fonts: font_view::FontPack::new(nt61_desktop::shell::layout::UI_FONT_SIZE)?,
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
        glass.saturate(aero::GLASS_SATURATION);
        glass.tint(aero::GLASS_TINT, aero::GLASS_TINT_STRENGTH);
        self.wallpaper = wallpaper;
        self.glass = glass;
        self.scene = core::cell::RefCell::new(nt61_desktop::compositor::scene::WindowScene::new(width, height));
        self.scene_dirty.set(true);
    }

    fn mark_window_dirty(&self,id:u32) {
        if self.window_dirty.get().is_some_and(|previous|previous!=id) {self.scene_dirty.set(true);}
        self.window_dirty.set(Some(id));
    }

    /// A cheap topology key catches taskbar/menu actions that change windows
    /// without invalidating the composed scene for every popup button click.
    fn window_geometry(&self) -> u64 {
        let mut key = self.model.selected_window.unwrap_or(0) as u64;
        for window in &self.model.taskbar.windows {
            for value in [window.id, window.x as u32, window.y as u32,
                window.width as u32, window.height as u32, window.minimized as u32] {
                key = key.wrapping_mul(0x100000001b3) ^ value as u64;
            }
        }
        key
    }

    fn draw_desktop(
        &self,
        framebuffer: &arch::DesktopFramebufferInfo,
        frame: &mut nt61_desktop::graphics::Surface,
        output_pixels: &mut [u8],
    ) {
        use nt61_desktop::compositor::scene::Repaint;
        let plan=self.scene.borrow_mut().begin(frame,self.scene_dirty.replace(false),self.window_dirty.take());
        if plan != Repaint::None {
            if plan == Repaint::All {
                frame.copy_from(&self.wallpaper);
                self.draw_icons(frame, framebuffer);
            }
            self.draw_windows(frame,plan);
            self.scene.borrow_mut().finish(frame);
        }
        self.draw_taskbar(frame, framebuffer);
        if self.model.start_menu.open {
            self.draw_start_menu(frame, framebuffer);
        }
        self.draw_context_menu(frame, framebuffer);
        self.draw_flyout(frame, framebuffer);
        present_surface_with(frame, output_pixels);
    }

    fn current_cursor(&self) -> &CursorImage {
        match self.cursor_kind {
            WindowCursor::Arrow => &self.cursor,
            WindowCursor::Horizontal => &self.resize_cursors[0],
            WindowCursor::Vertical => &self.resize_cursors[1],
            WindowCursor::NorthWestSouthEast => &self.resize_cursors[2],
            WindowCursor::NorthEastSouthWest => &self.resize_cursors[3],
        }
    }

    fn cursor_bounds(&self, x: i32, y: i32) -> nt61_desktop::graphics::Rect {
        let cursor = self.current_cursor();
        nt61_desktop::graphics::Rect::new(x - cursor.hotspot.0, y - cursor.hotspot.1,
            cursor.image.width, cursor.image.height)
    }

    fn draw_cursor(&self, base: &nt61_desktop::graphics::Surface, x: i32, y: i32, scratch: &mut [u8]) {
        let bounds = self.cursor_bounds(x, y);
        present_cursor_region(base, &self.current_cursor().image, bounds.x, bounds.y, scratch);
    }

    fn update_cursor(&self, base: &nt61_desktop::graphics::Surface, old: nt61_desktop::graphics::Rect,
        x: i32, y: i32, output: &mut [u8], scratch: &mut [u8]) {
        present_surface_region(base, output, old);
        self.draw_cursor(base, x, y, scratch);
    }

    fn draw_icons(&self, frame: &mut nt61_desktop::graphics::Surface, _fb: &arch::DesktopFramebufferInfo) {
        use nt61_desktop::{graphics::Rect, resources::ImageResource};
        for (index, item) in self.model.desktop.items.iter().enumerate() {
            let icon = match item.icon {
                ImageResource::Computer => &self.computer,
                ImageResource::Network => &self.network,
                ImageResource::RecycleBin => &self.recycle_bin,
                ImageResource::ControlPanel => &self.control_panel,
                ImageResource::Folder => &self.folder,
                _ => &self.document,
            };
            let bounds = item.bounds();
            let selected = self.model.desktop.selected.contains(&index);
            let hot = self.model.controls.hot() == CONTROL_DESKTOP_BASE + index as u32;
            // Paint the translucent selection behind both icon and label.
            if selected || hot { aero::selection(frame, bounds, selected, hot); }
            let size = icon.width.min(icon.height).min(ICON_CELL);
            let x = bounds.x + (bounds.width - size) as i32 / 2;
            blit_rgba(frame, &icon.pixels, icon.width as usize * 4, Rect::new(x, bounds.y + 4, size, size));
            let text_x = bounds.x + bounds.width.saturating_sub(self.fonts.measure(&item.name)) as i32 / 2;
            self.fonts.draw_labelled_onto(frame, &item.name, text_x, bounds.y + 68);
        }
    }

    fn draw_windows(&self, frame: &mut nt61_desktop::graphics::Surface, plan: nt61_desktop::compositor::scene::Repaint) {
        use nt61_desktop::compositor::scene::Repaint;
        let environment = self.model.application_environment();
        let mut windows: alloc::vec::Vec<_> = self.model.taskbar.windows.iter().collect();
        windows.sort_by_key(|w| (Some(w.id) == self.model.selected_window, w.id));
        let foremost=windows.iter().rev().find(|w|!w.minimized).map(|w|w.id);
        for window in windows {
            if window.minimized || matches!(plan,Repaint::Window(id) if id!=window.id) {
                continue;
            }
            if plan==Repaint::All && foremost==Some(window.id) {
                self.scene.borrow_mut().save_beneath(frame,window.id);
            }
            let x = window.x;
            let y = window.y;
            let width = window.width.max(1) as u32;
            let height = window.height.max(1) as u32;
            let bounds = nt61_desktop::graphics::Rect::new(x as i32, y as i32, width, height);
            aero::shadow(frame, bounds, 8);
            aero::window_glass_frame(frame, bounds, &self.glass,
                Some(window.id) == self.model.selected_window);
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
                x as i32 + 6,
                y as i32 + 30,
                width.saturating_sub(12),
                height.saturating_sub(36),
            );
            aero::window_body(frame, body);
            match window.kind {
                nt61_desktop::shell::WindowKind::Application(_) => {
                    if let Some(app)=self.model.application(window.id) {
                        nt61_desktop::applications::view::draw(app,frame,body,&environment,
                            |frame,text,b,color|self.fonts.draw_fit(frame,text,b.x,b.y,b.width,color));
                    }
                }
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
                    if let Some(state)=self.model.explorers.iter().find(|e|e.window_id==window.id) {
                        nt61_desktop::applications::explorer::draw(state,&self.model.files,frame,body,
                            |frame,text,b,color|self.fonts.draw_fit(frame,text,b.x,b.y,b.width,color));
                    }
                }
                nt61_desktop::shell::WindowKind::CommandPrompt => {
                    if let Some(app)=self.model.application(window.id) {
                        nt61_desktop::applications::view::draw(app,frame,body,&environment,
                            |frame,text,b,color|self.fonts.draw_fit(frame,text,b.x,b.y,b.width,color));
                    }else {self.draw_command_prompt(frame, body);}
                }
                nt61_desktop::shell::WindowKind::ShellSettings(kind) => {
                    self.draw_shell_settings(frame, body, kind);
                }
                nt61_desktop::shell::WindowKind::ControlPanel
                | nt61_desktop::shell::WindowKind::RecycleBin
                | nt61_desktop::shell::WindowKind::System
                | nt61_desktop::shell::WindowKind::ComputerManagement
                | nt61_desktop::shell::WindowKind::RecycleBinProperties
                | nt61_desktop::shell::WindowKind::ConfirmEmptyRecycleBin
                | nt61_desktop::shell::WindowKind::DisplaySettings
                | nt61_desktop::shell::WindowKind::Personalization => {
                    self.draw_namespace_window(frame, body, window.kind);
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
            aero::window_border(frame, bounds, 6);
            let title_x = window.x + 10;
            let title_y = window.y + 7;
            let title_width = width.saturating_sub(120);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                self.fonts.draw_fit(frame, &window.title, title_x + dx, title_y + dy,
                    title_width, 0x90FFFFFF);
            }
            self.fonts.draw_fit(frame, &window.title, title_x, title_y,
                title_width, 0xFF102030);
            aero::window_control_group(frame, window.caption_control_group(),
                Some(window.id) == self.model.selected_window);
            for (index, (bounds, kind)) in window.caption_controls().into_iter().zip([
                aero::WindowControlButtonKind::Minimize,
                if window.maximized { aero::WindowControlButtonKind::Restore }
                    else { aero::WindowControlButtonKind::Maximize },
                aero::WindowControlButtonKind::Close,
            ]).enumerate() {
                aero::window_control_button(frame, bounds, kind,
                    self.hovered_caption == Some((window.id, index as u8)));
            }
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

    fn draw_command_prompt(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        body: nt61_desktop::graphics::Rect,
    ) {
        frame.fill_rect(body.x, body.y, body.width, body.height, 0xFF0C_0C0C);
        self.fonts.draw_onto(
            frame,
            "Microsoft Windows [Version 6.1.7601]\r\nC:\\Users\\Administrator>",
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

    fn draw_context_menu(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        framebuffer: &arch::DesktopFramebufferInfo,
    ) {
        let Some(ref menu) = self.model.context_menu else {
            return;
        };
        let bounds = self
            .model
            .context_menu_bounds(framebuffer.width, framebuffer.height)
            .unwrap_or(nt61_desktop::graphics::Rect::new(menu.x, menu.y, 160, 100));
        let items = self.model.context_menu_items();
        aero::menu_surface(frame, bounds);
        for (index, item) in items.iter().enumerate() {
            let Some(row) = self.model.context_item_bounds(index, framebuffer.width, framebuffer.height) else { continue; };
            if item.0.is_empty() {
                frame.fill_rect(row.x+29,row.y+3,row.width.saturating_sub(32),1,0xFFD7D7D7);
                frame.fill_rect(row.x+29,row.y+4,row.width.saturating_sub(32),1,0xFFFFFFFF);
                continue;
            }
            let enabled = self.model.context_item_enabled(index);
            let id = nt61_desktop::shell::CONTROL_CONTEXT_MENU_BASE + index as u32;
            let highlighted = enabled && (self.model.controls.hot() == id || self.model.controls.focus() == id);
            aero::menu_item(frame, row, highlighted);
            self.fonts.draw_color(frame,item.0,row.x+32,row.y+4, if enabled { 0xFF111111 } else { 0xFF8C8C8C });
            if self.model.context_submenu_group(index).is_some() {
                aero::arrow_right(frame, nt61_desktop::graphics::Rect::new(row.x+row.width as i32-13,row.y+8,5,8), 0xFF111111);
            }
        }
        if let Some(submenu) = self
            .model
            .context_submenu_bounds(framebuffer.width, framebuffer.height)
        {
            let group = self.model.context_submenu.map(|submenu| submenu.group);
            let items = ShellModel::submenu_items(group.unwrap());
            aero::menu_surface(frame, submenu);
            for (index, item) in items.iter().enumerate() {
                let item_y = submenu.y + 4 + index as i32 * 28;
                let item_bounds =
                    nt61_desktop::graphics::Rect::new(submenu.x + 4, item_y, submenu.width - 8, 28);
                let enabled = ShellModel::submenu_item_enabled(group.unwrap(), index);
                let highlighted = enabled && self.model.controls.hot() == CONTROL_SUBMENU_BASE + index as u32;
                aero::menu_item(frame, item_bounds, highlighted);
                let checked = match group.unwrap() {
                    nt61_desktop::shell::MenuGroup::DesktopView => index == 1,
                    nt61_desktop::shell::MenuGroup::DesktopSortBy => match self.model.desktop.sort_mode {
                        nt61_desktop::shell::SortMode::Name => index == 0,
                        nt61_desktop::shell::SortMode::Type => index == 2,
                        _ => false,
                    },
                    _ => false,
                };
                if checked {
                    for n in 0..3 {
                        frame.fill_rect(submenu.x + 11 + n, item_y + 12 + n, 2, 2, 0xFF111111);
                    }
                    for n in 0..6 {
                        frame.fill_rect(submenu.x + 14 + n, item_y + 14 - n, 2, 2, 0xFF111111);
                    }
                }
                self.fonts.draw_color(
                    frame,
                    item,
                    submenu.x + 35,
                    item_y + 7,
                    if enabled { 0xFF111111 } else { 0xFF8C8C8C },
                );
            }
        }
    }
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
        ((visible_width - 1) << 16) / (target_width - 1) as u64
    } else {
        0
    };
    let y_ratio: u64 = if target_height > 1 {
        ((visible_height - 1) << 16) / (target_height - 1) as u64
    } else {
        0
    };
    for row in 0..target_height as usize {
        let source_y = offset_y + ((row as u64 * y_ratio) >> 16);
        let y_frac = (((row as u64 * y_ratio) & 0xFFFF) >> 8) as u32;
        let y0 = source_y as usize;
        let y1 = (y0 + 1).min(source.height as usize - 1);
        for column in 0..target_width as usize {
            let source_x = offset_x + ((column as u64 * x_ratio) >> 16);
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
                pixel[channel] = result.min(255) as u8;
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

fn scale_rgba(source: &png::Image, target_width: u32, target_height: u32) -> alloc::vec::Vec<u8> {
    if source.width == 0 || source.height == 0 {
        return alloc::vec![0u8; (target_width * target_height * 4) as usize];
    }
    let mut output = alloc::vec![0u8; (target_width * target_height * 4) as usize];
    let source_width = source.width as usize;
    let source_height = source.height as usize;
    let target_width = target_width as usize;
    let target_height = target_height as usize;
    for target_row in 0..target_height {
        let row_start = (target_row * source_height) / target_height;
        let row_end = ((target_row + 1) * source_height) / target_height;
        for target_column in 0..target_width {
            let column_start = (target_column * source_width) / target_width;
            let column_end = ((target_column + 1) * source_width) / target_width;
            let mut red = 0u64;
            let mut green = 0u64;
            let mut blue = 0u64;
            let mut alpha = 0u64;
            for source_row in row_start..row_end {
                for source_column in column_start..column_end {
                    let offset = (source_row * source_width + source_column) * 4;
                    let source_alpha = source.pixels[offset + 3] as u64;
                    alpha += source_alpha;
                    red += source.pixels[offset] as u64 * source_alpha;
                    green += source.pixels[offset + 1] as u64 * source_alpha;
                    blue += source.pixels[offset + 2] as u64 * source_alpha;
                }
            }
            let count = (row_end - row_start) * (column_end - column_start);
            let target_offset = (target_row * target_width + target_column) * 4;
            if alpha == 0 || count == 0 {
                continue;
            }
            output[target_offset] = (red / alpha).min(255) as u8;
            output[target_offset + 1] = (green / alpha).min(255) as u8;
            output[target_offset + 2] = (blue / alpha).min(255) as u8;
            output[target_offset + 3] = (alpha / count as u64) as u8;
        }
    }
    output
}

fn lerp(a: u8, b: u8, t: u32) -> u32 {
    ((u32::from(a) * (255 - t) + u32::from(b) * t) + 128) / 255
}

fn lerp_u32(a: u32, b: u32, t: u32) -> u32 {
    ((a * (255 - t) + b * t) + 128) / 255
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
    if let Some(damage) = nt61_desktop::graphics::present::update_rgba(surface, pixels) {
        present_surface_region(surface, pixels, damage);
    }
}

fn present_surface_region(surface: &nt61_desktop::graphics::Surface, pixels: &mut [u8], region: nt61_desktop::graphics::Rect) {
    let stride = surface.width() as usize * 4;
    if pixels.len() < stride * surface.height() as usize { return; }
    let screen = nt61_desktop::graphics::Rect::new(0, 0, surface.width(), surface.height());
    let Some(clip) = screen.intersection(&region) else { return; };
    let offset = clip.y as usize * stride + clip.x as usize * 4;
    // SAFETY: clipped rectangle is in the persistent staging allocation;
    // draw_rgba reads only the visible width on each row at the supplied stride.
    unsafe { arch::draw_rgba(pixels.as_ptr().add(offset), clip.x as u32, clip.y as u32, clip.width, clip.height, stride); }
}

fn present_cursor_region(base: &nt61_desktop::graphics::Surface, cursor: &png::Image, x: i32, y: i32, scratch: &mut [u8]) {
    use nt61_desktop::graphics::{Rect, present::cursor_over};
    let bounds = Rect::new(x, y, cursor.width, cursor.height);
    let screen = Rect::new(0, 0, base.width(), base.height());
    let Some(clip) = screen.intersection(&bounds) else { return; };
    let stride = clip.width as usize * 4;
    if scratch.len() < stride * clip.height as usize { return; }
    for row in 0..clip.height {
        for column in 0..clip.width {
            let (dx, dy) = (clip.x as u32 + column, clip.y as u32 + row);
            let source = (((dy as i32 - y) as u32 * cursor.width + (dx as i32 - x) as u32) * 4) as usize;
            let target = row as usize * stride + column as usize * 4;
            scratch[target..target + 4].copy_from_slice(&cursor_over(base.pixel(dx, dy).unwrap_or(0), &cursor.pixels[source..source+4]));
        }
    }
    // SAFETY: scratch holds the complete clipped cursor rectangle.
    unsafe { arch::draw_rgba(scratch.as_ptr(), clip.x as u32, clip.y as u32, clip.width, clip.height, stride); }
}

fn draw_fallback_desktop(framebuffer: &arch::DesktopFramebufferInfo) {
    let mut surface = nt61_desktop::graphics::Surface::new(framebuffer.width, framebuffer.height);
    draw_wallpaper_into(&mut surface, framebuffer.width, framebuffer.height);
    let mut glass = surface.clone();
    glass.box_blur(aero::GLASS_BLUR_RADIUS);
    glass.saturate(aero::GLASS_SATURATION);
    glass.tint(aero::GLASS_TINT, aero::GLASS_TINT_STRENGTH);
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

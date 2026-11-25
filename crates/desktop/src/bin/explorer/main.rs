#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![deny(warnings)]

mod arch;
mod chrome;
#[path = "../../font_view.rs"]
mod font_view;
mod ico;
use nt61_desktop::png;
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
use core::panic::PanicInfo;
use nt61_desktop::bmp;
use nt61_desktop::compositor::aero;
use nt61_desktop::controls::PointerAction;
use nt61_desktop::graphics::blit::blit_rgba;
use nt61_desktop::shell::{
    ShellModel, WindowCursor, CONTROL_DESKTOP_BASE, CONTROL_SUBMENU_BASE, TASKBAR_HEIGHT,
};
use nt61_desktop::theme::Theme;
use nt61_desktop::user_account_pictures::USER;

extern crate alloc;

#[cfg(not(test))]
use nt61_desktop::user_allocator::ApplicationHeap;

#[cfg(not(test))]
#[global_allocator]
// Keep the complete PE image below the loader's 128 MiB limit. The
// framebuffer/cache budget must fit here as well as the decoded shell assets.
static EXPLORER_ALLOCATOR: ApplicationHeap<{ 112 * 1024 * 1024 }> = ApplicationHeap::new();

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
    wallpaper_preference: nt61_desktop::personalization::Preference,
    wallpaper_revision: u64,
    session_background: nt61_desktop::graphics::Surface,
    fonts: font_view::FontPack,
    model: ShellModel,
    wallpaper: nt61_desktop::graphics::Surface,
    glass: nt61_desktop::graphics::Surface,
    window_glass: core::cell::RefCell<aero::WindowGlassCache>,
    hovered_caption: Option<(u32, u8)>,
    hovered_application: Option<u32>,
    scene: core::cell::RefCell<nt61_desktop::compositor::scene::WindowScene>,
    window_dirty: core::cell::Cell<Option<u32>>,
    scene_dirty: core::cell::Cell<bool>,
    hover_damage: core::cell::Cell<Option<nt61_desktop::graphics::Rect>>,
    task_menu_overlay: core::cell::Cell<bool>,
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

    if nt61_desktop::platform::services::display_preference("restore").is_ok() {
        arch::desktop_info(&mut framebuffer);
    }
    let mut frame = nt61_desktop::graphics::Surface::new(framebuffer.width, framebuffer.height);
    shell.update_wallpaper();
    shell.prepare_aero(framebuffer.width, framebuffer.height);
    shell.model.refresh_controls(framebuffer.width, framebuffer.height);
    shell.model.refresh_desktop_files();
    if shell.model.show_boot_memory_result() { arch::debug_str("[MEMORY-RESULT] desktop notification shown\r\n"); }
    shell.model.update_clock(arch::shell_time());
    let devices = arch::shell_devices();
    shell.model.network_adapters = (devices >> 8) as u32;
    shell.model.audio_available = devices & 1 != 0;
    let mut next_status = 0u64;
    let mut mouse = arch::DesktopMouseState::default();
    let mut previous_buttons = 0u32;
    let mut cursor_x = (framebuffer.width / 2) as i32;
    let mut cursor_y = (framebuffer.height / 2) as i32;
    shell.model.set_pointer_position(cursor_x, cursor_y);
    let mut previous_cursor_x = cursor_x;
    let mut previous_cursor_y = cursor_y;
    let mut cursor_rgba = alloc::vec![0u8; 32 * 32 * 4];
    let mut previous_cursor_bounds = shell.cursor_bounds(cursor_x, cursor_y);
    let mut last_desktop_click: Option<(usize, u64)> = None;
    let mut next_explorer_job = 0usize;
    let mut next_directory_poll = 0u64;
    let mut next_process_poll = 0u64;
    let mut telemetry_dirty = false;
    let mut directory_poll_index = 0usize;
    let mut session_decoder=nt61_desktop::applications::explorer::input::Decoder::default();
    let mut display_decoder=nt61_desktop::applications::explorer::input::Decoder::default();
    let mut secure_desktop=false;
    let mut committed_power:Option<(nt61_desktop::shell::PowerAction,u64)>=None;
    let mut frame_rgba = alloc::vec![0u8; frame.pixels().len() * 4];
    shell.compose_desktop(&framebuffer, &mut frame);
    fade_desktop(&frame, &mut frame_rgba, 450);
    shell.draw_cursor(&frame, cursor_x, cursor_y, &mut cursor_rgba);
    shell
        .model
        .refresh_controls(framebuffer.width, framebuffer.height);
    arch::debug_str("[EXPLORER] desktop frame ready\r\n");
    loop {
        let mut redraw = core::mem::take(&mut telemetry_dirty);
        let ticks = arch::shell_ticks();
        let previous_display_second = shell.model.display.now / 1000;
        if shell.model.display_tick(ticks) || shell.model.display.previous.is_some() && ticks / 1000 != previous_display_second {
            shell.scene_dirty.set(true); redraw = true;
        }
        if shell.model.taskbar.windows.iter().any(|w|w.kind==nt61_desktop::shell::WindowKind::DisplaySettings) {
            shell.model.load_display_modes();
        }
        let secure=matches!(shell.model.session.screen,nt61_desktop::session::Screen::Users|nt61_desktop::session::Screen::Security|nt61_desktop::session::Screen::Credentials|nt61_desktop::session::Screen::ChangePassword|nt61_desktop::session::Screen::Progress);
        if secure!=secure_desktop{if nt61_desktop::platform::services::session(if secure{"secure"}else{"unsecure"},&[]).is_ok(){secure_desktop=secure;}}
        if let Some((action,due))=committed_power.filter(|(_,due)|ticks>=*due) {
            let _=due;committed_power=None;
            use nt61_desktop::{platform::services,session::Screen,shell::PowerAction};
            let operation=match action {PowerAction::LogOff=>"logoff",PowerAction::Restart=>"restart",_=>"shutdown"};
            let result=services::session(operation,&[]);
            if action==PowerAction::LogOff && result.is_ok(){
                shell.model.session.logged_off=true;shell.model.session.locked=true;shell.model.session.switching=true;shell.model.session.select_users();
            }else{shell.model.session.show(Screen::ShutDown);shell.model.session.message=alloc::format!("The operation could not be completed (0x{:08X}).",result.err().unwrap_or(0));}
            shell.scene_dirty.set(true);redraw=true;
        }
        if let Ok(keys)=nt61_desktop::platform::services::desktop_hotkeys(){if keys!=0{shell.model.session_hotkeys(keys);session_decoder=Default::default();shell.scene_dirty.set(true);redraw=true;}}
        if shell.model.session.screen!=nt61_desktop::session::Screen::Desktop {
            if let Some(key)=session_decoder.expire(ticks){let intent=shell.model.session.key(key);shell.model.session_intent(intent);shell.scene_dirty.set(true);redraw=true;}
        }
        let display_focused = shell.model.taskbar.windows.iter().any(|w|
            Some(w.id) == shell.model.selected_window && shell.model.control_browser(w.id).is_some());
        if display_focused {
            if let Some(key) = display_decoder.expire(ticks) {
                let geometry = shell.window_geometry();
                if let Some(id)=shell.model.selected_window {
                    if !shell.model.control_key(id,key) { shell.model.handle_window_key(key); }
                    shell.mark_window_dirty(id);
                }
                if shell.window_geometry() != geometry { shell.scene_dirty.set(true); }
                redraw = true;
            }
        } else {
            display_decoder = Default::default();
        }
        if shell.model.poll_guest_dialogs() {
            shell.scene_dirty.set(true);
            redraw = true;
        }
        for app in &mut shell.model.applications {
            app.input_time = ticks;
        }
        for explorer in &mut shell.model.explorers {
            explorer.input_time = Some(ticks);
        }
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
            if unsafe { arch::poll_mouse(&mut mouse) == 0 } {
                break;
            }
            cursor_x = mouse.x.clamp(0, framebuffer.width.saturating_sub(1) as i32);
            cursor_y = mouse.y.clamp(0, framebuffer.height.saturating_sub(1) as i32);
            shell.model.set_pointer_position(cursor_x, cursor_y);
            if shell.model.session.screen!=nt61_desktop::session::Screen::Desktop {
                if mouse.buttons&1!=0 && previous_buttons&1==0 {let intent=shell.model.session.hit(cursor_x,cursor_y,framebuffer.width,framebuffer.height);shell.model.session_intent(intent);shell.scene_dirty.set(true);pointer_full=true;}
                pointer_moved|=cursor_x!=previous_cursor_x||cursor_y!=previous_cursor_y;previous_buttons=mouse.buttons;continue;
            }
            if shell.model.desktop_editor.is_some() || !shell.model.desktop_error.is_empty() {
                if mouse.buttons & 1 != 0 && previous_buttons & 1 == 0 {
                    shell.model.desktop_editor_click(cursor_x, cursor_y);
                    shell.scene_dirty.set(true); pointer_full = true;
                }
                pointer_moved |= cursor_x != previous_cursor_x || cursor_y != previous_cursor_y;
                previous_buttons = mouse.buttons; continue;
            }
            let previous_scene = shell.window_geometry();
            if mouse.wheel != 0 {
                if let Some(id) = shell.model.window_hit_test(cursor_x, cursor_y) {
                    if shell
                        .model
                        .application_wheel(id, cursor_x, cursor_y, mouse.wheel)
                    {
                        shell.mark_window_dirty(id);
                        pointer_full = true;
                    }
                }
            }
            let action = if mouse.buttons & 1 == 0 && previous_buttons & 1 != 0 {
                Some(PointerAction::Up)
            } else if mouse.buttons & 1 != 0 && previous_buttons & 1 == 0 {
                Some(PointerAction::Down)
            } else {
                Some(PointerAction::Move)
            };
            // Retain the composed scene for pointer motion. Invalidate below
            // only when a window, application or desktop icon actually changes.
            if mouse.buttons & 2 != 0
                && previous_buttons & 2 == 0
                && shell
                    .model
                    .window_hit_test(cursor_x, cursor_y)
                    .is_some_and(|id| {
                        shell.model.application(id).is_some()
                            || shell.model.explorers.iter().any(|e| e.window_id == id)
                    })
            {
                if let Some(id) = shell.model.window_hit_test(cursor_x, cursor_y) {
                    shell
                        .model
                        .application_pointer(id, cursor_x, cursor_y, true, false, true);
                }
                shell.scene_dirty.set(true);
                pointer_full = true;
                previous_buttons = mouse.buttons;
                continue;
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
                let over_window = shell.model.fullscreen_photo().is_some()
                    || shell.model.captured_pointer_window().is_some()
                    || (!popup_over
                        && cursor_y < framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32
                        && shell.model.window_hit_test(cursor_x, cursor_y).is_some());
                let hovered_application = if over_window && shell.model.drag_window.is_none() {
                    shell.model.window_hit_test(cursor_x, cursor_y)
                } else {
                    None
                };
                if shell.hovered_application != hovered_application {
                    if let Some(id) = shell.hovered_application {
                        if shell.model.clear_application_hover(id) {
                            shell.mark_window_dirty(id);
                            pointer_full = true;
                        }
                    }
                    shell.hovered_application = hovered_application;
                }
                let hovered_caption = if over_window && shell.model.drag_window.is_none() && shell.model.fullscreen_photo().is_none() {
                    shell
                        .model
                        .window_hit_test(cursor_x, cursor_y)
                        .and_then(|id| shell.model.taskbar.windows.iter().find(|w| w.id == id))
                        .and_then(|w| {
                            w.caption_control_at(cursor_x, cursor_y)
                                .map(|button| (w.id, button))
                        })
                } else {
                    None
                };
                if shell.hovered_caption != hovered_caption {
                    if let Some((id, _)) = shell.hovered_caption {
                        shell.mark_window_dirty(id);
                    }
                    if let Some((id, _)) = hovered_caption {
                        shell.mark_window_dirty(id);
                    }
                    shell.hovered_caption = hovered_caption;
                    pointer_full = true;
                }
                if action == PointerAction::Down && over_window {
                    if let Some(window_id) = shell.model.window_hit_test(cursor_x, cursor_y) {
                        let browser_content = Some(window_id) == shell.model.selected_window
                            && shell.model.control_browser(window_id).is_some()
                            && shell.model.taskbar.windows.iter().find(|w|w.id==window_id)
                                .is_some_and(|w|nt61_desktop::control_browser::body(w).contains(cursor_x,cursor_y));
                        if browser_content { shell.mark_window_dirty(window_id); }
                        else { shell.scene_dirty.set(true); }
                        if !shell.model.handle_window_control_click(
                            window_id,
                            cursor_x,
                            cursor_y,
                            framebuffer.width,
                            framebuffer.height.saturating_sub(TASKBAR_HEIGHT),
                        ) {
                            shell.model.select_window(window_id);
                            if !shell.model.begin_window_drag(window_id, cursor_x, cursor_y) {
                                shell.model.application_pointer(
                                    window_id, cursor_x, cursor_y, true, true, false,
                                );
                            }
                        }
                    }
                }
                if action == PointerAction::Down && over_window && shell.model.drag_window.is_none()
                {
                    if let Some(id) = shell.model.window_hit_test(cursor_x, cursor_y) {
                        if shell.model.needs_capture(id, cursor_x, cursor_y) {
                            let hidden =
                                shell.model.taskbar.windows.iter().position(|w| w.id == id);
                            if let Some(index) = hidden {
                                shell.model.taskbar.windows[index].minimized = true;
                                shell.scene_dirty.set(true);
                                shell.draw_desktop(&framebuffer, &mut frame, &mut frame_rgba);
                                shell.model.capture_application(id, frame.clone());
                                shell.model.taskbar.windows[index].minimized = false;
                                shell.scene_dirty.set(true);
                            }
                        }
                    }
                }
                if action == PointerAction::Move && shell.model.drag_window.is_none() && over_window
                {
                    if let Some(id) = shell
                        .model
                        .captured_pointer_window()
                        .or_else(|| shell.model.window_hit_test(cursor_x, cursor_y))
                    {
                        let old_hot = shell.model.application(id).and_then(|app| app.hot);
                        if shell.model.application_pointer(
                            id,
                            cursor_x,
                            cursor_y,
                            false,
                            mouse.buttons & 1 != 0,
                            false,
                        ) {
                            let safe_hover = mouse.buttons & 1 == 0
                                && shell.model.application(id).is_some_and(|app| {
                                    !app.menu
                                        && app.manager.menu.is_none()
                                        && app.dialog.is_none()
                                        && !app.confirm
                                        && app.search.is_none()
                                        && !matches!(
                                            app.id,
                                            nt61_desktop::applications::AppId::Notepad
                                                | nt61_desktop::applications::AppId::WordPad
                                                | nt61_desktop::applications::AppId::Paint
                                        )
                                });
                            let new_hot = shell.model.application(id).and_then(|app| app.hot);
                            let damage = if safe_hover {
                                union_rect(old_hot, new_hot)
                            } else {
                                None
                            };
                            let previous_damage = shell.hover_damage.get();
                            let already_dirty = shell.window_dirty.get().is_some();
                            shell.mark_window_dirty(id);
                            if !already_dirty || previous_damage.is_some() {
                                shell.hover_damage.set(union_rect(previous_damage, damage));
                            }
                            pointer_full = true;
                        }
                    }
                }
                if matches!(action, PointerAction::Move | PointerAction::Up)
                    && shell.model.drag_window.is_some()
                    && shell.model.drag_window_to(cursor_x, cursor_y)
                {
                    shell.scene_dirty.set(true);
                    pointer_full = true;
                }
                if action == PointerAction::Up {
                    let was_dragging = shell.model.drag_window.is_some();
                    if shell.model.drag_window.is_none() {
                        if let Some(id) = shell
                            .model
                            .captured_pointer_window()
                            .or(shell.model.selected_window)
                        {
                            if shell
                                .model
                                .application_pointer(id, cursor_x, cursor_y, false, false, false)
                            {
                                shell.mark_window_dirty(id);
                            }
                        }
                    }
                    shell.model.release_window_drag();
                    if was_dragging {
                        shell.scene_dirty.set(true);
                        pointer_full = true;
                    }
                }
                let previous_hot = shell.model.controls.hot();
                let changed = match shell.model.controls.route_pointer(
                    action,
                    if over_window || shell.model.drag_window.is_some() {
                        -1
                    } else {
                        cursor_x
                    },
                    cursor_y,
                ) {
                    Some(event) => {
                        if let nt61_desktop::controls::ControlEvent::Activated(id) = event {
                            if (CONTROL_DESKTOP_BASE..nt61_desktop::shell::CONTROL_MENU_BASE)
                                .contains(&id)
                            {
                                let index = (id - CONTROL_DESKTOP_BASE) as usize;
                                if last_desktop_click.is_some_and(|(previous, time)| {
                                    previous == index && ticks.saturating_sub(time) <= 500
                                }) {
                                    shell.model.open_desktop_item(index);
                                    last_desktop_click = None;
                                } else {
                                    last_desktop_click = Some((index, ticks));
                                }
                            }
                        }
                        shell.model.handle_control_event(event)
                    }
                    None => false,
                };
                // Popup and taskbar hover changes only their overlay. Repaint
                // the retained desktop when an icon changes its highlight.
                let icon_hot_changed =
                    [previous_hot, shell.model.controls.hot()].iter().any(|id| {
                        (CONTROL_DESKTOP_BASE..nt61_desktop::shell::CONTROL_MENU_BASE).contains(id)
                    });
                if changed
                    && (icon_hot_changed
                        || (action != PointerAction::Move
                            && !popup_over
                            && cursor_y < framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32))
                {
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
            if shell.window_geometry() != previous_scene {
                shell.scene_dirty.set(true);
            }
            previous_buttons = mouse.buttons;
        }
        let cursor_kind = if shell.model.drag_window.is_some()
            || (cursor_y < framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32
                && !shell.model.popup_contains(
                    cursor_x,
                    cursor_y,
                    framebuffer.width,
                    framebuffer.height,
                )) {
            shell.model.window_cursor_at(cursor_x, cursor_y)
        } else {
            WindowCursor::Arrow
        };
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
            let changed = if shell.model.session.screen!=nt61_desktop::session::Screen::Desktop {
                if let Some(decoded)=session_decoder.feed(key,ticks){let decoded=match decoded{nt61_desktop::applications::explorer::input::UP=>0x26,nt61_desktop::applications::explorer::input::DOWN=>0x28,_=>decoded};let intent=shell.model.session.key(decoded);shell.model.session_intent(intent);}
                shell.scene_dirty.set(true);true
            } else if display_focused && shell.model.context_menu.is_none()
                && !shell.model.start_menu.open && shell.model.flyout.is_none() {
                if let Some(decoded) = display_decoder.feed(key, ticks) {
                    let resolution_keys = shell.model.selected_window
                        .and_then(|id| shell.model.control_browser(id))
                        .is_some_and(|browser| browser.current().kind == nt61_desktop::shell::WindowKind::DisplaySettings && !browser.search_focus);
                    let decoded = match decoded {
                        nt61_desktop::applications::explorer::input::UP if resolution_keys => 0x26,
                        nt61_desktop::applications::explorer::input::DOWN if resolution_keys => 0x28,
                        b'&' | b'(' if resolution_keys => 0,
                        _ => decoded,
                    };
                    let geometry = shell.window_geometry();
                    if let Some(id)=shell.model.selected_window {
                        if !shell.model.control_key(id,decoded) { shell.model.handle_window_key(decoded); }
                        shell.mark_window_dirty(id);
                    }
                    if shell.window_geometry() != geometry { shell.scene_dirty.set(true); }
                    true
                } else { false }
            } else if shell.model.desktop_editor.is_some() || !shell.model.desktop_error.is_empty() {
                shell.scene_dirty.set(true);
                if shell.model.desktop_editor.is_some() { shell.model.desktop_editor_key(key) }
                else { if matches!(key, 27 | 13 | 10) { shell.model.desktop_error.clear(); } true }
            } else if shell.model.context_menu.is_some()
                || shell.model.start_menu.open
                || shell.model.flyout.is_some()
            {
                if key == 13 || key == 10 {
                    shell.scene_dirty.set(true);
                }
                shell.model.handle_popup_key(key)
            } else {
                shell.scene_dirty.set(true);
                if (key == 13 || key == 10) && shell.model.selected_window.is_none() {
                    shell
                        .model
                        .desktop
                        .selected
                        .first()
                        .copied()
                        .is_some_and(|index| shell.model.open_desktop_item(index))
                } else if shell.model.selected_window.is_none() {
                    shell.model.handle_shell_key(key)
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
        if shell.model.flush_application_input(ticks) {
            shell.scene_dirty.set(true);
            redraw = true;
        }
        for explorer in &mut shell.model.explorers {
            if explorer.flush_input(&mut shell.model.files) {
                shell.scene_dirty.set(true);
                redraw = true;
            }
        }
        if shell.model.sync_desktop_files() {
            shell.scene_dirty.set(true); redraw = true;
            shell.model.refresh_controls(framebuffer.width, framebuffer.height);
        }
        // Give input priority, then service one cancellable directory-search batch.
        let count = shell.model.explorers.len();
        for step in 0..count {
            let index = (next_explorer_job + step) % count;
            if shell.model.explorers[index].work_pending() {
                shell.model.explorers[index].work(&mut shell.model.files);
                next_explorer_job = (index + 1) % count;
                shell.scene_dirty.set(true);
                redraw = true;
                break;
            }
        }
        // Round-robin volume polling also observes changes made by CMD and
        // other applications. At most one directory is read per interval.
        if ticks >= next_directory_poll {
            next_directory_poll = ticks.saturating_add(250);
            let index = directory_poll_index % (count + 1);
            directory_poll_index = (index + 1) % (count + 1);
            let changed = if index == count { shell.model.refresh_desktop_files() }
                else { shell.model.explorers[index].poll_directory(&mut shell.model.files) };
            if changed {
                shell.scene_dirty.set(true);
                redraw = true;
                shell.model.refresh_controls(framebuffer.width, framebuffer.height);
            }
        }
        if old_audio != (shell.model.volume, shell.model.muted) {
            if arch::shell_audio(shell.model.volume, shell.model.muted)!=0 {
                shell.model.refresh_audio();
                shell.scene_dirty.set(true);redraw=true;
            }
        }
        if shell.model.personalization.work(&mut shell.model.files) {shell.scene_dirty.set(true);redraw=true;}
        if shell.update_wallpaper() {shell.prepare_aero(framebuffer.width,framebuffer.height);redraw=true;}
        let old_identity=shell.model.session.identity.clone();
        let old_session=shell.model.session.screen;
        let closing=shell.model.session.pending_close.is_some() || shell.model.power_action.is_some();
        if let Some(action)=shell.model.advance_session(){committed_power=Some((action,ticks.saturating_add(250)));}
        if old_identity!=shell.model.session.identity {shell.model.personalization.load();}
        if closing || old_session!=shell.model.session.screen {shell.scene_dirty.set(true);redraw=true;shell.model.refresh_controls(framebuffer.width,framebuffer.height);}
        let mut updated = arch::DesktopFramebufferInfo::default();
        arch::desktop_info(&mut updated);
        if updated.width != 0 && updated.height != 0 && (updated.width,updated.height)!=(framebuffer.width,framebuffer.height) {
            framebuffer = updated;
            frame = nt61_desktop::graphics::Surface::new(framebuffer.width,framebuffer.height);
            frame_rgba.resize(frame.pixels().len()*4,0);
            shell.prepare_aero(framebuffer.width,framebuffer.height);
            shell.model.refresh_controls(framebuffer.width,framebuffer.height);
            for window in &mut shell.model.taskbar.windows {
                window.x=window.x.clamp(0,framebuffer.width.saturating_sub(80)as i32);
                window.y=window.y.clamp(0,framebuffer.height.saturating_sub(TASKBAR_HEIGHT+60)as i32);
            }
            cursor_x=cursor_x.clamp(0,framebuffer.width as i32-1);
            cursor_y=cursor_y.clamp(0,framebuffer.height as i32-1);
            previous_cursor_bounds=shell.cursor_bounds(cursor_x,cursor_y);
            redraw=true;
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
        if ticks >= next_process_poll && !key_pending {
            next_process_poll = ticks.saturating_add(500);
            let observers: alloc::vec::Vec<_> = shell
                .model
                .applications
                .iter()
                .filter(|app| {
                    matches!(
                        app.id,
                        nt61_desktop::applications::AppId::TaskManager
                            | nt61_desktop::applications::AppId::SystemInformation
                            | nt61_desktop::applications::AppId::ResourceMonitor
                    )
                })
                .filter(|app| {
                    shell
                        .model
                        .taskbar
                        .windows
                        .iter()
                        .any(|w| w.id == app.window_id && (!w.minimized || (app.id == nt61_desktop::applications::AppId::TaskManager && app.manager.network_active)))
                })
                .map(|app| app.window_id)
                .collect();
            if !observers.is_empty() {
                shell.model.sample_task_manager();
                for id in &observers {shell.mark_window_dirty(*id);}
                // Paint telemetry on the next loop, after input has been consumed.
                telemetry_dirty = true;
                if let Ok(snapshot) = nt61_desktop::platform::services::processes() {
                    if shell.model.process_snapshot.as_ref() != Some(&snapshot) {
                        shell.model.process_snapshot = Some(snapshot);
                        for id in observers {
                            shell.mark_window_dirty(id);
                        }
                        telemetry_dirty = true;
                    }
                }
            }
        }
        if !key_pending && !pointer_moved {
            core::hint::spin_loop();
        }
        previous_cursor_bounds = shell.cursor_bounds(cursor_x, cursor_y);
        previous_cursor_x = cursor_x;
        previous_cursor_y = cursor_y;
    }
}

fn union_rect(
    a: Option<nt61_desktop::graphics::Rect>,
    b: Option<nt61_desktop::graphics::Rect>,
) -> Option<nt61_desktop::graphics::Rect> {
    use nt61_desktop::graphics::Rect;
    match (a, b) {
        (Some(a), Some(b)) => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let right = (a.x + a.width as i32).max(b.x + b.width as i32);
            let bottom = (a.y + a.height as i32).max(b.y + b.height as i32);
            Some(Rect::new(x, y, (right - x) as u32, (bottom - y) as u32))
        }
        (a, b) => a.or(b),
    }
}

fn fade_desktop(frame: &nt61_desktop::graphics::Surface, rgba: &mut [u8], duration: u64) {
    use nt61_desktop::compositor::transition::{incremental_alpha, opacity};
    arch::debug_str("[EXPLORER] desktop fade begin\r\n");
    let start = arch::shell_ticks();
    let mut previous = 0;
    let mut next_frame = start;
    loop {
        let now = arch::shell_ticks();
        if now < next_frame {
            core::hint::spin_loop();
            continue;
        }
        let current = opacity(now.saturating_sub(start), duration);
        let alpha = incremental_alpha(previous, current);
        if alpha > 0 {
            for (pixel, out) in frame.pixels().iter().zip(rgba.chunks_exact_mut(4)) {
                out.copy_from_slice(&[
                    (pixel >> 16) as u8,
                    (pixel >> 8) as u8,
                    *pixel as u8,
                    alpha,
                ]);
            }
            // SAFETY: rgba contains width * height pixels at the supplied stride.
            unsafe {
                arch::draw_rgba(
                    rgba.as_ptr(),
                    0,
                    0,
                    frame.width(),
                    frame.height(),
                    frame.width() as usize * 4,
                );
            }
        }
        previous = current;
        if current == 255 {
            break;
        }
        next_frame = now.saturating_add(16);
    }
    arch::debug_str("[EXPLORER] desktop fade complete\r\n");
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
        Some(Self {
            image,
            hotspot: (i32::from(hotspot.0), i32::from(hotspot.1)),
        })
    }
}

impl Shell {
    fn load() -> Option<Self> {
        let mut model = ShellModel::new(Theme::aero());
        // LogonUI installs the authenticated token before Explorer starts.
        // Resolve it before projecting profile folders or creating windows.
        let user=alloc::string::String::from_utf8(nt61_desktop::platform::services::session("user",&[]).ok()?).ok()?;
        if user.is_empty(){return None;}
        model.session.user=user.clone();
        model.session.identity=user;
        model.personalization.load();
        model.refresh_audio();
        model.files = match nt61_desktop::file::FileSystem::mounted(nt61_desktop::file::Backend(
            nt61_desktop::platform::services::transfer,
        )) {
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
        // Load the default editor face alongside the chrome face. Launching a
        // document and opening Font then reuse the process-local font library.
        model
            .fonts
            .get(nt61_desktop::graphics::font::MONOSPACE_FONT)
            .ok()?;
        Some(Self {
            window_glass: core::cell::RefCell::new(aero::WindowGlassCache::default()),
            hovered_caption: None,
            hovered_application: None,
            scene: core::cell::RefCell::new(nt61_desktop::compositor::scene::WindowScene::new(
                1, 1,
            )),
            window_dirty: core::cell::Cell::new(None),
            scene_dirty: core::cell::Cell::new(true),
            hover_damage: core::cell::Cell::new(None),
            task_menu_overlay: core::cell::Cell::new(false),
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
                CursorImage::load(include_bytes!(
                    "../../../assets/windows7/cursors/aero_ew.cur"
                ))?,
                CursorImage::load(include_bytes!(
                    "../../../assets/windows7/cursors/aero_ns.cur"
                ))?,
                CursorImage::load(include_bytes!(
                    "../../../assets/windows7/cursors/aero_nwse.cur"
                ))?,
                CursorImage::load(include_bytes!(
                    "../../../assets/windows7/cursors/aero_nesw.cur"
                ))?,
            ],
            cursor_kind: WindowCursor::Arrow,
            start_button_normal: png::decode_png(START_BUTTON_NORMAL_PNG).ok()?,
            start_button_hover: png::decode_png(START_BUTTON_HOVER_PNG).ok()?,
            start_button_pressed: png::decode_png(START_BUTTON_PRESSED_PNG).ok()?,
            wallpaper_asset: decoded_wallpaper()?,
            wallpaper_preference: Default::default(),
            wallpaper_revision: u64::MAX,
            session_background: {let asset=png::decode_png(include_bytes!("../../../assets/windows7/wallpapers/login-1024.png")).ok()?;let mut surface=nt61_desktop::graphics::Surface::new(1024,768);cover_image(&asset,&mut surface);surface},
            fonts: font_view::FontPack::with_library(
                nt61_desktop::shell::layout::UI_FONT_SIZE,
                &model.fonts,
            )?,
            model,
            wallpaper: nt61_desktop::graphics::Surface::new(1, 1),
            glass: nt61_desktop::graphics::Surface::new(1, 1),
        })
    }

    fn update_wallpaper(&mut self) -> bool {
        if self.wallpaper_revision==self.model.personalization.revision {return false;}
        let initial=self.wallpaper_revision==u64::MAX;
        self.wallpaper_revision=self.model.personalization.revision;
        let preference=self.model.personalization.current.clone();
        if initial || preference.path!=self.wallpaper_preference.path {
            let asset=self.model.files.resolve(&preference.path).and_then(|id|self.model.files.read_media(id).ok()).and_then(|b|nt61_desktop::applications::photo_viewer::codec::decode(&b,0).ok()).map(|(s,_)| {
                let mut pixels=alloc::vec::Vec::with_capacity(s.pixels().len()*4);
                for p in s.pixels(){pixels.extend([(p>>16)as u8,(p>>8)as u8,*p as u8,(p>>24)as u8]);}
                png::Image{width:s.width(),height:s.height(),pixels}
            });
            let Some(asset)=asset else {
                self.model.personalization.message="This picture could not be loaded.".into();
                self.model.personalization.current=self.wallpaper_preference.clone();
                return false;
            };
            self.wallpaper_asset=asset;
        }
        self.wallpaper_preference=preference;
        true
    }

    fn prepare_aero(&mut self, width: u32, height: u32) {
        if self.session_background.width()!=width||self.session_background.height()!=height {
            if let Ok(asset)=png::decode_png(include_bytes!("../../../assets/windows7/wallpapers/login-1024.png")){let mut surface=nt61_desktop::graphics::Surface::new(width,height);cover_image(&asset,&mut surface);self.session_background=surface;}
        }
        let Some(mut wallpaper) = nt61_desktop::graphics::Surface::try_new(width, height) else {
            return;
        };
        let Some(mut glass) = nt61_desktop::graphics::Surface::try_new(width, height) else {
            return;
        };
        let rendered=if self.wallpaper_preference.position==0 {cover_image(&self.wallpaper_asset,&mut wallpaper)}
            else {nt61_desktop::personalization::render(&self.wallpaper_asset,&mut wallpaper,self.wallpaper_preference.position)};
        if !rendered {
            draw_wallpaper_into(&mut wallpaper, width, height);
        }
        glass.copy_from(&wallpaper);
        glass.box_blur(aero::GLASS_BLUR_RADIUS);
        aero::colorize_glass(&mut glass, aero::GLASS_TINT, aero::GLASS_SATURATION);
        self.wallpaper = wallpaper;
        self.glass = glass;
        self.scene = core::cell::RefCell::new(nt61_desktop::compositor::scene::WindowScene::new(
            width, height,
        ));
        self.window_glass = core::cell::RefCell::new(aero::WindowGlassCache::default());
        self.scene_dirty.set(true);
    }

    fn mark_window_dirty(&self, id: u32) {
        self.hover_damage.set(None);
        if self
            .window_dirty
            .get()
            .is_some_and(|previous| previous != id)
        {
            self.scene_dirty.set(true);
        }
        self.window_dirty.set(Some(id));
    }

    /// A cheap topology key catches taskbar/menu actions that change windows
    /// without invalidating the composed scene for every popup button click.
    fn window_geometry(&self) -> u64 {
        let mut key = self.model.selected_window.unwrap_or(0) as u64;
        key ^= (self.model.fullscreen_photo().is_some() as u64) << 63;
        for window in &self.model.taskbar.windows {
            for value in [
                window.id,
                window.x as u32,
                window.y as u32,
                window.width as u32,
                window.height as u32,
                window.minimized as u32,
            ] {
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
        self.compose_desktop(framebuffer, frame);
        let damage = self.scene.borrow().damage();
        let has_overlay = self.model.session.screen!=nt61_desktop::session::Screen::Desktop || self.model.start_menu.open
            || self.model.context_menu.is_some()
            || self.model.flyout.is_some()
            || self.task_menu_overlay.get();
        if let Some(region) = damage.filter(|_| !has_overlay) {
            use nt61_desktop::graphics::{present::update_rgba_region, Rect};
            let changed = update_rgba_region(frame, output_pixels, region);
            let taskbar = Rect::new(
                0,
                framebuffer.height.saturating_sub(TASKBAR_HEIGHT) as i32,
                framebuffer.width,
                TASKBAR_HEIGHT,
            );
            let changed_taskbar = update_rgba_region(frame, output_pixels, taskbar);
            if let Some(changed) = union_rect(changed, changed_taskbar) {
                present_surface_region(frame, output_pixels, changed);
            }
        } else {
            present_surface_with(frame, output_pixels);
        }
    }

    fn compose_desktop(
        &self,
        framebuffer: &arch::DesktopFramebufferInfo,
        frame: &mut nt61_desktop::graphics::Surface,
    ) {
        use nt61_desktop::compositor::scene::Repaint;
        let damage = self.hover_damage.take();
        let dirty = self.window_dirty.take();
        let selected = self.model.selected_window;
        let task_menu = self.model.applications.iter().any(|a| a.manager.menu.is_some()
            && self.model.taskbar.windows.iter().any(|w|w.id==a.window_id && !w.minimized));
        let previous_task_menu = self.task_menu_overlay.replace(task_menu);
        let all = self.scene_dirty.replace(false) || task_menu || previous_task_menu
            || damage.zip(dirty).is_some_and(|(rect, dirty)| {
                let dirty_order = (Some(dirty) == selected, dirty);
                self.window_glass
                    .borrow()
                    .intersects_blur(rect, |id| (Some(id) == selected, id) > dirty_order)
            });
        let plan = self.scene.borrow_mut().begin_region(
            frame,
            all,
            dirty,
            if all { None } else { damage },
        );
        if plan != Repaint::None {
            if plan == Repaint::All {
                frame.copy_from(&self.wallpaper);
                self.draw_icons(frame, framebuffer);
            }
            self.draw_windows(frame, plan);
            self.scene.borrow_mut().finish(frame);
        }
        frame.set_draw_clip(None);
        self.draw_taskbar(frame, framebuffer);
        if self.model.start_menu.open {
            self.draw_start_menu(frame, framebuffer);
        }
        self.draw_context_menu(frame, framebuffer);
        self.draw_flyout(frame, framebuffer);
        self.draw_desktop_editor(frame);
        if self.model.display.now < self.model.display.identified_until {
            use nt61_desktop::{graphics::Rect, control_browser::paint};
            let r=Rect::new(frame.width() as i32/2-70,frame.height() as i32/2-80,140,160);
            frame.blend_rounded_rect(r,8,0xDD123C70);
            if let Ok(font)=self.model.fonts.get(paint::FONT) {paint::label(&font,frame,"1",Rect::new(r.x+42,r.y+22,80,120),0xFFFFFFFF,96);}
        }
        if let Some(app)=self.model.selected_window.and_then(|id|self.model.application(id)).filter(|app|matches!(&app.state,nt61_desktop::applications::AppState::PhotoViewer(v) if v.slideshow)&&self.model.taskbar.windows.iter().any(|w|w.id==app.window_id&&!w.minimized)) {
            let b=nt61_desktop::graphics::Rect::new(0,0,frame.width(),frame.height());
            nt61_desktop::applications::view::draw(app,frame,b,&self.model.application_environment(),|f,t,r,c|self.fonts.draw_fit(f,t,r.x,r.y,r.width,c));
        }
        nt61_desktop::session::draw_with_background(&self.model.session,frame,Some(&self.session_background),|frame,text,b,color|{if matches!(self.model.session.screen,nt61_desktop::session::Screen::Users|nt61_desktop::session::Screen::Credentials){self.centered(frame,text,b,color);}else{self.label(frame,text,b,color);}});
    }

    fn draw_desktop_editor(&self, frame: &mut nt61_desktop::graphics::Surface) {
        use nt61_desktop::{desktop_files::Editor, graphics::Rect};
        if self.model.desktop_editor.is_none() && self.model.desktop_error.is_empty() { return; }
        let b = self.model.desktop_editor_bounds();
        aero::blend_rounded_rect(frame, Rect::new(b.x-4,b.y-4,b.width+8,b.height+10),6,0x70546D88);
        frame.fill_rect(b.x,b.y,b.width,b.height,0xFFF0F0F0);
        frame.fill_rect(b.x+1,b.y+1,b.width-2,32,0xFFE1ECF7);
        let (title, prompt, value, button) = match &self.model.desktop_editor {
            Some(Editor::Rename { value, .. }) => ("Rename", "Type a name (Ctrl+A to replace):", Some(value.as_str()), "OK"),
            Some(Editor::Shortcut { target, name, naming }) => ("Create Shortcut",
                if *naming { "Type a name for this shortcut:" } else { "Type the location of the item:" },
                Some(if *naming { name.as_str() } else { target.as_str() }), if *naming { "Finish" } else { "Next" }),
            None => ("Desktop", "", None, "OK"),
        };
        self.label(frame,title,Rect::new(b.x+12,b.y+8,b.width-24,20),0xFF173F67);
        self.label(frame,prompt,Rect::new(b.x+16,b.y+52,b.width-32,22),0xFF111111);
        if let Some(value) = value {
            frame.fill_rect(b.x+16,b.y+82,b.width-32,28,0xFF7A9CBF);
            frame.fill_rect(b.x+17,b.y+83,b.width-34,26,0xFFFFFFFF);
            self.label(frame,value,Rect::new(b.x+22,b.y+88,b.width-44,20),0xFF111111);
        }
        self.label(frame,&self.model.desktop_error,Rect::new(b.x+16,b.y+122,b.width-32,48),0xFF9A1515);
        for (offset,label) in [(232,button),(332,"Cancel")] {
            let r=Rect::new(b.x+offset,b.y+178,88,26);
            frame.fill_rect(r.x,r.y,r.width,r.height,0xFF7F9DB9);
            frame.fill_rect(r.x+1,r.y+1,r.width-2,r.height-2,0xFFF7F7F7);
            self.label(frame,label,Rect::new(r.x+18,r.y+5,68,20),0xFF111111);
        }
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
        nt61_desktop::graphics::Rect::new(
            x - cursor.hotspot.0,
            y - cursor.hotspot.1,
            cursor.image.width,
            cursor.image.height,
        )
    }

    fn draw_cursor(
        &self,
        base: &nt61_desktop::graphics::Surface,
        x: i32,
        y: i32,
        scratch: &mut [u8],
    ) {
        let bounds = self.cursor_bounds(x, y);
        present_cursor_region(
            base,
            &self.current_cursor().image,
            bounds.x,
            bounds.y,
            scratch,
        );
    }

    fn update_cursor(
        &self,
        base: &nt61_desktop::graphics::Surface,
        old: nt61_desktop::graphics::Rect,
        x: i32,
        y: i32,
        output: &mut [u8],
        scratch: &mut [u8],
    ) {
        present_surface_region(base, output, old);
        self.draw_cursor(base, x, y, scratch);
    }

    fn draw_icons(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        _fb: &arch::DesktopFramebufferInfo,
    ) {
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
            aero::desktop_selection(frame, bounds, selected, hot);
            let size = icon.width.min(icon.height).min(ICON_CELL);
            let x = bounds.x + (bounds.width - size) as i32 / 2;
            blit_rgba(
                frame,
                &icon.pixels,
                icon.width as usize * 4,
                Rect::new(x, bounds.y + 4, size, size),
            );
            let text_x =
                bounds.x + bounds.width.saturating_sub(self.fonts.measure(&item.name)) as i32 / 2;
            self.fonts
                .draw_labelled_onto(frame, &item.name, text_x, bounds.y + 68);
        }
    }

    fn draw_windows(
        &self,
        frame: &mut nt61_desktop::graphics::Surface,
        plan: nt61_desktop::compositor::scene::Repaint,
    ) {
        use nt61_desktop::compositor::scene::Repaint;
        let environment = self.model.application_environment();
        let refresh_glass = plan == Repaint::All && self.scene.borrow().damage().is_none();
        if refresh_glass {
            self.window_glass.borrow_mut().retain(|id| {
                self.model
                    .taskbar
                    .windows
                    .iter()
                    .any(|window| window.id == id && !window.minimized)
            });
        }
        let mut windows: alloc::vec::Vec<_> = self.model.taskbar.windows.iter().collect();
        windows.sort_by_key(|w| self.model.window_z_order(w.id));
        let foremost = windows.iter().rev().find(|w| !w.minimized).map(|w| w.id);
        for window in windows {
            if window.minimized || matches!(plan,Repaint::Window(id) if id!=window.id) {
                continue;
            }
            if plan == Repaint::All && foremost == Some(window.id) {
                self.scene.borrow_mut().save_beneath(frame, window.id);
            }
            let x = window.x;
            let y = window.y;
            let width = window.width.max(1) as u32;
            let height = window.height.max(1) as u32;
            let bounds = nt61_desktop::graphics::Rect::new(x as i32, y as i32, width, height);
            let shadow_bounds = nt61_desktop::graphics::Rect::new(
                bounds.x - 8,
                bounds.y - 8,
                bounds.width + 16,
                bounds.height + 16,
            );
            if frame
                .drawing_bounds()
                .intersection(&shadow_bounds)
                .is_none()
            {
                continue;
            }
            let explorer_glass = self.model.control_browser(window.id).is_some() || matches!(
                window.kind,
                nt61_desktop::shell::WindowKind::Explorer
                    | nt61_desktop::shell::WindowKind::Computer
            );
            self.window_glass.borrow_mut().prepare_extended(
                frame,
                window.id,
                bounds,
                if explorer_glass { 36 } else { 0 },
                if matches!(window.kind,nt61_desktop::shell::WindowKind::Application(nt61_desktop::applications::AppId::PhotoViewer)){62}else{0},
                refresh_glass,
            );
            aero::shadow(frame, bounds, 8);
            let body = nt61_desktop::graphics::Rect::new(
                x as i32 + 6,
                y as i32 + 30,
                width.saturating_sub(12),
                height.saturating_sub(36),
            );
            aero::window_body(frame, body);
            self.window_glass.borrow().paint(
                frame,
                window.id,
                Some(window.id) == self.model.selected_window,
            );
            if self.model.control_browser(window.id).is_some() {
                nt61_desktop::control_browser::paint::draw(&self.model,window.id,frame,body,|f,t,r,c|self.label(f,t,r,c));
            } else { match window.kind {
                nt61_desktop::shell::WindowKind::Application(_) => {
                    if let Some(app) = self.model.application(window.id) {
                        nt61_desktop::applications::view::draw(
                            app,
                            frame,
                            body,
                            &environment,
                            |frame, text, b, color| {
                                self.fonts.draw_fit(frame, text, b.x, b.y, b.width, color)
                            },
                        );
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
                    if let Some(state) = self
                        .model
                        .explorers
                        .iter()
                        .find(|e| e.window_id == window.id)
                    {
                        nt61_desktop::applications::explorer::draw_on_glass(
                            state,
                            &self.model.files,
                            frame,
                            body,
                            |frame, text, b, color| {
                                self.fonts.draw_fit(frame, text, b.x, b.y, b.width, color)
                            },
                        );
                    }
                }
                nt61_desktop::shell::WindowKind::CommandPrompt => {
                    if let Some(app) = self.model.application(window.id) {
                        nt61_desktop::applications::view::draw(
                            app,
                            frame,
                            body,
                            &environment,
                            |frame, text, b, color| {
                                self.fonts.draw_fit(frame, text, b.x, b.y, b.width, color)
                            },
                        );
                    } else {
                        self.draw_command_prompt(frame, body);
                    }
                }
                nt61_desktop::shell::WindowKind::ShellSettings(kind) => {
                    self.draw_shell_settings(frame, body, kind);
                }
                nt61_desktop::shell::WindowKind::ControlPanel
                | nt61_desktop::shell::WindowKind::ControlCategory(_)
                | nt61_desktop::shell::WindowKind::ControlPage(_)
                | nt61_desktop::shell::WindowKind::UserAccounts
                | nt61_desktop::shell::WindowKind::SoundSettings
                | nt61_desktop::shell::WindowKind::RecycleBin
                | nt61_desktop::shell::WindowKind::System
                | nt61_desktop::shell::WindowKind::ComputerManagement
                | nt61_desktop::shell::WindowKind::RecycleBinProperties
                | nt61_desktop::shell::WindowKind::ConfirmEmptyRecycleBin
                | nt61_desktop::shell::WindowKind::DisplaySettings
                | nt61_desktop::shell::WindowKind::Personalization => {
                    self.draw_namespace_window(frame, body, window.kind);
                }
                nt61_desktop::shell::WindowKind::MemoryDiagnostic => {
                    if let Some(result) = self.model.memory_result {
                        let mut y = body.y + 22;
                        for line in [
                            alloc::string::String::from(result.summary()),
                            alloc::format!("Completed passes: {}", result.passes),
                            alloc::format!("Memory tested: {} MiB   Errors: {}", result.tested / 1048576, result.errors),
                            alloc::string::String::from("A detailed report is saved in C:\\Windows\\MemoryDiagnosticResults.txt"),
                        ] {
                            self.draw_wrapped_line(frame, &line, body, y);
                            y += 46;
                        }
                    }
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
            }
            aero::blend_rect(
                frame,
                nt61_desktop::graphics::Rect::new(body.x, body.y, body.width, 1),
                0xD0FF_FFFF,
            );
            aero::window_border(frame, bounds, 6);
            let application = self.model.application(window.id);
            if let Some(app) = application {
                nt61_desktop::applications::icons::draw(
                    frame,
                    app.id,
                    nt61_desktop::graphics::Rect::new(window.x + 8, window.y + 7, 16, 16),
                );
            }
            if application.is_none() {
                let name = match window.kind {
                    nt61_desktop::shell::WindowKind::Computer
                    | nt61_desktop::shell::WindowKind::System => "Computer",
                    _ => "Folder",
                };
                nt61_desktop::applications::explorer::theme::icon(
                    frame,
                    name,
                    nt61_desktop::graphics::Rect::new(window.x + 8, window.y + 7, 16, 16),
                );
            }
            let title_x = window.x + 30;
            let title_y = window.y + 7;
            let title_width = width.saturating_sub(if application.is_some() { 140 } else { 120 });
            if self.model.control_browser(window.id).is_some() {
                if let Ok(font)=self.model.fonts.get(nt61_desktop::control_browser::paint::FONT) {
                    nt61_desktop::control_browser::paint::label(&font,frame,&window.title,
                        nt61_desktop::graphics::Rect::new(title_x,title_y,title_width,20),0xFF102030,12);
                }
            } else {
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                self.fonts.draw_fit(
                    frame,
                    &window.title,
                    title_x + dx,
                    title_y + dy,
                    title_width,
                    0x90FFFFFF,
                );
            }
            self.fonts.draw_fit(
                frame,
                &window.title,
                title_x,
                title_y,
                title_width,
                0xFF102030,
            );
            }
            aero::window_control_group(
                frame,
                window.caption_control_group(),
                Some(window.id) == self.model.selected_window,
            );
            for (index, (bounds, kind)) in window
                .caption_controls()
                .into_iter()
                .zip([
                    aero::WindowControlButtonKind::Minimize,
                    if window.maximized {
                        aero::WindowControlButtonKind::Restore
                    } else {
                        aero::WindowControlButtonKind::Maximize
                    },
                    aero::WindowControlButtonKind::Close,
                ])
                .enumerate()
            {
                aero::window_control_button(
                    frame,
                    bounds,
                    kind,
                    self.hovered_caption == Some((window.id, index as u8)),
                );
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
            let Some(row) =
                self.model
                    .context_item_bounds(index, framebuffer.width, framebuffer.height)
            else {
                continue;
            };
            if item.0.is_empty() {
                frame.fill_rect(
                    row.x + 29,
                    row.y + 3,
                    row.width.saturating_sub(32),
                    1,
                    0xFFD7D7D7,
                );
                frame.fill_rect(
                    row.x + 29,
                    row.y + 4,
                    row.width.saturating_sub(32),
                    1,
                    0xFFFFFFFF,
                );
                continue;
            }
            let enabled = self.model.context_item_enabled(index);
            let id = nt61_desktop::shell::CONTROL_CONTEXT_MENU_BASE + index as u32;
            let highlighted =
                enabled && (self.model.controls.hot() == id || self.model.controls.focus() == id);
            aero::menu_item(frame, row, highlighted);
            self.fonts.draw_color(
                frame,
                item.0,
                row.x + 32,
                row.y + 4,
                if enabled { 0xFF111111 } else { 0xFF8C8C8C },
            );
            if self.model.context_submenu_group(index).is_some() {
                aero::arrow_right(
                    frame,
                    nt61_desktop::graphics::Rect::new(
                        row.x + row.width as i32 - 13,
                        row.y + 8,
                        5,
                        8,
                    ),
                    0xFF111111,
                );
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
                let highlighted =
                    enabled && self.model.controls.hot() == CONTROL_SUBMENU_BASE + index as u32;
                aero::menu_item(frame, item_bounds, highlighted);
                let checked = match group.unwrap() {
                    nt61_desktop::shell::MenuGroup::DesktopView => index == 1,
                    nt61_desktop::shell::MenuGroup::DesktopSortBy => {
                        match self.model.desktop.sort_mode {
                            nt61_desktop::shell::SortMode::Name => index == 0,
                            nt61_desktop::shell::SortMode::Type => index == 2,
                            _ => false,
                        }
                    }
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

fn present_surface_region(
    surface: &nt61_desktop::graphics::Surface,
    pixels: &mut [u8],
    region: nt61_desktop::graphics::Rect,
) {
    let stride = surface.width() as usize * 4;
    if pixels.len() < stride * surface.height() as usize {
        return;
    }
    let screen = nt61_desktop::graphics::Rect::new(0, 0, surface.width(), surface.height());
    let Some(clip) = screen.intersection(&region) else {
        return;
    };
    let offset = clip.y as usize * stride + clip.x as usize * 4;
    // SAFETY: clipped rectangle is in the persistent staging allocation;
    // draw_rgba reads only the visible width on each row at the supplied stride.
    unsafe {
        arch::draw_rgba(
            pixels.as_ptr().add(offset),
            clip.x as u32,
            clip.y as u32,
            clip.width,
            clip.height,
            stride,
        );
    }
}

fn present_cursor_region(
    base: &nt61_desktop::graphics::Surface,
    cursor: &png::Image,
    x: i32,
    y: i32,
    scratch: &mut [u8],
) {
    use nt61_desktop::graphics::{present::cursor_over, Rect};
    let bounds = Rect::new(x, y, cursor.width, cursor.height);
    let screen = Rect::new(0, 0, base.width(), base.height());
    let Some(clip) = screen.intersection(&bounds) else {
        return;
    };
    let stride = clip.width as usize * 4;
    if scratch.len() < stride * clip.height as usize {
        return;
    }
    for row in 0..clip.height {
        for column in 0..clip.width {
            let (dx, dy) = (clip.x as u32 + column, clip.y as u32 + row);
            let source =
                (((dy as i32 - y) as u32 * cursor.width + (dx as i32 - x) as u32) * 4) as usize;
            let target = row as usize * stride + column as usize * 4;
            scratch[target..target + 4].copy_from_slice(&cursor_over(
                base.pixel(dx, dy).unwrap_or(0),
                &cursor.pixels[source..source + 4],
            ));
        }
    }
    // SAFETY: scratch holds the complete clipped cursor rectangle.
    unsafe {
        arch::draw_rgba(
            scratch.as_ptr(),
            clip.x as u32,
            clip.y as u32,
            clip.width,
            clip.height,
            stride,
        );
    }
}

fn draw_fallback_desktop(framebuffer: &arch::DesktopFramebufferInfo) {
    let mut surface = nt61_desktop::graphics::Surface::new(framebuffer.width, framebuffer.height);
    draw_wallpaper_into(&mut surface, framebuffer.width, framebuffer.height);
    let mut glass = surface.clone();
    glass.box_blur(aero::GLASS_BLUR_RADIUS);
    aero::colorize_glass(&mut glass, aero::GLASS_TINT, aero::GLASS_SATURATION);
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

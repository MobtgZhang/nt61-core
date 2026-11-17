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
use nt61_desktop::graphics::{Rect, Surface};
use nt61_desktop::logon_power::{ButtonState, LogonPowerButtons};

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
const WALLPAPER_PNG: &[u8] = include_bytes!("../../../assets/windows7/wallpapers/login-1024.png");

const USER_NAME: &str = "Administrator";
const CONTROL_PASSWORD: u32 = 1001;
const CONTROL_SUBMIT: u32 = 1002;
const CONTROL_EASE: u32 = 1003;
const CONTROL_POWER: u32 = 1004;
const CONTROL_POWER_ARROW:u32=1005;

#[cfg(not(test))]
use nt61_desktop::user_allocator::StaticHeapAllocator;

#[cfg(not(test))]
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
        // Keep the AuthUX frame/picture ratio at a compact credential size.
        let tile = 160.min(height / 3);
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
    user_name: alloc::string::String,
    cursor: Image,
    brand: Image,
    tile: Image,
    edit: [Image;3],
    submit: [Image;3],
    ease: Image,
    power: LogonPowerButtons,
    menu_fonts: font_view::FontPack,
    welcome_fonts: font_view::FontPack,
    wallpaper: Surface,
    login_background: Surface,
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
    let mut assets = LogonAssets::load(framebuffer.width, framebuffer.height);
    let mut picker=nt61_desktop::session::Session::default();
    picker.switching=true;picker.logged_off=true;picker.select_users();
    let mut selected_user=picker.accounts.first().map(|a|a.name.clone()).unwrap_or_else(||USER_NAME.into());
    if picker.accounts.len()==1 {if let Some(screen)=assets.as_mut(){screen.set_account(&picker.accounts[0],framebuffer.width,framebuffer.height);}picker.screen=nt61_desktop::session::Screen::Credentials;}
    let mut credentials = Credentials::new();
    let layout = Layout::new(framebuffer.width, framebuffer.height);
    let mut controls = build_login_controls(framebuffer.width, framebuffer.height, &layout);
    let mut frame = Surface::new(framebuffer.width, framebuffer.height);
    let mut frame_rgba = alloc::vec![0u8; frame.pixels().len() * 4];
    let mut cursor_frame = frame.clone();
    let mut authentication_failed = false;
    let mut notice = None;
    let mut power_menu=nt61_desktop::power_menu::PowerMenu::default();
    let mut power_action=None;
    let mut decoder=nt61_desktop::applications::explorer::input::Decoder::default();
    if let Some(screen) = assets.as_ref() {
        screen.draw(&framebuffer, &mut frame, &credentials, false, &controls, false);
        if picker.screen==nt61_desktop::session::Screen::Users {draw_account_picker(screen,&picker,&mut frame);}
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
    let _ = nt61_desktop::platform::services::play_sound("startup");

    let mut previous_buttons = 0u32;
    let mut cursor_x = (framebuffer.width / 2) as i32;
    let mut cursor_y = (framebuffer.height / 2) as i32;
    loop {
        let mut key = 0u8;
        let mut mouse = arch::DesktopMouseState::default();
        let mut redraw = false;
        let mut popup_redraw = false;
        let mut submit = false;
        let old_cursor = (cursor_x, cursor_y);
        let raw_pending=unsafe { arch::poll_key(&mut key) == 1 };
        let decoded=if raw_pending{decoder.feed(key,arch::ticks())}else{decoder.expire(arch::ticks())};
        let key_pending=decoded.is_some();if let Some(decoded)=decoded{key=decoded;}
        if let Some(action)=power_action.take(){
            if let Some(screen)=assets.as_ref(){screen.transition_frame(&mut frame,&framebuffer,if action==nt61_desktop::shell::PowerAction::Restart{"Restarting..."}else{"Shutting down..."},0);present_surface_with(&frame,&mut frame_rgba);}
            match nt61_desktop::platform::services::session(if action==nt61_desktop::shell::PowerAction::Restart{"restart"}else{"shutdown"},&[]){Ok(_)=>{},Err(_)=>{notice=Some("The power operation failed.");redraw=true;}}
        }
        let mouse_pending = unsafe { arch::poll_mouse(&mut mouse) != 0 };
        if !key_pending && !mouse_pending && !redraw {
            core::hint::spin_loop();
            continue;
        }
        if picker.screen==nt61_desktop::session::Screen::Users {
            let mut intent=nt61_desktop::session::Intent::None;
            if key_pending {intent=picker.key(match key{nt61_desktop::applications::explorer::input::UP=>0x26,nt61_desktop::applications::explorer::input::DOWN=>0x28,nt61_desktop::applications::explorer::input::LEFT=>0x25,nt61_desktop::applications::explorer::input::RIGHT=>0x27,_=>key});}
            if mouse_pending {
                cursor_x=mouse.x.clamp(0,framebuffer.width.saturating_sub(1)as i32);cursor_y=mouse.y.clamp(0,framebuffer.height.saturating_sub(1)as i32);
                if mouse.buttons&1!=0&&previous_buttons&1==0{intent=picker.hit(cursor_x,cursor_y,framebuffer.width,framebuffer.height);}
                previous_buttons=mouse.buttons;
            }
            if let nt61_desktop::session::Intent::Action(action)=intent {power_action=Some(action);}
            if picker.screen==nt61_desktop::session::Screen::Credentials {
                selected_user=picker.user.clone();credentials.clear();authentication_failed=false;notice=None;
                if let Some(account)=picker.accounts.iter().find(|a|a.name==selected_user){if let Some(screen)=assets.as_mut(){screen.set_account(account,framebuffer.width,framebuffer.height);}}
            }
            if let Some(screen)=assets.as_ref(){if picker.screen==nt61_desktop::session::Screen::Users{draw_account_picker(screen,&picker,&mut frame);}else{screen.draw(&framebuffer,&mut frame,&credentials,false,&controls,false);}cursor_frame.copy_from(&frame);draw_login_cursor(screen,&mut cursor_frame,cursor_x,cursor_y);present_surface_with(&cursor_frame,&mut frame_rgba);}
            continue;
        }
        if key_pending&&key==27&&!power_menu.open&&picker.accounts.len()>1 {picker.select_users();if let Some(screen)=assets.as_ref(){draw_account_picker(screen,&picker,&mut frame);cursor_frame.copy_from(&frame);draw_login_cursor(screen,&mut cursor_frame,cursor_x,cursor_y);present_surface_with(&cursor_frame,&mut frame_rgba);}continue;}
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
                let was_open=power_menu.open;
                let anchor=controls.control(CONTROL_POWER_ARROW).unwrap().rect;
                if was_open {
                    let before = power_menu.clone();
                    power_action = power_menu.pointer(anchor, cursor_x, cursor_y, action == PointerAction::Up);
                    popup_redraw = before != power_menu;
                    if !power_menu.open {
                        controls.route_pointer(PointerAction::Move, cursor_x, cursor_y);
                    }
                }
                if !was_open {if let Some(event) = controls.route_pointer(action, cursor_x, cursor_y) {
                    match event {
                        ControlEvent::Activated(CONTROL_SUBMIT) => submit = true,
                        ControlEvent::Activated(CONTROL_EASE) => {
                            notice =
                                Some("Accessibility services are not available in this session.");
                            redraw = true;
                        }
                        ControlEvent::Activated(CONTROL_POWER) => {
                            power_action=Some(nt61_desktop::shell::PowerAction::ShutDown);
                            redraw = true;
                        }
                        ControlEvent::Activated(CONTROL_POWER_ARROW) => {
                            power_menu.toggle();
                            popup_redraw = true;
                        }
                        _ => {}
                    }
                }
            }
            }
            let new_controls = (controls.hot(), controls.focus(), controls.pressed());
            if old_controls != new_controls {
                if old_controls.1 == new_controls.1
                    && [old_controls.0, old_controls.2, new_controls.0, new_controls.2]
                        .iter().all(|id| matches!(*id, 0 | CONTROL_POWER | CONTROL_POWER_ARROW)) {
                    popup_redraw = true;
                } else {
                    redraw = true;
                }
            }
            previous_buttons = mouse.buttons;
        }
        let mut key_redraw = false;
        if key_pending && power_menu.open {
            let before = power_menu.clone();
            power_action = power_menu.key(key);
            popup_redraw = before != power_menu;
        }
        else if key_pending {
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
                    power_action=Some(nt61_desktop::shell::PowerAction::ShutDown);
                    true
                }
                Some(ControlEvent::Activated(CONTROL_POWER_ARROW)) => {
                    power_menu.toggle();
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
            let authenticated = nt61_desktop::platform::services::logon(&selected_user, password).is_ok();
            credentials.clear();
            if authenticated {
                if let Some(screen) = assets.as_ref() {
                    let started = arch::ticks();
                    let mut next_frame = started;
                    loop {
                        let now = arch::ticks();
                        if now < next_frame { core::hint::spin_loop(); continue; }
                        screen.welcome_frame(&mut frame, &framebuffer, ((now-started)/50) as usize);
                        present_surface_with(&frame, &mut frame_rgba);
                        if now.saturating_sub(started) >= 240 { break; }
                        next_frame = now.saturating_add(50).min(started.saturating_add(240));
                    }
                }
                arch::debug_str("[LOGONUI] welcome frame ready\r\n");
                arch::exit_process(0);
            }
            authentication_failed = true;
            notice = None;
            redraw = true;
        }

        if redraw || key_redraw || popup_redraw {
            if let Some(screen) = assets.as_ref() {
                let anchor = controls.control(CONTROL_POWER_ARROW).unwrap().rect;
                // Hover/close updates restore and repaint only the popup, its shadow,
                // and the split button. The retained login image never gets rescaled.
                let damage = if popup_redraw && !redraw && !key_redraw {
                    Some(nt61_desktop::power_menu::PowerMenu::damage_bounds(anchor))
                } else { None };
                frame.set_draw_clip(damage);
                screen.draw(
                    &framebuffer,
                    &mut frame,
                    &credentials,
                    authentication_failed,
                    &controls,
                    power_menu.open,
                );
                power_menu.draw(&mut frame,controls.control(CONTROL_POWER_ARROW).unwrap().rect,|f,t,r,color|screen.menu_fonts.draw_color(f,t,r.x,r.y,color));
                if let Some(message) = notice {
                    screen.draw_centered(
                        &mut frame,
                        message,
                        framebuffer.width,
                        (layout.password_y + layout.password_height + layout.padding + 28) as i32,
                        font_view::Ink::White,
                    );
                }
                frame.set_draw_clip(None);
                if let Some(damage) = damage {
                    let old_bounds = Rect::new(old_cursor.0, old_cursor.1,
                        screen.cursor.width, screen.cursor.height);
                    cursor_frame.copy_region(&frame, old_bounds, old_bounds.x, old_bounds.y);
                    cursor_frame.copy_region(&frame, damage, damage.x, damage.y);
                    draw_login_cursor(screen, &mut cursor_frame, cursor_x, cursor_y);
                    present_surface_region(&cursor_frame, &mut frame_rgba, damage);
                    present_surface_region(&cursor_frame, &mut frame_rgba, old_bounds);
                    present_surface_region(&cursor_frame, &mut frame_rgba,
                        Rect::new(cursor_x, cursor_y, screen.cursor.width, screen.cursor.height));
                } else {
                    cursor_frame.copy_from(&frame);
                    draw_login_cursor(screen, &mut cursor_frame, cursor_x, cursor_y);
                    present_surface_with(&cursor_frame, &mut frame_rgba);
                }
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
    let (power, arrow) = LogonPowerButtons::bounds(width, layout.bottom_y as i32);
    tree.add(CONTROL_POWER, ControlKind::Button, power);
    tree.add(CONTROL_POWER_ARROW, ControlKind::Button, arrow);
    tree
}

fn draw_account_picker(assets:&LogonAssets,session:&nt61_desktop::session::Session,frame:&mut Surface) {
    nt61_desktop::session::draw_with_background(session,frame,Some(&assets.wallpaper),|f,t,r,c|{let x=r.x+(r.width.saturating_sub(assets.fonts.measure(t))/2)as i32;assets.fonts.draw_color(f,t,x,r.y,c);});
}
impl LogonAssets {
    fn set_account(&mut self,account:&nt61_desktop::accounts::Account,width:u32,height:u32){
        self.user_name=account.name.clone();
        let bytes=if account.picture==0 {if account.rid==501{nt61_desktop::user_account_pictures::GUEST.bytes}else{USER.bytes}}else{nt61_desktop::user_account_pictures::DEFAULT_PICTURES.get(account.picture as usize-1).map(|p|p.bytes).unwrap_or(USER.bytes)};
        if let Ok(image)=bmp::decode_bmp(bytes){self.avatar=Image{width:image.width,height:image.height,pixels:image.pixels};}
        let mut background=self.wallpaper.clone();self.draw_static(&mut background,width,height);self.login_background=background;
    }
    fn load(width: u32, height: u32) -> Option<Self> {
        let library = nt61_desktop::graphics::font::FontLibrary::default();
        let fonts = font_view::FontPack::with_library(16, &library);
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
        let button_size = Layout::new(width, height).password_height;
        let submit = [SUBMIT_BMP, SUBMIT_HOVER_BMP, SUBMIT_PRESSED_BMP]
            .map(decoded_button_bitmap);
        let submit = [scaled_image(submit[0].as_ref()?, button_size),
            scaled_image(submit[1].as_ref()?, button_size),
            scaled_image(submit[2].as_ref()?, button_size)];
        arch::debug_str("[LOGONUI] asset flag ok\r\n");
        let ease = decoded_button_bitmap(EASE_BMP);
        let ease = scaled_image(&ease?, button_size);
        arch::debug_str("[LOGONUI] asset ease ok\r\n");
        let power = LogonPowerButtons::load()?;
        let menu_fonts = font_view::FontPack::with_library(12, &library)?;
        let welcome_fonts = font_view::FontPack::with_library(28, &library)?;
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
        let mut assets = Self {
            fonts,
            avatar,
            user_name:USER_NAME.into(),
            cursor,
            brand,
            tile,
            edit,
            submit,
            ease,
            power,
            menu_fonts,
            welcome_fonts,
            login_background: background.clone(),
            wallpaper: background,
        };
        let mut login = assets.wallpaper.clone();
        assets.draw_static(&mut login, width, height);
        assets.login_background = login;
        Some(assets)
    }

    fn draw(
        &self,
        framebuffer: &arch::DesktopFramebufferInfo,
        frame: &mut Surface,
        credentials: &Credentials,
        authentication_failed: bool,
        controls: &ControlTree,
        menu_open: bool,
    ) {
        let width = framebuffer.width;
        let height = framebuffer.height;
        let layout = Layout::new(width, height);

        frame.copy_from(&self.login_background);

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
        if controls.hot() == CONTROL_EASE || controls.focus() == CONTROL_EASE {
            aero::blend_rounded_rect(frame, ease_layout.rect, 4,
                if controls.pressed() == CONTROL_EASE { 0x705994BE } else { 0x60B7E4FF });
        }
        self.draw_image(
            frame,
            &self.ease,
            ease_layout.rect.x as u32,
            ease_layout.rect.y as u32,
            ease_layout.rect.width,
        );
        let arrow = controls.control(CONTROL_POWER_ARROW).unwrap().rect;
        self.power.draw(frame, power_layout.rect, arrow,
            ButtonState::from_input(controls.hot() == CONTROL_POWER,
                controls.focus() == CONTROL_POWER, controls.pressed() == CONTROL_POWER),
            ButtonState::from_input(controls.hot() == CONTROL_POWER_ARROW,
                controls.focus() == CONTROL_POWER_ARROW,
                controls.pressed() == CONTROL_POWER_ARROW || menu_open));
    }

    fn draw_static(&self, frame: &mut Surface, width: u32, height: u32) {
        let layout = Layout::new(width, height);
        let tile_x = width.saturating_sub(layout.tile) / 2;
        // AuthUX's tile has a glass frame and a transparent picture aperture.
        // Bitmap 12223's picture aperture occupies the central 126/190 pixels.
        let picture = layout.tile * 126 / 190;
        let inset = (layout.tile - picture) / 2;
        self.draw_image(frame,&self.avatar,tile_x+inset,layout.tile_y+inset,picture);
        self.draw_image(frame,&self.tile,tile_x,layout.tile_y,layout.tile);

        self.draw_centered_labelled(frame, &self.user_name, width, layout.user_y);

        let brand_width = (350 * height / 768).clamp(280,438).min(width);
        self.draw_image(frame, &self.brand, width.saturating_sub(brand_width) / 2,
            layout.bottom_y, brand_width);
    }

    fn welcome_frame(&self, frame: &mut Surface, fb: &arch::DesktopFramebufferInfo, phase: usize) {
        self.transition_frame(frame, fb, "Welcome", phase);
    }

    fn transition_frame(&self, frame: &mut Surface, fb: &arch::DesktopFramebufferInfo, text: &str, phase: usize) {
        frame.copy_from(&self.wallpaper);
        let text_width = self.welcome_fonts.measure(text);
        let x = fb.width.saturating_sub(text_width + 32) / 2;
        let y = (fb.height / 2) as i32;
        self.welcome_fonts.draw_onto(frame, text, x as i32, y, font_view::Ink::White);
        const BUSY: &[u8] = include_bytes!("../../../assets/windows7/logon-buttons/busy-20.rgba");
        let offset = phase % (BUSY.len() / 1600) * 1600;
        blit_rgba(frame, &BUSY[offset..offset+1600], 80, Rect::new((x+text_width+12) as i32, y+7, 20, 20));
        let layout = Layout::new(fb.width, fb.height);
        let brand_width = (350 * fb.height / 768).clamp(280, 438).min(fb.width);
        self.draw_image(frame, &self.brand, fb.width.saturating_sub(brand_width) / 2, layout.bottom_y, brand_width);
    }

    fn draw_image(&self, frame: &mut Surface, image: &Image, x: u32, y: u32, target_width: u32) {
        let target_height = target_width * image.height as u32 / image.width as u32;
        if image.width == target_width && image.height == target_height {
            blit_rgba(frame, &image.pixels, image.stride(),
                Rect::new(x as i32, y as i32, target_width, target_height));
            return;
        }
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

/// The kernel's RGBA bridge blends onto the previous frame. Incremental alpha
/// gives the same crossfade as keeping a second full-screen copy in the kernel.
fn scaled_image(image: &Image, width: u32) -> Image {
    let height = width * image.height / image.width;
    Image { width, height, pixels: scale_rgba_bilinear(image, width, height) }
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

//! Painting uses the same rectangles as hit testing, clipped to the client area.
use super::*;
use crate::control_browser::strings;
use crate::graphics::font::FontLib;
const BLUE: u32 = 0xFF0066CC;
const TEXT: u32 = 0xFF151515;
pub const FONT: &[u8] = crate::graphics::font::UI_FONT;

pub fn label(font: &FontLib, f: &mut Surface, value: &str, b: Rect, color: u32, size: u32) {
    if b.width == 0 || b.height == 0 {
        return;
    }
    let Some(clip) = f.drawing_bounds().intersection(&b) else {
        return;
    };
    let previous = f.drawing_bounds();
    f.set_draw_clip(Some(clip));
    let prefix = font.fitting_prefix(value, size, b.width).unwrap_or("");
    let _ = font.draw_text(f, prefix, b.x, b.y, size, color);
    f.set_draw_clip(Some(previous));
}
pub fn wrapped_lines(font: &FontLib, value: &str, width: u32, size: u32) -> u32 {
    let mut rest = value.trim();
    let mut count = 0;
    while !rest.is_empty() {
        let fit = font.fitting_prefix(rest, size, width).unwrap_or("");
        if fit.is_empty() {
            return count.max(1);
        }
        let len = if fit.len() < rest.len() {
            fit.rfind(' ').filter(|n| *n > 0).unwrap_or(fit.len())
        } else {
            fit.len()
        };
        rest = rest[len..].trim_start();
        count += 1;
    }
    count.max(1)
}
pub fn wrapped_label(
    font: &FontLib,
    f: &mut Surface,
    value: &str,
    b: Rect,
    color: u32,
    size: u32,
    line_height: u32,
) {
    let mut rest = value.trim();
    let mut y = b.y;
    while !rest.is_empty() && y < b.y + b.height as i32 {
        let fit = font.fitting_prefix(rest, size, b.width).unwrap_or("");
        if fit.is_empty() {
            break;
        }
        let len = if fit.len() < rest.len() {
            fit.rfind(' ').filter(|n| *n > 0).unwrap_or(fit.len())
        } else {
            fit.len()
        };
        label(
            font,
            f,
            &rest[..len],
            Rect::new(b.x, y, b.width, line_height),
            color,
            size,
        );
        rest = rest[len..].trim_start();
        y += line_height as i32;
    }
}
pub fn button(f: &mut Surface, b: Rect, value: &str, enabled: bool, font: &FontLib) {
    f.stroke_rounded_rect(b, 3, 1, if enabled { 0xFF707070 } else { 0xFFBABABA });
    f.blend_rounded_rect(
        Rect::new(
            b.x + 1,
            b.y + 1,
            b.width.saturating_sub(2),
            b.height.saturating_sub(2),
        ),
        2,
        0xFFF0F0F0,
    );
    f.fill_rect(
        b.x + 2,
        b.y + 2,
        b.width.saturating_sub(4),
        b.height.saturating_sub(4) / 2,
        0xFFFAFAFA,
    );
    let w = font.measure(value, 12).map_or(0, |m| m.width);
    label(
        font,
        f,
        value,
        Rect::new(
            b.x + (b.width.saturating_sub(w) / 2) as i32,
            b.y + 5,
            b.width,
            18,
        ),
        if enabled { TEXT } else { 0xFF999999 },
        12,
    );
}
fn chrome(browser: &Browser, f: &mut Surface, b: Rect, font: &FontLib) {
    f.blend_rect(Rect::new(b.x, b.y, b.width, TOOLBAR), 0xA0EFF5FA);
    theme::navigation(f, back(b), false, browser.can_back(), false, false);
    theme::navigation(f, forward(b), true, browser.can_forward(), false, false);
    let a = address(b);
    f.fill_rect(a.x - 1, a.y - 1, a.width + 25, a.height + 2, 0xFF91A7BA);
    f.fill_rect(a.x, a.y, a.width, a.height, 0xFFFFFFFF);
    theme::control_category(f, 0, Rect::new(a.x + 4, a.y + 3, 16, 16));
    for (r, value, _) in breadcrumbs(b, browser.current().kind, font) {
        label(
            font,
            f,
            value,
            Rect::new(r.x, r.y + 4, r.width.saturating_sub(16), 18),
            TEXT,
            12,
        );
        label(
            font,
            f,
            "›",
            Rect::new(r.x + r.width as i32 - 13, r.y + 4, 12, 18),
            TEXT,
            12,
        );
    }
    theme::icon(
        f,
        "Refresh",
        Rect::new(a.x + a.width as i32 + 4, a.y + 3, 16, 16),
    );
    let r = search(b);
    f.fill_rect(
        r.x - 1,
        r.y - 1,
        r.width + 2,
        r.height + 2,
        if browser.search_focus {
            0xFF3A88CD
        } else {
            0xFF91A7BA
        },
    );
    f.fill_rect(r.x, r.y, r.width, r.height, 0xFFFFFFFF);
    let query = &browser.current().query;
    label(
        font,
        f,
        if query.is_empty() {
            strings::SEARCH_CONTROL_PANEL
        } else {
            query
        },
        Rect::new(r.x + 5, r.y + 4, r.width.saturating_sub(26), 18),
        if query.is_empty() { 0xFF707070 } else { TEXT },
        12,
    );
    theme::icon(
        f,
        "Search",
        Rect::new(r.x + r.width as i32 - 20, r.y + 4, 16, 16),
    );
    f.fill_rect(b.x, b.y + 35, b.width, 1, 0xFFD7DDE4);
}
fn heading(font: &FontLib, f: &mut Surface, b: Rect, value: &str) {
    label(
        font,
        f,
        value,
        Rect::new(
            b.x + (b.width / 8).clamp(28, 72) as i32,
            b.y + 20,
            b.width.saturating_sub(200),
            25,
        ),
        0xFF003399,
        16,
    );
}
fn view_menu(browser: &Browser, f: &mut Surface, b: Rect, font: &FontLib) {
    let r = view_rect(b);
    label(
        font,
        f,
        strings::VIEW_BY,
        Rect::new(r.x, r.y + 4, 62, 20),
        TEXT,
        12,
    );
    let text = match browser.current().view {
        View::Category => strings::CATEGORY,
        View::LargeIcons => strings::LARGE_ICONS,
        View::SmallIcons => strings::SMALL_ICONS,
    };
    label(
        font,
        f,
        text,
        Rect::new(r.x + 52, r.y + 4, 80, 20),
        BLUE,
        12,
    );
    if browser.view_open {
        f.shadow_rect(Rect::new(r.x, r.y + 25, r.width, 80), 3);
        f.fill_rect(r.x, r.y + 25, r.width, 80, 0xFF909090);
        f.fill_rect(r.x + 1, r.y + 26, r.width - 2, 78, 0xFFFFFFFF);
        for (i, value) in [
            strings::CATEGORY_2,
            strings::LARGE_ICONS_2,
            strings::SMALL_ICONS_2,
        ]
        .iter()
        .enumerate()
        {
            let y = r.y + 29 + i as i32 * 26;
            label(
                font,
                f,
                value,
                Rect::new(r.x + 24, y, r.width - 28, 20),
                TEXT,
                12,
            );
            if i == match browser.current().view {
                View::Category => 0,
                View::LargeIcons => 1,
                View::SmallIcons => 2,
            } {
                label(font, f, "✓", Rect::new(r.x + 6, y, 16, 20), TEXT, 12);
            }
        }
    }
}
pub fn draw(
    model: &ShellModel,
    id: u32,
    f: &mut Surface,
    b: Rect,
    english: impl Fn(&mut Surface, &str, Rect, u32),
) {
    let Some(browser) = model.control_browser(id) else {
        return;
    };
    let Ok(font) = model.fonts.get(FONT) else {
        return;
    };
    let previous = f.drawing_bounds();
    let Some(clip) = previous.intersection(&b) else {
        return;
    };
    f.set_draw_clip(Some(clip));
    let c = content(b);
    let visit = browser.current();
    f.fill_rect(c.x, c.y, c.width, c.height, 0xFFFFFFFF);
    let height = visit_height(visit, c.width, &font);
    let page = page_rect(c, height, visit.scroll);
    let Some(page_clip) = clip.intersection(&c) else {
        f.set_draw_clip(Some(previous));
        return;
    };
    f.set_draw_clip(Some(page_clip));
    let t = |f: &mut Surface, value: &str, r: Rect, color| label(&font, f, value, r, color, 12);
    if !visit.query.is_empty()
        || matches!(visit.kind, WindowKind::ControlPage(Page::AllItems))
        || visit.kind == WindowKind::ControlPanel && visit.view != View::Category
    {
        heading(
            &font,
            f,
            page,
            if visit.query.is_empty() {
                strings::ALL_CONTROL_PANEL_ITEMS
            } else {
                strings::SEARCH_RESULTS
            },
        );
        let mut any = false;
        for (i, item) in filtered_items(&visit.query).enumerate() {
            any = true;
            let r = item_rect(page, i, visit.view);
            let size = if visit.view == View::SmallIcons {
                16
            } else {
                32
            };
            item_icon(f, item.kind, Rect::new(r.x, r.y + 8, size, size));
            t(
                f,
                item.text,
                Rect::new(
                    r.x + size as i32 + 8,
                    r.y + 12,
                    r.width.saturating_sub(size + 12),
                    22,
                ),
                BLUE,
            );
        }
        if !any {
            t(
                f,
                strings::NO_ITEMS_MATCH_YOUR_SEARCH,
                Rect::new(page.x + 30, page.y + 70, page.width.saturating_sub(50), 24),
                TEXT,
            );
        }
    } else {
        match visit.kind {
            WindowKind::ControlPanel => {
                heading(&font, f, page, strings::ADJUST_YOUR_COMPUTER_S_SETTINGS);
                for index in 0..8 {
                    let (r, title, links) = category_layout(page, index, &font);
                    theme::control_category(f, index, Rect::new(r.x, r.y + 2, 48, 48));
                    wrapped_label(&font, f, CATEGORIES[index], title, 0xFF008000, 16, 21);
                    for (rect, link) in links.iter().zip(SHORTCUTS[index]) {
                        wrapped_label(&font, f, link.text, *rect, BLUE, 12, 18);
                    }
                }
            }
            WindowKind::ControlCategory(index) => {
                f.fill_rect(page.x, page.y, 174, page.height, 0xFFEAF1FB);
                t(
                    f,
                    strings::CONTROL_PANEL_HOME,
                    Rect::new(page.x + 14, page.y + 12, 150, 24),
                    BLUE,
                );
                for (i, cat) in CATEGORIES.iter().enumerate() {
                    t(
                        f,
                        cat,
                        Rect::new(page.x + 14, page.y + 58 + i as i32 * 30, 156, 24),
                        if index as usize == i { TEXT } else { BLUE },
                    );
                }
                label(
                    &font,
                    f,
                    title(visit.kind),
                    Rect::new(
                        page.x + 196,
                        page.y + 22,
                        page.width.saturating_sub(210),
                        28,
                    ),
                    0xFF003399,
                    16,
                );
                for (i, (_, kind)) in ShellModel::namespace_links(visit.kind).iter().enumerate() {
                    let r = category_item_rect(page, i);
                    item_icon(f, *kind, Rect::new(r.x, r.y + 4, 36, 36));
                    label(
                        &font,
                        f,
                        title(*kind),
                        Rect::new(r.x + 48, r.y + 3, r.width.saturating_sub(50), 24),
                        0xFF008000,
                        15,
                    );
                    t(
                        f,
                        category_description(*kind),
                        Rect::new(r.x + 48, r.y + 29, r.width.saturating_sub(50), 22),
                        TEXT,
                    );
                }
            }
            WindowKind::System => crate::applications::explorer::system::draw_scrolled(
                f,
                c,
                page,
                model.process_snapshot.as_ref().map(|s| s.cpu_count),
                model.monitor_sample.as_ref().map(|s| s.total),
                &font,
                t,
            ),
            WindowKind::DisplaySettings => {
                crate::display_settings::draw(&model.display, f, page, model.display.now, &font, t)
            }
            WindowKind::ControlPage(kind) => draw_page(model, kind, f, page, &font),
            WindowKind::UserAccounts => {
                crate::control_panel::draw(&model.account_settings, f, c, &english)
            }
            WindowKind::SoundSettings => {
                crate::sound_settings::draw(&model.sound_settings, f, c, &english)
            }
            WindowKind::Personalization => {
                crate::personalization::draw(&model.personalization, f, c, &english)
            }
            WindowKind::ShellSettings(Flyout::Network) => {
                crate::network_settings::draw(&model.network_settings, f, c, &english)
            }
            WindowKind::Application(_) => {
                if let Some(app) = model.application(id) {
                    crate::applications::view::draw(
                        app,
                        f,
                        c,
                        &model.application_environment(),
                        &english,
                    );
                }
            }
            WindowKind::ComputerManagement => {
                heading(&font, f, page, strings::ADMINISTRATIVE_TOOLS);
                for (i, (_, kind)) in ShellModel::namespace_links(visit.kind).iter().enumerate() {
                    let r = category_item_rect(page, i);
                    t(f, title(*kind), r, BLUE);
                }
            }
            WindowKind::ShellSettings(Flyout::Clock) => {
                heading(&font, f, page, strings::DATE_AND_TIME);
                t(
                    f,
                    &model.taskbar.clock,
                    Rect::new(page.x + 40, page.y + 80, 300, 25),
                    TEXT,
                );
                t(
                    f,
                    strings::DATE_AND_TIME_ARE_PROVIDED_BY_THE_SYSTEM_CLOCK,
                    Rect::new(page.x + 40, page.y + 120, 400, 25),
                    TEXT,
                );
            }
            _ => {}
        }
    }
    // Page clipping never permits content to overpaint the navigation bar.
    f.set_draw_clip(Some(clip));
    chrome(browser, f, b, &font);
    if matches!(
        visit.kind,
        WindowKind::ControlPanel | WindowKind::ControlPage(Page::AllItems)
    ) && visit.query.is_empty()
    {
        f.set_draw_clip(Some(page_clip));
        view_menu(browser, f, c, &font);
    }
    if c.height < height {
        let x = c.x + c.width as i32 - 15;
        f.fill_rect(x, c.y, 15, c.height, 0xFFF1F1F1);
        t(f, "▴", Rect::new(x + 2, c.y, 13, 16), TEXT);
        t(
            f,
            "▾",
            Rect::new(x + 2, c.y + c.height as i32 - 16, 13, 16),
            TEXT,
        );
        let track = c.height.saturating_sub(34);
        let thumb = (track * c.height / height).max(20).min(track);
        let top = c.y
            + 17
            + (track.saturating_sub(thumb) * visit.scroll / (height - c.height).max(1)) as i32;
        f.fill_rect(x + 2, top, 11, thumb, 0xFFCDD3D9);
    }
    f.set_draw_clip(Some(previous));
}
fn category_description(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::System => strings::VIEW_SYSTEM_INFORMATION_AND_ADVANCED_SETTINGS,
        WindowKind::DisplaySettings => strings::CHANGE_THE_RESOLUTION_OF_YOUR_DISPLAY,
        WindowKind::Personalization => strings::CHANGE_YOUR_THEME_BACKGROUND_AND_WINDOW_COLOR,
        WindowKind::Application(AppId::DeviceManager) => strings::VIEW_HARDWARE_DEVICES_AND_DRIVERS,
        WindowKind::UserAccounts => strings::CHANGE_ACCOUNT_TYPES_PASSWORDS_AND_PICTURES,
        WindowKind::SoundSettings => strings::CONFIGURE_PLAYBACK_DEVICES_AND_SYSTEM_SOUNDS,
        _ => strings::OPEN_THIS_CONTROL_PANEL_ITEM,
    }
}
fn item_icon(f: &mut Surface, kind: WindowKind, r: Rect) {
    match kind {
        WindowKind::System => theme::large_icon(f, "Computer", r),
        WindowKind::DisplaySettings | WindowKind::ControlPage(Page::TextSize) => {
            theme::control_category(f, 5, r)
        }
        WindowKind::Personalization => theme::control_category(f, 5, r),
        WindowKind::UserAccounts => theme::control_category(f, 4, r),
        WindowKind::ShellSettings(Flyout::Network) => theme::control_category(f, 1, r),
        WindowKind::Application(app) => crate::applications::icons::draw(f, app, r),
        WindowKind::SoundSettings => theme::control_category(f, 2, r),
        _ => theme::control_category(f, 0, r),
    }
}
pub fn property_tab(b: Rect, i: usize) -> Rect {
    let offsets = [0, 24, 39, 57, 82, 100];
    let w = b.width.saturating_sub(40);
    Rect::new(
        b.x + 20 + (w * offsets[i] / 100) as i32,
        b.y + 30,
        w * (offsets[i + 1] - offsets[i]) / 100,
        28,
    )
}
pub fn property_device_link(b: Rect) -> Rect {
    Rect::new(b.x + 40, b.y + 115, 170, 28)
}
pub fn property_ok(b: Rect) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 258,
        b.y + b.height as i32 - 42,
        72,
        25,
    )
}
pub fn property_cancel(b: Rect) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 178,
        b.y + b.height as i32 - 42,
        72,
        25,
    )
}
pub fn return_link(b: Rect) -> Rect {
    Rect::new(b.x + 30, b.y + 55, 280, 25)
}
fn draw_page(model: &ShellModel, page: Page, f: &mut Surface, b: Rect, font: &FontLib) {
    let t = |f: &mut Surface, value: &str, r: Rect, color| {
        wrapped_label(font, f, value, r, color, 12, 17)
    };
    match page {
        Page::Properties(tab) => {
            f.fill_rect(b.x, b.y, b.width, b.height, 0xFFF0F0F0);
            let panel = Rect::new(
                b.x + 18,
                b.y + 56,
                b.width.saturating_sub(36),
                b.height.saturating_sub(112),
            );
            f.fill_rect(panel.x, panel.y, panel.width, panel.height, 0xFF898C95);
            f.fill_rect(
                panel.x + 1,
                panel.y + 1,
                panel.width - 2,
                panel.height - 2,
                0xFFFFFFFF,
            );
            for (i, value) in [
                strings::COMPUTER_NAME,
                strings::HARDWARE,
                strings::ADVANCED,
                strings::SYSTEM_PROTECTION,
                strings::REMOTE,
            ]
            .iter()
            .enumerate()
            {
                let r = property_tab(b, i);
                button(f, r, value, true, font);
                if i == tab as usize {
                    f.fill_rect(
                        r.x + 1,
                        r.y + r.height as i32 - 2,
                        r.width - 2,
                        3,
                        0xFFFFFFFF,
                    );
                }
            }
            let x = b.x + 40;
            let w = b.width.saturating_sub(80);
            match tab {
                0 => {
                    t(f,strings::WINDOWS_USES_THIS_INFORMATION_TO_IDENTIFY_YOUR_COMPUTER_ON_THE_NETWORK,Rect::new(x,b.y+85,w,24),TEXT);
                    t(
                        f,
                        strings::FULL_COMPUTER_NAME_NT61,
                        Rect::new(x, b.y + 130, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::WORKGROUP_NOT_JOINED,
                        Rect::new(x, b.y + 170, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::COMPUTER_RENAMING_AND_DOMAIN_MEMBERSHIP_ARE_NOT_AVAILABLE,
                        Rect::new(x, b.y + 230, w, 40),
                        0xFF666666,
                    );
                    button(
                        f,
                        Rect::new(x, b.y + 275, 100, 26),
                        strings::CHANGE,
                        false,
                        font,
                    );
                }
                1 => {
                    t(
                        f,
                        strings::DEVICE_MANAGER,
                        Rect::new(x, b.y + 85, w, 24),
                        TEXT,
                    );
                    button(
                        f,
                        property_device_link(b),
                        strings::DEVICE_MANAGER,
                        true,
                        font,
                    );
                    t(
                        f,
                        strings::VIEW_AND_MANAGE_THE_DEVICES_AND_DRIVERS_INSTALLED_ON_THIS_COMPUTER,
                        Rect::new(x, b.y + 155, w, 24),
                        TEXT,
                    );
                }
                2 => {
                    for (i,(name,desc)) in [(strings::PERFORMANCE,strings::VISUAL_EFFECTS_PROCESSOR_SCHEDULING_MEMORY_USAGE_AND_VIRTUAL_MEMORY),(strings::USER_PROFILES,strings::DESKTOP_SETTINGS_RELATED_TO_YOUR_SIGN_IN),(strings::STARTUP_AND_RECOVERY,strings::SYSTEM_STARTUP_SYSTEM_FAILURE_AND_DEBUGGING_INFORMATION)].iter().enumerate(){let y=b.y+83+i as i32*86;t(f,name,Rect::new(x,y,w,22),TEXT);t(f,desc,Rect::new(x,y+26,w.saturating_sub(110),32),TEXT);button(f,Rect::new(x+w as i32-100,y+24,95,26),strings::SETTINGS,false,font);}
                    t(
                        f,
                        strings::THESE_SYSTEM_CONFIGURATION_SERVICES_ARE_NOT_AVAILABLE,
                        Rect::new(x, b.y + 350, w, 24),
                        0xFF666666,
                    );
                }
                3 => {
                    t(
                        f,
                        strings::USE_SYSTEM_PROTECTION_TO_UNDO_UNWANTED_SYSTEM_CHANGES,
                        Rect::new(x, b.y + 85, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::PROTECTION_SETTINGS,
                        Rect::new(x, b.y + 130, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::AVAILABLE_DRIVES_PROTECTION,
                        Rect::new(x, b.y + 165, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::LOCAL_DISK_C_OFF,
                        Rect::new(x, b.y + 200, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::SYSTEM_RESTORE_SNAPSHOTS_ARE_NOT_AVAILABLE,
                        Rect::new(x, b.y + 245, w, 24),
                        0xFF666666,
                    );
                    button(
                        f,
                        Rect::new(x, b.y + 285, 110, 26),
                        strings::SYSTEM_RESTORE,
                        false,
                        font,
                    );
                }
                _ => {
                    t(
                        f,
                        strings::REMOTE_ASSISTANCE,
                        Rect::new(x, b.y + 85, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::ALLOW_REMOTE_ASSISTANCE_CONNECTIONS_TO_THIS_COMPUTER,
                        Rect::new(x, b.y + 125, w, 24),
                        0xFF888888,
                    );
                    t(
                        f,
                        strings::REMOTE_DESKTOP,
                        Rect::new(x, b.y + 185, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::DON_T_ALLOW_CONNECTIONS_TO_THIS_COMPUTER,
                        Rect::new(x, b.y + 225, w, 24),
                        TEXT,
                    );
                    t(
                        f,
                        strings::REMOTE_DESKTOP_SERVICES_ARE_NOT_AVAILABLE,
                        Rect::new(x, b.y + 285, w, 24),
                        0xFF666666,
                    );
                }
            }
            button(f, property_ok(b), strings::OK, true, font);
            button(f, property_cancel(b), strings::CANCEL, true, font);
            button(
                f,
                Rect::new(
                    b.x + b.width as i32 - 98,
                    b.y + b.height as i32 - 42,
                    80,
                    25,
                ),
                strings::APPLY,
                false,
                font,
            );
        }
        Page::DisplayAdvanced => {
            heading(font, f, b, strings::DISPLAY_ADAPTER_PROPERTIES);
            t(
                f,
                strings::RETURN_TO_SCREEN_RESOLUTION,
                return_link(b),
                BLUE,
            );
            theme::large_icon(f, "Computer", Rect::new(b.x + 35, b.y + 100, 48, 48));
            for (i, value) in [
                strings::ADAPTER_CURRENT_SYSTEM_DISPLAY_DRIVER,
                strings::MONITOR_GENERIC_NON_PNP_MONITOR,
                strings::COLORS_TRUE_COLOR_32_BIT,
                strings::MODES_REPORTED_BY_THE_CURRENT_DISPLAY_DRIVER,
            ]
            .iter()
            .enumerate()
            {
                t(
                    f,
                    value,
                    Rect::new(
                        b.x + 105,
                        b.y + 100 + i as i32 * 30,
                        b.width.saturating_sub(130),
                        24,
                    ),
                    TEXT,
                );
            }
            let columns = (b.width.saturating_sub(80) / 150).max(1);
            for (i, (w, h)) in model.display.modes.iter().enumerate() {
                t(
                    f,
                    &alloc::format!("{w} × {h}"),
                    Rect::new(
                        b.x + 40 + (i as u32 % columns * 150) as i32,
                        b.y + 245 + (i as u32 / columns * 24) as i32,
                        140,
                        24,
                    ),
                    TEXT,
                );
            }
        }
        Page::TextSize => {
            heading(
                font,
                f,
                b,
                strings::MAKE_IT_EASIER_TO_READ_WHAT_S_ON_YOUR_SCREEN,
            );
            t(
                f,
                strings::RETURN_TO_SCREEN_RESOLUTION,
                return_link(b),
                BLUE,
            );
            t(
                f,
                strings::YOU_CAN_CHANGE_THE_SIZE_OF_TEXT_AND_OTHER_ITEMS_ON_YOUR_SCREEN,
                Rect::new(b.x + 30, b.y + 105, b.width.saturating_sub(60), 24),
                TEXT,
            );
            for (i, value) in [
                strings::SMALLER_100_DEFAULT,
                strings::MEDIUM_125,
                strings::LARGER_150,
            ]
            .iter()
            .enumerate()
            {
                t(
                    f,
                    value,
                    Rect::new(b.x + 40, b.y + 155 + i as i32 * 42, 350, 30),
                    if i == 0 { TEXT } else { 0xFF888888 },
                );
            }
            t(
                f,
                strings::THE_DESKTOP_USES_96_DPI_OTHER_SCALING_LEVELS_ARE_NOT_AVAILABLE,
                Rect::new(b.x + 30, b.y + 310, b.width.saturating_sub(60), 28),
                0xFF666666,
            );
        }
        Page::ResolutionHelp => {
            heading(font, f, b, strings::WHAT_DISPLAY_SETTINGS_SHOULD_I_CHOOSE);
            t(
                f,
                strings::RETURN_TO_SCREEN_RESOLUTION,
                return_link(b),
                BLUE,
            );
            for (i, value) in [
                strings::RESOLUTION_DETERMINES_THE_NUMBER_OF_PIXELS_ON_YOUR_SCREEN,
                strings::CHOOSE_A_RESOLUTION_SUPPORTED_BY_THE_DISPLAY_DRIVER,
                strings::CLICK_APPLY_TO_SWITCH_TO_THE_SELECTED_RESOLUTION,
                strings::CONFIRM_WITHIN_15_SECONDS_TO_KEEP_YOUR_CHANGES,
                strings::IF_THE_SCREEN_IS_UNREADABLE_WAIT_FOR_THE_SETTINGS_TO_REVERT,
                strings::CANCEL_DISCARDS_CHANGES_THAT_HAVE_NOT_BEEN_APPLIED,
            ]
            .iter()
            .enumerate()
            {
                t(
                    f,
                    value,
                    Rect::new(
                        b.x + 35,
                        b.y + 115 + i as i32 * 38,
                        b.width.saturating_sub(70),
                        28,
                    ),
                    TEXT,
                );
            }
        }
        Page::AllItems => {}
    }
}

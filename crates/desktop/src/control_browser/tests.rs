use super::*;
use crate::theme::Theme;
#[test]
fn history_is_per_window_and_branching_discards_forward_entries() {
    let mut shell = ShellModel::new(Theme::aero());
    let id =
        shell.open_window_with_kind("Control Panel", 40, 40, 800, 580, WindowKind::ControlPanel);
    let second = shell.open_window_with_kind("System", 100, 100, 800, 580, WindowKind::System);
    shell.navigate_control(id, WindowKind::ControlCategory(0));
    shell.navigate_control(id, WindowKind::System);
    assert_eq!(shell.taskbar.windows.len(), 2);
    shell.control_browser_mut(id).unwrap().current_mut().scroll = 96;
    shell.control_history(id, false);
    assert_eq!(
        shell.control_browser(id).unwrap().current().kind,
        WindowKind::ControlCategory(0)
    );
    shell.control_history(id, true);
    assert_eq!(shell.control_browser(id).unwrap().current().scroll, 96);
    shell.control_history(id, false);
    shell.navigate_control(id, WindowKind::DisplaySettings);
    assert!(!shell.control_browser(id).unwrap().can_forward());
    assert_eq!(shell.control_browser(second).unwrap().history.len(), 1);
    shell.close_window(id);
    assert!(shell.control_browser(id).is_none());
    assert!(shell.control_browser(second).is_some());
}
#[test]
fn every_reference_link_and_navigation_button_stays_in_one_window() {
    let mut shell = ShellModel::new(Theme::aero());
    let id =
        shell.open_window_with_kind("Control Panel", 40, 40, 800, 580, WindowKind::ControlPanel);
    let b = Rect::new(46, 70, 788, 544);
    let page = content(b);
    let font = crate::graphics::font::FontLib::ui().unwrap();
    let height = visit_height(
        shell.control_browser(id).unwrap().current(),
        page.width,
        &font,
    );
    let (_, _, links) = category_layout(page_rect(page, height, 0), 5, &font);
    let link = links[2];
    assert!(shell.handle_window_control_click(id, link.x + 5, link.y + 5, 1024, 728));
    assert_eq!(shell.taskbar.windows[0].kind, WindowKind::DisplaySettings);
    let r = back(b);
    assert!(shell.handle_window_control_click(id, r.x + 12, r.y + 12, 1024, 728));
    assert_eq!(shell.taskbar.windows[0].kind, WindowKind::ControlPanel);
    let r = forward(b);
    assert!(shell.handle_window_control_click(id, r.x + 12, r.y + 12, 1024, 728));
    assert_eq!(shell.taskbar.windows[0].kind, WindowKind::DisplaySettings);
    let r = crate::display_settings::advanced_link(page);
    shell.handle_window_control_click(id, r.x + 5, r.y + 5, 1024, 728);
    assert_eq!(
        shell.taskbar.windows[0].kind,
        WindowKind::ControlPage(Page::DisplayAdvanced)
    );
    assert_eq!(shell.taskbar.windows.len(), 1);
}
#[test]
fn system_sidebar_opens_properties_and_device_manager_in_place() {
    let mut shell = ShellModel::new(Theme::aero());
    let id = shell.open_window_with_kind("System", 40, 40, 800, 580, WindowKind::System);
    let b = Rect::new(46, 106, 772, 508);
    for (index, kind) in [
        (0, WindowKind::Application(AppId::DeviceManager)),
        (1, WindowKind::ControlPage(Page::Properties(4))),
        (2, WindowKind::ControlPage(Page::Properties(3))),
        (3, WindowKind::ControlPage(Page::Properties(2))),
    ] {
        let r = crate::applications::explorer::system::sidebar_link(b, index);
        shell.handle_window_control_click(id, r.x + 4, r.y + 4, 1024, 728);
        assert_eq!(shell.taskbar.windows[0].kind, kind);
        assert_eq!(shell.taskbar.windows.len(), 1);
        shell.control_history(id, false);
    }
}
#[test]
fn english_catalog_and_search_are_usable() {
    let f = crate::graphics::font::FontLib::from_bytes(paint::FONT).unwrap();
    assert!(f
        .measure("Control Panel System Resolution Windows 7", 12)
        .is_some());
    assert!(filtered_items("resolution").any(|l| l.kind == WindowKind::DisplaySettings));
    assert!(filtered_items("System").any(|l| l.kind == WindowKind::System));
    assert_eq!(filtered_items("nothing matches this").count(), 0);
}

#[test]
fn category_sidebar_and_management_links_navigate_in_place() {
    let mut shell = ShellModel::new(Theme::aero());
    let id = shell.open_window_with_kind(
        "Control Panel",
        40,
        40,
        800,
        580,
        WindowKind::ControlCategory(0),
    );
    let page = content(body(&shell.taskbar.windows[0]));
    shell.handle_window_control_click(id, page.x + 30, page.y + 58 + 5 * 30 + 5, 1024, 728);
    assert_eq!(
        shell.taskbar.windows[0].kind,
        WindowKind::ControlCategory(5)
    );
    shell.navigate_control(id, WindowKind::ComputerManagement);
    let r = category_item_rect(page, 0);
    shell.handle_window_control_click(id, r.x + 5, r.y + 5, 1024, 728);
    assert_eq!(
        shell.taskbar.windows[0].kind,
        WindowKind::Application(AppId::DeviceManager)
    );
    assert_eq!(shell.taskbar.windows.len(), 1);
}

#[test]
fn properties_tabs_do_not_pollute_page_history_or_expose_hidden_controls() {
    let mut shell = ShellModel::new(Theme::aero());
    let id = shell.open_window_with_kind("System", 40, 40, 800, 580, WindowKind::System);
    shell.navigate_control(id, WindowKind::ControlPage(Page::Properties(0)));
    let page = content(body(&shell.taskbar.windows[0]));
    let r = paint::property_device_link(page);
    shell.handle_window_control_click(id, r.x + 5, r.y + 5, 1024, 728);
    assert_eq!(
        shell.taskbar.windows[0].kind,
        WindowKind::ControlPage(Page::Properties(0))
    );
    for i in [1, 2, 3, 4] {
        let r = paint::property_tab(page, i);
        shell.handle_window_control_click(id, r.x + 5, r.y + 5, 1024, 728);
    }
    let r = paint::property_ok(page);
    shell.handle_window_control_click(id, r.x + 5, r.y + 5, 1024, 728);
    assert_eq!(shell.taskbar.windows[0].kind, WindowKind::System);
}

#[test]
fn wheel_scrolls_system_and_retains_scroll_on_back() {
    let mut shell = ShellModel::new(Theme::aero());
    let id = shell.open_window_with_kind("System", 40, 40, 600, 420, WindowKind::System);
    assert!(shell.application_wheel(id, 500, 300, -1));
    let scroll = shell.control_browser(id).unwrap().current().scroll;
    assert!(scroll > 0);
    shell.navigate_control(id, WindowKind::ControlPanel);
    shell.control_history(id, false);
    assert_eq!(shell.control_browser(id).unwrap().current().scroll, scroll);
}

#[test]
fn measured_breadcrumbs_navigate_to_the_label_that_was_clicked() {
    let mut shell = ShellModel::new(Theme::aero());
    let id = shell.open_window_with_kind("System", 40, 40, 800, 600, WindowKind::System);
    let b = body(&shell.taskbar.windows[0]);
    let font = crate::graphics::font::FontLib::ui().unwrap();
    let crumbs = breadcrumbs(b, WindowKind::System, &font);
    assert_eq!(crumbs.len(), 3);
    for (r, text, _) in &crumbs {
        assert!(r.width >= font.measure(text, 12).unwrap().width + 16);
    }
    let (r, _, destination) = crumbs[1];
    shell.handle_window_control_click(id, r.x + 5, r.y + 5, 1024, 728);
    assert_eq!(shell.taskbar.windows[0].kind, destination);
    assert_eq!(shell.taskbar.windows.len(), 1);
}

#[test]
fn open_mode_list_captures_clicks_over_underlying_page_links() {
    let mut shell = ShellModel::new(Theme::aero());
    let id = shell.open_window_with_kind(
        "Screen Resolution",
        40,
        40,
        700,
        500,
        WindowKind::DisplaySettings,
    );
    shell.display.modes = alloc::vec![
        (640, 480),
        (800, 600),
        (1024, 768),
        (1280, 720),
        (1280, 800),
        (1280, 960),
        (1280, 1024),
        (1600, 900)
    ];
    let page = content(body(&shell.taskbar.windows[0]));
    let d = crate::display_settings::dropdown(page);
    shell.handle_window_control_click(id, d.x + 5, d.y + 5, 1024, 728);
    assert!(shell.display.open);
    // This row lies over the text-size hyperlink behind the open list.
    shell.handle_window_control_click(id, d.x + 5, d.y + 23 + 22 + 22 + 5, 1024, 728);
    assert_eq!(shell.display.selected, 1);
    assert!(!shell.display.open);
    assert_eq!(shell.taskbar.windows[0].kind, WindowKind::DisplaySettings);
}

#[test]
fn category_view_from_all_items_returns_to_category_page() {
    let mut shell = ShellModel::new(Theme::aero());
    let id = shell.open_window_with_kind(
        "Control Panel",
        40,
        40,
        800,
        600,
        WindowKind::ControlPage(Page::AllItems),
    );
    let b = body(&shell.taskbar.windows[0]);
    let r = view_rect(content(b));
    shell.handle_window_control_click(id, r.x + 5, r.y + 5, 1024, 728);
    shell.handle_window_control_click(id, r.x + 5, r.y + 30, 1024, 728);
    assert_eq!(shell.taskbar.windows[0].kind, WindowKind::ControlPanel);
    assert_eq!(
        shell.control_browser(id).unwrap().current().view,
        View::Category
    );
}

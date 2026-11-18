//! Menu structure measured from Taskmgr.exe 6.1.7601 (SP1), not inferred from newer Windows.
use super::super::explorer::input;
use super::*;
const SEP: usize = usize::MAX;
fn check(label: &str, checked: bool) -> String {
    format!("{}{}", if checked { "✓  " } else { "    " }, label)
}
pub(super) fn title_bounds(m: &Manager, b: Rect, i: usize) -> Rect {
    let mut r = menu_button(b, i);
    if m.page != 0 && i == 4 {
        r.x -= MENU_WIDTHS[3] as i32;
    }
    r
}
pub(super) fn items(m: &Manager, child: bool) -> Vec<(String, usize)> {
    if child {
        return match m.submenu {
            Some(10) => [("High", 11), ("Normal", 12), ("Low", 13), ("Paused", 14)]
                .into_iter()
                .map(|(s, n)| {
                    (
                        check(
                            s,
                            if n == 14 {
                                m.paused
                            } else {
                                !m.paused
                                    && m.interval
                                        == match n {
                                            11 => 500,
                                            12 => 1000,
                                            _ => 4000,
                                        }
                            },
                        ),
                        n,
                    )
                })
                .collect(),
            Some(20) => alloc::vec![
                (check("One Graph, All CPUs", !m.per_cpu), 21),
                (check("One Graph Per CPU", m.per_cpu), 22)
            ],
            Some(30) => [
                ("Bytes Sent", 31),
                ("Bytes Received", 32),
                ("Bytes Total", 33),
            ]
            .into_iter()
            .map(|(s, n)| (check(s, m.network_lines & (1 << (n - 31)) != 0), n))
            .collect(),
            _ => Vec::new(),
        };
    }
    let mut v = match m.menu {
        Some(0) => alloc::vec![
            ("New Task (Run...)".into(), 0),
            (String::new(), SEP),
            ("Exit Task Manager".into(), 1)
        ],
        Some(1) => alloc::vec![
            (check("Always On Top", m.always_on_top), 0),
            (check("Minimize On Use", m.minimize_on_use), 1),
            (check("Hide When Minimized", m.hide_minimized), 2)
        ],
        Some(2) => alloc::vec![("Refresh Now                F5".into(), 0)],
        Some(3) => [
            "Tile Horizontally",
            "Tile Vertically",
            "Minimize",
            "Maximize",
            "Cascade",
            "Bring To Front",
        ]
        .into_iter()
        .enumerate()
        .map(|(i, s)| (s.into(), i))
        .collect(),
        Some(4) => alloc::vec![
            ("Task Manager Help Topics   F1".into(), 0),
            (String::new(), SEP),
            ("About Task Manager".into(), 1)
        ],
        Some(5) => [
            "End Process...",
            "End Process Tree...",
            "Priority: Low",
            "Priority: Below Normal",
            "Priority: Normal",
            "Priority: Above Normal",
            "Priority: High",
            "Affinity: All CPUs",
            "Affinity: CPU 0",
        ]
        .into_iter()
        .enumerate()
        .map(|(i, s)| (s.into(), i))
        .collect(),
        _ => Vec::new(),
    };
    if m.menu == Some(2) && m.page != 2 {
        v.push(("Update Speed                       >".into(), 10));
        if matches!(m.page, 1 | 3..=5) {
            v.push((String::new(), SEP));
        }
        if matches!(m.page, 1 | 4 | 5) {
            v.push(("Select Columns...".into(), 40));
        }
        if m.page == 3 {
            v.push(("CPU History                          >".into(), 20));
            v.push((check("Show Kernel Times", m.kernel_times), 23));
        }
        if m.page == 4 {
            v.insert(3, ("Network Adapter History         >".into(), 30));
        }
    }
    if m.menu == Some(1) {
        if m.page == 4 {
            v.extend([
                (String::new(), SEP),
                (check("Tab Always Active", m.network_active), 3),
                (check("Show Cumulative Data", m.cumulative), 4),
                (check("Auto Scale", m.auto_scale), 5),
                ("Reset".into(), 6),
                (check("Show Scale", m.show_scale), 7),
            ]);
        } else if m.page == 5 {
            v.extend([
                (String::new(), SEP),
                (check("Show Full Account Name", m.full_names), 8),
            ]);
        }
    }
    v
}
fn child_bounds(m: &Manager, b: Rect) -> Rect {
    let r = menu_bounds(m, b);
    let row = items(m, false)
        .iter()
        .position(|(_, id)| Some(*id) == m.submenu)
        .unwrap_or(0);
    let x = if r.x + 488 <= m.menu_viewport.0 as i32 {
        r.x + 241
    } else {
        r.x - 241
    };
    let height = items(m, true).len() as u32 * 26 + 6;
    Rect::new(x.max(0), (r.y + 3 + row as i32 * 26)
        .min(m.menu_viewport.1.saturating_sub(height) as i32), 244, height)
}
pub(super) fn contains(m: &Manager, b: Rect, x: i32, y: i32) -> bool {
    m.menu.is_some() && (menu_bounds(m, b).contains(x, y)
        || m.submenu.is_some() && child_bounds(m, b).contains(x, y))
}
pub(super) fn buttons(m: &Manager, b: Rect, out: &mut Vec<Button>) {
    for child in [false, true] {
        if child && m.submenu.is_none() {
            continue;
        }
        let r = if child {
            child_bounds(m, b)
        } else {
            menu_bounds(m, b)
        };
        for (i, (label, id)) in items(m, child).into_iter().enumerate() {
            if id != SEP {
                add(
                    out,
                    Rect::new(r.x + 3, r.y + 3 + i as i32 * 26, r.width - 6, 26),
                    label,
                    Command::Item(id),
                );
            }
        }
    }
}
pub(super) fn hover(app: &mut ApplicationSession, b: Rect, x: i32, y: i32) -> bool {
    if app.manager.menu.is_none() || app.manager.context.is_some() {
        return false;
    }
    for i in 0..5 {
        if i == 3 && app.manager.page != 0 {
            continue;
        }
        if title_bounds(&app.manager, b, i).contains(x, y) && app.manager.menu != Some(i) {
            app.manager.menu = Some(i);
            app.manager.submenu = None;
            app.manager.menu_focus = 0;
            return true;
        }
    }
    if app.manager.submenu.is_some() && child_bounds(&app.manager, b).contains(x, y) {
        return false;
    }
    let r = menu_bounds(&app.manager, b);
    if !r.contains(x, y) || y < r.y + 3 {
        return false;
    }
    let row = ((y - r.y - 3) / 26) as usize;
    let Some((_, id)) = items(&app.manager, false).get(row).cloned() else {
        return false;
    };
    let child = if app.manager.menu == Some(2) && matches!(id, 10 | 20 | 30) {
        Some(id)
    } else {
        None
    };
    if child != app.manager.submenu {
        app.manager.submenu = child;
        app.manager.menu_focus = if child.is_some() { 0 } else { row };
        return true;
    }
    false
}
pub(super) fn draw(
    app: &ApplicationSession,
    f: &mut Surface,
    b: Rect,
    e: &Environment,
    text: &impl Fn(&mut Surface, &str, Rect, u32),
) {
    let m = &app.manager;
    for child in [false, true] {
        if child && m.submenu.is_none() {
            continue;
        }
        let r = if child {
            child_bounds(m, b)
        } else {
            menu_bounds(m, b)
        };
        aero::menu_surface(f, r);
        for (i, (label, id)) in items(m, child).into_iter().enumerate() {
            let row = Rect::new(r.x + 3, r.y + 3 + i as i32 * 26, r.width - 6, 26);
            if id == SEP {
                f.fill_rect(row.x + 28, row.y + 12, row.width - 33, 1, 0xffd6d6d6);
                continue;
            }
            let enabled = enabled(m, &Command::Item(id), e);
            if enabled
                && (app.hot == Some(row) || (child == m.submenu.is_some() && i == m.menu_focus))
            {
                aero::menu_item(f, row, true);
            }
            text(
                f,
                &label,
                Rect::new(row.x + 8, row.y + 5, row.width - 12, 20),
                if enabled { 0xff111111 } else { 0xff888888 },
            );
        }
    }
}
pub(super) fn execute(app: &mut ApplicationSession, n: usize, _e: &Environment) -> AppAction {
    let m = &mut app.manager;
    // Keyboard activation must obey the same disabled state as mouse input.
    if !enabled(m, &Command::Item(n), _e) {
        return AppAction::None;
    }
    if m.menu == Some(2) && matches!(n, 10 | 20 | 30) {
        m.submenu = Some(n);
        m.menu_focus = 0;
        return AppAction::None;
    }
    let menu = m.menu.take();
    m.submenu = None;
    m.context = None;
    match (menu,n) {
        (Some(0),0)=>return AppAction::Manage(Action::NewTask),
        (Some(0),1)=>return AppAction::RequestClose,
        (Some(1),0)=>m.always_on_top=!m.always_on_top,
        (Some(1),1)=>m.minimize_on_use=!m.minimize_on_use,
        (Some(1),2)=>m.hide_minimized=!m.hide_minimized,
        (Some(1),3)=>m.network_active=!m.network_active,
        (Some(1),4)=>m.cumulative=!m.cumulative,
        (Some(1),5)=>m.auto_scale=!m.auto_scale,
        (Some(1),6)=>{m.network_reset=m.last.clone();},
        (Some(1),7)=>m.show_scale=!m.show_scale,
        (Some(1),8)=>m.full_names=!m.full_names,
        (Some(2),0)=>{m.refresh=true;return AppAction::Manage(Action::Refresh);},
        (Some(2),11..=13)=>{m.interval=match n{11=>500,12=>1000,_=>4000};m.paused=false;},
        (Some(2),14)=>m.paused=true,
        (Some(2),21..=22)=>m.per_cpu=n==22,
        (Some(2),23)=>m.kernel_times=!m.kernel_times,
        (Some(2),31..=33)=>m.network_lines^=1<<(n-31),
        (Some(2),40)=>m.column_dialog=Some(m.columns[m.page]),
        (Some(3),0..=5)=>return AppAction::Manage(Action::Arrange(n)),
        (Some(4),0)=>app.status="Task Manager Help Topics\nApplications: switch, arrange or end desktop tasks.\nProcesses: select columns, sort by CPU or memory; right-click for process controls.\nServices: start or stop services; F5 refreshes status.\nPerformance / Networking: view live system counters.\nOptions controls window behavior; View controls sampling.\nUse Alt+F/O/V/W/H, arrows and Enter to navigate menus.".into(),
        (Some(4),1)=>app.status="Windows Task Manager\nNT61 Rust implementation\nWindows 7 SP1 menu and behavior reference (6.1.7601).".into(),
        (Some(5),0..=1)=>if let Some(id)=m.selected {m.confirmation=Some((id,n==1));},
        (Some(5),2..=8)=>if let Some(id)=m.selected {return AppAction::Manage(Action::Process(id,match n{2=>"priority:4",3=>"priority:6",4=>"priority:8",5=>"priority:10",6=>"priority:13",7=>"affinity:all",_=>"affinity:1"}.into()));},
        _=>{},
    }
    AppAction::None
}
pub(super) fn key(app: &mut ApplicationSession, k: u8) -> Option<AppAction> {
    let m = &mut app.manager;
    if m.column_dialog.is_some() {
        match k {
            27 => m.column_dialog = None,
            13 => {
                if let Some(mask) = m.column_dialog.take() {
                    if mask != 0 {
                        m.columns[m.page] = mask;
                    }
                }
            }
            _ => {}
        }
        return Some(AppAction::None);
    }
    let menu = match k {
        input::ALT_FILE => Some(0),
        input::ALT_FORMAT => Some(1),
        input::ALT_VIEW => Some(2),
        0xa5 if m.page == 0 => Some(3),
        input::ALT_HELP => Some(4),
        _ => None,
    };
    if let Some(menu) = menu {
        m.menu = Some(menu);
        m.submenu = None;
        m.menu_focus = 0;
        return Some(AppAction::None);
    }
    if k == input::REFRESH {
        m.refresh = true;
        return Some(AppAction::Manage(Action::Refresh));
    }
    if k == input::HELP {
        m.menu = Some(4);
        return Some(execute(app, 0, &empty()));
    }
    m.menu?;
    let entries = items(m, m.submenu.is_some());
    match k {
        input::DOWN | input::UP => {
            let d = if k == input::DOWN {
                1
            } else {
                entries.len() - 1
            };
            for _ in 0..entries.len() {
                m.menu_focus = (m.menu_focus + d) % entries.len();
                if entries[m.menu_focus].1 != SEP
                    && enabled(m, &Command::Item(entries[m.menu_focus].1), &empty())
                {
                    break;
                }
            }
        }
        input::LEFT if m.submenu.is_some() => {
            m.submenu = None;
            m.menu_focus = 0;
        }
        input::LEFT | input::RIGHT => {
            if k == input::RIGHT
                && matches!(entries.get(m.menu_focus).map(|e| e.1), Some(10 | 20 | 30))
            {
                let n = entries[m.menu_focus].1;
                return Some(execute(app, n, &empty()));
            }
            let d = if k == input::RIGHT { 1 } else { 4 };
            let mut next = (m.menu.unwrap_or(0) + d) % 5;
            if next == 3 && m.page != 0 {
                next = (next + d) % 5;
            }
            m.menu = Some(next);
            m.submenu = None;
            m.menu_focus = 0;
        }
        13 | 10 => {
            if let Some((_, id)) = entries.get(m.menu_focus) {
                let id = *id;
                return Some(execute(app, id, &empty()));
            }
        }
        27 => {
            if m.submenu.take().is_none() {
                m.menu = None;
                m.context = None;
            }
        }
        _ => {}
    }
    Some(AppAction::None)
}
fn empty() -> Environment {
    Environment {
        width: 0,
        height: 0,
        network_adapters: 0,
        audio: false,
        tasks: Vec::new(),
        processes: None,
        monitor: None,
        services: Vec::new(),
        users: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sp1_tab_specific_view_menus_and_defaults() {
        let mut m = Manager::default();
        assert!(!m.hide_minimized);
        assert!(m.minimize_on_use);
        assert!(m.per_cpu);
        assert!(m.auto_scale);
        assert!(m.show_scale);
        assert_eq!(m.network_lines, 4);
        m.menu = Some(2);
        for page in 0..6 {
            m.page = page;
            let ids: Vec<_> = items(&m, false).into_iter().map(|(_, id)| id).collect();
            assert_eq!(
                ids,
                match page {
                    0 => alloc::vec![0, 10],
                    1 | 5 => alloc::vec![0, 10, SEP, 40],
                    2 => alloc::vec![0],
                    3 => alloc::vec![0, 10, SEP, 20, 23],
                    _ => alloc::vec![0, 10, SEP, 30, 40],
                }
            );
        }
    }
    #[test]
    fn keyboard_nested_speed_and_refresh_preserve_paused() {
        let mut app = ApplicationSession::new(1, AppId::TaskManager, 0);
        assert_eq!(key(&mut app, input::ALT_VIEW), Some(AppAction::None));
        key(&mut app, input::DOWN);
        key(&mut app, input::RIGHT);
        assert_eq!(app.manager.submenu, Some(10));
        for _ in 0..3 {
            key(&mut app, input::DOWN);
        }
        key(&mut app, 13);
        assert!(app.manager.paused);
        assert_eq!(app.manager.menu, None);
        assert_eq!(
            key(&mut app, input::REFRESH),
            Some(AppAction::Manage(Action::Refresh))
        );
        assert!(app.manager.paused);
        assert!(app.manager.refresh);
    }
    #[test]
    fn keyboard_skips_disabled_window_actions_and_enter_cannot_invoke_them() {
        let mut app = ApplicationSession::new(1, AppId::TaskManager, 0);
        key(&mut app, 0xa5);
        app.manager.menu_focus = 2; // Minimize is disabled without a selected task.
        assert_eq!(key(&mut app, 13), Some(AppAction::None));
        assert_eq!(app.manager.menu, Some(3));
        app.manager.menu_focus = 1;
        key(&mut app, input::DOWN);
        assert_eq!(app.manager.menu_focus, 4); // Cascade, skipping Minimize / Maximize.
        key(&mut app, input::DOWN);
        assert_eq!(app.manager.menu_focus, 0); // Bring To Front is also disabled.
        app.manager.selected = Some(8);
        app.manager.menu_focus = 1;
        key(&mut app, input::DOWN);
        assert_eq!(key(&mut app, 13), Some(AppAction::Manage(Action::Arrange(2))));
    }
}

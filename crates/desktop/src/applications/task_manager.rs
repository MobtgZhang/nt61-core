//! Task Manager state and UI shared by the desktop and native image.
mod menu;
use super::{
    system_monitor::Sample,
    view::{Button, Command as ViewCommand},
    AppAction, AppId, ApplicationSession, Environment,
};
use crate::{
    compositor::aero,
    graphics::{Rect, Surface},
    shell::PowerAction,
};
use alloc::{format, string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Process(u64, String),
    Service(String, bool),
    Power(PowerAction),
    User(String, bool),
    Minimize(u32),
    NewTask,
    Refresh,
    Arrange(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Page(usize),
    Row(u64),
    Scroll(i32),
    Sort(usize),
    Menu(usize),
    Item(usize),
    End,
    Switch,
    New,
    Start(bool),
    Confirm(bool),
    All,
    Resource,
    Dismiss,
    Column(usize),
    ColumnsDone(bool),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manager {
    pub page: usize,
    pub selected: Option<u64>,
    pub scroll_grab: Option<i32>,
    pub offset: usize,
    pub sort: usize,
    pub descending: bool,
    pub menu: Option<usize>,
    pub submenu: Option<usize>,
    pub menu_focus: usize,
    pub menu_viewport: (u32, u32),
    pub columns: [u32; 6],
    pub column_dialog: Option<u32>,
    pub per_cpu: bool,
    pub kernel_times: bool,
    pub network_active: bool,
    pub cumulative: bool,
    pub auto_scale: bool,
    pub show_scale: bool,
    pub network_lines: u8,
    pub full_names: bool,
    pub refresh: bool,
    pub network_reset: Option<Sample>,
    pub cpu_history: Vec<Vec<(u32, u32)>>,
    pub kernel_history: Vec<(u32, u32)>,
    pub network_history: Vec<(u64, Vec<(u32, u32)>)>,
    pub context: Option<(i32, i32)>,
    pub confirmation: Option<(u64, bool)>,
    pub always_on_top: bool,
    pub minimize_on_use: bool,
    pub hide_minimized: bool,
    pub show_all: bool,
    pub interval: u64,
    pub paused: bool,
    pub process_cpu: Vec<(u64, u32)>,
    pub previous_processes: Option<super::processes::Snapshot>,
    pub process_total: Option<u64>,
    pub last: Option<Sample>,
    pub cpu: Option<u32>,
    pub history: Vec<(u32, u32)>,
    pub sampled: u64,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            page: 0,
            selected: None,
            scroll_grab: None,
            offset: 0,
            sort: 0,
            descending: false,
            menu: None,
            submenu: None,
            menu_focus: 0,
            menu_viewport: (1024, 768),
            columns: [u32::MAX; 6],
            column_dialog: None,
            per_cpu: true,
            kernel_times: false,
            network_active: false,
            cumulative: false,
            auto_scale: true,
            show_scale: true,
            network_lines: 4,
            full_names: false,
            refresh: false,
            network_reset: None,
            cpu_history: Vec::new(),
            kernel_history: Vec::new(),
            network_history: Vec::new(),
            context: None,
            confirmation: None,
            always_on_top: false,
            minimize_on_use: true,
            hide_minimized: false,
            show_all: false,
            interval: 1000,
            paused: false,
            process_cpu: Vec::new(),
            previous_processes: None,
            process_total: None,
            last: None,
            cpu: None,
            history: Vec::new(),
            sampled: 0,
        }
    }
}
impl Manager {
    pub fn observe_processes(&mut self, s: &super::processes::Snapshot, total: u64) {
        self.process_cpu.clear();
        if let (Some(previous), Some(old_total)) = (&self.previous_processes, self.process_total) {
            if let Some(delta) = total.checked_sub(old_total).filter(|d| *d > 0) {
                for p in &s.processes {
                    if let Some(old) = previous.processes.iter().find(|old| old.pid == p.pid) {
                        if let Some(ticks) = p.cpu_ticks.checked_sub(old.cpu_ticks) {
                            self.process_cpu.push((
                                p.pid,
                                ((ticks as u128 * 100 / delta as u128).min(100)) as u32,
                            ));
                        }
                    }
                }
            }
        }
        self.previous_processes = Some(s.clone());
        self.process_total = Some(total);
    }

    pub fn observe(&mut self, s: &Sample) {
        if self.paused || self.sampled != 0 && s.uptime.saturating_sub(self.sampled) < self.interval
        {
            return;
        }
        self.cpu = self.last.as_ref().and_then(|p| s.cpu_since(p));
        if let Some(cpu) = self.cpu {
            let mem = if s.total > 0 {
                ((s.total - s.available) as u128 * 100 / s.total as u128) as u32
            } else {
                0
            };
            self.history.push((cpu, mem));
            if self.history.len() > 120 {
                self.history.remove(0);
            }
        }
        if let Some(previous) = &self.last {
            while self.cpu_history.len() < s.cpus.len() {
                self.cpu_history.push(Vec::new());
            }
            let mut total_delta = 0u64;
            let mut kernel_delta = 0u64;
            for (i, (cpu, old)) in s.cpus.iter().zip(&previous.cpus).enumerate() {
                if let (Some(total), Some(idle), Some(kernel)) = (
                    cpu.total.checked_sub(old.total),
                    cpu.idle.checked_sub(old.idle),
                    cpu.kernel.checked_sub(old.kernel),
                ) {
                    if total > 0 && idle <= total {
                        let value = ((total - idle) as u128 * 100 / total as u128) as u32;
                        let kernel = (kernel as u128 * 100 / total as u128).min(100) as u32;
                        let h = &mut self.cpu_history[i];
                        h.push((value, kernel));
                        if h.len() > 120 {
                            h.remove(0);
                        }
                        total_delta += total;
                        kernel_delta += cpu.kernel.saturating_sub(old.kernel);
                    }
                }
            }
            self.kernel_history.push((
                if total_delta > 0 {
                    (kernel_delta as u128 * 100 / total_delta as u128).min(100) as u32
                } else {
                    0
                },
                0,
            ));
            if self.kernel_history.len() > 120 {
                self.kernel_history.remove(0);
            }
            let elapsed = s.uptime.saturating_sub(previous.uptime);
            if elapsed > 0 {
                for network in &s.network {
                    if let Some(old) = previous.network.iter().find(|n| n.index == network.index) {
                        let sent = (network.sent.saturating_sub(old.sent) as u128 * 1000
                            / elapsed as u128)
                            .min(u32::MAX as u128) as u32;
                        let received = (network.received.saturating_sub(old.received) as u128
                            * 1000
                            / elapsed as u128)
                            .min(u32::MAX as u128) as u32;
                        let i = if let Some(i) = self
                            .network_history
                            .iter()
                            .position(|(id, _)| *id == network.index)
                        {
                            i
                        } else {
                            self.network_history.push((network.index, Vec::new()));
                            self.network_history.len() - 1
                        };
                        let h = &mut self.network_history[i].1;
                        h.push((sent, received));
                        if h.len() > 120 {
                            h.remove(0);
                        }
                    }
                }
            }
        }
        self.last = Some(s.clone());
        self.sampled = s.uptime;
    }
}
const TABS: [&str; 6] = [
    "Applications",
    "Processes",
    "Services",
    "Performance",
    "Networking",
    "Users",
];
const MENU_WIDTHS: [u32; 5] = [38, 62, 45, 70, 45];
const TAB_WIDTHS: [u32; 6] = [88, 75, 63, 89, 85, 50];
const ROW_HEIGHT: u32 = 20;
fn menu_button(b: Rect, i: usize) -> Rect {
    Rect::new(
        b.x + MENU_WIDTHS[..i].iter().sum::<u32>() as i32,
        b.y,
        MENU_WIDTHS[i],
        23,
    )
}
fn tab(b: Rect, i: usize) -> Rect {
    Rect::new(
        b.x + 8 + TAB_WIDTHS[..i].iter().sum::<u32>() as i32,
        b.y + 28,
        TAB_WIDTHS[i],
        23,
    )
}
fn pane(b: Rect) -> Rect {
    Rect::new(
        b.x + 8,
        b.y + 50,
        b.width.saturating_sub(16),
        b.height.saturating_sub(76),
    )
}
pub fn list_bounds(b: Rect) -> Rect {
    Rect::new(
        b.x + 16,
        b.y + 64,
        b.width.saturating_sub(32),
        b.height.saturating_sub(130),
    )
}
fn rows(_m: &Manager, b: Rect) -> usize {
    (list_bounds(b).height.saturating_sub(23) / ROW_HEIGHT).max(1) as usize
}
fn row_bounds(b: Rect, index: usize) -> Rect {
    let r = list_bounds(b);
    Rect::new(
        r.x + 1,
        r.y + 23 + index as i32 * ROW_HEIGHT as i32,
        r.width.saturating_sub(18),
        ROW_HEIGHT,
    )
}
fn headers(page: usize) -> &'static [(&'static str, u32)] {
    match page {
        0 => &[("Task", 72), ("Status", 28)],
        1 => &[
            ("Image Name", 29),
            ("PID", 12),
            ("User Name", 23),
            ("CPU", 9),
            ("Memory (K)", 27),
        ],
        2 => &[
            ("Name", 23),
            ("PID", 11),
            ("Description", 32),
            ("Status", 18),
            ("Group", 16),
        ],
        4 => &[
            ("Adapter", 20),
            ("Utilization", 15),
            ("Link Speed", 15),
            ("State", 14),
            ("Bytes Sent", 12),
            ("Bytes Received", 12),
            ("Bytes Total", 12),
        ],
        5 => &[
            ("User", 36),
            ("ID", 10),
            ("Status", 24),
            ("Client Name", 15),
            ("Session", 15),
        ],
        _ => &[],
    }
}
fn cell(b: Rect, m: &Manager, index: usize, y: i32, height: u32) -> Rect {
    let r = list_bounds(b);
    let columns = headers(m.page);
    let total = r.width.saturating_sub(18);
    let visible = |i: usize| m.columns[m.page] & (1 << i) != 0;
    let sum = columns
        .iter()
        .enumerate()
        .filter(|(i, _)| visible(*i))
        .map(|(_, (_, w))| *w)
        .sum::<u32>()
        .max(1);
    let left = columns[..index]
        .iter()
        .enumerate()
        .filter(|(i, _)| visible(*i))
        .map(|(_, (_, w))| *w)
        .sum::<u32>();
    let right = left + columns[index].1;
    Rect::new(
        r.x + 1 + (total * left / sum) as i32,
        y,
        total * right / sum - total * left / sum,
        height,
    )
}
fn entry_count(m: &Manager, e: &Environment) -> usize {
    match m.page {
        0 => e.tasks.len(),
        1 => processes(m, e).len(),
        2 => e.services.len(),
        4 => m.last.as_ref().map_or(0, |s| s.network.len()),
        5 => e.users.len(),
        _ => 0,
    }
}
fn processes<'a>(m: &Manager, e: &'a Environment) -> Vec<&'a super::processes::Process> {
    let mut v: Vec<_> = e
        .processes
        .iter()
        .flat_map(|s| s.processes.iter())
        .filter(|p| {
            m.show_all
                || e.users
                    .first()
                    .is_some_and(|u| p.user.eq_ignore_ascii_case(&u.0))
        })
        .collect();
    v.sort_by(|a, b| match m.sort {
        1 => a.pid.cmp(&b.pid),
        2 => a.user.cmp(&b.user),
        3 => process_cpu_value(m, a.pid).cmp(&process_cpu_value(m, b.pid)),
        4 => a.working_set.cmp(&b.working_set),
        _ => a.name.cmp(&b.name),
    });
    if m.descending {
        v.reverse();
    }
    v
}
fn add(out: &mut Vec<Button>, r: Rect, label: impl Into<String>, c: Command) {
    out.push(Button {
        bounds: r,
        label: label.into(),
        command: ViewCommand::TaskManager(c),
    });
}
fn menu_items(m: &Manager) -> Vec<(String, usize)> {
    menu::items(m, false)
}
fn menu_origin(m: &Manager, b: Rect) -> (i32, i32) {
    m.context.unwrap_or((
        menu::title_bounds(m, b, m.menu.unwrap_or(0).min(4)).x,
        b.y + 23,
    ))
}
fn menu_bounds(m: &Manager, b: Rect) -> Rect {
    let (x, y) = menu_origin(m, b);
    let height = menu_items(m).len() as u32 * 26 + 6;
    Rect::new(x.clamp(0, m.menu_viewport.0.saturating_sub(244) as i32),
        y.clamp(0, m.menu_viewport.1.saturating_sub(height) as i32), 244, height)
}
pub fn buttons(app: &ApplicationSession, b: Rect, e: &Environment) -> Vec<Button> {
    let m = &app.manager;
    let mut out = Vec::new();
    for (i, t) in ["File", "Options", "View", "Windows", "Help"]
        .iter()
        .enumerate()
    {
        if i == 3 && m.page != 0 {
            continue;
        }
        add(&mut out, menu::title_bounds(m, b, i), *t, Command::Menu(i));
    }
    for (i, t) in TABS.iter().enumerate() {
        add(&mut out, tab(b, i), *t, Command::Page(i));
    }
    let y = b.y + b.height as i32 - 55;
    let count = rows(m, b);
    for (i, (label, _)) in headers(m.page).iter().enumerate() {
        if m.columns[m.page] & (1 << i) == 0 {
            continue;
        }
        add(
            &mut out,
            cell(b, m, i, b.y + 65, 22),
            *label,
            Command::Sort(i),
        );
    }
    for (i, id) in row_ids(m, e)
        .into_iter()
        .skip(m.offset)
        .take(count)
        .enumerate()
    {
        add(&mut out, row_bounds(b, i), "", Command::Row(id));
    }
    match m.page {
        0 => {
            for (i, (label, command)) in [
                ("End Task", Command::End),
                ("Switch To", Command::Switch),
                ("New Task...", Command::New),
            ]
            .into_iter()
            .enumerate()
            {
                add(
                    &mut out,
                    Rect::new(b.x + b.width as i32 - 286 + i as i32 * 90, y, 86, 23),
                    label,
                    command,
                );
            }
        }
        1 => {
            add(
                &mut out,
                Rect::new(b.x + 16, y, b.width.saturating_sub(130), 23),
                "Show processes from all users",
                Command::All,
            );
            add(
                &mut out,
                Rect::new(b.x + b.width as i32 - 112, y, 96, 23),
                "End Process...",
                Command::End,
            );
        }
        2 => {
            add(
                &mut out,
                Rect::new(b.x + 16, y, 96, 23),
                "Start Service",
                Command::Start(true),
            );
            add(
                &mut out,
                Rect::new(b.x + 118, y, 96, 23),
                "Stop Service",
                Command::Start(false),
            );
        }
        3 => add(
            &mut out,
            Rect::new(b.x + b.width as i32 - 162, y, 146, 23),
            "Resource Monitor...",
            Command::Resource,
        ),
        5 => {
            for (i, (label, command)) in [
                ("Disconnect", Command::Start(false)),
                ("Log Off", Command::End),
                ("Connect", Command::Switch),
            ]
            .into_iter()
            .enumerate()
            {
                add(
                    &mut out,
                    Rect::new(b.x + b.width as i32 - 286 + i as i32 * 90, y, 86, 23),
                    label,
                    command,
                );
            }
        }
        _ => {}
    }
    if m.menu.is_some() {
        menu::buttons(m, b, &mut out);
    }
    if let Some(mask) = m.column_dialog {
        let r = confirm_rect(b);
        for (i, (name, _)) in headers(m.page).iter().enumerate() {
            add(
                &mut out,
                Rect::new(r.x + 20, r.y + 32 + i as i32 * 26, 330, 24),
                format!("{} {}", if mask & (1 << i) != 0 { "✓" } else { "  " }, name),
                Command::Column(i),
            );
        }
        let y = r.y + 40 + headers(m.page).len() as i32 * 26;
        add(
            &mut out,
            Rect::new(r.x + 175, y, 80, 24),
            "OK",
            Command::ColumnsDone(true),
        );
        add(
            &mut out,
            Rect::new(r.x + 265, y, 90, 24),
            "Cancel",
            Command::ColumnsDone(false),
        );
    }
    if m.confirmation.is_some() {
        let r = confirm_rect(b);
        add(
            &mut out,
            Rect::new(r.x + 125, r.y + 95, 120, 27),
            "End Process",
            Command::Confirm(true),
        );
        add(
            &mut out,
            Rect::new(r.x + 255, r.y + 95, 90, 27),
            "Cancel",
            Command::Confirm(false),
        );
    }
    if !app.status.is_empty() {
        add(&mut out, notice_button(app, b), "OK", Command::Dismiss);
    }
    out
}
fn confirm_rect(b: Rect) -> Rect {
    Rect::new(
        b.x + (b.width as i32 - 380) / 2,
        b.y + (b.height as i32 - 148) / 2,
        380,
        148,
    )
}
fn notice_lines(message: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in message.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > 46 {
                lines.push(core::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}
fn notice_rect(app: &ApplicationSession, b: Rect) -> Rect {
    let mut r = confirm_rect(b);
    r.height = (88 + notice_lines(&app.status).len() as u32 * 20).max(176);
    // Keep short operation notices at their existing position. Longer help
    // text grows upward as well as downward so the OK button remains visible.
    r.y = r.y.min(b.y + b.height as i32 - r.height as i32 - 8).max(b.y + 4);
    r
}
fn notice_button(app: &ApplicationSession, b: Rect) -> Rect {
    let r = notice_rect(app, b);
    Rect::new(r.x + r.width as i32 - 98, r.y + r.height as i32 - 39, 80, 24)
}
pub fn click(
    app: &mut ApplicationSession,
    b: Rect,
    e: &Environment,
    x: i32,
    y: i32,
    right: bool,
) -> AppAction {
    let hit = buttons(app, b, e)
        .into_iter()
        .rev()
        .find(|v| v.bounds.contains(x, y));
    let Some(Button {
        command: ViewCommand::TaskManager(c),
        ..
    }) = hit
    else {
        app.manager.menu = None;
        app.manager.context = None;
        return AppAction::None;
    };
    if app.manager.column_dialog.is_some()
        && !matches!(c, Command::Column(_) | Command::ColumnsDone(_))
    {
        return AppAction::None;
    }
    if !app.status.is_empty() && c != Command::Dismiss {
        return AppAction::None;
    }
    if app.manager.menu.is_some() && !matches!(c, Command::Menu(_) | Command::Item(_)) {
        app.manager.menu = None;
        app.manager.context = None;
        return AppAction::None;
    }
    if app.manager.confirmation.is_some() && !matches!(c, Command::Confirm(_)) {
        return AppAction::None;
    }
    if right {
        if let Command::Row(id) = c {
            app.manager.selected = Some(id);
            if app.manager.page == 1 {
                app.manager.menu = Some(5);
                app.manager.context = Some((
                    x.min(b.x + b.width as i32 - 270),
                    y.min(b.y + b.height as i32 - 238),
                ));
            }
        }
        return AppAction::None;
    }
    execute(app, c, e)
}
fn execute(app: &mut ApplicationSession, c: Command, e: &Environment) -> AppAction {
    if !enabled(&app.manager, &c, e) {
        return AppAction::None;
    }
    if c == Command::Dismiss {
        app.status.clear();
        return AppAction::None;
    }
    let m = &mut app.manager;
    match c {
        Command::Page(p) => {
            m.submenu = None;
            m.column_dialog = None;
            m.page = p.min(5);
            m.selected = None;
            m.offset = 0;
            m.sort = 0;
            m.descending = false;
            m.menu = None;
            m.context = None;
        }
        Command::Row(id) => {
            m.selected = Some(id);
            m.menu = None;
            m.context = None;
        }
        Command::Scroll(d) => {
            let len = match m.page {
                0 => e.tasks.len(),
                1 => processes(m, e).len(),
                2 => e.services.len(),
                4 => m.last.as_ref().map_or(0, |s| s.network.len()),
                5 => e.users.len(),
                _ => 0,
            };
            m.offset = if d < 0 {
                m.offset.saturating_sub(1)
            } else {
                (m.offset + 1).min(len.saturating_sub(1))
            };
        }
        Command::Sort(s) => {
            if m.sort == s {
                m.descending = !m.descending;
            } else {
                m.sort = s;
                m.descending = false;
            }
        }
        Command::Menu(i) => {
            m.menu = if m.menu == Some(i) { None } else { Some(i) };
            m.context = None;
            m.submenu = None;
            m.menu_focus = 0;
        }
        Command::New => return AppAction::Manage(Action::NewTask),
        Command::Resource => return AppAction::Launch(AppId::ResourceMonitor),
        Command::All => {
            m.show_all = !m.show_all;
            m.offset = 0;
        }
        Command::End => {
            if let Some(id) = m.selected {
                match m.page {
                    0 => return AppAction::EndTask(id as u32),
                    1 => m.confirmation = Some((id, false)),
                    5 => {
                        if let Some(u) = e.users.get(id as usize) {
                            return AppAction::Manage(Action::User(u.0.clone(), true));
                        }
                    }
                    _ => {}
                }
            }
        }
        Command::Switch => {
            if let Some(id) = m.selected {
                if m.page == 0 {
                    return AppAction::Activate(id as u32);
                }
                if m.page == 5 {
                    if let Some(u) = e.users.get(id as usize) {
                        return AppAction::Manage(Action::User(u.0.clone(), false));
                    }
                }
            }
        }
        Command::Start(start) => {
            if let Some(id) = m.selected {
                if m.page == 2 {
                    if let Some(s) = e.services.get(id as usize) {
                        return AppAction::Manage(Action::Service(s.name.clone(), start));
                    }
                } else if m.page == 5 {
                    return AppAction::Manage(Action::Power(PowerAction::SwitchUser));
                }
            }
        }
        Command::Confirm(yes) => {
            if let Some((id, tree)) = m.confirmation.take() {
                if yes {
                    return AppAction::Manage(Action::Process(
                        id,
                        if tree { "tree" } else { "end" }.into(),
                    ));
                }
            }
        }
        Command::Item(n) => return menu::execute(app, n, e),
        Command::Column(i) => {
            if let Some(mask) = &mut m.column_dialog {
                *mask ^= 1 << i;
            }
        }
        Command::ColumnsDone(ok) => {
            if let Some(mask) = m.column_dialog.take() {
                if ok && mask != 0 {
                    m.columns[m.page] = mask;
                    m.sort = 0;
                }
            }
        }
        Command::Dismiss => {}
    }
    AppAction::None
}
pub fn key(app: &mut ApplicationSession, key: u8) -> AppAction {
    if !app.status.is_empty() {
        if matches!(key, 27 | 13 | 10) {
            app.status.clear();
        }
        return AppAction::None;
    }
    if let Some(action) = menu::key(app, key) {
        return action;
    }
    let m = &mut app.manager;
    if key == 27 {
        m.menu = None;
        m.context = None;
        m.confirmation = None;
        app.status.clear();
    } else if key == 9 {
        m.page = (m.page + 1) % 6;
        m.offset = 0;
        m.selected = None;
        m.menu = None;
        m.submenu = None;
        m.context = None;
        m.sort = 0;
    } else if matches!(key, 13 | 10) && m.confirmation.is_some() {
        let (id, tree) = m.confirmation.take().unwrap();
        return AppAction::Manage(Action::Process(
            id,
            if tree { "tree" } else { "end" }.into(),
        ));
    } else if key == 127 {
        if let Some(id) = m.selected {
            if m.page == 0 {
                return AppAction::EndTask(id as u32);
            }
            if m.page == 1 {
                m.confirmation = Some((id, false));
            }
        }
    }
    AppAction::None
}
fn graph(frame: &mut Surface, r: Rect, h: &[(u32, u32)], memory: bool) {
    frame.fill_rect(r.x, r.y, r.width, r.height, 0xFF000000);
    for x in (0..r.width).step_by(16) {
        frame.fill_rect(r.x + x as i32, r.y, 1, r.height, 0xFF003C18);
    }
    for y in (0..r.height).step_by(16) {
        frame.fill_rect(r.x, r.y + y as i32, r.width, 1, 0xFF003C18);
    }
    trace(frame, r, h, memory, 0xff00ff00);
}
fn trace(frame: &mut Surface, r: Rect, h: &[(u32, u32)], memory: bool, color: u32) {
    let values: Vec<_> = h.iter().rev().take(r.width as usize).rev().collect();
    let mut last = None;
    for (i, v) in values.iter().enumerate() {
        let x = r.x + r.width as i32 - values.len() as i32 + i as i32;
        let y = r.y + r.height as i32
            - 1
            - (if memory { v.1 } else { v.0 }.min(100) * r.height.saturating_sub(1) / 100) as i32;
        if let Some(py) = last {
            let top = y.min(py);
            frame.fill_rect(x, top, 1, (y - py).unsigned_abs() + 1, color);
        } else {
            frame.fill_rect(x, y, 1, 1, color);
        }
        last = Some(y);
    }
}
// Sort references/indices, never cloned service text, on the input path.
fn service_order(m: &Manager, e: &Environment) -> Vec<usize> {
    let mut order: Vec<_> = (0..e.services.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&e.services[a], &e.services[b]);
        let result = match m.sort {
            1 => a.pid.cmp(&b.pid),
            2 => a.description.cmp(&b.description),
            3 => service_state(a.state).cmp(service_state(b.state)),
            4 => a.group.cmp(&b.group),
            _ => a.name.cmp(&b.name),
        };
        if m.descending {
            result.reverse()
        } else {
            result
        }
    });
    order
}
fn service_state(state: u32) -> &'static str {
    match state {
        4 => "Running",
        1 => "Stopped",
        2 => "Starting",
        3 => "Stopping",
        7 => "Paused",
        _ => "Pending",
    }
}
fn service_cells(s: &super::system_monitor::Service) -> Vec<String> {
    alloc::vec![
        s.name.clone(),
        if s.pid == 0 {
            String::new()
        } else {
            format!("{}", s.pid)
        },
        s.description.clone(),
        service_state(s.state).into(),
        s.group.clone()
    ]
}
fn row_ids(m: &Manager, e: &Environment) -> Vec<u64> {
    if m.page == 2 {
        return service_order(m, e).into_iter().map(|i| i as u64).collect();
    }
    row_values(m, e).into_iter().map(|r| r.0).collect()
}
/// List rows in Windows Task Manager do not change appearance on pointer hover.
/// This path runs before the host clones telemetry or builds hit targets.
pub fn menu_hover(app: &mut ApplicationSession, b: Rect, x: i32, y: i32) -> bool {
    menu::hover(app, b, x, y)
}
pub fn popup_contains(app: &ApplicationSession, b: Rect, x: i32, y: i32) -> bool {
    menu::contains(&app.manager, b, x, y)
}
pub fn quiet_pointer(app: &mut ApplicationSession, b: Rect, x: i32, y: i32) -> Option<bool> {
    if app.manager.menu.is_none()
        && app.manager.column_dialog.is_none()
        && app.manager.confirmation.is_none()
        && app.status.is_empty()
        && app.dialog.is_none()
        && app.common_dialog.is_none()
        && app.manager.scroll_grab.is_none()
        && list_bounds(b).contains(x, y)
        && y >= list_bounds(b).y + 23
    {
        return Some(app.hot.take().is_some());
    }
    None
}
fn row_values(m: &Manager, e: &Environment) -> Vec<(u64, Vec<String>, Option<AppId>)> {
    let mut values: Vec<(u64, Vec<String>, Option<AppId>)> = match m.page {
        0 => e
            .tasks
            .iter()
            .map(|t| {
                (
                    t.id as u64,
                    alloc::vec![t.title.clone(), String::from("Running")],
                    t.icon,
                )
            })
            .collect(),
        1 => processes(m, e)
            .into_iter()
            .map(|p| {
                (
                    p.pid,
                    alloc::vec![
                        if p.name.is_empty() {
                            format!("Process {}", p.pid)
                        } else {
                            p.name.clone()
                        },
                        format!("{}", p.pid),
                        p.user.clone(),
                        process_cpu(m, p),
                        format!("{} K", p.working_set / 1024)
                    ],
                    None,
                )
            })
            .collect(),
        2 => e
            .services
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    i as u64,
                    alloc::vec![
                        s.name.clone(),
                        if s.pid == 0 {
                            String::new()
                        } else {
                            format!("{}", s.pid)
                        },
                        s.description.clone(),
                        String::from(match s.state {
                            4 => "Running",
                            1 => "Stopped",
                            2 => "Starting",
                            3 => "Stopping",
                            7 => "Paused",
                            _ => "Pending",
                        }),
                        s.group.clone()
                    ],
                    None,
                )
            })
            .collect(),
        4 => m
            .last
            .iter()
            .flat_map(|s| s.network.iter())
            .enumerate()
            .map(|(i, n)| {
                let base = m
                    .network_reset
                    .as_ref()
                    .and_then(|s| s.network.iter().find(|x| x.index == n.index));
                let sent = n.sent.saturating_sub(base.map_or(0, |n| n.sent));
                let received = n.received.saturating_sub(base.map_or(0, |n| n.received));
                let rate = m
                    .network_history
                    .iter()
                    .find(|(id, _)| *id == n.index)
                    .and_then(|(_, h)| h.last())
                    .map_or(0, |v| u64::from(v.0) + u64::from(v.1));
                let (a, b) = if m.cumulative {
                    (sent, received)
                } else {
                    m.network_history
                        .iter()
                        .find(|(id, _)| *id == n.index)
                        .and_then(|(_, h)| h.last())
                        .map_or((0, 0), |v| (u64::from(v.0), u64::from(v.1)))
                };
                (
                    i as u64,
                    alloc::vec![
                        format!("Adapter {}", n.index),
                        if n.speed > 0 {
                            format!("{}%", (rate as u128 * 800 / n.speed as u128).min(100))
                        } else {
                            "--".into()
                        },
                        format!("{} Mbps", n.speed / 1_000_000),
                        if n.up {
                            "Connected".into()
                        } else {
                            "Disconnected".into()
                        },
                        format!("{a}"),
                        format!("{b}"),
                        format!("{}", a.saturating_add(b))
                    ],
                    None,
                )
            })
            .collect(),
        5 => e
            .users
            .iter()
            .enumerate()
            .map(|(i, (name, active))| {
                (
                    i as u64,
                    alloc::vec![
                        if m.full_names {
                            name.clone()
                        } else {
                            name.rsplit('\\').next().unwrap_or(name).into()
                        },
                        format!("{}", i + 1),
                        String::from(if *active { "Active" } else { "Disconnected" }),
                        String::new(),
                        String::from("Console")
                    ],
                    None,
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    if m.page != 1 {
        values.sort_by(|a, b| {
            if m.page == 2 && m.sort == 1 {
                e.services[a.0 as usize]
                    .pid
                    .cmp(&e.services[b.0 as usize].pid)
            } else {
                a.1.get(m.sort).cmp(&b.1.get(m.sort))
            }
        });
        if m.descending {
            values.reverse();
        }
    }
    values
}
fn row_cells(
    app: &ApplicationSession,
    f: &mut Surface,
    b: Rect,
    e: &Environment,
    text: &impl Fn(&mut Surface, &str, Rect, u32),
) {
    let m = &app.manager;
    let values = if m.page == 2 {
        service_order(m, e)
            .into_iter()
            .skip(m.offset)
            .take(rows(m, b))
            .map(|i| (i as u64, service_cells(&e.services[i]), None))
            .collect()
    } else {
        row_values(m, e)
            .into_iter()
            .skip(m.offset)
            .take(rows(m, b))
            .collect::<Vec<_>>()
    };
    for (i, (_, cells, icon)) in values.iter().enumerate() {
        let row = row_bounds(b, i);
        for (j, value) in cells.iter().enumerate() {
            if m.columns[m.page] & (1 << j) == 0 {
                continue;
            }
            let mut r = cell(b, m, j, row.y + 3, 17);
            r.x += 5;
            r.width = r.width.saturating_sub(10);
            if j == 0 && m.page == 0 {
                if let Some(icon) = icon {
                    super::icons::draw(f, *icon, Rect::new(r.x, row.y + 2, 16, 16));
                } else {
                    f.stroke_rounded_rect(Rect::new(r.x + 1, row.y + 3, 14, 13), 0, 1, 0xFF8498A8);
                }
                r.x += 21;
                r.width = r.width.saturating_sub(21);
            }
            text(f, value, r, 0xFF000000);
        }
    }
}
fn process_cpu_value(m: &Manager, pid: u64) -> u32 {
    m.process_cpu
        .iter()
        .find(|(id, _)| *id == pid)
        .map_or(0, |(_, v)| *v)
}
fn process_cpu(m: &Manager, p: &super::processes::Process) -> String {
    let value = process_cpu_value(m, p.pid);
    format!("{value:02}")
}
fn draw_button(f: &mut Surface, r: Rect, hot: bool, enabled: bool) {
    f.stroke_rounded_rect(r, 2, 1, if hot { 0xFF3C7FB1 } else { 0xFF707070 });
    let inner = Rect::new(
        r.x + 1,
        r.y + 1,
        r.width.saturating_sub(2),
        r.height.saturating_sub(2),
    );
    for y in 0..inner.height {
        let color = crate::graphics::lerp_color(
            if hot { 0xFFF4FCFF } else { 0xFFF8F8F8 },
            if hot { 0xFFC4E5F6 } else { 0xFFE1E1E1 },
            y * 255 / inner.height.max(1),
        );
        f.fill_rect(
            inner.x,
            inner.y + y as i32,
            inner.width,
            1,
            if enabled { color } else { 0xFFF0F0F0 },
        );
    }
}
fn enabled(m: &Manager, c: &Command, e: &Environment) -> bool {
    match c {
        Command::Menu(3) => m.page == 0,
        Command::Item(2..=3 | 5) if m.menu == Some(3) => m.page == 0 && m.selected.is_some(),
        Command::End | Command::Switch => m.selected.is_some(),
        Command::Start(start) if m.page == 2 => m
            .selected
            .and_then(|i| e.services.get(i as usize))
            .is_some_and(|s| {
                if *start {
                    s.state == 1 && s.start_type != 4
                } else {
                    matches!(s.state, 4 | 7) && s.accepted & 1 != 0
                }
            }),
        Command::Start(_) => m.selected.is_some(),
        _ => true,
    }
}
pub fn draw(
    app: &ApplicationSession,
    f: &mut Surface,
    b: Rect,
    e: &Environment,
    text: impl Fn(&mut Surface, &str, Rect, u32),
) {
    let m = &app.manager;
    f.fill_rect(b.x, b.y, b.width, b.height, 0xFFF0F0F0);
    let pane = pane(b);
    f.fill_rect(pane.x, pane.y, pane.width, pane.height, 0xFFF9F9F9);
    f.stroke_rounded_rect(pane, 0, 1, 0xFF898C95);
    let list = list_bounds(b);
    if matches!(m.page, 0..=2 | 4..=5) {
        f.fill_rect(list.x, list.y, list.width, list.height, 0xFFFFFFFF);
        f.stroke_rounded_rect(list, 0, 1, 0xFF828790);
        let r = Rect::new(
            list.x + list.width as i32 - 17,
            list.y + 23,
            16,
            list.height.saturating_sub(24),
        );
        f.fill_rect(r.x, r.y, r.width, r.height, 0xFFF0F0F0);
    }
    if m.page == 3 {
        let gw = b.width.saturating_sub(48) / 2;
        let gh = (b.height / 4).clamp(50, 110);
        let a = Rect::new(b.x + 16, b.y + 96, gw, gh);
        let z = Rect::new(a.x + gw as i32 + 12, a.y, gw, gh);
        text(
            f,
            "CPU Usage History",
            Rect::new(a.x, a.y - 24, gw, 20),
            0xFF111111,
        );
        text(
            f,
            "Physical Memory",
            Rect::new(z.x, z.y - 24, gw, 20),
            0xFF111111,
        );
        if m.per_cpu && !m.cpu_history.is_empty() {
            let count = m.cpu_history.len().min(16);
            let columns = if count > 4 { 4 } else { count } as u32;
            let rows = (count as u32).div_ceil(columns);
            for (i, h) in m.cpu_history.iter().take(count).enumerate() {
                let r = Rect::new(
                    a.x + (i as u32 % columns * (a.width / columns)) as i32,
                    a.y + (i as u32 / columns * (a.height / rows)) as i32,
                    (a.width / columns).saturating_sub(2),
                    (a.height / rows).saturating_sub(2),
                );
                graph(f, r, h, false);
                if m.kernel_times {
                    trace(f, r, h, true, 0xffff3030);
                }
            }
        } else {
            graph(f, a, &m.history, false);
            if m.kernel_times {
                trace(f, a, &m.kernel_history, false, 0xffff3030);
            }
        }
        graph(f, z, &m.history, true);
        if let Some(s) = &m.last {
            let y = a.y + gh as i32 + 16;
            for (i, line) in [
                format!("Physical Memory (MB)            Kernel Memory (MB)"),
                format!(
                    "Total: {}                         Nonpaged: {}",
                    s.total / 1048576,
                    s.pool / 1048576
                ),
                format!(
                    "Available: {}                     Threads: {}",
                    s.available / 1048576,
                    s.threads
                ),
                format!(
                    "Processes: {}",
                    e.processes.as_ref().map_or(0, |p| p.processes.len())
                ),
                format!(
                    "Up Time: {} days {:02}:{:02}:{:02}",
                    s.uptime / 86400000,
                    s.uptime / 3600000 % 24,
                    s.uptime / 60000 % 60,
                    s.uptime / 1000 % 60
                ),
            ]
            .iter()
            .enumerate()
            {
                text(
                    f,
                    line,
                    Rect::new(b.x + 18, y + i as i32 * 20, b.width - 36, 22),
                    0xFF111111,
                );
            }
        } else {
            text(
                f,
                "Waiting for kernel performance counters...",
                Rect::new(b.x + 18, a.y + gh as i32 + 20, b.width - 36, 24),
                0xFF333333,
            );
        }
    } else if m.page == 4 {
        if let Some(sample) = &m.last {
            let top = b.y + 95 + (sample.network.len().min(5) as i32) * 20;
            let height = (b.y + b.height as i32 - 90 - top).max(30) as u32;
            let count = sample.network.len().min(4).max(1) as u32;
            let width = b.width.saturating_sub(40) / count;
            for (i, n) in sample.network.iter().take(count as usize).enumerate() {
                let r = Rect::new(
                    b.x + 18 + i as i32 * width as i32,
                    top,
                    width.saturating_sub(8),
                    height,
                );
                let history = m
                    .network_history
                    .iter()
                    .find(|(id, _)| *id == n.index)
                    .map(|(_, h)| h.as_slice())
                    .unwrap_or(&[]);
                let peak = history
                    .iter()
                    .map(|(a, b)| u64::from(*a) + u64::from(*b))
                    .max()
                    .unwrap_or(1)
                    .max(1);
                let scale = if m.auto_scale {
                    peak.max(n.speed / 8000).max(1)
                } else {
                    (n.speed / 8).max(1)
                };
                let percent: Vec<_> = history
                    .iter()
                    .map(|(a, b)| {
                        (
                            (u64::from(*a) * 100 / scale).min(100) as u32,
                            (u64::from(*b) * 100 / scale).min(100) as u32,
                        )
                    })
                    .collect();
                graph(f, r, &[], false);
                if m.network_lines & 1 != 0 {
                    trace(f, r, &percent, false, 0xffff3030);
                }
                if m.network_lines & 2 != 0 {
                    trace(f, r, &percent, true, 0xffffff00);
                }
                if m.network_lines & 4 != 0 {
                    let total: Vec<_> =
                        percent.iter().map(|(a, b)| ((a + b).min(100), 0)).collect();
                    trace(f, r, &total, false, 0xff00ff00);
                }
                if m.show_scale {
                    text(
                        f,
                        &format!("{} B/s", scale),
                        Rect::new(r.x, r.y + r.height as i32 + 2, r.width, 18),
                        0xff111111,
                    );
                }
            }
            if sample.network.is_empty() {
                text(
                    f,
                    "No network adapters available.",
                    Rect::new(b.x + 18, b.y + 100, b.width - 36, 22),
                    0xff333333,
                );
            }
        }
    }

    let controls = buttons(app, b, e);
    // Client contents first; popup menus and confirmations are composited last.
    for btn in &controls {
        let ViewCommand::TaskManager(c) = &btn.command else {
            continue;
        };
        if matches!(
            c,
            Command::Item(_)
                | Command::Confirm(_)
                | Command::Dismiss
                | Command::Column(_)
                | Command::ColumnsDone(_)
        ) {
            continue;
        }
        let hot = app.hot == Some(btn.bounds);
        let selected = matches!(c,Command::Row(id) if Some(*id)==m.selected);
        let mut label = btn.bounds;
        label.x += 5;
        label.y += 4;
        label.width = label.width.saturating_sub(10);
        match c {
            Command::Row(_) => {
                if selected {
                    aero::menu_item(f, btn.bounds, true);
                }
                continue;
            }
            Command::Page(i) => {
                let selected = *i == m.page;
                let mut r = btn.bounds;
                if selected {
                    r.y -= 2;
                    r.height += 2;
                }
                f.fill_rect(
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    if selected {
                        0xFFF9F9F9
                    } else if hot {
                        0xFFE8F4FC
                    } else {
                        0xFFE9E9E9
                    },
                );
                f.stroke_rounded_rect(r, 0, 1, 0xFF898C95);
                if selected {
                    f.fill_rect(
                        r.x + 1,
                        r.y + r.height as i32 - 1,
                        r.width - 2,
                        1,
                        0xFFF9F9F9,
                    );
                }
                label.y = btn.bounds.y + 5;
            }
            Command::Menu(i) => {
                if hot || m.menu == Some(*i) {
                    aero::menu_item(f, btn.bounds, true);
                }
            }
            Command::Sort(_) => {
                f.fill_rect(
                    btn.bounds.x,
                    btn.bounds.y,
                    btn.bounds.width,
                    btn.bounds.height,
                    0xFFF5F5F5,
                );
                f.fill_rect(
                    btn.bounds.x + btn.bounds.width as i32 - 1,
                    btn.bounds.y,
                    1,
                    btn.bounds.height,
                    0xFFD5D5D5,
                );
                f.fill_rect(
                    btn.bounds.x,
                    btn.bounds.y + btn.bounds.height as i32 - 1,
                    btn.bounds.width,
                    1,
                    0xFFD5D5D5,
                );
            }
            Command::All => {
                let r = Rect::new(btn.bounds.x, btn.bounds.y + 5, 13, 13);
                f.fill_rect(r.x, r.y, r.width, r.height, 0xFFFFFFFF);
                f.stroke_rounded_rect(r, 0, 1, 0xFF707070);
                if m.show_all {
                    f.fill_rect(r.x + 3, r.y + 3, 7, 7, 0xFF277AB5);
                }
                label.x += 15;
                label.width = label.width.saturating_sub(15);
            }
            Command::Scroll(d) => {
                draw_button(f, btn.bounds, hot, true);
                let r = btn.bounds;
                for i in 0..4 {
                    let y = if *d < 0 { r.y + 5 + i } else { r.y + 9 - i };
                    f.fill_rect(r.x + 7 - i, y, 1 + i as u32 * 2, 1, 0xFF444444);
                }
                continue;
            }
            _ => draw_button(f, btn.bounds, hot, enabled(m, c, e)),
        }
        text(
            f,
            &btn.label,
            label,
            if enabled(m, c, e) {
                0xFF000000
            } else {
                0xFF888888
            },
        );
    }
    row_cells(app, f, b, e, &text);
    let cpu = m.cpu.map_or_else(|| "--".into(), |v| format!("{v}%"));
    let mem = m.last.as_ref().filter(|s| s.total > 0).map_or_else(
        || "--".into(),
        |s| {
            format!(
                "{}%",
                (s.total - s.available) as u128 * 100 / s.total as u128
            )
        },
    );
    let status_y = b.y + b.height as i32 - 21;
    f.fill_rect(b.x, status_y - 2, b.width, 1, 0xFFA0A0A0);
    for (i, (label, x, w)) in [
        (
            format!(
                "Processes: {}",
                e.processes.as_ref().map_or(0, |s| s.processes.len())
            ),
            0,
            100,
        ),
        (format!("CPU Usage: {cpu}"), 100, 125),
        (
            format!("Physical Memory: {mem}"),
            225,
            b.width.saturating_sub(225),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            f.fill_rect(b.x + x - 2, status_y, 1, 20, 0xFFCCCCCC);
        }
        text(
            f,
            &label,
            Rect::new(b.x + x + 5, status_y + 3, w.saturating_sub(8), 18),
            0xFF000000,
        );
    }
    if let Some(scroll) = scrollbar(m, b, e) {
        scroll.draw(f, false, m.scroll_grab.is_some());
    }
    if m.menu.is_some() {
        menu::draw(app, f, b, e, &text);
    }
    if m.column_dialog.is_some() {
        let mut r = confirm_rect(b);
        r.height = 90 + headers(m.page).len() as u32 * 26;
        aero::shadow(f, r, 8);
        f.fill_rect(r.x, r.y, r.width, r.height, 0xfff0f0f0);
        text(
            f,
            "Select Columns",
            Rect::new(r.x + 16, r.y + 6, 300, 24),
            0xff173e64,
        );
        for btn in &controls {
            if matches!(
                btn.command,
                ViewCommand::TaskManager(Command::Column(_) | Command::ColumnsDone(_))
            ) {
                draw_button(f, btn.bounds, app.hot == Some(btn.bounds), true);
                text(
                    f,
                    &btn.label,
                    Rect::new(
                        btn.bounds.x + 6,
                        btn.bounds.y + 3,
                        btn.bounds.width - 12,
                        20,
                    ),
                    0xff111111,
                );
            }
        }
    }
    if m.confirmation.is_some() {
        let r = confirm_rect(b);
        aero::shadow(f, r, 10);
        f.fill_rect(r.x, r.y, r.width, r.height, 0xFFF0F0F0);
        text(
            f,
            "Do you want to end this process?",
            Rect::new(r.x + 16, r.y + 12, 350, 25),
            0xFF173E64,
        );
        text(
            f,
            "Unsaved data in this process will be lost.",
            Rect::new(r.x + 16, r.y + 47, 350, 34),
            0xFF111111,
        );
        for btn in &controls {
            if matches!(btn.command, ViewCommand::TaskManager(Command::Confirm(_))) {
                draw_button(f, btn.bounds, app.hot == Some(btn.bounds), true);
                text(
                    f,
                    &btn.label,
                    Rect::new(
                        btn.bounds.x + 5,
                        btn.bounds.y + 5,
                        btn.bounds.width - 10,
                        20,
                    ),
                    0xFF000000,
                );
            }
        }
    }
    if !app.status.is_empty() {
        let r = notice_rect(app, b);
        aero::shadow(f, r, 10);
        f.fill_rect(r.x, r.y, r.width, r.height, 0xFFF0F0F0);
        f.stroke_rounded_rect(r, 0, 1, 0xFF777777);
        text(
            f,
            "Windows Task Manager",
            Rect::new(r.x + 16, r.y + 12, r.width - 32, 24),
            0xFF173E64,
        );
        let mut y = r.y + 42;
        for line in notice_lines(&app.status) {
            text(f, &line, Rect::new(r.x + 16, y, r.width - 32, 20), 0xFF111111);
            y += 20;
        }
        let button = notice_button(app, b);
        draw_button(f, button, app.hot == Some(button), true);
        text(
            f,
            "OK",
            Rect::new(button.x + 30, button.y + 4, 40, 20),
            0xFF000000,
        );
    }
}
impl crate::shell::ShellModel {
    pub fn sample_task_manager(&mut self) {
        let mut refresh_services = self.service_snapshot.is_empty();
        if let Ok(sample) = crate::platform::services::monitor() {
            for app in &mut self.applications {
                if app.id == AppId::TaskManager {
                    if (!app.manager.paused || app.manager.refresh)
                        && (app.manager.refresh
                            || app.manager.sampled == 0
                            || sample.uptime.saturating_sub(app.manager.sampled)
                                >= app.manager.interval)
                    {
                        refresh_services |= app.manager.page == 2;
                        if let Some(snapshot) = &self.process_snapshot {
                            app.manager.observe_processes(snapshot, sample.ticks);
                        }
                    }
                    let paused = app.manager.paused;
                    if app.manager.refresh {
                        app.manager.paused = false;
                        app.manager.sampled = 0;
                    }
                    app.manager.observe(&sample);
                    app.manager.paused = paused;
                    app.manager.refresh = false;
                }
            }
            self.monitor_sample = Some(sample);
        }
        // Service metadata changes independently of process CPU sampling. Query
        // it only while the Services tab is visible, or before the first snapshot.
        if refresh_services {
            if let Ok(services) = crate::platform::services::service_list() {
                self.service_snapshot = services;
            }
        }
    }
    pub fn task_manager_action(&mut self, id: u32, action: Action) {
        let result = match action {
            Action::Process(pid, c) => crate::platform::services::task_control(pid, &c),
            Action::Service(name, start) => {
                crate::platform::services::service_control(&name, start)
            }
            Action::Power(a) => {
                self.power_action = Some(a);
                Ok(())
            }
            Action::Refresh => {
                self.sample_task_manager();
                Ok(())
            }
            Action::Arrange(kind) => {
                let selected = self
                    .application(id)
                    .and_then(|a| a.manager.selected)
                    .map(|v| v as u32);
                let targets: Vec<_> = self
                    .taskbar
                    .windows
                    .iter()
                    .filter(|w| w.id != id)
                    .map(|w| w.id)
                    .collect();
                let count = targets.len().max(1) as i32;
                let (width, height) = (
                    self.viewport.0 as i32,
                    self.viewport.1.saturating_sub(40) as i32,
                );
                for (i, target) in targets.iter().enumerate() {
                    if let Some(w) = self.taskbar.windows.iter_mut().find(|w| w.id == *target) {
                        if (kind <= 1 || kind == 4 || (kind == 3 && Some(*target) == selected))
                            && w.restore_geometry.is_none()
                        {
                            w.restore_geometry = Some((w.x, w.y, w.width, w.height));
                        }
                        if kind <= 1 {
                            w.minimized = false;
                            w.maximized = false;
                            if kind == 0 {
                                w.x = 0;
                                w.y = i as i32 * height / count;
                                w.width = width;
                                w.height = height / count;
                            } else {
                                w.x = i as i32 * width / count;
                                w.y = 0;
                                w.width = width / count;
                                w.height = height;
                            }
                        } else if kind == 4 {
                            w.minimized = false;
                            w.maximized = false;
                            w.x = 24 * i as i32;
                            w.y = 24 * i as i32;
                            w.width = (width * 3 / 4).max(200);
                            w.height = (height * 3 / 4).max(150);
                        } else if Some(*target) == selected {
                            if kind == 2 {
                                w.minimized = true;
                            } else if kind == 3 {
                                w.minimized = false;
                                w.maximized = true;
                                w.x = 0;
                                w.y = 0;
                                w.width = width;
                                w.height = height;
                            }
                        }
                    }
                }
                if kind == 5 {
                    if let Some(target) = selected {
                        if let Some(w) = self.taskbar.windows.iter_mut().find(|w| w.id == target) {
                            w.minimized = false;
                        }
                        self.select_window(target);
                    }
                }
                Ok(())
            }
            Action::NewTask => {
                self.launch_application(AppId::Run);
                Ok(())
            }
            Action::Minimize(target) => {
                if let Some(w) = self.taskbar.windows.iter_mut().find(|w| w.id == target) {
                    w.minimized = true;
                }
                Ok(())
            }
            Action::User(name, logoff) => {
                if name.eq_ignore_ascii_case(&self.session.identity) {
                    self.power_action = Some(if logoff {
                        PowerAction::LogOff
                    } else {
                        PowerAction::Lock
                    });
                    Ok(())
                } else if logoff {
                    Err(nt61_abi::desktop_bridge::STATUS_NOT_SUPPORTED)
                } else {
                    self.power_action = Some(PowerAction::SwitchUser);
                    self.session.user = name;
                    Ok(())
                }
            }
        };
        if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
            app.status = match result {
                Ok(()) => String::new(),
                Err(s) if s & 0xFFFF0000 == 0xC0070000 => {
                    format!("{} (Error {})", service_error(s & 0xFFFF), s & 0xFFFF)
                }
                Err(s) => format!("Operation failed (0x{s:08X})."),
            };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_history_and_counter_reset() {
        let mut m = Manager::default();
        m.observe(&Sample {
            ticks: 100,
            idle: 20,
            uptime: 1000,
            ..Sample::default()
        });
        m.observe(&Sample {
            ticks: 200,
            idle: 70,
            uptime: 2000,
            ..Sample::default()
        });
        assert_eq!(m.cpu, Some(50));
        m.paused = true;
        m.observe(&Sample {
            ticks: 400,
            uptime: 3000,
            ..Sample::default()
        });
        assert_eq!(m.history.len(), 1);
        m.paused = false;
        m.observe(&Sample {
            ticks: 1,
            uptime: 4000,
            ..Sample::default()
        });
        assert_eq!(m.cpu, None);
    }
    #[test]
    fn cancel_cannot_end_process() {
        let mut a = ApplicationSession::new(1, AppId::TaskManager, 0);
        a.manager.page = 1;
        a.manager.selected = Some(44);
        assert_eq!(key(&mut a, 127), AppAction::None);
        assert!(a.manager.confirmation.is_some());
        assert_eq!(key(&mut a, 27), AppAction::None);
        assert_eq!(key(&mut a, 13), AppAction::None);
    }
    #[test]
    fn popup_is_painted_after_every_client_control() {
        use core::cell::RefCell;
        let mut a = ApplicationSession::new(1, AppId::TaskManager, 0);
        a.manager.page = 1;
        a.manager.menu = Some(0);
        let mut f = Surface::new(480, 400);
        let e = Environment {
            width: 480,
            height: 400,
            network_adapters: 0,
            audio: false,
            tasks: Vec::new(),
            processes: None,
            monitor: None,
            services: Vec::new(),
            users: Vec::new(),
        };
        let labels = RefCell::new(Vec::new());
        draw(&a, &mut f, Rect::new(0, 0, 480, 400), &e, |_, t, _, _| {
            labels.borrow_mut().push(String::from(t))
        });
        assert_eq!(
            labels.borrow().last().map(String::as_str),
            Some("Exit Task Manager")
        );
        let mut expected = Surface::new(480, 400);
        aero::menu_surface(
            &mut expected,
            menu_bounds(&a.manager, Rect::new(0, 0, 480, 400)),
        );
        assert_eq!(
            f.pixel(100, 58),
            expected.pixel(100, 58),
            "tab background must not paint through menu"
        );
    }
}

fn service_error(code: u32) -> &'static str {
    match code {
        1051 => "A dependent service is still running.",
        1052 => "This service does not accept this control.",
        1056 => "This service is already running.",
        1058 => "This service is disabled.",
        1059 => "Circular service dependency.",
        1060 => "This service is not installed.",
        1061 => "The service is busy.",
        1062 => "This service is not running.",
        1068 => "A dependency failed to start.",
        1075 => "A required dependency is missing.",
        120 => "The executable service host is not implemented.",
        _ => "The service operation failed.",
    }
}

fn scrollbar(m: &Manager, b: Rect, e: &Environment) -> Option<super::shared::scrollbar::Scrollbar> {
    if !matches!(m.page, 0..=2 | 5) {
        return None;
    }
    let r = list_bounds(b);
    super::shared::scrollbar::Scrollbar::new(
        Rect::new(
            r.x + r.width as i32 - 17,
            r.y + 23,
            16,
            r.height.saturating_sub(24),
        ),
        entry_count(m, e),
        rows(m, b),
        m.offset,
    )
}
pub fn wheel(app: &mut ApplicationSession, b: Rect, e: &Environment, delta: i32) -> bool {
    if !app.status.is_empty() || app.manager.menu.is_some() || app.manager.confirmation.is_some() {
        return false;
    }
    let Some(scroll) = scrollbar(&app.manager, b, e) else {
        return false;
    };
    let old = app.manager.offset;
    let amount = delta.unsigned_abs() as usize * 3;
    app.manager.offset = if delta > 0 {
        old.saturating_sub(amount)
    } else {
        old.saturating_add(amount).min(scroll.maximum)
    };
    old != app.manager.offset
}
pub fn scroll_pointer(
    app: &mut ApplicationSession,
    b: Rect,
    e: &Environment,
    x: i32,
    y: i32,
    down: bool,
    held: bool,
) -> bool {
    let m = &mut app.manager;
    if !held && !down && m.scroll_grab.take().is_some() {
        return true;
    }
    if !app.status.is_empty() || m.menu.is_some() || m.confirmation.is_some() {
        return false;
    }
    let Some(scroll) = scrollbar(m, b, e) else {
        return false;
    };
    if let Some(grab) = m.scroll_grab {
        if held {
            m.offset = scroll.drag_offset(y, grab);
            return true;
        }
    }
    if !down || !scroll.bounds.contains(x, y) {
        return false;
    }
    if scroll.thumb.contains(x, y) {
        m.scroll_grab = Some(y - scroll.thumb.y);
    } else {
        let amount = if scroll.arrow(y).is_some() {
            1
        } else {
            rows(m, b)
        };
        m.offset = if y < scroll.thumb.y {
            m.offset.saturating_sub(amount)
        } else {
            m.offset.saturating_add(amount).min(scroll.maximum)
        };
    }
    true
}

#[cfg(test)]
mod table_tests {
    use super::*;
    fn environment() -> Environment {
        Environment {
            width: 480,
            height: 400,
            network_adapters: 0,
            audio: false,
            tasks: (0..40)
                .map(|i| super::super::Task {
                    id: i,
                    title: format!("Task {i:02}"),
                    icon: None,
                    minimized: i % 2 == 0,
                })
                .collect(),
            processes: None,
            monitor: None,
            services: Vec::new(),
            users: Vec::new(),
        }
    }
    #[test]
    fn help_notice_keeps_every_line_and_dismiss_button_inside_default_window() {
        let b = Rect::new(66, 90, 468, 404);
        let mut app = ApplicationSession::new(1, AppId::TaskManager, 0);
        key(&mut app, super::super::explorer::input::HELP);
        let r = notice_rect(&app, b);
        let lines = notice_lines(&app.status);
        let button = notice_button(&app, b);
        assert!(lines.len() > 5);
        assert!(r.y >= b.y && r.y + r.height as i32 <= b.y + b.height as i32);
        assert!(r.y + 42 + lines.len() as i32 * 20 <= button.y);
        assert!(b.contains(button.x + 40, button.y + 12));
        click(&mut app, b, &environment(), button.x + 40, button.y + 12, false);
        assert!(app.status.is_empty());
    }
    #[test]
    fn operation_notice_blocks_task_actions_until_dismissed() {
        let e = environment();
        let b = Rect::new(0, 0, 480, 400);
        let mut app = ApplicationSession::new(1, AppId::TaskManager, 0);
        app.manager.selected = Some(7);
        app.status = "This service does not accept this control. (Error 1052)".into();
        assert_eq!(key(&mut app, 127), AppAction::None);
        assert_eq!(key(&mut app, 9), AppAction::None);
        assert_eq!(app.manager.page, 0);
        assert!(!wheel(&mut app, b, &e, -1));
        let row = row_bounds(b, 0);
        assert_eq!(
            click(&mut app, b, &e, row.x + 5, row.y + 5, false),
            AppAction::None
        );
        assert_eq!(app.manager.selected, Some(7));
        let button = notice_button(&app, b);
        assert_eq!(
            click(&mut app, b, &e, button.x + 10, button.y + 10, false),
            AppAction::None
        );
        assert!(app.status.is_empty());
        assert_eq!(key(&mut app, 127), AppAction::EndTask(7));
    }
    #[test]
    fn sorting_scroll_and_hit_testing_use_the_same_rows() {
        let e = environment();
        let b = Rect::new(0, 0, 480, 400);
        let mut app = ApplicationSession::new(1, AppId::TaskManager, 0);
        app.manager.descending = true;
        let rows = row_values(&app.manager, &e);
        assert_eq!(rows[0].0, 39);
        assert_eq!(rows[0].1[1], "Running");
        let bounds = row_bounds(b, 0);
        click(&mut app, b, &e, bounds.x + 5, bounds.y + 5, false);
        assert_eq!(app.manager.selected, Some(39));
        assert!(wheel(&mut app, b, &e, -1));
        assert_eq!(app.manager.offset, 3);
        let scrollbar = scrollbar(&app.manager, b, &e).unwrap();
        assert!(scroll_pointer(
            &mut app,
            b,
            &e,
            scrollbar.thumb.x + 2,
            scrollbar.thumb.y + 2,
            true,
            true
        ));
        assert!(scroll_pointer(
            &mut app,
            b,
            &e,
            scrollbar.thumb.x + 2,
            500,
            false,
            true
        ));
        assert_eq!(app.manager.offset, scrollbar.maximum);
        assert!(scroll_pointer(&mut app, b, &e, 100, 500, false, false));
        assert_eq!(app.manager.scroll_grab, None);
    }
    #[test]
    fn process_cpu_uses_counter_deltas_and_numeric_column_sort() {
        use super::super::processes::{Process, Snapshot};
        let mut e = environment();
        e.processes = Some(Snapshot {
            cpu_count: 1,
            processes: alloc::vec![
                Process {
                    pid: 9,
                    name: "z.exe".into(),
                    user: "User".into(),
                    session: 1,
                    working_set: 1024,
                    cpu_ticks: 10
                },
                Process {
                    pid: 100,
                    name: "a.exe".into(),
                    user: "SYSTEM".into(),
                    session: 0,
                    working_set: 2048,
                    cpu_ticks: 20
                }
            ],
        });
        let mut m = Manager::default();
        m.show_all = true;
        m.sort = 1;
        assert_eq!(processes(&m, &e)[0].pid, 9);
        m.observe_processes(e.processes.as_ref().unwrap(), 100);
        e.processes.as_mut().unwrap().processes[0].cpu_ticks = 35;
        m.observe_processes(e.processes.as_ref().unwrap(), 200);
        assert_eq!(process_cpu_value(&m, 9), 25);
        m.sort = 3;
        m.descending = true;
        assert_eq!(processes(&m, &e)[0].pid, 9);
        m.observe_processes(e.processes.as_ref().unwrap(), 1);
        assert!(m.process_cpu.is_empty());
    }
}

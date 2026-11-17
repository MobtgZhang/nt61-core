//! Task Manager state and UI shared by the desktop and native image.
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
            context: None,
            confirmation: None,
            always_on_top: false,
            minimize_on_use: true,
            hide_minimized: true,
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
fn cell(b: Rect, page: usize, index: usize, y: i32, height: u32) -> Rect {
    let r = list_bounds(b);
    let columns = headers(page);
    let total = r.width.saturating_sub(18);
    let left = columns[..index].iter().map(|(_, w)| w).sum::<u32>();
    let right = left + columns[index].1;
    Rect::new(
        r.x + 1 + (total * left / 100) as i32,
        y,
        total * right / 100 - total * left / 100,
        height,
    )
}
fn entry_count(m: &Manager, e: &Environment) -> usize {
    match m.page {
        0 => e.tasks.len(),
        1 => processes(m, e).len(),
        2 => e.services.len(),
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
    match m.menu {
        Some(0) => alloc::vec![
            ("New Task (Run...)".into(), 0),
            ("Exit Task Manager".into(), 1)
        ],
        Some(1) => alloc::vec![
            (
                format!(
                    "{} Always On Top",
                    if m.always_on_top { "[x]" } else { "[ ]" }
                ),
                0
            ),
            (
                format!(
                    "{} Minimize On Use",
                    if m.minimize_on_use { "[x]" } else { "[ ]" }
                ),
                1
            ),
            (
                format!(
                    "{} Hide When Minimized",
                    if m.hide_minimized { "[x]" } else { "[ ]" }
                ),
                2
            )
        ],
        Some(2) => alloc::vec![
            ("Refresh Now".into(), 0),
            ("Update Speed: High (0.5 s)".into(), 1),
            ("Update Speed: Normal (1 s)".into(), 2),
            ("Update Speed: Low (4 s)".into(), 3),
            (
                format!("{} Paused", if m.paused { "[x]" } else { "[ ]" }),
                4
            )
        ],
        Some(3) => alloc::vec![
            ("Switch To".into(), 0),
            ("Minimize".into(), 1),
            ("Close Window".into(), 2)
        ],
        Some(4) => alloc::vec![
            ("Task Manager Help".into(), 0),
            ("About Task Manager".into(), 1)
        ],
        Some(5) => alloc::vec![
            ("End Process...".into(), 0),
            ("End Process Tree...".into(), 1),
            ("Priority: Low".into(), 2),
            ("Priority: Below Normal".into(), 3),
            ("Priority: Normal".into(), 4),
            ("Priority: Above Normal".into(), 5),
            ("Priority: High".into(), 6),
            ("Affinity: All CPUs".into(), 7),
            ("Affinity: CPU 0".into(), 8)
        ],
        _ => Vec::new(),
    }
}
fn menu_origin(m: &Manager, b: Rect) -> (i32, i32) {
    m.context
        .unwrap_or((menu_button(b, m.menu.unwrap_or(0).min(4)).x, b.y + 23))
}
fn menu_bounds(m: &Manager, b: Rect) -> Rect {
    let (x, y) = menu_origin(m, b);
    let width = match m.menu {
        Some(0) => 190,
        Some(1) => 216,
        Some(2) => 224,
        Some(3) => 148,
        Some(4) => 182,
        _ => 216,
    };
    Rect::new(x, y, width, menu_items(m).len() as u32 * 26 + 6)
}
pub fn buttons(app: &ApplicationSession, b: Rect, e: &Environment) -> Vec<Button> {
    let m = &app.manager;
    let mut out = Vec::new();
    for (i, t) in ["File", "Options", "View", "Windows", "Help"]
        .iter()
        .enumerate()
    {
        add(&mut out, menu_button(b, i), *t, Command::Menu(i));
    }
    for (i, t) in TABS.iter().enumerate() {
        add(&mut out, tab(b, i), *t, Command::Page(i));
    }
    let y = b.y + b.height as i32 - 55;
    let count = rows(m, b);
    for (i, (label, _)) in headers(m.page).iter().enumerate() {
        add(
            &mut out,
            cell(b, m.page, i, b.y + 65, 22),
            *label,
            Command::Sort(i),
        );
    }
    for (i, (id, _, _)) in row_values(m, e)
        .iter()
        .skip(m.offset)
        .take(count)
        .enumerate()
    {
        add(&mut out, row_bounds(b, i), "", Command::Row(*id));
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
        let r = menu_bounds(m, b);
        for (i, (label, n)) in menu_items(m).into_iter().enumerate() {
            add(
                &mut out,
                Rect::new(r.x + 3, r.y + 3 + i as i32 * 26, r.width - 6, 26),
                label,
                Command::Item(n),
            );
        }
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
        add(&mut out, notice_button(b), "OK", Command::Dismiss);
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
fn notice_button(b: Rect) -> Rect {
    let r = confirm_rect(b);
    Rect::new(r.x + r.width as i32 - 98, r.y + 137, 80, 24)
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
        Command::Item(n) => {
            let menu = m.menu.take();
            m.context = None;
            match(menu,n){
   (Some(0),0)=>return AppAction::Manage(Action::NewTask),(Some(0),1)=>return AppAction::RequestClose,
   (Some(1),0)=>m.always_on_top=!m.always_on_top,(Some(1),1)=>m.minimize_on_use=!m.minimize_on_use,(Some(1),2)=>m.hide_minimized=!m.hide_minimized,
   (Some(2),0)=>{m.sampled=0;m.paused=false;},(Some(2),1..=3)=>{m.interval=match n{1=>500,2=>1000,_=>4000};m.paused=false;},(Some(2),4)=>m.paused=!m.paused,
   (Some(3),0)=>if let Some(id)=m.selected{return AppAction::Activate(id as u32);},(Some(3),1)=>if let Some(id)=m.selected{return AppAction::Manage(Action::Minimize(id as u32));},(Some(3),2)=>if let Some(id)=m.selected{return AppAction::EndTask(id as u32);},
   (Some(4),_)=>app.status=if n==0{"Select a row, then use an action. Right-click a process for priority and affinity. Delete ends the selected task; Ctrl+Tab changes tabs."}else{"Windows Task Manager - NT61 desktop"}.into(),
   (Some(5),0..=1)=>if let Some(id)=m.selected{m.confirmation=Some((id,n==1));},
   (Some(5),2..=8)=>if let Some(id)=m.selected{return AppAction::Manage(Action::Process(id,match n{2=>"priority:4",3=>"priority:6",4=>"priority:8",5=>"priority:10",6=>"priority:13",7=>"affinity:all",_=>"affinity:1"}.into()));},_=>{}
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
    let values: Vec<_> = h.iter().rev().take(r.width as usize).rev().collect();
    let mut last = None;
    for (i, v) in values.iter().enumerate() {
        let x = r.x + r.width as i32 - values.len() as i32 + i as i32;
        let y = r.y + r.height as i32
            - 1
            - (if memory { v.1 } else { v.0 }.min(100) * r.height.saturating_sub(1) / 100) as i32;
        if let Some(py) = last {
            let top = y.min(py);
            frame.fill_rect(x, top, 1, (y - py).unsigned_abs() + 1, 0xFF00FF00);
        } else {
            frame.fill_rect(x, y, 1, 1, 0xFF00FF00);
        }
        last = Some(y);
    }
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
        5 => e
            .users
            .iter()
            .enumerate()
            .map(|(i, (name, active))| {
                (
                    i as u64,
                    alloc::vec![
                        name.clone(),
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
    let values = row_values(m, e);
    for (i, (_, cells, icon)) in values.iter().skip(m.offset).take(rows(m, b)).enumerate() {
        let row = row_bounds(b, i);
        for (j, value) in cells.iter().enumerate() {
            let mut r = cell(b, m.page, j, row.y + 3, 17);
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
        Command::Item(_) if m.menu == Some(3) => m.page == 0 && m.selected.is_some(),
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
    if matches!(m.page, 0..=2 | 5) {
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
        graph(f, a, &m.history, false);
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
        if let Some(s) = &m.last {
            for (i, n) in s.network.iter().take(5).enumerate() {
                let y = b.y + 76 + i as i32 * 64;
                text(
                    f,
                    &format!(
                        "Adapter {}  |  {}  |  {} Mbps",
                        n.index,
                        if n.up { "Connected" } else { "Disconnected" },
                        n.speed / 1_000_000
                    ),
                    Rect::new(b.x + 18, y, b.width - 36, 22),
                    0xFF111111,
                );
                text(
                    f,
                    &format!("Bytes Sent: {}    Bytes Received: {}", n.sent, n.received),
                    Rect::new(b.x + 18, y + 25, b.width - 36, 22),
                    0xFF111111,
                );
            }
            if s.network.is_empty() {
                text(
                    f,
                    "No network adapters available.",
                    Rect::new(b.x + 18, b.y + 90, b.width - 36, 22),
                    0xFF333333,
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
        if matches!(c, Command::Item(_) | Command::Confirm(_) | Command::Dismiss) {
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
                if selected || hot {
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
        let r = menu_bounds(m, b);
        aero::menu_surface(f, r);
        for btn in &controls {
            if matches!(btn.command, ViewCommand::TaskManager(Command::Item(_))) {
                if app.hot == Some(btn.bounds) {
                    aero::menu_item(f, btn.bounds, true);
                }
                text(
                    f,
                    &btn.label,
                    Rect::new(
                        btn.bounds.x + 30,
                        btn.bounds.y + 5,
                        btn.bounds.width - 35,
                        20,
                    ),
                    0xFF111111,
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
        let mut r = confirm_rect(b);
        r.height = 176;
        aero::shadow(f, r, 10);
        f.fill_rect(r.x, r.y, r.width, r.height, 0xFFF0F0F0);
        f.stroke_rounded_rect(r, 0, 1, 0xFF777777);
        text(
            f,
            "Windows Task Manager",
            Rect::new(r.x + 16, r.y + 12, r.width - 32, 24),
            0xFF173E64,
        );
        let mut line = String::new();
        let mut y = r.y + 42;
        for word in app.status.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > 46 {
                text(
                    f,
                    &line,
                    Rect::new(r.x + 16, y, r.width - 32, 20),
                    0xFF111111,
                );
                line.clear();
                y += 20;
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        text(
            f,
            &line,
            Rect::new(r.x + 16, y, r.width - 32, 20),
            0xFF111111,
        );
        let button = notice_button(b);
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
        if let Ok(sample) = crate::platform::services::monitor() {
            for app in &mut self.applications {
                if app.id == AppId::TaskManager {
                    if !app.manager.paused
                        && (app.manager.sampled == 0
                            || sample.uptime.saturating_sub(app.manager.sampled)
                                >= app.manager.interval)
                    {
                        if let Some(snapshot) = &self.process_snapshot {
                            app.manager.observe_processes(snapshot, sample.ticks);
                        }
                    }
                    app.manager.observe(&sample);
                }
            }
            self.monitor_sample = Some(sample);
        }
        // Service metadata changes independently of process CPU sampling. Query
        // it only while the Services tab is visible, or before the first snapshot.
        if self.service_snapshot.is_empty() || self.applications.iter().any(|a|
            a.id == AppId::TaskManager && a.manager.page == 2 && !a.manager.paused) {
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
            f.pixel(100, 40),
            expected.pixel(100, 40),
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
        let button = notice_button(b);
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

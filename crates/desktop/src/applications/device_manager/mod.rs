//! Device Manager backed by bounded PnP snapshots, with no synthetic installed devices.
use super::{
    explorer::input,
    system_icons::{self, Glyph},
    view::{Button, Command as ViewCommand},
    AppAction, AppState, ApplicationSession,
};
use crate::{
    compositor::aero,
    graphics::{Rect, Surface},
};
use alloc::{format, string::String, vec::Vec};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub bus: u32,
    pub address: u32,
    pub function: u32,
    pub state: u32,
    pub bound: bool,
    pub hardware: String,
    pub compatible: String,
}
impl Device {
    pub fn class(&self) -> usize {
        let cc = self
            .compatible
            .split("CC_")
            .nth(1)
            .and_then(|v| v.get(..2))
            .and_then(|v| u8::from_str_radix(v, 16).ok());
        let id = self.hardware.to_ascii_uppercase();
        if id.contains("PNP030") {
            return 10;
        } // Keyboard controller
        if id.contains("PNP0F") {
            return 11;
        } // PS/2 pointing device
        if id.contains("ACPI0007") {
            return 14;
        }
        let subclass = self
            .compatible
            .split("CC_")
            .nth(1)
            .and_then(|s| s.get(..4))
            .and_then(|s| u16::from_str_radix(s, 16).ok());
        if subclass == Some(0x0101) {
            return 9;
        }
        match (self.bus, cc) {
            (5..=8, _) => 8,
            (_, Some(1)) => 0,
            (11, _) | (_, Some(3)) => 1,
            (9, _) | (_, Some(2)) => 2,
            (10, _) | (_, Some(4)) => 3,
            (12, _) => 4,
            (3, _) => 5,
            (_, Some(12)) if subclass == Some(0x0c03) => 5,
            (2, _) | (_, Some(6)) => 6,
            _ => 7,
        }
    }
    pub fn name(&self) -> String {
        let id = self.hardware.to_ascii_uppercase();
        let known = [
            ("VEN_1234&DEV_1111", "Standard VGA Graphics Adapter"),
            (
                "VEN_8086&DEV_100E",
                "Intel(R) PRO/1000 MT Network Connection",
            ),
            (
                "VEN_8086&DEV_2415",
                "Intel(R) 82801AA AC'97 Audio Controller",
            ),
            (
                "VEN_8086&DEV_2922",
                "Standard AHCI 1.0 Serial ATA Controller",
            ),
            (
                "VEN_8086&DEV_7010",
                "Intel(R) 82371SB PCI Bus Master IDE Controller",
            ),
        ];
        known
            .iter()
            .find(|(hw, _)| id.contains(hw))
            .map_or_else(|| self.hardware.clone(), |(_, name)| (*name).into())
    }
    pub fn status(&self) -> &'static str {
        match (self.state, self.bound) {
            (4, true) => "This device is working properly.",
            (6, _) => "This device is stopped.",
            (7 | 10, _) => {
                "Currently, this hardware device is not connected to the computer. (Code 45)"
            }
            (_, false) => "The drivers for this device are not installed. (Code 28)",
            _ => "This device has not completed its Plug and Play start sequence.",
        }
    }
    fn hidden(&self) -> bool {
        matches!(self.state, 7 | 10)
    }
}
const CLASSES: [(&str, Glyph); 16] = [
    ("Storage controllers", Glyph::Disk),
    ("Display adapters", Glyph::Display),
    ("Network adapters", Glyph::Network),
    ("Sound, video and game controllers", Glyph::Sound),
    ("Human Interface Devices", Glyph::Input),
    ("Universal Serial Bus controllers", Glyph::Usb),
    ("System devices", Glyph::Computer),
    ("Other devices", Glyph::Warning),
    ("Disk drives", Glyph::Disk),
    ("IDE ATA/ATAPI controllers", Glyph::Disk),
    ("Keyboards", Glyph::Input),
    ("Mice and other pointing devices", Glyph::Mouse),
    ("Computer", Glyph::Computer),
    ("Monitors", Glyph::Display),
    ("Processors", Glyph::Processor),
    ("DVD/CD-ROM drives", Glyph::Optical),
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Root,
    Class(usize),
    Device(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Menu(u8),
    Row(usize),
    Toggle(usize),
    Properties,
    Tab(u8),
    Detail(u8),
    DetailMenu,
    Refresh,
    Hidden,
    Connections(bool),
    History(bool),
    Expand,
    Collapse,
    Help,
    Close,
    Cancel,
    Unavailable(&'static str),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Manager {
    pub devices: Vec<Device>,
    pub selected: Row,
    pub expanded: [bool; 16],
    pub buses: [bool; 13],
    pub last_click: Option<(usize, u64)>,
    pub history: Vec<Row>,
    pub history_at: usize,
    pub scroll_grab: Option<i32>,
    pub horizontal_grab: Option<i32>,
    pub viewport: Rect,
    pub root_open: bool,
    pub offset: usize,
    pub hidden: bool,
    pub connections: bool,
    pub properties: Option<usize>,
    pub tab: u8,
    pub detail: u8,
    pub detail_open: bool,
    pub menu: Option<u8>,
    pub menu_selection: usize,
    pub menu_point: Option<(i32, i32)>,
    pub message: String,
    pub error: Option<u32>,
    pub refresh: bool,
    pub horizontal: u32,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            selected: Row::Root,
            expanded: [false; 16],
            buses: [true; 13],
            last_click: None,
            history: alloc::vec![Row::Root],
            history_at: 0,
            scroll_grab: None,
            horizontal_grab: None,
            viewport: Rect::new(0, 0, 780, 540),
            root_open: true,
            offset: 0,
            hidden: false,
            connections: false,
            properties: None,
            tab: 0,
            detail: 0,
            detail_open: false,
            menu: None,
            menu_selection: 0,
            menu_point: None,
            message: String::new(),
            error: None,
            refresh: true,
            horizontal: 0,
        }
    }
}
impl Manager {
    pub fn captures_pointer(&self) -> bool {
        self.scroll_grab.is_some() || self.horizontal_grab.is_some()
    }
    pub fn rows(&self) -> Vec<(Row, usize, String, Glyph)> {
        let mut rows = alloc::vec![(Row::Root, 0, "NT61".into(), Glyph::Computer)];
        if !self.root_open {
            return rows;
        }
        let mut classes: Vec<_> = (0..if self.connections { 13 } else { CLASSES.len() }).collect();
        if !self.connections {
            classes.sort_by_key(|i| CLASSES[*i].0);
        }
        for c in classes {
            let group: Vec<_> = self
                .devices
                .iter()
                .enumerate()
                .filter(|(_, d)| {
                    (self.hidden || !d.hidden())
                        && if self.connections {
                            d.bus as usize == c
                        } else {
                            d.class() == c
                        }
                })
                .collect();
            if group.is_empty() {
                continue;
            }
            let (name, glyph) = if self.connections {
                (bus_name(c as u32), Glyph::Device)
            } else {
                CLASSES[c]
            };
            rows.push((Row::Class(c), 1, name.into(), glyph));
            let expanded = if self.connections {
                self.buses[c]
            } else {
                self.expanded[c]
            };
            if expanded {
                for (index, d) in group {
                    rows.push((
                        Row::Device(index),
                        2,
                        d.name(),
                        if d.bound { glyph } else { Glyph::Warning },
                    ));
                }
            }
        }
        rows
    }
    pub fn refresh(&mut self) {
        self.refresh = false;
        match crate::platform::services::transfer(nt61_abi::session_bridge::DEVICES, "", &[])
            .and_then(|b| decode(&b))
        {
            Ok(devices) => {
                self.accept_snapshot(devices);
            }
            Err(s) => self.error = Some(s),
        }
    }
    fn accept_snapshot(&mut self, devices: Vec<Device>) {
        let old = if let Row::Device(i) = self.selected {
            self.devices.get(i).cloned()
        } else {
            None
        };
        self.devices = devices;
        if let Some(old) = old {
            self.selected = self
                .devices
                .iter()
                .position(|d| {
                    d.hardware == old.hardware
                        && d.bus == old.bus
                        && d.address == old.address
                        && d.function == old.function
                })
                .map_or(Row::Root, Row::Device);
        }
        self.error = None;
        self.offset = self.offset.min(self.rows().len().saturating_sub(1));
        self.properties = None;
        // Device indices belong to this snapshot. Old navigation entries may
        // otherwise select an unrelated device after enumeration changes.
        self.history = alloc::vec![self.selected.clone()];
        self.history_at = 0;
        self.last_click = None;
        self.scroll_grab = None;
    }
    pub fn scroll(&mut self, delta: i32, b: Rect) {
        self.viewport = b;
        if self.properties.is_some() || !self.message.is_empty() || self.menu.is_some() {
            return;
        }
        let max = tree_layout(self, b).vertical_max;
        self.offset = (self.offset as i64 - delta as i64 * 3).clamp(0, max as i64) as usize;
    }
    fn toggle(&mut self, row: &Row) {
        match row {
            Row::Root => self.root_open = !self.root_open,
            Row::Class(i) => {
                if self.connections {
                    self.buses[*i] = !self.buses[*i];
                } else {
                    self.expanded[*i] = !self.expanded[*i];
                }
            }
            _ => {}
        }
    }
    fn ensure_selected(&mut self, b: Rect) {
        let visible = tree_layout(self, b).visible;
        if let Some(i) = self.rows().iter().position(|(r, ..)| r == &self.selected) {
            if i < self.offset {
                self.offset = i;
            }
            if i >= self.offset + visible {
                self.offset = i + 1 - visible;
            }
        }
        let layout = tree_layout(self, b);
        self.offset = self.offset.min(layout.vertical_max);
        self.horizontal = self.horizontal.min(layout.horizontal_max);
    }
}
fn bus_name(bus: u32) -> &'static str {
    match bus {
        1 => "PCI bus",
        2 => "ACPI",
        3 => "USB",
        4 => "ISA",
        5 => "SCSI",
        6 => "IDE",
        7 => "SATA",
        8 => "NVM Express",
        9 => "Network",
        10 => "Audio",
        11 => "Video",
        12 => "Input",
        _ => "Unknown bus",
    }
}
/// DEV1: count followed by 24-byte numeric header and two NUL-padded 32-byte IDs.
pub fn decode(b: &[u8]) -> Result<Vec<Device>, u32> {
    let bad = nt61_abi::desktop_bridge::STATUS_INVALID_PARAMETER;
    if b.len() < 8 || &b[..4] != b"DEV1" {
        return Err(bad);
    }
    let count = u32::from_le_bytes(b[4..8].try_into().map_err(|_| bad)?) as usize;
    if count > 64 || b.len() != 8 + count * 88 {
        return Err(bad);
    }
    let mut out = Vec::with_capacity(count);
    for r in b[8..].chunks_exact(88) {
        let number = |i| u32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
        let string = |start| {
            let s = &r[start..start + 32];
            let end = s.iter().position(|b| *b == 0).unwrap_or(32);
            if s[..end]
                .iter()
                .any(|b| !b.is_ascii() || b.is_ascii_control())
            {
                return Err(bad);
            }
            core::str::from_utf8(&s[..end])
                .map(String::from)
                .map_err(|_| bad)
        };
        if number(20) > 1 || number(0) > 12 || number(12) > 10 {
            return Err(bad);
        }
        out.push(Device {
            bus: number(0),
            address: number(4),
            function: number(8),
            state: number(12),
            bound: number(20) == 1,
            hardware: string(24)?,
            compatible: string(56)?,
        });
    }
    Ok(out)
}
fn tree(b: Rect) -> Rect {
    Rect::new(
        b.x + 1,
        b.y + 56,
        b.width.saturating_sub(2),
        b.height.saturating_sub(79),
    )
}
struct TreeLayout {
    viewport: Rect,
    visible: usize,
    vertical_max: usize,
    horizontal_max: u32,
    horizontal: Option<Rect>,
    vertical: Option<Rect>,
}
fn tree_layout(m: &Manager, b: Rect) -> TreeLayout {
    let t = tree(b);
    let rows = m.rows();
    // Keep complete labels in the scrollable surface. The font callback clips
    // each label to this estimated extent; it must not clip to the window width.
    let width = rows
        .iter()
        .map(|(_, depth, name, _)| {
            4 + *depth as u32 * 18 + 32 + name.chars().count() as u32 * 8 + 7
        })
        .max()
        .unwrap_or(0);
    let mut viewport = t;
    // Either scrollbar can cause the other to become necessary.
    for _ in 0..2 {
        viewport.height = t
            .height
            .saturating_sub(if width > viewport.width { 16 } else { 0 });
        viewport.width = t
            .width
            .saturating_sub(if rows.len() * 20 > viewport.height as usize {
                16
            } else {
                0
            });
    }
    let visible = (viewport.height / 20).max(1) as usize;
    TreeLayout {
        viewport,
        visible,
        vertical_max: rows.len().saturating_sub(visible),
        horizontal_max: width.saturating_sub(viewport.width),
        horizontal: (viewport.height < t.height).then_some(Rect::new(
            t.x,
            t.y + viewport.height as i32,
            viewport.width,
            16,
        )),
        vertical: (viewport.width < t.width).then_some(Rect::new(
            t.x + viewport.width as i32,
            t.y,
            16,
            viewport.height,
        )),
    }
}
fn popup(kind: u8, m: &Manager) -> Vec<(&'static str, Command)> {
    match kind {
        0 => alloc::vec![("Exit", Command::Close)],
        1 => alloc::vec![
            ("Scan for hardware changes", Command::Refresh),
            ("Properties", Command::Properties)
        ],
        2 => alloc::vec![
            ("Devices by type", Command::Connections(false)),
            ("Devices by connection", Command::Connections(true)),
            (
                if m.hidden {
                    "Hide hidden devices"
                } else {
                    "Show hidden devices"
                },
                Command::Hidden
            ),
            ("Expand", Command::Expand),
            ("Collapse", Command::Collapse)
        ],
        _ => alloc::vec![
            ("Device Manager Help", Command::Help),
            ("About Device Manager", Command::Help)
        ],
    }
}
fn menu_bounds(b: Rect, m: &Manager, kind: u8) -> Rect {
    let width = 244.min(b.width.saturating_sub(4));
    let height = popup(kind, m).len() as u32 * 26;
    let (x, y) = m
        .menu_point
        .unwrap_or((b.x + 3 + kind as i32 * 56, b.y + 24));
    Rect::new(
        x.clamp(b.x + 2, b.x + (b.width - width).max(2) as i32),
        y.clamp(b.y + 2, b.y + b.height.saturating_sub(height).max(2) as i32),
        width,
        height,
    )
}
const DETAILS: [&str; 4] = [
    "Hardware IDs",
    "Compatible IDs",
    "Location information",
    "Device status",
];
fn property_bounds(b: Rect) -> Rect {
    Rect::new(
        b.x + (b.width as i32 - 440) / 2,
        b.y + (b.height as i32 - 390) / 2,
        440,
        390,
    )
}
pub fn buttons(app: &ApplicationSession, b: Rect) -> Vec<Button> {
    let AppState::DeviceManager(m) = &app.state else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push = |r, label: &str, c| {
        out.push(Button {
            bounds: r,
            label: label.into(),
            command: ViewCommand::Device(c),
        })
    };
    if !m.message.is_empty() {
        push(
            Rect::new(
                b.x + b.width as i32 / 2 - 40,
                b.y + b.height as i32 / 2 + 62,
                80,
                25,
            ),
            "OK",
            Command::Cancel,
        );
        return out;
    }
    if m.properties.is_some() {
        let p = property_bounds(b);
        for (i, label) in ["General", "Driver", "Details", "Resources"]
            .iter()
            .enumerate()
        {
            push(
                Rect::new(p.x + 12 + i as i32 * 97, p.y + 35, 96, 25),
                label,
                Command::Tab(i as u8),
            );
        }
        push(
            Rect::new(p.x + 340, p.y + 350, 80, 26),
            "OK",
            Command::Cancel,
        );
        push(
            Rect::new(p.x + 389, p.y + 1, 44, 20),
            "Close",
            Command::Cancel,
        );
        if m.tab == 2 {
            push(
                Rect::new(p.x + 22, p.y + 134, 396, 24),
                "Property",
                Command::DetailMenu,
            );
            if m.detail_open {
                for (i, label) in DETAILS.iter().enumerate() {
                    push(
                        Rect::new(p.x + 22, p.y + 158 + i as i32 * 24, 396, 24),
                        label,
                        Command::Detail(i as u8),
                    );
                }
            }
        }
        if m.tab == 1 {
            for (i, label) in [
                "Driver Details",
                "Update Driver...",
                "Roll Back Driver",
                "Disable",
                "Uninstall",
            ]
            .iter()
            .enumerate()
            {
                push(
                    Rect::new(p.x + 22, p.y + 205 + i as i32 * 27, 150, 25),
                    label,
                    Command::Unavailable(label),
                );
            }
        }
        return out;
    }
    for (i, label) in ["File", "Action", "View", "Help"].iter().enumerate() {
        push(
            Rect::new(b.x + 3 + i as i32 * 56, b.y, 55, 23),
            label,
            Command::Menu(i as u8),
        );
    }
    for (i, label, c) in [
        (0, "Back", Command::History(false)),
        (1, "Forward", Command::History(true)),
        (2, "Properties", Command::Properties),
        (3, "Help", Command::Help),
        (4, "Scan for hardware changes", Command::Refresh),
    ] {
        push(Rect::new(b.x + 5 + i * 29, b.y + 26, 25, 25), label, c);
    }
    let layout = tree_layout(m, b);
    let t = layout.viewport;
    for (i, (_, _, name, _)) in m
        .rows()
        .iter()
        .enumerate()
        .skip(m.offset)
        .take(layout.visible)
    {
        push(
            Rect::new(
                t.x + 4,
                t.y + (i - m.offset) as i32 * 20,
                t.width.saturating_sub(4),
                20,
            ),
            name,
            Command::Row(i),
        );
    }
    if let Some(kind) = m.menu {
        for (i, (label, c)) in popup(kind, m).into_iter().enumerate() {
            push(
                Rect::new(
                    menu_bounds(b, m, kind).x,
                    menu_bounds(b, m, kind).y + i as i32 * 26,
                    menu_bounds(b, m, kind).width,
                    26,
                ),
                label,
                c,
            );
        }
    }
    out
}
pub fn execute(app: &mut ApplicationSession, c: Command, b: Rect) -> AppAction {
    let AppState::DeviceManager(m) = &mut app.state else {
        return AppAction::None;
    };
    if !matches!(c, Command::Menu(_)) {
        m.menu = None;
    }
    match c {
        Command::Menu(i) => {m.menu=if m.menu==Some(i){None}else{Some(i)};m.menu_point=None;m.menu_selection=0;},
        Command::Row(i) => if let Some((r,..))=m.rows().get(i) {
            m.selected=r.clone();
            if m.history.get(m.history_at)!=Some(r) {
                m.history.truncate(m.history_at+1);m.history.push(r.clone());
                if m.history.len()>128 {m.history.remove(0);}
                m.history_at=m.history.len()-1;
            }
        },
        Command::Toggle(i) => if let Some((r,..))=m.rows().get(i) {m.toggle(r);},
        Command::Expand | Command::Collapse => {
            let opened=matches!(c,Command::Expand);
            match m.selected {Row::Root=>m.root_open=opened,Row::Class(i)=>{
                if m.connections {m.buses[i]=opened;}else{m.expanded[i]=opened;}
            },_=>{}}
        },
        Command::Properties => if let Row::Device(i)=m.selected {m.properties=Some(i);m.tab=0;m.detail_open=false;},
        Command::Tab(i)=>{m.tab=i.min(3);m.detail_open=false;},
        Command::Detail(i)=>{m.detail=i.min(3);m.detail_open=false;},
        Command::DetailMenu=>m.detail_open=!m.detail_open,
        Command::Refresh=>m.refresh=true,
        Command::Hidden=>{m.hidden=!m.hidden;m.offset=0;},
        Command::Connections(connections)=>{
            m.connections=connections;m.offset=0;m.selected=Row::Root;
            m.history=alloc::vec![Row::Root];m.history_at=0;
        },
        Command::History(forward)=>{
            let at=if forward {m.history_at.saturating_add(1)}else{m.history_at.saturating_sub(1)};
            if let Some(row)=m.history.get(at) {m.history_at=at;m.selected=row.clone();}
        },
        Command::Help=>m.message="Device Manager\nView devices registered by Plug and Play.\nArrow keys navigate the tree; Enter opens properties.\nF5 refreshes the device snapshot.\nDriver installation and resource allocation require the corresponding kernel services.".into(),
        Command::Cancel=>{if !m.message.is_empty(){m.message.clear();}else if m.detail_open{m.detail_open=false;}else{m.properties=None;}},
        Command::Close=>return AppAction::RequestClose,
        Command::Unavailable(name)=>m.message=format!("{name}\nThe current Plug and Play service does not provide this operation. No device configuration was changed."),
    }
    m.ensure_selected(b);
    AppAction::None
}
pub fn key(app: &mut ApplicationSession, k: u8) -> AppAction {
    app.hot = None;
    let AppState::DeviceManager(m) = &mut app.state else {
        return AppAction::None;
    };
    let b = m.viewport;
    if let Some(kind) = m.menu {
        let items = popup(kind, m);
        match k {
            27 => m.menu = None,
            input::UP => m.menu_selection = (m.menu_selection + items.len() - 1) % items.len(),
            input::DOWN => m.menu_selection = (m.menu_selection + 1) % items.len(),
            input::LEFT | input::RIGHT => {
                m.menu = Some((kind + if k == input::RIGHT { 1 } else { 3 }) % 4);
                m.menu_selection = 0;
                m.menu_point = None;
            }
            13 => {
                let command = items[m.menu_selection.min(items.len() - 1)].1.clone();
                return execute(app, command, b);
            }
            _ => {}
        }
        return AppAction::None;
    }
    if k == 27 || (!m.message.is_empty() && k == 13) {
        return execute(app, Command::Cancel, b);
    }
    if m.properties.is_some() && m.message.is_empty() {
        if m.detail_open && matches!(k, input::DOWN | input::UP) {
            m.detail = (m.detail + if k == input::DOWN { 1 } else { 3 }) % 4;
            return AppAction::None;
        }
        let command = match k {
            13 if m.detail_open => Command::Detail(m.detail),
            13 => Command::Cancel,
            input::LEFT => Command::Tab((m.tab + 3) % 4),
            input::RIGHT | 9 => Command::Tab((m.tab + 1) % 4),
            _ => return AppAction::None,
        };
        return execute(app, command, b);
    }
    if !m.message.is_empty() {
        return AppAction::None;
    }
    let rows = m.rows();
    let at = rows
        .iter()
        .position(|(r, ..)| r == &m.selected)
        .unwrap_or(0);
    let c = match k {
        input::UP => Command::Row(at.saturating_sub(1)),
        input::DOWN => Command::Row((at + 1).min(rows.len().saturating_sub(1))),
        input::HOME => Command::Row(0),
        input::END => Command::Row(rows.len().saturating_sub(1)),
        input::PAGE_UP => Command::Row(at.saturating_sub(tree_layout(m, b).visible)),
        input::PAGE_DOWN => {
            Command::Row((at + tree_layout(m, b).visible).min(rows.len().saturating_sub(1)))
        }
        input::LEFT => {
            match m.selected {
                Row::Device(i) => {
                    m.selected = Row::Class(if m.connections {
                        m.devices[i].bus as usize
                    } else {
                        m.devices[i].class()
                    })
                }
                _ => return execute(app, Command::Collapse, b),
            };
            return AppAction::None;
        }
        input::RIGHT => Command::Expand,
        13 => Command::Properties,
        input::REFRESH => Command::Refresh,
        input::HELP => Command::Help,
        input::ALT_FILE => Command::Menu(0),
        input::ALT_VIEW => Command::Menu(2),
        input::ALT_HELP => Command::Menu(3),
        input::BACK => Command::History(false),
        input::FORWARD => Command::History(true),
        _ => return AppAction::None,
    };
    execute(app, c, b)
}
pub fn click(app: &mut ApplicationSession, b: Rect, x: i32, y: i32, right: bool) -> AppAction {
    let hit = buttons(app, b)
        .into_iter()
        .rev()
        .find(|a| a.bounds.contains(x, y));
    if let Some(Button {
        command: ViewCommand::Device(mut c),
        ..
    }) = hit
    {
        if let Command::Row(i) = c {
            let AppState::DeviceManager(m) = &mut app.state else {
                return AppAction::None;
            };
            if let Some((r, depth, ..)) = m.rows().get(i) {
                if !right && x < tree(b).x + 20 + *depth as i32 * 18 - m.horizontal as i32 {
                    c = Command::Toggle(i);
                } else if !right {
                    let double = m.last_click.is_some_and(|(previous, time)| {
                        previous == i && app.input_time >= time && app.input_time - time <= 500
                    });
                    m.last_click = Some((i, app.input_time));
                    if double {
                        c = if matches!(r, Row::Device(_)) {
                            Command::Properties
                        } else {
                            Command::Toggle(i)
                        };
                        m.last_click = None;
                    }
                }
            }
        }
        let action = execute(app, c, b);
        if right {
            if let AppState::DeviceManager(m) = &mut app.state {
                m.menu = Some(1);
                m.menu_point = Some((x, y));
                m.menu_selection = 0;
            }
        }
        return action;
    }
    if let AppState::DeviceManager(m) = &mut app.state {
        m.menu = None;
    }
    AppAction::None
}
/// Scroll input stays local to the tree and never triggers another PnP query.
pub fn pointer(
    app: &mut ApplicationSession,
    b: Rect,
    x: i32,
    y: i32,
    down: bool,
    held: bool,
    right: bool,
) -> (bool, AppAction) {
    let AppState::DeviceManager(m) = &mut app.state else {
        return (false, AppAction::None);
    };
    m.viewport = b;
    let layout = tree_layout(m, b);
    m.offset = m.offset.min(layout.vertical_max);
    m.horizontal = m.horizontal.min(layout.horizontal_max);
    if !held {
        m.scroll_grab = None;
        m.horizontal_grab = None;
    }
    if m.message.is_empty() && m.properties.is_none() && m.menu.is_none() {
        if let Some(track) = layout.vertical {
            let max = layout.vertical_max;
            let h = ((track.height as usize * layout.visible / (max + layout.visible)).max(18)
                as u32)
                .min(track.height);
            let travel = track.height.saturating_sub(h).max(1);
            let thumb = track.y + (travel as usize * m.offset / max.max(1)) as i32;
            if held {
                if let Some(grab) = m.scroll_grab {
                    m.offset = ((y - track.y - grab).clamp(0, travel as i32) as u64 * max as u64
                        / travel as u64) as usize;
                    return (true, AppAction::None);
                }
            }
            if down && !right && track.contains(x, y) {
                if y >= thumb && y < thumb + h as i32 {
                    m.scroll_grab = Some(y - thumb);
                } else if y < thumb {
                    m.offset = m.offset.saturating_sub(layout.visible);
                } else {
                    m.offset = (m.offset + layout.visible).min(max);
                }
                return (true, AppAction::None);
            }
        }
        if let Some(track) = layout.horizontal {
            let max = layout.horizontal_max;
            let w = (track.width * track.width / (track.width + max).max(1))
                .max(18)
                .min(track.width);
            let travel = track.width.saturating_sub(w).max(1);
            let thumb = track.x
                + (u64::from(travel) * u64::from(m.horizontal) / u64::from(max.max(1))) as i32;
            if held {
                if let Some(grab) = m.horizontal_grab {
                    m.horizontal = (u64::from((x - track.x - grab).clamp(0, travel as i32) as u32)
                        * u64::from(max)
                        / u64::from(travel)) as u32;
                    return (true, AppAction::None);
                }
            }
            if down && !right && track.contains(x, y) {
                if x >= thumb && x < thumb + w as i32 {
                    m.horizontal_grab = Some(x - thumb);
                } else if x < thumb {
                    m.horizontal = m.horizontal.saturating_sub(track.width);
                } else {
                    m.horizontal = (m.horizontal + track.width).min(max);
                }
                return (true, AppAction::None);
            }
        }
    }
    let hot = buttons(app, b)
        .into_iter()
        .rev()
        .find(|button| button.bounds.contains(x, y))
        .map(|button| button.bounds);
    let changed = app.hot != hot;
    app.hot = hot;
    if down {
        (true, click(app, b, x, y, right))
    } else {
        (changed, AppAction::None)
    }
}
pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
    app: &ApplicationSession,
    s: &mut Surface,
    b: Rect,
    text: &F,
) {
    let raw_text = text;
    let text = &|s: &mut Surface, value: &str, r: Rect, color: u32| {
        super::system_text::paragraph(s, value, r, color, raw_text)
    };
    let AppState::DeviceManager(m) = &app.state else {
        return;
    };
    s.fill_rect(b.x, b.y, b.width, b.height, 0xfff0f0f0);
    s.fill_rect(b.x, b.y, b.width, 23, 0xfff5f6fa);
    s.fill_rect(b.x, b.y + 24, b.width, 29, 0xffd8dfed);
    s.fill_rect(b.x, b.y + 53, b.width, 1, 0xff929aa5);
    for (i, label) in ["File", "Action", "View", "Help"].iter().enumerate() {
        let r = Rect::new(b.x + 3 + i as i32 * 56, b.y, 55, 23);
        if app.hot == Some(r) {
            aero::menu_item(s, r, true);
        }
        text(s, label, Rect::new(r.x + 7, r.y + 4, 50, 20), 0xff111111);
    }
    for (i, g) in [
        Glyph::Back,
        Glyph::Forward,
        Glyph::Device,
        Glyph::Help,
        Glyph::Scan,
    ]
    .iter()
    .enumerate()
    {
        let r = Rect::new(b.x + 5 + i as i32 * 29, b.y + 26, 25, 25);
        if app.hot == Some(r) {
            aero::glass_button(s, r, true);
        }
        let disabled = match i {
            0 => m.history_at == 0,
            1 => m.history_at + 1 >= m.history.len(),
            2 => !matches!(m.selected, Row::Device(_)),
            _ => false,
        };
        let icon = Rect::new(r.x + 3, r.y + 3, 19, 19);
        if disabled {
            system_icons::draw_disabled(s, icon, *g);
        } else {
            system_icons::draw(s, icon, *g);
        }
    }
    let layout = tree_layout(m, b);
    let t = layout.viewport;
    let rows = m.rows();
    // Draw into a tree-local surface, preserving the compositor's damage clip
    // and preventing horizontally scrolled labels from overwriting the frame.
    let mut tree_surface = Surface::new(t.width, t.height);
    tree_surface.fill_rect(0, 0, t.width, t.height, 0xffffffff);
    let offset = m.offset.min(layout.vertical_max);
    for (i, (r, depth, name, glyph)) in rows.iter().enumerate().skip(offset).take(layout.visible) {
        let y = (i - offset) as i32 * 20;
        let x = 4 + *depth as i32 * 18 - m.horizontal.min(layout.horizontal_max) as i32;
        if *depth > 0 {
            for dy in (0..20).step_by(2) {
                tree_surface.fill_rect(x - 5, y + dy, 1, 1, 0xffb1b1b1);
            }
            for dx in (0..10).step_by(2) {
                tree_surface.fill_rect(x - 5 + dx, y + 10, 1, 1, 0xffb1b1b1);
            }
        }
        if matches!(r, Row::Root | Row::Class(_)) {
            let opened = match r {
                Row::Root => m.root_open,
                Row::Class(c) => {
                    if m.connections {
                        m.buses[*c]
                    } else {
                        m.expanded[*c]
                    }
                }
                _ => false,
            };
            if opened {
                for n in 0..5 {
                    tree_surface.fill_rect(x + 2 + n, y + 8 + n, (5 - n) as u32, 1, 0xff43494c);
                }
            } else {
                for n in 0..7 {
                    let len = if n < 4 { n + 1 } else { 7 - n };
                    tree_surface.fill_rect(x + 3, y + 6 + n, len as u32, 1, 0xff73797d);
                }
            }
        }
        system_icons::draw(&mut tree_surface, Rect::new(x + 13, y + 2, 16, 16), *glyph);
        let label = Rect::new(x + 32, y + 1, name.chars().count() as u32 * 8 + 7, 18);
        if &m.selected == r {
            tree_surface.fill_rect(label.x, label.y, label.width, label.height, 0xffcce8ff);
            tree_surface.stroke_rounded_rect(label, 0, 1, 0xff8ec4e8);
        }
        text(&mut tree_surface, name, label, 0xff101010);
    }
    s.copy_region(&tree_surface, Rect::new(0, 0, t.width, t.height), t.x, t.y);
    if let Some(track) = layout.vertical {
        s.fill_rect(track.x, track.y, track.width, track.height, 0xfff0f0f0);
        let h = ((track.height as usize * layout.visible / rows.len()).max(18) as u32)
            .min(track.height);
        let y =
            track.y + ((track.height - h) as usize * offset / layout.vertical_max.max(1)) as i32;
        let thumb = Rect::new(track.x + 1, y, 14, h);
        aero::glass_button(s, thumb, m.scroll_grab.is_some());
    }
    if let Some(track) = layout.horizontal {
        s.fill_rect(track.x, track.y, track.width, track.height, 0xfff0f0f0);
        let w = (track.width * track.width / (track.width + layout.horizontal_max).max(1))
            .max(18)
            .min(track.width);
        let x = track.x
            + (u64::from(track.width - w) * u64::from(m.horizontal.min(layout.horizontal_max))
                / u64::from(layout.horizontal_max.max(1))) as i32;
        aero::glass_button(
            s,
            Rect::new(x, track.y + 1, w, 14),
            m.horizontal_grab.is_some(),
        );
    }
    text(
        s,
        &m.error.map_or_else(
            || String::new(),
            |e| format!("Device enumeration failed (0x{e:08X})"),
        ),
        Rect::new(b.x + 6, b.y + b.height as i32 - 20, b.width - 12, 20),
        0xff444444,
    );
    if let Some(kind) = m.menu {
        let list = popup(kind, m);
        let menu = menu_bounds(b, m, kind);
        let (x, y) = (menu.x, menu.y);
        aero::menu_surface(s, Rect::new(x - 2, y - 2, menu.width + 4, menu.height + 4));
        for (i, (label, _)) in list.iter().enumerate() {
            let r = Rect::new(x, y + i as i32 * 26, menu.width, 26);
            if app.hot == Some(r) || (app.hot.is_none() && m.menu_selection == i) {
                aero::menu_item(s, r, true);
            }
            text(
                s,
                label,
                Rect::new(r.x + 12, r.y + 5, r.width.saturating_sub(24), 20),
                0xff111111,
            );
        }
    }
    if let Some(index) = m.properties {
        if let Some(d) = m.devices.get(index) {
            let p = property_bounds(b);
            aero::shadow(s, p, 6);
            let mut glass = aero::WindowGlassCache::default();
            glass.prepare(s, 0, p, 0, true);
            s.fill_rect(p.x + 6, p.y + 30, p.width - 12, p.height - 36, 0xfff0f0f0);
            glass.paint(s, 0, true);
            aero::window_border(s, p, 6);
            system_icons::draw(s, Rect::new(p.x + 8, p.y + 7, 16, 16), CLASSES[d.class()].1);
            text(
                s,
                &format!("{} Properties", d.name()),
                Rect::new(p.x + 29, p.y + 8, 350, 20),
                0xff111111,
            );
            aero::window_control_button(
                s,
                Rect::new(p.x + 389, p.y + 1, 44, 20),
                aero::WindowControlButtonKind::Close,
                app.hot == Some(Rect::new(p.x + 389, p.y + 1, 44, 20)),
            );
            s.fill_rect(p.x + 12, p.y + 59, 416, 282, 0xffffffff);
            s.stroke_rounded_rect(Rect::new(p.x + 12, p.y + 59, 416, 282), 0, 1, 0xffb5bdc5);
            for (i, label) in ["General", "Driver", "Details", "Resources"]
                .iter()
                .enumerate()
            {
                let r = Rect::new(p.x + 12 + i as i32 * 97, p.y + 35, 96, 25);
                s.fill_rect(
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    if m.tab as usize == i {
                        0xffffffff
                    } else {
                        0xffe6e6e6
                    },
                );
                s.stroke_rounded_rect(r, 0, 1, 0xff9b9b9b);
                text(s, label, Rect::new(r.x + 9, r.y + 6, 90, 20), 0xff111111);
            }
            system_icons::draw(
                s,
                Rect::new(p.x + 22, p.y + 76, 28, 28),
                CLASSES[d.class()].1,
            );
            text(
                s,
                &d.name(),
                Rect::new(p.x + 65, p.y + 80, 350, 28),
                0xff111111,
            );
            let content=match m.tab {0=>format!("Device type: {}\nManufacturer: Not reported\nLocation: {} / device {} / function {}\n\nDevice status\n\n{}",CLASSES[d.class()].0,bus_name(d.bus),d.address,d.function,d.status()),1=>format!("Driver provider: Not reported\nDriver date: Not reported\nDriver version: Not reported\nDigital signer: Not reported\nDriver bound: {}",if d.bound{"Yes"}else{"No"}),2=>match m.detail{0=>d.hardware.clone(),1=>d.compatible.clone(),2=>format!("{} / device {} / function {}",bus_name(d.bus),d.address,d.function),_=>format!("PnP state: {}\n{}",d.state,d.status())},_=>"Resource settings\n\nThe Plug and Play service has not published resource descriptors for this device.".into()};
            text(
                s,
                &content,
                Rect::new(
                    p.x + 22,
                    p.y + if m.tab == 2 { 193 } else { 115 },
                    396,
                    if m.tab == 1 {
                        90
                    } else if m.tab == 2 {
                        135
                    } else {
                        215
                    },
                ),
                0xff222222,
            );
            for btn in buttons(app, b) {
                let ViewCommand::Device(c) = btn.command else {
                    continue;
                };
                if matches!(c, Command::Cancel | Command::Unavailable(_)) {
                    if btn.label == "Close" {
                        continue;
                    }
                    aero::glass_button(s, btn.bounds, app.hot == Some(btn.bounds));
                    text(
                        s,
                        &btn.label,
                        Rect::new(
                            btn.bounds.x + 8,
                            btn.bounds.y + 5,
                            btn.bounds.width - 12,
                            20,
                        ),
                        0xff222222,
                    );
                }
            }
            if m.tab == 2 {
                text(
                    s,
                    "Property",
                    Rect::new(p.x + 22, p.y + 113, 396, 20),
                    0xff222222,
                );
                let combo = Rect::new(p.x + 22, p.y + 134, 396, 24);
                s.fill_rect(combo.x, combo.y, combo.width, combo.height, 0xffffffff);
                s.stroke_rounded_rect(combo, 0, 1, 0xff7f9db9);
                aero::glass_button(
                    s,
                    Rect::new(combo.x + 374, combo.y + 1, 21, 22),
                    m.detail_open || app.hot == Some(combo),
                );
                for n in 0..4 {
                    s.fill_rect(
                        combo.x + 380 + n,
                        combo.y + 9 + n,
                        (7 - 2 * n) as u32,
                        1,
                        0xff333333,
                    );
                }
                text(
                    s,
                    DETAILS[m.detail as usize],
                    Rect::new(combo.x + 4, combo.y + 4, 364, 20),
                    0xff222222,
                );
                text(
                    s,
                    "Value",
                    Rect::new(p.x + 22, p.y + 167, 396, 20),
                    0xff222222,
                );
                s.stroke_rounded_rect(Rect::new(p.x + 21, p.y + 188, 397, 143), 0, 1, 0xffa0a0a0);
                if m.detail_open {
                    let list = Rect::new(combo.x, combo.y + 24, combo.width, 96);
                    s.fill_rect(list.x, list.y, list.width, list.height, 0xffffffff);
                    s.stroke_rounded_rect(list, 0, 1, 0xff7f9db9);
                    for (i, label) in DETAILS.iter().enumerate() {
                        let r = Rect::new(list.x + 1, list.y + i as i32 * 24, list.width - 2, 24);
                        if i == m.detail as usize {
                            s.fill_rect(r.x, r.y, r.width, r.height, 0xff3399ff);
                        }
                        text(
                            s,
                            label,
                            Rect::new(r.x + 4, r.y + 4, r.width - 8, 20),
                            if i == m.detail as usize {
                                0xffffffff
                            } else {
                                0xff222222
                            },
                        );
                    }
                }
            }
        }
    }
    if !m.message.is_empty() {
        let p = Rect::new(
            b.x + b.width as i32 / 2 - 210,
            b.y + b.height as i32 / 2 - 95,
            420,
            190,
        );
        s.fill_rect(p.x, p.y, p.width, p.height, 0xfff0f0f0);
        s.stroke_rounded_rect(p, 0, 1, 0xff778793);
        text(
            s,
            &m.message,
            Rect::new(p.x + 16, p.y + 12, 388, 140),
            0xff222222,
        );
        for btn in buttons(app, b) {
            aero::glass_button(s, btn.bounds, false);
            text(
                s,
                &btn.label,
                Rect::new(btn.bounds.x + 24, btn.bounds.y + 5, 50, 20),
                0xff111111,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn display_device(hardware: &str) -> Device {
        Device {
            bus: 1,
            address: 3,
            function: 0,
            state: 4,
            bound: true,
            hardware: hardware.into(),
            compatible: "PCI\\CC_030000".into(),
        }
    }
    #[test]
    fn refresh_preserves_device_identity_without_reusing_old_indices() {
        let mut m = Manager::default();
        let a = display_device("A");
        let b = display_device("B");
        m.devices = alloc::vec![a.clone(), b.clone()];
        m.selected = Row::Device(1);
        m.history = alloc::vec![Row::Root, Row::Device(0), Row::Device(1)];
        m.history_at = 2;
        m.last_click = Some((2, 400));
        m.properties = Some(1);
        m.accept_snapshot(alloc::vec![b, a]);
        assert_eq!(m.selected, Row::Device(0));
        assert_eq!(m.history, alloc::vec![Row::Device(0)]);
        assert_eq!(m.history_at, 0);
        assert!(m.last_click.is_none());
        assert!(m.properties.is_none());
        m.accept_snapshot(alloc::vec![]);
        assert_eq!(m.selected, Row::Root);
    }
    #[test]
    fn power_buttons_are_not_classified_as_processors() {
        let mut device = display_device("ACPI\\PNP0C0C");
        device.bus = 2;
        device.compatible.clear();
        assert_eq!(device.class(), 6);
        device.hardware = "ACPI\\ACPI0007".into();
        assert_eq!(device.class(), 14);
    }
    #[test]
    fn repeated_single_clicks_do_not_open_properties_but_double_click_does() {
        let mut app = ApplicationSession::new(1, super::super::AppId::DeviceManager, 0);
        if let AppState::DeviceManager(m) = &mut app.state {
            m.devices = alloc::vec![display_device("DISPLAY")];
            m.expanded[1] = true;
        }
        let b = Rect::new(0, 0, 790, 580);
        let y = tree(b).y + 50;
        for now in [1000, 2000] {
            app.input_time = now;
            click(&mut app, b, 100, y, false);
            let AppState::DeviceManager(m) = &app.state else {
                panic!()
            };
            assert!(m.properties.is_none());
        }
        app.input_time = 2200;
        click(&mut app, b, 100, y, false);
        let AppState::DeviceManager(m) = &app.state else {
            panic!()
        };
        assert_eq!(m.properties, Some(0));
    }
    #[test]
    fn malformed_or_pointer_bearing_snapshots_are_rejected() {
        assert!(decode(b"DEV1\xff\xff\xff\xff").is_err());
        let mut b = b"DEV1\x01\0\0\0".to_vec();
        b.resize(96, 0);
        assert_eq!(decode(&b).unwrap().len(), 1);
        b[28] = 2;
        assert!(decode(&b).is_err());
    }
    #[test]
    fn tree_uses_only_enumerated_devices_and_properties_report_real_state() {
        let mut m = Manager::default();
        assert_eq!(m.rows().len(), 1);
        m.devices.push(Device {
            bus: 1,
            address: 3,
            function: 0,
            state: 4,
            bound: true,
            hardware: "PCI\\VEN_1234&DEV_1111".into(),
            compatible: "PCI\\CC_030000".into(),
        });
        assert_eq!(m.rows().len(), 2);
        m.expanded[1] = true;
        assert_eq!(m.rows().len(), 3);
        assert!(m.devices[0].status().contains("working properly"));
        m.devices[0].bound = false;
        assert!(m.devices[0].status().contains("Code 28"));
    }
    #[test]
    fn horizontal_drag_reaches_full_label_and_keeps_row_clicks_aligned() {
        let mut app = ApplicationSession::new(1, super::super::AppId::DeviceManager, 0);
        let AppState::DeviceManager(m) = &mut app.state else {
            panic!()
        };
        m.devices
            .push(display_device("PCI\\A_VERY_LONG_UNRECOGNIZED_DEVICE_NAME"));
        m.expanded[1] = true;
        let b = Rect::new(10, 20, 250, 240);
        let layout = tree_layout(m, b);
        let track = layout
            .horizontal
            .expect("long device label needs a scrollbar");
        let max = layout.horizontal_max;
        pointer(&mut app, b, track.x + 2, track.y + 7, true, true, false);
        pointer(
            &mut app,
            b,
            track.x + track.width as i32,
            track.y + 7,
            false,
            true,
            false,
        );
        pointer(
            &mut app,
            b,
            track.x + track.width as i32,
            track.y + 7,
            false,
            false,
            false,
        );
        let AppState::DeviceManager(m) = &app.state else {
            panic!()
        };
        assert_eq!(m.horizontal, max);
        assert!(m.horizontal_grab.is_none());
        click(&mut app, b, tree(b).x + 70, tree(b).y + 50, false);
        let AppState::DeviceManager(m) = &app.state else {
            panic!()
        };
        assert_eq!(m.selected, Row::Device(0));
    }
    #[test]
    fn keyboard_menus_and_property_combo_do_not_navigate_background_tree() {
        let mut app = ApplicationSession::new(1, super::super::AppId::DeviceManager, 0);
        let AppState::DeviceManager(m) = &mut app.state else {
            panic!()
        };
        m.devices.push(display_device("DISPLAY"));
        m.selected = Row::Device(0);
        key(&mut app, 13);
        key(&mut app, input::RIGHT);
        key(&mut app, input::RIGHT);
        execute(&mut app, Command::DetailMenu, Rect::new(0, 0, 780, 540));
        key(&mut app, input::DOWN);
        let AppState::DeviceManager(m) = &mut app.state else {
            panic!()
        };
        assert_eq!((m.tab, m.detail, m.detail_open), (2, 1, true));
        let offset = m.offset;
        m.scroll(-100, Rect::new(0, 0, 780, 540));
        assert_eq!(m.offset, offset);
        key(&mut app, 27); // Close the property combo first.
        let AppState::DeviceManager(m) = &app.state else {
            panic!()
        };
        assert_eq!(m.properties, Some(0));
        assert!(!m.detail_open);
        key(&mut app, 27);
        key(&mut app, input::ALT_VIEW);
        key(&mut app, input::DOWN);
        key(&mut app, 13);
        let AppState::DeviceManager(m) = &app.state else {
            panic!()
        };
        assert!(m.connections);
        assert!(m.menu.is_none());
    }
}

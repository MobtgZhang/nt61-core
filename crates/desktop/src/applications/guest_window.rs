//! HWND-backed controls hosted in the Aero compositor. Protocol values never
//! contain pointers: nested Win32 structures are unpacked by the guest runtime.
use crate::{
    controls::*,
    graphics::{Rect, Surface},
    shell::ShellModel,
    widgets::{
        combo_box::ComboBox,
        popup_menu::{PopupItem, PopupKey, PopupResult},
    },
};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use nt61_abi::dialog_bridge as w;
#[derive(Clone, Debug, PartialEq, Eq)]
enum Model {
    List(ListView),
    Tree(TreeView),
    Bar(Toolbar),
    Combo {
        items: Vec<PopupItem>,
        combo: ComboBox,
    },
    Text(String),
    Button(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Control {
    hwnd: u32,
    id: u32,
    parent: u32,
    rect: Rect,
    model: Model,
    visible: bool,
    enabled: bool,
    previous:u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    pub client: u32,
    pub hwnd: u32,
    controls: Vec<Control>,
    events: Vec<w::Packet>,
    focus: Option<u32>,
    body:Rect,
    capture:Option<u32>,
}
fn scalar(p: &mut w::Packet, value: u64) {
    p.values[20] = value as u32;
    p.values[21] = (value >> 32) as u32;
}
fn value(p: &w::Packet, index: usize) -> u64 {
    p.values[index] as u64 | ((p.values[index + 1] as u64) << 32)
}
impl Scene {
    pub fn captures_pointer(&self)->bool{self.capture.is_some()||self.controls.iter().any(|c|matches!(&c.model,Model::Combo{combo,..}if combo.popup().is_open()))}
    fn contains(&self, hwnd: u32) -> bool {
        self.hwnd == hwnd || self.controls.iter().any(|c| c.hwnd == hwnd)
    }
    fn event(&mut self, hwnd: u32, message: u32, wp: u64, lp: u64) {
        if self.events.len() >= 256 {
            return;
        }
        let mut p = w::EMPTY;
        p.values[5] = hwnd;
        p.values[8] = message;
        p.values[9] = wp as u32;
        p.values[10] = (wp >> 32) as u32;
        p.values[11] = lp as u32;
        p.values[12] = (lp >> 32) as u32;
        self.events.push(p);
    }
    pub fn close_request(&mut self) {
        if !self.events.iter().any(|p| p.values[8] == 0x10) {
            self.event(self.hwnd, 0x10, 0, 0);
        }
    }
    fn notify(&mut self, hwnd: u32, id: u32, notification: CommonNotification) {
        let mut detail=[0u32;8];
        if let Some(c)=self.controls.iter_mut().find(|c|c.hwnd==hwnd){match &c.model{
            Model::List(v)=>{let item=match notification{CommonNotification::SelectionChanged{item,..}|CommonNotification::Activated{item,..}|CommonNotification::Checked{item,..}=>item,_=>0};if let Some(i)=v.rows().iter().position(|r|r.id==item){detail[0]=i as u32;detail[1]=v.rows()[i].data as u32;detail[2]=(v.rows()[i].data as u64>>32)as u32;detail[3]=if v.selected().any(|r|r.id==item){2}else{0};}},
            Model::Tree(v)=>{let item=match notification{CommonNotification::SelectionChanged{item,..}|CommonNotification::Activated{item,..}|CommonNotification::Expanded{item,..}|CommonNotification::Checked{item,..}=>item,_=>0};detail[0]=item as u32;if let Some(i)=v.item(item){detail[1]=i.data as u32;detail[2]=(i.data as u64>>32)as u32;detail[3]=u32::from(i.expanded)*0x20;}detail[4]=c.previous as u32;if let Some(i)=v.item(c.previous){detail[5]=i.data as u32;detail[6]=(i.data as u64>>32)as u32;}c.previous=v.selected().unwrap_or(0);},_=>{}
        }}
        match notification {
            CommonNotification::Command { command, .. } => {
                self.event(self.hwnd, 0x111, command as u64, hwnd as u64)
            }
            CommonNotification::DropDown { command, .. } => {
                self.event(self.hwnd, 0x4e, id as u64, hwnd as u64);
                if let Some(p) = self.events.last_mut() {
                    p.values[22] = (-710i32) as u32;
                    p.values[23] = command;
                }
            }
            CommonNotification::SelectionChanged { item, .. }
            | CommonNotification::Activated { item, .. }
            | CommonNotification::Expanded { item, .. }
            | CommonNotification::Checked { item, .. } => {
                let tree = self
                    .controls
                    .iter()
                    .find(|c| c.hwnd == hwnd)
                    .is_some_and(|c| matches!(c.model, Model::Tree(_)));
                let code = match notification {
                    CommonNotification::Activated { .. } => -3,
                    CommonNotification::Expanded { .. } => -455,
                    _ => {
                        if tree {
                            -451
                        } else {
                            -101
                        }
                    }
                };
                self.event(self.hwnd, 0x4e, id as u64, hwnd as u64);
                if let Some(p) = self.events.last_mut() {
                    p.values[22] = code as u32;
                    p.values[23] = item as u32;
                }
            }
            CommonNotification::ColumnClick { column, .. } => {
                self.event(self.hwnd, 0x4e, id as u64, hwnd as u64);
                if let Some(p) = self.events.last_mut() {
                    p.values[22] = (-108i32) as u32;
                    p.values[23] = column as u32;
                }
            }
        }
        if let Some(p)=self.events.last_mut(){p.values[24..32].copy_from_slice(&detail);}
    }
    fn bounds(control: &Control, body: Rect) -> Rect {
        Rect::new(
            body.x + control.rect.x,
            body.y + control.rect.y,
            control.rect.width,
            control.rect.height,
        )
    }
    pub fn draw<F: Fn(&mut Surface, &str, Rect, u32)>(
        &self,
        frame: &mut Surface,
        body: Rect,
        text: &F,
    ) {
        frame.fill_rect(body.x, body.y, body.width, body.height, 0xfff0f0f0);
        let old = frame.drawing_bounds();
        let clip = old.intersection(&body).unwrap_or(Rect::new(0, 0, 0, 0));
        frame.set_draw_clip(Some(clip));
        for c in self.controls.iter().filter(|c| c.visible) {
            let r = Self::bounds(c, body);
            match &c.model {
                Model::List(v) => v.draw(frame, r, text),
                Model::Tree(v) => v.draw(frame, r, text),
                Model::Bar(v) => v.draw(frame, r, text),
                Model::Combo { combo, .. } => combo.draw(frame, r, text),
                Model::Text(s) => text(frame, s, r, 0xff000000),
                Model::Button(s) => {
                    crate::compositor::aero::menu_item(frame, r, true);
                    text(
                        frame,
                        s,
                        Rect::new(
                            r.x + 6,
                            r.y + 4,
                            r.width.saturating_sub(12),
                            r.height.saturating_sub(8),
                        ),
                        0xff000000,
                    );
                }
            }
        }
        for c in &self.controls {
            if let Model::Combo { combo, .. } = &c.model {
                combo.draw_popup(frame, text);
            }
        }
        frame.set_draw_clip(Some(old));
    }
    pub fn pointer(&mut self, body: Rect, x: i32, y: i32, down: bool, held: bool) {
        self.body=body;
        let release = !down && !held;
        let popup = self
            .controls
            .iter()
            .position(|c| matches!(&c.model,Model::Combo{combo,..}if combo.popup().is_open()));
        let capture=self.capture.and_then(|h|self.controls.iter().position(|c|c.hwnd==h));
        let target = popup.or(capture).or_else(|| {
            self.controls
                .iter()
                .rposition(|c| c.visible && c.enabled && Self::bounds(c, body).contains(x, y))
        });
        let Some(index) = target else {
            return;
        };
        let c = &mut self.controls[index];
        let r = Self::bounds(c, body);
        let (hwnd, id) = (c.hwnd, c.id);
        if down {
            self.capture=Some(hwnd);
            self.focus = Some(hwnd);
        }
        let notification = match &mut c.model {
            Model::List(v) => {
                if down {
                    v.click(r, x, y, Modifiers::default(), false)
                } else {
                    None
                }
            }
            Model::Tree(v) => {
                if down {
                    v.click(r, x, y, false)
                } else {
                    None
                }
            }
            Model::Bar(v) => v.pointer(
                if down {
                    PointerAction::Down
                } else if release {
                    PointerAction::Up
                } else {
                    PointerAction::Move
                },
                r,
                x,
                y,
            ),
            Model::Combo { combo, .. } => {
                if matches!(combo.pointer(x, y, down, r, body), PopupResult::Selected(_)) {
                    self.event(self.hwnd, 0x111, (id | 0x10000) as u64, hwnd as u64);
                }
                None
            }
            Model::Button(_) => {
                if down {
                    Some(CommonNotification::Command {
                        control: id,
                        command: id,
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if release{self.capture=None;}
        if let Some(n) = notification {
            self.notify(hwnd, id, n);
        }
    }
    pub fn key(&mut self, key: u8) {
        use super::explorer::input;
        let common = match key {
            input::UP => Some(CommonKey::Up),
            input::DOWN => Some(CommonKey::Down),
            input::LEFT => Some(CommonKey::Left),
            input::RIGHT => Some(CommonKey::Right),
            input::HOME => Some(CommonKey::Home),
            input::END => Some(CommonKey::End),
            input::PAGE_UP => Some(CommonKey::PageUp),
            input::PAGE_DOWN => Some(CommonKey::PageDown),
            13 => Some(CommonKey::Enter),
            b' ' => Some(CommonKey::Space),
            _ => None,
        };
        if key==27{for c in &mut self.controls{if let Model::Combo{combo,..}=&mut c.model{if combo.popup().is_open(){combo.close();return;}}}}
        if key == 9 {
            let next = self
                .controls
                .iter()
                .position(|c| Some(c.hwnd) == self.focus)
                .map_or(0, |i| i + 1);
            self.focus = self
                .controls
                .get(next % self.controls.len().max(1))
                .map(|c| c.hwnd);
            return;
        }
        if let Some(index) = self
            .controls
            .iter()
            .position(|c| Some(c.hwnd) == self.focus)
        {
            let c = &mut self.controls[index];
            let (hwnd, id) = (c.hwnd, c.id);
            if let Some(key) = common {
                let anchor=Self::bounds(c,self.body);
                let n = match &mut c.model {
                    Model::List(v) => v.key(key, Modifiers::default(), 10),
                    Model::Tree(v) => v.key(key, 10),
                    Model::Bar(v) => v.key(key),
                    Model::Combo { combo, .. } => {
                        let k = match key {
                            CommonKey::Up => PopupKey::Up,
                            CommonKey::Down => PopupKey::Down,
                            CommonKey::Enter => PopupKey::Enter,
                            CommonKey::Space => PopupKey::Space,
                            _ => return,
                        };
                        combo.focused=true;
                        if matches!(combo.key(k, anchor, self.body),PopupResult::Selected(_)){self.event(self.hwnd,0x111,(id|0x10000)as u64,hwnd as u64);}
                        None
                    }
                    _ => None,
                };
                if let Some(n) = n {
                    self.notify(hwnd, id, n);
                }
                return;
            }
        }
        self.event(self.hwnd, 0x102, key as u64, 1);
    }
}
pub fn execute(shell: &mut ShellModel, p: &mut w::Packet) {
    p.values[1] = w::ACCEPTED;
    p.values[19] = 0;
    let action = p.values[4];
    let hwnd = p.values[5];
    let parent = p.values[6];
    let client = p.values[63];
    if action==8{let ids:Vec<u32>=shell.applications.iter().filter(|a|a.guest_window.as_ref().is_some_and(|s|s.client==client)).map(|a|a.window_id).collect();for id in ids{if let Some(a)=shell.applications.iter_mut().find(|a|a.window_id==id){a.guest_window=None;}shell.close_window(id);}scalar(p,1);return;}
    if action == 1 && parent == 0 {
        if shell
            .applications
            .iter()
            .filter(|a| a.guest_window.is_some())
            .count()
            >= 64
        {
            p.values[19] = 8;
            return;
        }
        let id = shell.launch_application(super::AppId::Notepad);
        if let Some(win) = shell.taskbar.windows.iter_mut().find(|v| v.id == id) {
            win.title = p.string(0).unwrap_or("").into();
            win.width = p.values[15].clamp(160, 1920) as i32;
            win.height = p.values[16].clamp(100, 1080) as i32;
        }
        let body=shell.application_body(id).unwrap_or(Rect::new(0,0,640,480));
        shell
            .applications
            .iter_mut()
            .find(|a| a.window_id == id)
            .unwrap()
            .guest_window = Some(Scene {
            client,
            hwnd,
            controls: Vec::new(),
            events: Vec::new(),
            focus: None,
            body,capture:None,
        });
        scalar(p, hwnd as u64);
        return;
    }
    let Some(index) = shell.applications.iter().position(|a| {
        a.guest_window.as_ref().is_some_and(|s| {
            s.client == client && s.contains(if action == 1 { parent } else { hwnd })
        })
    }) else {
        p.values[19] = 1400;
        return;
    };
    let id = shell.applications[index].window_id;
    if action == 2
        && shell.applications[index]
            .guest_window
            .as_ref()
            .unwrap()
            .hwnd
            == hwnd
    {
        shell.applications[index].guest_window = None;
        shell.close_window(id);
        scalar(p, 1);
        return;
    }
    if action == 5 && shell.applications[index].guest_window.as_ref().unwrap().hwnd==hwnd {
        if let Some(win) = shell.taskbar.windows.iter_mut().find(|v| v.id == id) {
            win.minimized = p.values[9] == 0;
        }
        scalar(p, 1);
        return;
    }
    let scene = shell.applications[index].guest_window.as_mut().unwrap();
    if action == 4 {
        if let Some(event) = scene.events.first().copied() {
            if p.values[9] != 0 {
                scene.events.remove(0);
            }
            let client = p.values[63];
            *p = event;
            p.values[0] = w::WINDOW;
            p.values[1] = w::ACCEPTED;
            p.values[63] = client;
            p.values[20] = 1;
        } else {
            p.values[20] = 0;
        }
        return;
    }
    if action == 1 {
        if scene.controls.len() >= 256 || scene.contains(hwnd) {
            p.values[19] = 8;
            return;
        }
        let id = p.values[17];
        let text = p.string(0).unwrap_or("");
        let model = match p.values[7] {
            1 => {
                let mut list = ListView::new(id);
                list.multi_select = p.values[18] & 4 == 0;
                Model::List(list)
            }
            2 => Model::Tree(TreeView::new(id)),
            3 => Model::Bar(Toolbar::new(id)),
            4 => Model::Combo {
                items: Vec::new(),
                combo: ComboBox::new(id, Vec::new()),
            },
            6 => Model::Button(text.into()),
            _ => Model::Text(text.into()),
        };
        scene.controls.push(Control {
            hwnd,
            id,
            parent,
            rect: Rect::new(
                p.values[13] as i32,
                p.values[14] as i32,
                p.values[15].min(4096),
                // CBS_DROPDOWN/CBS_DROPDOWNLIST height includes the dropped
                // list. Only the closed field participates in sibling layout.
                if p.values[7] == 4 { 25 } else { p.values[16].min(4096) },
            ),
            model,
            visible: p.values[18] & 0x10000000 != 0,
            enabled: p.values[18] & 0x8000000 == 0,
            previous:0,
        });
        scalar(p, hwnd as u64);
        return;
    }
    if action == 2 {
        scene
            .controls
            .retain(|c| c.hwnd != hwnd && c.parent != hwnd);
        scalar(p, 1);
        return;
    }
    let Some(control) = scene.controls.iter_mut().find(|c| c.hwnd == hwnd) else {
        if action == 6 {
            if let Some(win) = shell.taskbar.windows.iter_mut().find(|v| v.id == id) {
                win.title = p.string(0).unwrap_or("").into();
            }
            scalar(p, 1);
            return;
        }
        p.values[19] = 120;
        return;
    };
    if action==5{let old=control.visible;control.visible=p.values[9]!=0;scalar(p,u64::from(old));return;}
    if action == 7 {
        control.rect = Rect::new(
            p.values[13] as i32,
            p.values[14] as i32,
            p.values[15].min(4096),
            p.values[16].min(4096),
        );
        scalar(p, 1);
        return;
    }
    if action == 6 {
        match &mut control.model {
            Model::Text(s) | Model::Button(s) => *s = p.string(0).unwrap_or("").into(),
            _ => {
                p.values[19] = 120;
                return;
            }
        }
        scalar(p, 1);
        return;
    }
    let wp = value(p, 9);
    let lp = value(p, 11);
    let text = p.string(0).unwrap_or("").to_string();
    let result = match &mut control.model {
        Model::List(v) => match p.values[8] {
            0x1004 => v.rows().len() as u64,
            0x1007 | 0x104d => {
                if v.rows().len() >= 16384 {
                    p.values[19] = 8;
                    return;
                }
                let i = p.values[24] as usize;
                v.insert(i, alloc::vec![text], lp as isize)
                    .map(|_| i as u64)
                    .unwrap_or(u64::MAX)
            }
            0x101b | 0x1061 => {
                if wp as usize != v.columns.len() {
                    p.values[19] = 120;
                    return;
                }
                v.add_column(&text, p.values[25]);
                wp
            }
            0x102e | 0x1074 => {
                let Some(row) = v.rows().get(wp as usize) else {
                    scalar(p, 0);
                    return;
                };
                u64::from(v.set_text(row.id, p.values[25] as usize, &text))
            }
            0x102d | 0x1073 => {
                let value = v
                    .rows()
                    .get(wp as usize)
                    .and_then(|r| r.cells.get(p.values[25] as usize))
                    .cloned()
                    .unwrap_or_default();
                let length = value.encode_utf16().count();
                p.set_string(0, &value);
                length as u64
            }
            0x1008 => {
                let item = v.rows().get(wp as usize).map(|r| r.id);
                u64::from(item.is_some_and(|id| v.remove(id)))
            }
            0x1009 => {
                v.clear();
                1
            }
            0x1032 => v.selected().count() as u64,
            0x1013 => {
                let item = v.rows().get(wp as usize).map(|r| r.id);
                u64::from(item.is_some_and(|id| v.ensure_visible(id, 10)))
            }
            _ => {
                p.values[19] = 120;
                return;
            }
        },
        Model::Tree(v) => match p.values[8] {
            0x1100 | 0x1132 => {
                if v.items().len() >= 16384 {
                    p.values[19] = 8;
                    return;
                }
                v.insert(
                    if wp == 0 || wp as u32 == 0xffff0000 {
                        None
                    } else {
                        Some(wp)
                    },
                    &text,
                    lp as isize,
                )
                .unwrap_or(0)
            }
            0x1101 => {
                if lp as u32 == 0xffff0000 {
                    v.clear();
                    1
                } else {
                    u64::from(v.remove(lp))
                }
            }
            0x1102 => {
                v.expand(lp, wp & 2 != 0);
                1
            }
            0x1105 => v.items().len() as u64,
            0x110a => v.selected().unwrap_or(0),
            0x110b => {
                v.select(lp);
                1
            }
            0x110d | 0x113f => u64::from(v.set_text(lp, &text)),
            _ => {
                p.values[19] = 120;
                return;
            }
        },
        Model::Bar(v) => match p.values[8] {
            0x41e => v.buttons().len() as u64,
            0x414 => {
                let Some(command) = v.buttons().get(wp as usize).map(|b| b.command) else {
                    scalar(p, 0);
                    return;
                };
                u64::from(v.remove(command))
            }
            0x415 | 0x444 => {
                let style = p.values[25];
                let kind = if style & 1 != 0 {
                    ToolbarKind::Separator
                } else if style & 8 != 0 {
                    ToolbarKind::DropDown
                } else if style & 2 != 0 {
                    ToolbarKind::Check
                } else {
                    ToolbarKind::Button
                };
                let mut button = ToolbarButton::new(p.values[24], &text, kind);
                button.enabled = p.values[26] & 4 != 0;
                button.checked = p.values[26] & 1 != 0;
                u64::from(v.add(button))
            }
            0x401 => u64::from(v.set_enabled(wp as u32, lp != 0)),
            0x402 => u64::from(v.set_checked(wp as u32, lp != 0)),
            0x404 => u64::from(v.set_hidden(wp as u32, lp != 0)),
            0x421 => 1,
            _ => {
                p.values[19] = 120;
                return;
            }
        },
        Model::Combo { items, combo } => match p.values[8] {
            0x143 => {
                if items.len() >= 16384 {
                    p.values[19] = 8;
                    return;
                }
                let i = items.len() as u32;
                items.push(PopupItem::new(i, &text));
                let selected=combo.selected();*combo = ComboBox::new(control.id, items.clone());if let Some(id)=selected{combo.select(id);}else{combo.clear_selection();}
                i as u64
            }
            0x146 => items.len() as u64,
            0x147 => combo.selected().map_or(u64::MAX, |i| i as u64),
            0x14e => {
                if combo.select(wp as u32) {
                    wp
                } else {
                    u64::MAX
                }
            }
            0x148 | 0x149 => {
                let s = items
                    .get(wp as usize)
                    .map(|v| v.text.as_str())
                    .unwrap_or("");
                let n = s.encode_utf16().count();
                p.set_string(0, s);
                n as u64
            }
            0x14b => {
                items.clear();
                *combo = ComboBox::new(control.id, Vec::new());
                0
            }
            _ => {
                p.values[19] = 120;
                return;
            }
        },
        Model::Text(s) | Model::Button(s) => match p.values[8] {
            0x000e => s.encode_utf16().count() as u64,
            0x000c => {
                *s = text;
                1
            }
            _ => {
                p.values[19] = 120;
                return;
            }
        },
    };
    scalar(p, result);
}

#[cfg(test)]
mod tests {
 use super::*;
 use crate::theme::Theme;
 fn call(shell:&mut ShellModel,action:u32,hwnd:u32,parent:u32,kind:u32,msg:u32,wp:u64,lp:u64,text:&str)->w::Packet{
    let mut p=w::EMPTY;p.values[0]=w::WINDOW;p.values[4]=action;p.values[5]=hwnd;p.values[6]=parent;p.values[7]=kind;p.values[8]=msg;p.values[9]=wp as u32;p.values[10]=(wp>>32)as u32;p.values[11]=lp as u32;p.values[12]=(lp>>32)as u32;p.values[63]=41;p.values[15]=640;p.values[16]=460;p.values[18]=0x10000000;p.set_string(0,text);execute(shell,&mut p);assert_eq!(p.values[19],0);p
 }
 #[test]
 fn hwnd_models_messages_notifications_and_client_isolation(){
    let mut shell=ShellModel::new(Theme::aero());call(&mut shell,1,100,0,0,0,0,0,"Guest");
    for(hwnd,kind)in [(101,1),(102,2),(103,3),(104,4)]{call(&mut shell,1,hwnd,100,kind,0,0,0,"");}
    let mut p=w::EMPTY;p.values[0]=w::WINDOW;p.values[4]=3;p.values[5]=101;p.values[63]=41;p.values[8]=0x1061;p.values[25]=120;p.set_string(0,"Column");execute(&mut shell,&mut p);assert_eq!(p.values[19],0);
    p.values[8]=0x104d;p.values[24]=0;p.values[11]=0x12345678;p.set_string(0,"First row");execute(&mut shell,&mut p);assert_eq!(p.values[20],0);
    assert_eq!(call(&mut shell,3,101,0,0,0x1004,0,0,"").values[20],1);
    let tree=call(&mut shell,3,102,0,0,0x1132,0,77,"Root").values[20];assert_ne!(tree,0);
    call(&mut shell,3,102,0,0,0x1132,tree as u64,88,"Child");assert_eq!(call(&mut shell,3,102,0,0,0x1105,0,0,"").values[20],2);
    call(&mut shell,3,104,0,0,0x143,0,0,"One");call(&mut shell,3,104,0,0,0x143,0,0,"Two");assert_eq!(call(&mut shell,3,104,0,0,0x147,0,0,"").values[20],u32::MAX);
    call(&mut shell,3,104,0,0,0x14e,1,0,"");assert_eq!(call(&mut shell,3,104,0,0,0x148,1,0,"").string(0),Some("Two"));
    let scene=shell.applications[0].guest_window.as_mut().unwrap();scene.notify(101,0,CommonNotification::SelectionChanged{control:0,item:1});
    let event=call(&mut shell,4,100,0,0,0,1,0,"");assert_eq!(event.values[8],0x4e);assert_eq!(event.values[22],(-101i32)as u32);assert_eq!(event.values[24],0);assert_eq!(event.values[25],0x12345678);
    let mut foreign=w::EMPTY;foreign.values[4]=3;foreign.values[5]=101;foreign.values[63]=42;execute(&mut shell,&mut foreign);assert_eq!(foreign.values[19],1400);
    call(&mut shell,5,101,0,0,0,0,0,"");assert!(!shell.applications[0].guest_window.as_ref().unwrap().controls[0].visible);
    call(&mut shell,2,102,0,0,0,0,0,"");call(&mut shell,8,0,0,0,0,0,0,"");assert!(shell.applications.is_empty());
 }
 #[test]
 fn modal_owner_blocks_input_close_and_activation_only_for_its_client(){
    let mut shell=ShellModel::new(Theme::aero());
    call(&mut shell,1,100,0,0,0,0,0,"Owner");
    call(&mut shell,1,101,100,4,0,0,0,"");
    let owner=shell.applications[0].window_id;
    let mut foreign=w::EMPTY;foreign.values[4]=1;foreign.values[5]=100;foreign.values[63]=42;foreign.values[15]=640;foreign.values[16]=460;
    execute(&mut shell,&mut foreign);let other=shell.applications[1].window_id;
    let mut p=w::EMPTY;p.values[0]=w::FONT;p.values[60]=100;p.values[63]=41;
    shell.show_guest_dialog(7,p);let modal=shell.applications.last().unwrap().window_id;
    assert_eq!(shell.guest_modal_for(owner),Some(modal));assert_eq!(shell.guest_modal_for(other),None);
    shell.select_window(owner);assert_eq!(shell.selected_window,Some(modal));
    assert!(shell.application_key(owner,9));assert!(shell.application_pointer(owner,100,150,true,false,false));
    shell.close_window(owner);assert!(shell.application(owner).is_some());
    let scene=shell.application(owner).unwrap().guest_window.as_ref().unwrap();assert_eq!(scene.focus,None);assert!(scene.events.is_empty());
    shell.select_window(other);assert_eq!(shell.selected_window,Some(other));
    shell.applications.iter_mut().find(|a|a.window_id==modal).unwrap().guest_dialog.as_mut().unwrap().packet.values[1]=w::CANCELLED;
    assert_eq!(shell.guest_modal_for(owner),None);
    shell.application_key(owner,9);assert_eq!(shell.application(owner).unwrap().guest_window.as_ref().unwrap().focus,Some(101));
 }
 #[test]
 fn dropdown_keyboard_uses_real_body_and_escape_captures_first(){
    let mut shell=ShellModel::new(Theme::aero());call(&mut shell,1,100,0,0,0,0,0,"");call(&mut shell,1,101,100,4,0,0,0,"");call(&mut shell,3,101,0,0,0x143,0,0,"One");call(&mut shell,3,101,0,0,0x143,0,0,"Two");
    let scene=shell.applications[0].guest_window.as_mut().unwrap();scene.key(9);scene.key(13);assert!(scene.captures_pointer());scene.key(27);assert!(!scene.captures_pointer());assert!(scene.events.is_empty());
 }
}

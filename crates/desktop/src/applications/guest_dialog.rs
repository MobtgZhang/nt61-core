//! Desktop side of copied guest common-dialog requests.
use super::{
    common_dialogs::{self, Dialog},
    file_dialog::{self, FileDialog},
    AppId, ApplicationSession,
};
use crate::{platform::services, shell::ShellModel};
use alloc::{format, string::ToString, vec::Vec};
use nt61_abi::dialog_bridge as w;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub ticket: u64,
    pub packet: w::Packet,
}
pub fn complete(request: &Request) -> Result<(), u32> {
    let mut bytes = alloc::vec![0;w::PACKET_SIZE];
    request.packet.encode(&mut bytes);
    services::transfer(w::COMPLETE, &format!("{}", request.ticket), &bytes).map(|_| ())
}
fn rgb(value: u32) -> u32 {
    0xff000000 | ((value & 255) << 16) | (value & 0xff00) | ((value >> 16) & 255)
}
fn colorref(value: u32) -> u32 {
    ((value & 255) << 16) | (value & 0xff00) | ((value >> 16) & 255)
}
pub fn finish(app: &mut ApplicationSession, dialog: &Dialog, accepted: bool) {
    let Some(request) = &mut app.guest_dialog else {
        return;
    };
    let p = &mut request.packet;
    p.values[1] = if accepted { w::ACCEPTED } else { w::CANCELLED };
    if !accepted {
        return;
    }
    match dialog {
        Dialog::Color { draft, .. } => {
            p.values[4] = colorref(draft.color);
            for (out, color) in p.values[8..24].iter_mut().zip(draft.custom) {
                *out = colorref(color);
            }
        }
        Dialog::Font(d) => {
            p.values[4] = d.font.points;
            p.values[5] = d.font.style as u32;
            p.values[6] = u32::from(d.underline);
            p.values[7] = u32::from(d.strike);
            p.set_string(2, if d.font.sans { "Arial" } else { "Consolas" });
        }
        Dialog::Page { draft, .. } => {
            let scale = if p.values[3] & 8 != 0 {
                100.0
            } else {
                1000.0 / 25.4
            };
            for i in 0..4 {
                p.values[8 + i] =
                    (draft.margins[i].parse::<f32>().unwrap_or(0.0) * scale + 0.5) as u32;
            }
            let (mut width, mut height) = if draft.letter {
                (215.9, 279.4)
            } else {
                (210.0, 297.0)
            };
            if draft.landscape {
                core::mem::swap(&mut width, &mut height);
            }
            p.values[4] = (width * scale + 0.5) as u32;
            p.values[5] = (height * scale + 0.5) as u32;
        }
        Dialog::Print(d) => {
            p.values[3] = (p.values[3] & !0x13)
                | u32::from(d.selection)
                | (u32::from(d.range) << 1)
                | (u32::from(d.collate) << 4);
            p.values[4] = d.from.parse().unwrap_or(1);
            p.values[5] = d.to.parse().unwrap_or(1);
            p.values[6] = d.copies.parse().unwrap_or(1);
        }
    }
}
impl ShellModel {
    /// Locate a pending modal owned by this guest HWND. Other processes keep
    /// receiving input, while activation of the owner returns to its dialog.
    pub fn guest_modal_for(&self, id: u32) -> Option<u32> {
        let scene = self.application(id)?.guest_window.as_ref()?;
        self.applications.iter().find_map(|app| {
            let request = app.guest_dialog.as_ref()?;
            let p = &request.packet;
            (p.values[1] == w::PENDING && p.values[63] == scene.client
                && p.values[60] == scene.hwnd).then_some(app.window_id)
        })
    }
    pub fn poll_guest_dialogs(&mut self) -> bool {
        let mut changed = false;
        let done: Vec<u32> = self
            .applications
            .iter()
            .filter(|a| {
                a.guest_dialog
                    .as_ref()
                    .is_some_and(|r| r.packet.values[1] != w::PENDING)
            })
            .map(|a| a.window_id)
            .collect();
        for id in done {
            self.close_window(id);
            changed = true;
        }
        // Poll even with a modal open: this is also the broker liveness heartbeat.
        let Ok(bytes) = services::transfer(w::POLL, "", &[]) else {
            return changed;
        };
        if bytes.is_empty() {
            return changed;
        }
        if bytes.len() != 8 + w::PACKET_SIZE {
            return changed;
        }
        let Ok(ticket) = bytes[..8].try_into().map(u64::from_le_bytes) else {
            return changed;
        };
        let Some(packet) = w::Packet::decode(&bytes[8..]) else {
            return changed;
        };
        self.show_guest_dialog(ticket, packet);
        true
    }
    pub fn show_guest_dialog(&mut self, ticket: u64, packet: w::Packet) {
        let kind = packet.values[0];
        if kind == w::WINDOW {
            let mut packet = packet;
            super::guest_window::execute(self, &mut packet);
            let _ = complete(&Request { ticket, packet });
            return;
        }
        let id = self.launch_application(if kind == w::COLOR {
            AppId::Paint
        } else {
            AppId::WordPad
        });
        let index = self
            .applications
            .iter()
            .position(|a| a.window_id == id)
            .unwrap();
        self.applications[index].guest_dialog = Some(Request { ticket, packet });
        let title = packet
            .string(1)
            .filter(|s| !s.is_empty())
            .unwrap_or(match kind {
                w::OPEN => "Open",
                w::SAVE => "Save As",
                w::FONT => "Font",
                w::COLOR => "Color",
                w::PAGE => "Page Setup",
                _ => "Print",
            });
        if let Some(window) = self.taskbar.windows.iter_mut().find(|v| v.id == id) {
            window.title = title.into();
        }
        if matches!(kind, w::OPEN | w::SAVE) {
            let path = packet.string(0).unwrap_or("");
            let current = self.files.resolve(path);
            let is_dir = current
                .and_then(|n| self.files.node(n))
                .is_some_and(|n| n.kind == crate::file::NodeKind::Directory);
            let name = if is_dir {
                ""
            } else {
                path.rsplit(['\\', '/']).next().unwrap_or("")
            };
            let mut dialog = FileDialog::new(
                &mut self.files,
                if kind == w::OPEN {
                    file_dialog::Mode::Open
                } else {
                    file_dialog::Mode::SaveAs
                },
                current,
                name,
                file_dialog::Filter::All,
            );
            let directory = if is_dir {
                current
            } else {
                path.rsplit_once(['\\', '/'])
                    .and_then(|(p, _)| self.files.resolve(p))
            };
            if let Some(directory) = directory {
                dialog.guest_directory(&mut self.files, directory);
            }
            let fields: Vec<&str> = packet.text[2]
                .split(|b| *b == 0)
                .map_while(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        core::str::from_utf8(s).ok()
                    }
                })
                .collect();
            dialog.guest_filters = fields
                .chunks_exact(2)
                .map(|p| (p[0].into(), p[1].into()))
                .collect();
            dialog.guest_filter_index = packet.values[4].saturating_sub(1) as usize;
            if dialog.guest_filter_index >= dialog.guest_filters.len() {
                dialog.guest_filter_index = 0;
            }
            dialog.guest_extension = packet.string(3).unwrap_or("").into();
            dialog.refresh(&mut self.files);
            self.applications[index].dialog = Some(dialog);
            return;
        }
        let app = &mut self.applications[index];
        match kind {
            w::COLOR => {
                common_dialogs::open_color(app, common_dialogs::ColorTarget::Paint);
                if let Some(Dialog::Color { draft, .. }) = &mut app.common_dialog {
                    *draft = common_dialogs::ColorDialog::new(
                        rgb(packet.values[4]),
                        core::array::from_fn(|i| rgb(packet.values[8 + i])),
                    );
                }
            }
            w::FONT => {
                common_dialogs::open_font(app);
                if let Some(Dialog::Font(d)) = &mut app.common_dialog {
                    d.font.points = packet.values[4].clamp(1, 144);
                    d.points = d.font.points.to_string();
                    d.font.style = packet.values[5] as u8 & 3;
                    d.font.sans = packet
                        .string(2)
                        .is_some_and(|s| s.eq_ignore_ascii_case("Arial"));
                    d.underline = packet.values[6] != 0;
                    d.strike = packet.values[7] != 0;
                }
            }
            w::PRINT => {
                common_dialogs::open_print(app);
                if let Some(Dialog::Print(d)) = &mut app.common_dialog {
                    d.selection_available = packet.values[3] & 4 == 0;
                    d.selection = packet.values[3] & 1 != 0;
                    d.range = packet.values[3] & 2 != 0;
                    d.collate = packet.values[3] & 16 != 0;
                    d.from = packet.values[4].max(1).to_string();
                    d.to = packet.values[5].max(1).to_string();
                    d.copies = packet.values[6].max(1).to_string();
                }
            }
            w::PAGE => {
                common_dialogs::open_page(app);
                if let Some(Dialog::Page { draft, .. }) = &mut app.common_dialog {
                    draft.landscape=packet.values[12]!=0;draft.letter=packet.values[13]!=0;
                    if packet.values[3] & 2 != 0 {
                        let scale = if packet.values[3] & 8 != 0 {
                            100.0
                        } else {
                            1000.0 / 25.4
                        };
                        for i in 0..4 {
                            draft.margins[i] =
                                format!("{:.2}", packet.values[8 + i] as f32 / scale);
                        }
                    }
                }
            }
            _ => {
                if let Some(r) = &mut app.guest_dialog {
                    r.packet.values[1] = w::FAILED;
                    r.packet.values[2] = 0xffff;
                }
            }
        }
    }
    pub(super) fn finish_guest_file(
        &mut self,
        id: u32,
        dialog: &FileDialog,
        result: &file_dialog::Result,
    ) -> bool {
        if !self
            .application(id)
            .is_some_and(|a| a.guest_dialog.is_some())
            || matches!(result, file_dialog::Result::None)
        {
            return false;
        }
        let path = match result {
            file_dialog::Result::Open(file) => self.files.path(*file),
            file_dialog::Result::Save { parent, name, .. } => self
                .files
                .path(*parent)
                .map(|p| format!("{}\\{name}", p.trim_end_matches('\\'))),
            _ => None,
        };
        if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
            let r = app.guest_dialog.as_mut().unwrap();
            r.packet.values[1] = if matches!(result, file_dialog::Result::Cancel) {
                w::CANCELLED
            } else {
                w::ACCEPTED
            };
            if let Some(path) = path {
                if !r.packet.set_string(0, &path) {
                    r.packet.values[1] = w::FAILED;
                    r.packet.values[2] = 0x3003;
                }
            }
            r.packet.values[4] = dialog.guest_filter_index as u32 + 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
 use super::*;
 use crate::theme::Theme;
 #[test]
 fn copied_guest_dialogs_preserve_filters_and_complete_without_native_file_writes(){
    let mut shell=ShellModel::new(Theme::aero());let mut p=w::EMPTY;p.values[0]=w::SAVE;p.values[4]=2;p.set_string(0,"example.txt");p.set_string(3,"txt");let filters=b"Text\0*.txt;*.log\0All files\0*.*\0\0";p.text[2][..filters.len()].copy_from_slice(filters);
    shell.show_guest_dialog(41,p);let id=shell.applications[0].window_id;let dialog=shell.applications[0].dialog.clone().unwrap();assert_eq!(dialog.guest_filters.len(),2);assert_eq!(dialog.guest_filter_index,1);assert_eq!(dialog.guest_extension,"txt");
    assert!(shell.finish_guest_file(id,&dialog,&file_dialog::Result::Cancel));assert_eq!(shell.applications[0].guest_dialog.as_ref().unwrap().packet.values[1],w::CANCELLED);
    p.values[0]=w::PAGE;p.values[3]=2|8;p.values[8..12].copy_from_slice(&[100,200,300,400]);p.values[12]=1;p.values[13]=1;shell.show_guest_dialog(42,p);
    let app=shell.applications.last_mut().unwrap();let dialog=app.common_dialog.clone().unwrap();let Dialog::Page{draft,..}=&dialog else{panic!()};assert!(draft.landscape&&draft.letter);assert_eq!(draft.margins[0],"1.00");finish(app,&dialog,true);let reply=app.guest_dialog.as_ref().unwrap().packet;assert_eq!(reply.values[1],w::ACCEPTED);assert_eq!(&reply.values[8..12],&[100,200,300,400]);assert!(reply.values[4]>reply.values[5]);
 }
}

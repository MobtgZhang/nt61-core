//! Window/session integration, shared by Explorer and the native application runner.
use super::{explorer, AppAction, AppId, AppState, ApplicationSession, Environment, Task};
use crate::{
    graphics::{Rect, Surface},
    shell::{ShellModel, WindowKind},
};
use alloc::{format, string::String};
impl ShellModel {
    pub fn application_environment(&self) -> Environment {
        Environment {
            width: self.viewport.0,
            height: self.viewport.1,
            network_adapters: self.network_adapters,
            audio: self.audio_available,
            processes: self.process_snapshot.clone(),
            monitor: self.monitor_sample.clone(),
            services: self.service_snapshot.clone(),
            users: core::iter::once((self.session.identity.clone(),true)).chain(self.saved_desktops.iter().map(|d|(d.user.clone(),false))).collect(),
            tasks: self
                .taskbar
                .windows
                .iter()
                .map(|w| Task {
                    id: w.id,
                    icon: self.applications.iter().find(|app| app.window_id==w.id).map(|app|app.id).or_else(||self.explorers.iter().any(|ex|ex.window_id==w.id).then_some(AppId::Explorer)),
                    title: w.title.clone(),
                    minimized: w.minimized,
                })
                .collect(),
        }
    }
    pub fn application(&self, id: u32) -> Option<&ApplicationSession> {
        self.applications.iter().find(|a| a.window_id == id)
    }
    pub fn launch_application(&mut self, id: AppId) -> u32 {
        if matches!(id,AppId::TaskManager|AppId::MediaPlayer|AppId::DeviceManager) {if let Some(window)=self.applications.iter().find(|a|a.id==id).map(|a|a.window_id){if let Some(w)=self.taskbar.windows.iter_mut().find(|w|w.id==window){w.minimized=false;}self.select_window(window);return window;}}
        if id == AppId::Explorer {
            return self.open_window_with_kind(
                "Windows Explorer",
                60,
                60,
                640,
                450,
                WindowKind::Explorer,
            );
        }
        let (width, height) = match id {
            AppId::PhotoViewer => (720, 550),
            AppId::DeviceManager => (790, 580),
            AppId::MediaPlayer => (780, 540),
            AppId::WordPad => (800, 600),
            AppId::Calculator => (350, 430),
            AppId::StickyNotes => (430, 350),
            AppId::Run => (540, 240),
            AppId::TaskManager => (480, 440),
            AppId::Minesweeper => (510, 420),
            AppId::OnScreenKeyboard => (520, 340),
            _ => (640, 460),
        };
        let offset = (self.taskbar.windows.len() % 5) as i32 * 22;
        let window = self.open_window_with_kind(
            id.application().title,
            60 + offset,
            60 + offset,
            width,
            height,
            if id == AppId::CommandPrompt {
                WindowKind::CommandPrompt
            } else {
                WindowKind::Application(id)
            },
        );
        let mut app = ApplicationSession::new(window, id, window as u64 * 7919);
        app.notepad.fonts = self.fonts.clone();
        if let AppState::DeviceManager(m)=&mut app.state {m.refresh();}
        self.applications.push(app);
        if id==AppId::MediaPlayer {self.media_action(window,super::media_player::Request::Refresh);}
        if id == AppId::Notepad {
            self.update_application_title(window);
        }
        window
    }
    pub fn application_body(&self, id: u32) -> Option<Rect> {
        if self.application(id).is_some_and(|a|matches!(&a.state,AppState::PhotoViewer(v) if v.slideshow)){return Some(Rect::new(0,0,self.viewport.0,self.viewport.1));}
        self.taskbar.windows.iter().find(|w| w.id == id).map(|w| {
            let b = Rect::new(
                w.x + 6,
                w.y + 30,
                w.width.saturating_sub(12) as u32,
                w.height.saturating_sub(36) as u32,
            );
            if self.control_browser(id).is_some(){crate::control_browser::content(b)}else{b}
        })
    }
    pub fn application_key(&mut self, id: u32, key: u8) -> bool {
        if self.taskbar.windows.iter().any(|w|w.id==id&&w.kind==WindowKind::UserAccounts){return self.account_settings.key(key); }
        if let Some(modal) = self.guest_modal_for(id) { self.select_window(modal); return true; }
        if let Some(explorer) = self.explorers.iter_mut().find(|e| e.window_id == id) {
            let changed = explorer.stream_key(&mut self.files, key);
            let open = explorer.pending_open.take();
            if let Some(file) = open {
                self.open_application_file(file);
            }
            return changed;
        }
        let Some(index) = self.applications.iter().position(|a| a.window_id == id) else {
            return false;
        };
        if matches!(self.applications[index].state, AppState::CommandPrompt(_)) {
            let AppState::CommandPrompt(c) = &mut self.applications[index].state else {
                unreachable!()
            };
            let action = c.key(&mut self.files, key);
            self.apply_application_action(id, action);
            return true;
        }
        let now = self.applications[index].input_time;
        let Some(key) = self.applications[index].decoder.feed(key, now) else {
            return false;
        };
        self.application_decoded_key(id, key);
        true
    }
    fn application_decoded_key(&mut self, id: u32, key: u8) {
        let Some(index) = self.applications.iter().position(|a| a.window_id == id) else {
            return;
        };
        self.applications[index].manager.menu_viewport = self.viewport;
        if self.applications[index].id == AppId::Notepad
            && !self.applications[index].status.starts_with("Saved ")
            && !self.applications[index].status.is_empty()
        {
            if key == 13 || key == 27 {
                self.applications[index].status.clear();
            }
            return;
        }
        self.applications[index].notepad.clipboard = !self.files.clipboard_text().is_empty();
        if self.applications[index].confirm {
            match key {
                b's' | b'S' | 13 => {
                    self.applications[index].confirm = false;
                    let action = self.applications[index].save();
                    self.apply_application_action(id, action);
                }
                b'n' | b'N' => {
                    self.applications[index].confirm = false;
                    self.finish_pending(id);
                }
                27 => {
                    self.applications[index].confirm = false;
                    self.applications[index].pending = None;
                }
                _ => {}
            }
            return;
        }
        if let Some(mut dialog) = self.applications[index].dialog.take() {
            if let Some(body) = self.application_body(id) {
                dialog.visible_rows =
                    (super::file_dialog::list_bounds(body).height / 24).max(1) as usize;
            }
            let result = if let Some(body) = self.application_body(id) {
                dialog.key_at(&mut self.files, key, body)
            } else {
                dialog.key(&mut self.files, key)
            };
            self.finish_file_dialog(id, dialog, result);
            return;
        }
        if let Some(b) = self.application_body(id) {
            if self.applications[index].common_dialog.is_some() {
                let action = super::common_dialogs::key_at(&mut self.applications[index], key, b);
                self.apply_application_action(id, action);
                return;
            }
            if let AppState::Document(d) = &self.applications[index].state {
                let r = super::notepad::editor_bounds(b, d, &self.applications[index].notepad);
                self.applications[index].notepad.editor_size = (r.width, r.height);
            }
        }
        if let Some(body) = self.application_body(id) {
            if let AppState::PhotoViewer(v) = &mut self.applications[index].state { v.viewport = body; }
            if let AppState::DeviceManager(m) = &mut self.applications[index].state { m.viewport = body; }
        }
        let action = self.applications[index].key(key);
        self.apply_application_action(id, action);
    }
    /// Complete a solitary Escape only after allowing ANSI navigation bytes to arrive.
    pub fn flush_application_input(&mut self, now: u64) -> bool {
        let mut expired = alloc::vec::Vec::new();
        for app in &mut self.applications {
            app.input_time = now;
            if let Some(key) = app.decoder.expire(now) {
                expired.push((app.window_id, key));
            }
        }
        let mut changed = !expired.is_empty();
        let mut closed=alloc::vec::Vec::new();
        for app in &mut self.applications {
            if let AppState::MediaPlayer(p)=&mut app.state {changed|=p.poll(now);}
            if let AppState::CommandPrompt(console)=&mut app.state {
                let (updated,exit)=console.poll_command();changed|=updated;
                if exit {closed.push(app.window_id);}
            }
        }
        let media:alloc::vec::Vec<_>=self.applications.iter_mut().filter_map(|a|if let AppState::MediaPlayer(p)=&mut a.state {p.pending.take().map(|r|(a.window_id,r))}else{None}).collect();
        for (id,request) in media {self.media_action(id,request);changed=true;}
        let mut advance=alloc::vec::Vec::new();
        for app in &mut self.applications {
            if let AppState::DeviceManager(m)=&mut app.state {if m.refresh {m.refresh();changed=true;}}
            if let AppState::PhotoViewer(v)=&mut app.state {if v.tick(now) {advance.push(app.window_id);}}
        }
        for id in advance {self.photo_action(id,super::photo_viewer::Request::Navigate(1));changed=true;}
        for id in closed {self.close_window(id);}

        for (id, key) in expired {
            self.application_decoded_key(id, key);
        }
        changed
    }
    pub fn request_application_close(&mut self, id: u32) {
        self.request_pending(id, super::Pending::Close);
    }
    fn request_pending(&mut self, id: u32, pending: super::Pending) {
        let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) else {
            if pending == super::Pending::Close {
                self.close_window(id);
            }
            return;
        };
        if app.dialog.is_some() || app.common_dialog.is_some() || app.confirm {
            return;
        }
        app.menu = false;
        app.search = None;
        app.notepad.search.active = false;
        app.notepad.dialog = None;
        app.pending = Some(pending);
        if app.dirty() {
            app.confirm = true;
        } else {
            self.finish_pending(id);
        }
    }
    fn finish_pending(&mut self, id: u32) {
        let Some(index) = self.applications.iter().position(|a| a.window_id == id) else {
            return;
        };
        let pending = self.applications[index].pending.take();
        match pending {
            Some(super::Pending::Close) => self.close_window(id),
            Some(super::Pending::Open) => {
                self.begin_file_dialog(id, super::file_dialog::Mode::Open)
            }
            Some(super::Pending::New) => {
                let app = &mut self.applications[index];
                let wrap = if let AppState::Document(d) = &app.state {
                    d.word_wrap
                } else {
                    false
                };
                app.state = ApplicationSession::new(id, app.id, 0).state;
                if let AppState::Document(d) = &mut app.state {
                    d.word_wrap = wrap;
                }
                app.wordpad.reset_document();
                app.notepad.top = 0;
                app.notepad.left = 0;
                app.status.clear();
                self.update_application_title(id);
            }
            None => {}
        }
    }
    fn begin_file_dialog(&mut self, id: u32, mode: super::file_dialog::Mode) {
        let Some(index) = self.applications.iter().position(|a| a.window_id == id) else {
            return;
        };
        let app = &self.applications[index];
        let current = if let AppState::MediaPlayer(p)=&app.state {if mode==super::file_dialog::Mode::SaveAs {p.playlist_file}else{p.current}}else{app.current_file()};
        let name = if let AppState::MediaPlayer(p)=&app.state {if mode==super::file_dialog::Mode::SaveAs {p.playlist_name.clone()}else{String::new()}}else if mode == super::file_dialog::Mode::Open {
            String::new()
        } else {
            app.file_name().into()
        };
        let name = if app.id == AppId::WordPad && name.to_ascii_lowercase().ends_with(".doc") {
            format!(
                "{}.rtf",
                name.get(..name.len().saturating_sub(4))
                    .unwrap_or("Document")
            )
        } else {
            name
        };
        let name = if app.id == AppId::WordPad
            && mode == super::file_dialog::Mode::SaveAs
            && matches!(&app.state,AppState::Document(d) if d.rich.as_ref().is_some_and(|r|!r.warning.is_empty()))
        {
            let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, "rtf"));
            format!("{stem}-imported.{ext}")
        } else {
            name
        };
        let filter = match app.state {
            AppState::PhotoViewer(_) => super::file_dialog::Filter::Images,
            AppState::Paint(_) | AppState::Snipping { .. } => super::file_dialog::Filter::Bitmap,
            AppState::MediaPlayer(_) => if mode==super::file_dialog::Mode::SaveAs {super::file_dialog::Filter::Playlist}else{super::file_dialog::Filter::Media},
            _ if app.id == AppId::WordPad => super::file_dialog::Filter::Documents,
            _ => super::file_dialog::Filter::Text,
        };
        let mut dialog =
            super::file_dialog::FileDialog::new(&mut self.files, mode, current, &name, filter);
        if app.id == AppId::Notepad {
            dialog.text_encoding = true;
            if mode == super::file_dialog::Mode::SaveAs {
                if let AppState::Document(d) = &app.state {
                    dialog.encoding = Some(d.encoding);
                }
            }
        }
        let app = &mut self.applications[index];
        app.dialog = Some(dialog);
        app.menu = false;
        app.hot = None;
    }
    fn finish_file_dialog(
        &mut self,
        id: u32,
        mut dialog: super::file_dialog::FileDialog,
        result: super::file_dialog::Result,
    ) {
        use super::file_dialog::Result as R;
        if self.finish_guest_file(id, &dialog, &result) { return; }
        let outcome: Result<(), String> = match result {
            R::None => {
                if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
                    app.dialog = Some(dialog);
                }
                return;
            }
            R::Cancel => {
                if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
                    app.pending = None;
                }
                return;
            }
            R::Open(file) => self.load_into_application_encoding(id, file, dialog.encoding),
            R::Save {
                parent,
                name,
                existing,
            } => {
                let playlist_bytes=self.application(id).and_then(|a|if let AppState::MediaPlayer(p)=&a.state {Some(super::media_player::playlist::encode(&name,p.playlist.iter().filter_map(|t|self.files.path(t.id)).collect::<alloc::vec::Vec<_>>().iter().map(String::as_str)))}else{None});
                // "Make a copy" must preserve the original container, metadata,
                // animation and TIFF pages when the destination format is unchanged.
                let original_photo=self.application(id).and_then(|a|if let AppState::PhotoViewer(v)=&a.state {
                    super::photo_viewer::codec::same_format(&v.name,&name).then_some(v.current).flatten()
                }else{None});
                let photo_copy=original_photo.map(|file|self.files.read_media(file)
                    .map_err(|s|format!("Cannot copy the picture (0x{s:08X}).")));
                let action = self
                    .applications
                    .iter_mut()
                    .find(|a| a.window_id == id)
                    .map(|a| {
                        if let Some(bytes)=playlist_bytes {return AppAction::Save(name.clone(),bytes);}
                        if let AppState::PhotoViewer(v)=&a.state {
                            if let Some(result)=photo_copy {
                                return match result {Ok(bytes)=>AppAction::Save(name.clone(),bytes),Err(error)=>{a.status=error;AppAction::None}};
                            }
                            return match v.image.as_ref().ok_or("No picture is loaded.").and_then(|s|super::photo_viewer::codec::encode(&name,s)) {Ok(bytes)=>AppAction::Save(name.clone(),bytes),Err(e)=>{a.status=e.into();AppAction::None}};
                        }
                        if a.id == AppId::WordPad {
                            if let AppState::Document(d) = &a.state {
                                if let Some(r) = &d.rich {
                                    return match super::rich_text::Format::for_name(&name)
                                        .and_then(|f| r.encode(&d.editor.text, f))
                                    {
                                        Ok(bytes) => AppAction::Save(name.clone(), bytes),
                                        Err(e) => {
                                            a.status = e.into();
                                            AppAction::None
                                        }
                                    };
                                }
                            }
                        }
                        if a.id == AppId::Notepad {
                            if let AppState::Document(d) = &a.state {
                                let encoding = dialog.encoding.unwrap_or(d.encoding);
                                if encoding == super::document::Encoding::Ansi
                                    && !d.ansi_representable()
                                {
                                    return AppAction::None;
                                }
                                return AppAction::Save(d.filename.clone(), d.encoded_as(encoding));
                            }
                        }
                        a.save()
                    });
                if let Some(AppAction::Save(_, bytes)) = action {
                    let result = if let Some(file) = existing {
                        self.files.save(file, &bytes).map(|()| file)
                    } else {
                        let file = self.files.touch(parent, &name, &bytes);
                        if file == 0 {
                            Err(self
                                .files
                                .last_error
                                .unwrap_or(nt61_abi::desktop_bridge::STATUS_NOT_FOUND))
                        } else {
                            Ok(file)
                        }
                    };
                    match result {
                        Ok(file) => {
                            if let Some(app) =
                                self.applications.iter_mut().find(|a| a.window_id == id)
                            {
                                if let (Some(encoding), AppState::Document(d)) =
                                    (dialog.encoding, &mut app.state)
                                {
                                    d.encoding = encoding;
                                }
                                app.saved(file, name);
                                app.status =
                                    format!("Saved {}", self.files.path(file).unwrap_or_default());
                            }
                            if self.application(id).is_some_and(|a|a.id==AppId::PhotoViewer){if let Err(error)=self.load_photo(id,file,0){self.photo_error(id,error);}}
                            self.update_application_title(id);
                            self.finish_pending(id);
                            Ok(())
                        }
                        Err(status) => Err(format!("Save failed (0x{status:08X})")),
                    }
                } else {
                    Err(self.application(id).filter(|a|matches!(a.id,AppId::WordPad|AppId::PhotoViewer)).map_or_else(||String::from("Some characters cannot be saved as ANSI. Choose Unicode or UTF-8."),|a|a.status.clone()))
                }
            }
        };
        if let Err(message) = outcome {
            dialog.status = message;
            dialog.overwrite = None;
            if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
                app.dialog = Some(dialog);
            }
        }
    }
    #[cfg(test)]
    fn load_into_application(&mut self, id: u32, file: u32) -> Result<(), String> {
        self.load_into_application_encoding(id, file, None)
    }
    fn load_into_application_encoding(
        &mut self,
        id: u32,
        file: u32,
        encoding: Option<super::document::Encoding>,
    ) -> Result<(), String> {
        if self.application(id).is_some_and(|a|a.id==AppId::PhotoViewer){return self.load_photo(id,file,0);}
        if self.application(id).is_some_and(|a|a.id==AppId::MediaPlayer){return self.load_media(id,file);}
        self.files
            .load_file(file)
            .map_err(|s| format!("Open failed (0x{s:08X})"))?;
        let node = self
            .files
            .node(file)
            .ok_or_else(|| String::from("File not found."))?;
        let name = node.name.clone();
        let app = self
            .applications
            .iter_mut()
            .find(|a| a.window_id == id)
            .ok_or_else(|| String::from("Window closed."))?;
        match &mut app.state {
            AppState::Document(d) => {
                if let Some(encoding) = encoding {
                    d.load_encoding(&node.contents, encoding)
                        .map_err(String::from)?;
                } else {
                    d.load(&node.contents, app.id == AppId::WordPad)
                        .map_err(String::from)?;
                }
            }
            AppState::Paint(p) => p.load_bmp(&node.contents).map_err(String::from)?,
            _ => return Err("Unsupported document type.".into()),
        }
        app.saved(file, name);
        app.status = if let AppState::Document(d) = &app.state {
            d.rich
                .as_ref()
                .map_or_else(String::new, |r| r.warning.clone())
        } else {
            String::new()
        };
        app.wordpad.reset_document();
        app.search = None;
        app.notepad.top = 0;
        app.notepad.left = 0;
        app.notepad.reveal();
        if app.id == AppId::Notepad {
            if let AppState::Document(d) = &mut app.state {
                d.move_to(0, 0);
            }
        }
        self.update_application_title(id);
        Ok(())
    }
    fn update_application_title(&mut self, id: u32) {
        let Some(app) = self.application(id) else {
            return;
        };
        let name = if app.id == AppId::Notepad && app.current_file().is_none() {
            "Untitled"
        } else {
            app.file_name()
        };
        let title = format!("{} - {}", name, app.id.application().title);
        if let Some(w) = self.taskbar.windows.iter_mut().find(|w| w.id == id) {
            w.title = title;
        }
    }
    /// Clear hover when a popup or another window takes pointer ownership.
    pub fn clear_application_hover(&mut self, id: u32) -> bool {
        if let Some(explorer) = self.explorers.iter_mut().find(|e| e.window_id == id) {
            return explorer.hot.take().is_some();
        }
        self.applications
            .iter_mut()
            .find(|a| a.window_id == id)
            .is_some_and(|app| app.hot.take().is_some())
    }
    /// A dragged thumb keeps receiving motion outside its window until release.
    pub fn captured_pointer_window(&self) -> Option<u32> {
        self.explorers.iter().find(|e| e.captures_pointer()).map(|e| e.window_id)
            .or_else(|| self.applications.iter().find(|app| (matches!(&app.state,AppState::PhotoViewer(v) if v.captures_pointer()) || matches!(&app.state,AppState::DeviceManager(m) if m.captures_pointer())) || app.manager.scroll_grab.is_some() || app.dialog.as_ref().is_some_and(|d| d.captures_pointer()) || super::common_dialogs::captures_pointer(app) || app.guest_window.as_ref().is_some_and(|s|s.captures_pointer())).map(|app| app.window_id))
    }

    pub fn application_wheel(&mut self, id: u32, x: i32, y: i32, delta: i32) -> bool {
        if let Some(modal) = self.guest_modal_for(id) { self.select_window(modal); return true; }
        let Some(body) = self.application_body(id) else { return false; };
        if !body.contains(x, y) { return false; }
        if self.control_browser(id).is_some() && self.application(id).is_none() {
            if self.taskbar.windows.iter().any(|w| w.id == id && w.kind == WindowKind::DisplaySettings) && self.display.open {
                let rows=crate::display_settings::list_rows(body);
                self.display.first_visible=self.display.first_visible.saturating_add_signed(-(delta as isize)).min(self.display.modes.len().saturating_sub(rows));
                return true;
            }
            return self.control_scroll(id, -delta.saturating_mul(48));
        }
        if let Some(explorer) = self.explorers.iter_mut().find(|e| e.window_id == id) {
            return explorer.wheel(&self.files, body, delta);
        }
        let env = self.application_environment();
        if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
            if app.id == AppId::TaskManager { return super::task_manager::wheel(app,body,&env,delta); }
            if let Some(dialog) = &mut app.dialog { return dialog.wheel(body, delta); }
            if app.common_dialog.is_some() { return super::common_dialogs::wheel(app, body, delta); }
            if let AppState::PhotoViewer(v)=&mut app.state {if !v.message.is_empty()||v.delete_confirm||v.print.is_some(){return false;}v.zoom_by(delta,super::photo_viewer::image_bounds(body,v.slideshow));return true;}
            if let AppState::DeviceManager(m)=&mut app.state {m.scroll(delta,body);return true;}
        }
        false
    }

    pub fn application_pointer(
        &mut self,
        id: u32,
        x: i32,
        y: i32,
        down: bool,
        held: bool,
        right: bool,
    ) -> bool {
        if let Some(modal) = self.guest_modal_for(id) { self.select_window(modal); return true; }
        let Some(body) = self.application_body(id) else {
            return false;
        };
        // Native Control Panel pages have no application hover state. Avoid
        // cloning process/service telemetry on every pointer move over them.
        if self.control_browser(id).is_some() && self.application(id).is_none() {
            return false;
        }
        if let Some(index) = self.explorers.iter().position(|e| e.window_id == id) {
            let (changed, action) =
                self.explorers[index].pointer(&mut self.files, body, x, y, down, held, right);
            if let explorer::Action::Open(file_id) = action {
                self.open_application_file(file_id);
            }
            return changed;
        }
        if let Some(scene)=self.applications.iter_mut().find(|a|a.window_id==id).and_then(|a|a.guest_window.as_mut()) {
            scene.pointer(body,x,y,down,held);return true;
        }
        let dialog_capture = self.applications.iter().find(|app| app.window_id == id)
            .is_some_and(|app| (matches!(&app.state,AppState::PhotoViewer(v) if v.captures_pointer()) || matches!(&app.state,AppState::DeviceManager(m) if m.captures_pointer())) || app.manager.scroll_grab.is_some() || app.dialog.as_ref().is_some_and(|d| d.captures_pointer()) || super::common_dialogs::captures_pointer(app) || app.guest_window.as_ref().is_some_and(|s|s.captures_pointer()));
        let menu_capture = self.applications.iter().find(|a|a.window_id==id && a.id==AppId::TaskManager)
            .is_some_and(|a|super::task_manager::popup_contains(a,body,x,y));
        if !body.contains(x, y) && !dialog_capture && !menu_capture {
            if !held {
                if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
                    match &mut app.state {
                        AppState::Paint(p) => p.end(),
                        AppState::Document(d) => {
                            d.drag_anchor = None;
                            app.wordpad.release_pointer();
                        }
                        _ => {}
                    }
                }
            }
            return self.clear_application_hover(id);
        }
        if !down && !held && !right {
            if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id && a.id == AppId::TaskManager) {
                if let Some(changed) = super::task_manager::quiet_pointer(app, body, x, y) { return changed; }
            }
        }
        let clipboard = !self.files.clipboard_text().is_empty();
        let env = self.application_environment();
        let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) else {
            return false;
        };
        app.manager.menu_viewport = self.viewport;
        if app.id == AppId::TaskManager && super::task_manager::scroll_pointer(app,body,&env,x,y,down,held) { return true; }
        app.notepad.clipboard = clipboard;
        if app.confirm {
            if down {
                let r = if app.id == AppId::Notepad {
                    super::notepad::confirm_buttons(body)
                } else {
                    super::view::confirm_buttons(body)
                };
                let key = if r[0].contains(x, y) {
                    Some(b's')
                } else if r[1].contains(x, y) {
                    Some(b'n')
                } else if r[2].contains(x, y) {
                    Some(27)
                } else {
                    None
                };
                if let Some(key) = key {
                    self.application_decoded_key(id, key);
                }
            }
            return down;
        }
        if let Some(mut dialog) = app.dialog.take() {
            dialog.input_time = app.input_time;
            let (changed, result) = dialog.pointer(&mut self.files, body, x, y, down, held);
            self.finish_file_dialog(id, dialog, result);
            return changed;
        }
        if app.common_dialog.is_some() {
            return !right && super::common_dialogs::pointer_event(app, body, x, y, down, held);
        }
        if app.id==AppId::DeviceManager {
            let (changed,action)=super::device_manager::pointer(app,body,x,y,down,held,right);
            self.apply_application_action(id,action);return changed;
        }
        if app.id==AppId::PhotoViewer {
            let (changed,action)=super::photo_viewer::pointer(app,body,x,y,down,held,right);
            self.apply_application_action(id,action);return changed;
        }
        if app.id == AppId::Notepad {
            let (changed, action) = super::notepad::pointer(app, body, x, y, down, held, right);
            self.apply_application_action(id, action);
            return changed;
        }
        if let Some(search) = &mut app.search {
            if down {
                if let AppState::Document(d) = &mut app.state {
                    if super::editor_view::search_click(search, d, body, x, y) {
                        app.search = None;
                    }
                }
            }
            return down;
        }
        let menu_changed = app.id == AppId::TaskManager && !down && !held && !right
            && super::task_manager::menu_hover(app,body,x,y);
        let hot = super::view::buttons(app, body, &env)
            .into_iter()
            .rev()
            .find(|b| b.bounds.contains(x, y))
            .map(|b| b.bounds);
        let hover_changed = app.hot != hot;
        app.hot = hot;
        if let AppState::Paint(p) = &mut app.state {
            let canvas = super::view::paint_bounds(body);
            if !app.menu && canvas.contains(x, y) && (held || right) {
                let px = ((x - canvas.x) as u32 * 100 / p.zoom + p.scroll.0) as i32;
                let py = ((y - canvas.y) as u32 * 100 / p.zoom + p.scroll.1) as i32;
                if down {
                    p.begin_button(px, py, right);
                } else {
                    p.stroke(px, py);
                }
                return true;
            }
            if !held {
                p.end();
            }
        }
        let mut editor_changed = false;
        if !app.menu {
            if let AppState::Document(d) = &mut app.state {
                if app.id != AppId::WordPad {
                    editor_changed = super::editor_view::pointer(d, body, x, y, down, held);
                }
            }
        }
        if !app.menu
            && app.id == AppId::WordPad
            && (hot.is_none() || app.wordpad.dragging_controls())
        {
            editor_changed = super::wordpad::pointer(app, body, x, y, down, held);
        }
        if down {
            let action = super::view::click(app, body, &env, x, y, right);
            self.apply_application_action(id, action);
        }
        down || hover_changed || editor_changed || menu_changed
    }
    pub fn apply_application_action(&mut self, id: u32, action: AppAction) {
        match action {
            AppAction::Photo(request)=>self.photo_action(id,request),
            AppAction::None | AppAction::Capture => {}
            AppAction::InsertTime => {
                let value = if let Some(date) = self.taskbar.date {
                    format!(
                        "{} {}/{}/{}",
                        self.taskbar.clock, date.month, date.day, date.year
                    )
                } else {
                    String::new()
                };
                if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
                    if value.is_empty() {
                        app.notepad.dialog = Some(super::notepad::Dialog::Message(
                            "The system clock is unavailable.".into(),
                        ));
                    } else {
                        app.paste(&value);
                    }
                }
            }
            AppAction::Close => self.close_window(id),
            AppAction::RequestClose => self.request_application_close(id),
            AppAction::Open => self.request_pending(id, super::Pending::Open),
            AppAction::NewDocument => self.request_pending(id, super::Pending::New),
            AppAction::SaveAs => self.begin_file_dialog(id, super::file_dialog::Mode::SaveAs),
            AppAction::Launch(app) => {
                self.launch_application(app);
            }
            AppAction::Save(name, bytes) => {
                let needs_unicode=self.application(id).is_some_and(|app|app.id==AppId::Notepad&&matches!(&app.state,AppState::Document(d) if d.encoding==super::document::Encoding::Ansi&&!d.ansi_representable()));
                if needs_unicode {
                    self.begin_file_dialog(id, super::file_dialog::Mode::SaveAs);
                    if let Some(dialog) = self
                        .applications
                        .iter_mut()
                        .find(|a| a.window_id == id)
                        .and_then(|a| a.dialog.as_mut())
                    {
                        dialog.encoding = Some(super::document::Encoding::Utf8Bom);
                        dialog.status =
                            "Unicode characters require Unicode or UTF-8 encoding.".into();
                    }
                    return;
                }

                if self
                    .application(id)
                    .is_some_and(|a| a.current_file().is_none())
                {
                    self.begin_file_dialog(id, super::file_dialog::Mode::SaveAs);
                    return;
                }
                let result = self.save_application_file(id, &name, &bytes);
                if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
                    match &result {
                        Ok(path) => {
                            app.status = format!("Saved {path}");
                            let file = app.current_file();
                            if let Some(file) = file {
                                app.saved(file, name.clone());
                            }
                        }
                        Err(status) => app.status = format!("Save failed (0x{status:08X})"),
                    }
                }
                if result.is_ok() {
                    self.finish_pending(id);
                }
            }
            AppAction::Copy(text) => self.files.set_clipboard_text(&text),
            AppAction::Paste => {
                let text = String::from(self.files.clipboard_text());
                if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
                    app.paste(&text);
                }
            }
            AppAction::Activate(target) => {
                let minimize=self.application(id).is_some_and(|a|a.id==AppId::TaskManager&&a.manager.minimize_on_use);
                if minimize {if let Some(w)=self.taskbar.windows.iter_mut().find(|w|w.id==id){w.minimized=true;}}
                if let Some(w) = self.taskbar.windows.iter_mut().find(|w| w.id == target) {
                    w.minimized = false;
                }
                self.select_window(target);
            }
            AppAction::EndTask(target) => self.request_application_close(target),
            AppAction::Manage(action) => self.task_manager_action(id, action),
            AppAction::Media(request) => self.media_action(id,request),
            AppAction::Type(key) => {
                if let Some(target) = self.text_target.filter(|target| *target != id) {
                    if self.applications.iter().any(|a| a.window_id == target) {
                        self.application_key(target, key);
                    } else {
                        let old = self.selected_window;
                        self.selected_window = Some(target);
                        self.handle_window_key(key);
                        self.selected_window = old;
                    }
                }
            }
        }
    }
    fn photo_error(&mut self,id:u32,error:String) {
        if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){if let AppState::PhotoViewer(v)=&mut app.state {v.message=error;v.delete_confirm=false;}}
    }
    fn load_photo(&mut self,id:u32,file:u32,page:usize)->Result<(),String>{
        let bytes=self.files.read_media(file).map_err(|s|format!("Open failed (0x{s:08X}); the picture must be at most 32 MiB."))?;
        let parent=self.files.node(file).map(|n|n.parent);
        if let Some(parent)=parent {self.files.load_directory(parent).map_err(|s|format!("Cannot enumerate pictures (0x{s:08X})"))?;}
        let siblings=parent.map(|p|{let mut children=self.files.children(p).into_iter().filter(|n|n.kind!=crate::file::NodeKind::Directory&&super::associations::resolve(&n.name)==Some(AppId::PhotoViewer)).collect::<alloc::vec::Vec<_>>();children.sort_by_cached_key(|n|n.name.to_ascii_lowercase());children.into_iter().take(10000).map(|n|n.id).collect()}).unwrap_or_else(||alloc::vec![file]);
        let node=self.files.node(file).ok_or("Picture not found.")?;
        let app=self.applications.iter_mut().find(|a|a.window_id==id).ok_or("Window closed.")?;
        let AppState::PhotoViewer(v)=&mut app.state else{return Err("Invalid picture window.".into());};
        v.load(file,node.name.clone(),&bytes,siblings,page).map_err(String::from)?;
        self.update_application_title(id);Ok(())
    }
    fn photo_action(&mut self,id:u32,request:super::photo_viewer::Request){
        use super::photo_viewer::Request;
        let result:Result<(),String>=(||{
            let app=self.application(id).ok_or("Window closed.")?;
            let AppState::PhotoViewer(v)=&app.state else{return Err("Invalid picture window.".into());};
            match request {
                Request::Print=>{
                    let options=v.print.as_ref().ok_or("The Print Pictures window is closed.")?;
                    let data=super::photo_viewer::printing::document(v.image.as_ref().ok_or("No picture is loaded.")?,options)?;
                    let reply=crate::platform::services::transfer(nt61_abi::dialog_bridge::PRINT_WRITE,"",&data).map_err(|s|format!("Printing failed (0x{s:08X})."))?;
                    let bytes: [u8;4]=reply.as_slice().try_into().map_err(|_|"Invalid print backend response.")?;
                    if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){if let AppState::PhotoViewer(v)=&mut app.state {v.print=None;v.message=format!("Your picture was sent to NT61 PostScript Document Writer.\nPrint job: {}",u32::from_le_bytes(bytes));}}
                    Ok(())
                },
                Request::Navigate(delta)=>{let next=v.neighbor(delta).ok_or("There are no pictures in this folder.")?;self.load_photo(id,next,0)},
                Request::Page(delta)=>{let file=v.current.ok_or("No picture is loaded.")?;let page=v.page as i64+delta as i64;if page<0||page>=v.pages as i64{return Ok(());}self.load_photo(id,file,page as usize)},
                Request::Rotate(right)=>{
                    let file=v.current.ok_or("No picture is loaded.")?;
                    let rotated=super::photo_viewer::codec::rotate(v.image.as_ref().ok_or("No picture is loaded.")?,right).map_err(String::from)?;
                    let name=v.name.clone();let page=v.page;
                    let original=if super::photo_viewer::codec::same_format(&name,"picture.tif") {self.files.read_media(file).map_err(|s|format!("The original TIFF could not be read (0x{s:08X})."))?}else{alloc::vec::Vec::new()};
                    let bytes=super::photo_viewer::codec::encode_rotated(&name,&original,page,&rotated).map_err(String::from)?;
                    self.files.save(file,&bytes).map_err(|s|format!("The picture could not be saved (0x{s:08X})."))?;
                    if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){if let AppState::PhotoViewer(v)=&mut app.state {v.image=Some(rotated);v.reset_view();}}
                    Ok(())
                },
                Request::Delete=>{
                    let file=v.current.ok_or("No picture is loaded.")?;let next=v.neighbor(1).filter(|n|*n!=file);
                    if !self.files.recycle(file,self.applications.iter().find(|a|a.window_id==id).map_or(0,|a|a.input_time)){return Err(format!("The picture could not be moved to the Recycle Bin (0x{:08X}).",self.files.last_error.unwrap_or(nt61_abi::desktop_bridge::STATUS_NOT_SUPPORTED)));}
                    if let Some(next)=next {self.load_photo(id,next,0)}else{if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){if let AppState::PhotoViewer(v)=&mut app.state {**v=super::photo_viewer::Viewer::default();}}Ok(())}
                },
                Request::Edit=>{let file=v.current.ok_or("No picture is loaded.")?;let source=v.image.as_ref().ok_or("No picture is loaded.")?;if source.width().checked_mul(source.height()).is_none_or(|n| n>super::paint::MAX_PIXELS){return Err("This picture is too large for Paint. Save a smaller copy before editing.".into());}let image=source.clone();let name=v.name.clone();let bitmap=name.to_ascii_lowercase().ends_with(".bmp");let edit=self.launch_application(AppId::Paint);if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==edit){if let AppState::Paint(p)=&mut app.state{p.canvas=image;p.filename=if bitmap{name}else{format!("{name}.bmp")};p.file_id=bitmap.then_some(file);p.mark_saved();}}self.update_application_title(edit);Ok(())},
                Request::Wallpaper=>{let file=v.current.ok_or("No picture is loaded.")?;let path=self.files.path(file).ok_or("Picture path not found.")?;let mut data=alloc::vec![0];data.extend_from_slice(path.as_bytes());crate::platform::services::session("wallpaper-set",&data).map_err(|s|format!("Desktop background could not be changed (0x{s:08X})."))?;let preference=crate::personalization::Preference{path,position:0};self.personalization.current=preference.clone();self.personalization.saved=preference;self.personalization.revision+=1;Ok(())},
                Request::Properties=>{let file=v.current.ok_or("No picture is loaded.")?;let node=self.files.node(file).ok_or("Picture not found.")?;let image=v.image.as_ref().ok_or("No picture is loaded.")?;let message=format!("{}\nLocation: {}\nSize: {} bytes\nDimensions: {} x {} pixels\nPage: {} / {}",v.name,self.files.path(file).unwrap_or_default(),node.size.unwrap_or(node.contents.len() as u64),image.width(),image.height(),v.page+1,v.pages);self.photo_error(id,message);Ok(())}
            }
        })();
        if let Err(error)=result {self.photo_error(id,error);}
    }
    fn load_media(&mut self,id:u32,file:u32)->Result<(),String>{
        let name=self.files.node(file).ok_or("File not found.")?.name.clone();
        let bytes=self.files.read_media(file).map_err(|s|format!("Cannot open media (0x{s:08X}); maximum file size is 32 MiB."))?;
        if name.to_ascii_lowercase().ends_with(".wpl") {
            let paths=super::media_player::playlist::decode(&bytes).map_err(String::from)?;
            let location=self.files.path(file).ok_or("Invalid playlist path.")?.replace('\\',"/");
            let directory=location.rsplit_once('/').map(|(p,_)|p).ok_or("Invalid playlist path.")?;
            let mut tracks=alloc::vec::Vec::new();
            for path in paths {
                let path=path.replace('\\',"/");let path=if path.as_bytes().get(1)==Some(&b':'){path}else{format!("{directory}/{path}")};
                let target=self.files.resolve(&path).ok_or_else(||format!("Playlist item not found: {path}"))?;
                let node=self.files.node(target).ok_or("Playlist item not found.")?;
                if node.name.to_ascii_lowercase().ends_with(".wpl"){return Err("Nested playlists are not supported.".into());}
                tracks.push(super::media_player::Track{id:target,name:node.name.clone()});
            }
            let first=tracks.first().map(|t|t.id);
            if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){if let AppState::MediaPlayer(p)=&mut app.state {p.playlist=tracks;p.playlist_file=Some(file);p.playlist_name=name;}}
            return if let Some(first)=first {self.load_media(id,first)}else{Ok(())};
        }
        let stream=super::media_player::audio::Stream::open(bytes).map_err(String::from)?;
        let app=self.applications.iter_mut().find(|a|a.window_id==id).ok_or("Window closed.")?;
        let AppState::MediaPlayer(p)=&mut app.state else{return Err("Invalid media window.".into());};
        if !p.library.iter().any(|t|t.id==file){p.library.push(super::media_player::Track{id:file,name});}
        p.load(file,stream);p.start_output();Ok(())
    }
    fn media_action(&mut self,id:u32,request:super::media_player::Request){
        match request {
            super::media_player::Request::Load(file)=>{if let Err(error)=self.load_media(id,file){if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){if let AppState::MediaPlayer(p)=&mut app.state{p.error=error;}}}},
            super::media_player::Request::Refresh=>{
                let mut dirs=alloc::vec::Vec::new();
                for path in [format!("C:/Users/{}/Music",self.session.identity),"C:/Users/Public/Music".into(),"C:/Music".into()] {if let Some(dir)=self.files.resolve(&path){dirs.push(dir);}}let mut library=alloc::vec::Vec::new();let mut visited=0;
                while let Some(dir)=dirs.pop(){if visited>=128||library.len()>=2048{break;}visited+=1;
                    if self.files.load_directory(dir).is_err(){continue;}
                    for n in self.files.children(dir){if n.kind==crate::file::NodeKind::Directory {dirs.push(n.id);}else if super::associations::resolve(&n.name)==Some(AppId::MediaPlayer)&&!n.name.to_ascii_lowercase().ends_with(".wpl"){library.push(super::media_player::Track{id:n.id,name:n.name.clone()});}}
                }
                library.sort_by(|a,b|a.name.cmp(&b.name));
                if let Some(app)=self.applications.iter_mut().find(|a|a.window_id==id){if let AppState::MediaPlayer(p)=&mut app.state{for old in &p.playlist {if !library.iter().any(|t|t.id==old.id){library.push(old.clone());}}p.library=library;}}
            },
        }
    }
    fn save_application_file(&mut self, id: u32, _name: &str, bytes: &[u8]) -> Result<String, u32> {
        let file = self
            .application(id)
            .and_then(|a| a.current_file())
            .ok_or(nt61_abi::desktop_bridge::STATUS_NOT_FOUND)?;
        self.files.save(file, bytes)?;
        self.files
            .path(file)
            .ok_or(nt61_abi::desktop_bridge::STATUS_NOT_FOUND)
    }
    pub fn open_application_file(&mut self, file_id: u32) -> Option<u32> {
        if self.files.node(file_id).is_some_and(|n|super::associations::resolve(&n.name)==Some(AppId::PhotoViewer)) {
            let id=self.launch_application(AppId::PhotoViewer);
            let result=self.load_photo(id,file_id,0);
            if let Err(error)=result {self.photo_error(id,error);}
            return Some(id);
        }
        if self.files.node(file_id).is_some_and(|n|super::associations::resolve(&n.name)==Some(AppId::MediaPlayer)) {
            let id=self.launch_application(AppId::MediaPlayer);self.media_action(id,super::media_player::Request::Load(file_id));return Some(id);
        }
        if let Err(status) = self.files.load_file(file_id) {
            self.report_file_open(file_id, &format!("Open failed (0x{status:08X})"));
            return None;
        }
        let node = self.files.node(file_id)?;
        // Open only a registered executable through its canonical path. Never
        // display a PE/DLL as replacement characters in an editor.
        if node.name.to_ascii_lowercase().ends_with(".exe") {
            let path = self.files.path(file_id)?;
            if let Some(id) = super::catalog::APPLICATIONS
                .iter()
                .find(|app| {
                    app.install_path().is_some_and(|p| {
                        path.get(3..).is_some_and(|relative| {
                            relative.replace('\\', "/").eq_ignore_ascii_case(p)
                        })
                    })
                })
                .map(|app| app.id)
            {
                return Some(self.launch_application(id));
            }
            self.report_file_open(
                file_id,
                "This executable is not supported by the application host.",
            );
            return None;
        }
        let app_id = match super::associations::resolve(&node.name) {
            Some(app) => app,
            None if super::document::decode_text(&node.contents).is_ok() => AppId::Notepad,
            None => {
                self.report_file_open(file_id, "No application is associated with this file type.");
                return None;
            }
        };
        let name = node.name.clone();
        let mut session = ApplicationSession::new(0, app_id, 0);
        match &mut session.state {
            AppState::Document(document) => {
                if let Err(message) = document.load(&node.contents, app_id == AppId::WordPad) {
                    self.report_file_open(file_id, message);
                    return None;
                }
                document.file_id = Some(file_id);
                document.filename = name;
                if app_id == AppId::Notepad {
                    document.editor.cursor = 0;
                }
            }
            AppState::Paint(p) => match p.load_bmp(&node.contents) {
                Ok(()) => {
                    p.file_id = Some(file_id);
                    p.filename = name.clone();
                }
                Err(_) => {
                    self.report_file_open(file_id, "Invalid or unsupported bitmap.");
                    return None;
                }
            },
            _ => {}
        }
        let window = self.launch_application(app_id);
        if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == window) {
            app.state = session.state;
            if let AppState::Document(d) = &app.state {
                app.status = d
                    .rich
                    .as_ref()
                    .map_or_else(String::new, |r| r.warning.clone());
            }
        }
        self.update_application_title(window);
        Some(window)
    }
    fn report_file_open(&mut self, file_id: u32, message: &str) {
        for explorer in &mut self.explorers {
            if explorer.selected == Some(file_id) {
                explorer.status = message.into();
            }
        }
    }
    /// Capture only the composed desktop beneath the application, supplied by the host renderer.
    pub fn capture_application(&mut self, id: u32, image: Surface) {
        if let Some(app) = self.applications.iter_mut().find(|a| a.window_id == id) {
            app.capture(image);
        }
    }
    pub fn needs_capture(&self, id: u32, x: i32, y: i32) -> bool {
        let Some(app) = self.application(id) else {
            return false;
        };
        let Some(body) = self.application_body(id) else {
            return false;
        };
        super::view::buttons(app, body, &self.application_environment())
            .iter()
            .any(|b| b.bounds.contains(x, y) && b.command == super::view::Command::Capture)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    #[test]
    fn device_horizontal_thumb_retains_shell_capture_outside_the_window() {
        let mut shell=ShellModel::new(Theme::aero());
        let id=shell.launch_application(AppId::DeviceManager);
        let AppState::DeviceManager(m)=&mut shell.applications.iter_mut().find(|a|a.window_id==id).unwrap().state else {panic!()};
        m.horizontal_grab=Some(3);
        assert_eq!(shell.captured_pointer_window(),Some(id));
        let body=shell.application_body(id).unwrap();
        shell.application_pointer(id,body.x-50,body.y-50,false,false,false);
        assert_eq!(shell.captured_pointer_window(),None);
    }
    #[test]
    fn making_a_photo_copy_preserves_original_container_bytes() {
        use super::super::{file_dialog::{Mode,Result as DialogResult},photo_viewer::codec};
        let mut shell=ShellModel::new(Theme::aero());
        let source=crate::graphics::Surface::new(2,3);
        let mut bytes=codec::encode("a.jpg",&source).unwrap();
        // JPEG allows application metadata that must survive a same-format copy.
        bytes.splice(2..2,[0xff,0xe2,0,8,b'N',b'T',b'6',b'1',b'!',b'!']);
        let file=shell.files.touch(1,"a.jpg",&bytes);
        let id=shell.open_application_file(file).unwrap();
        shell.begin_file_dialog(id,Mode::SaveAs);
        let dialog=shell.applications.iter_mut().find(|a|a.window_id==id).unwrap().dialog.take().unwrap();
        shell.finish_file_dialog(id,dialog,DialogResult::Save{parent:1,name:"copy.jpeg".into(),existing:None});
        let copy=shell.files.children(1).into_iter().find(|n|n.name=="copy.jpeg").unwrap();
        assert_eq!(copy.contents,bytes);
        let AppState::PhotoViewer(v)=&shell.application(id).unwrap().state else {panic!()};
        assert_eq!(v.current,Some(copy.id));
    }
    #[test]
    fn photo_browsing_rotation_and_recycling_update_real_file_contents() {
        use super::super::photo_viewer::{codec,Request};
        let mut shell=ShellModel::new(Theme::aero());
        let mut source=crate::graphics::Surface::new(2,3);
        for y in 0..3 {for x in 0..2 {source.set_pixel(x,y,0xff102030+x*0x100000+y*0x001000);}}
        let a=shell.files.touch(1,"a.png",&codec::encode("a.png",&source).unwrap());
        let b=shell.files.touch(1,"b.bmp",&codec::encode("b.bmp",&source).unwrap());
        shell.files.touch(1,"skip.txt",b"not a picture");
        let id=shell.open_application_file(a).unwrap();
        shell.photo_action(id,Request::Rotate(true));
        assert_eq!(codec::decode(&shell.files.node(a).unwrap().contents,0).unwrap().0,codec::rotate(&source,true).unwrap());
        shell.photo_action(id,Request::Navigate(1));
        let AppState::PhotoViewer(v)=&shell.application(id).unwrap().state else {panic!()};
        assert_eq!(v.current,Some(b));assert_eq!(v.siblings,alloc::vec![a,b]);
        shell.photo_action(id,Request::Delete);
        assert!(shell.files.node(b).unwrap().deleted);
        let AppState::PhotoViewer(v)=&shell.application(id).unwrap().state else {panic!()};
        assert_eq!(v.current,Some(a));assert_eq!(v.siblings,alloc::vec![a]);
    }
    #[test]
    fn unsupported_rotation_leaves_both_picture_and_file_intact() {
        use super::super::photo_viewer::Request;
        let mut shell=ShellModel::new(Theme::aero());
        let bytes=b"GIF89a\x01\0\x01\0\x80\0\0\xff\0\0\0\0\0\x2c\0\0\0\0\x01\0\x01\0\0\x02\x02\x44\x01\0\x3b";
        let file=shell.files.touch(1,"a.gif",bytes);let id=shell.open_application_file(file).unwrap();
        shell.photo_action(id,Request::Rotate(true));
        let AppState::PhotoViewer(v)=&shell.application(id).unwrap().state else {panic!()};
        assert!(!v.message.is_empty());assert_eq!(v.image.as_ref().unwrap().pixel(0,0),Some(0xffff0000));
        assert_eq!(&shell.files.node(file).unwrap().contents,bytes);
    }
    #[test]
    fn full_screen_slideshow_owns_pointer_without_invisible_caption_controls() {
        let mut shell=ShellModel::new(Theme::aero());shell.viewport=(1024,768);
        let id=shell.launch_application(AppId::PhotoViewer);
        let bounds=shell.taskbar.windows.iter().find(|w|w.id==id).unwrap().bounds();
        let AppState::PhotoViewer(v)=&mut shell.applications.iter_mut().find(|a|a.window_id==id).unwrap().state else {panic!()};
        v.slideshow=true;v.image=Some(crate::graphics::Surface::new(1,1));
        assert_eq!(shell.window_hit_test(1023,767),Some(id));
        assert_eq!(shell.application_body(id),Some(Rect::new(0,0,1024,768)));
        assert_eq!(shell.window_cursor_at(bounds.x,bounds.y),crate::shell::WindowCursor::Arrow);
        assert!(!shell.begin_window_drag(id,bounds.x,bounds.y));
        assert!(!shell.handle_window_control_click(id,bounds.x+bounds.width as i32-20,bounds.y+10,1024,728));
        assert!(shell.taskbar.windows.iter().any(|w|w.id==id));
    }
    #[test]
    fn task_manager_file_and_window_menu_actions_reach_shell_windows() {
        use super::super::explorer::input;
        let mut shell = ShellModel::new(Theme::aero());
        let note = shell.launch_application(AppId::Notepad);
        let manager = shell.launch_application(AppId::TaskManager);
        shell.application_decoded_key(manager, input::ALT_FILE);
        shell.application_decoded_key(manager, 13);
        assert!(shell.applications.iter().any(|a| a.id == AppId::Run));
        shell.applications.iter_mut().find(|a| a.window_id == manager).unwrap().manager.selected = Some(u64::from(note));
        for (item, minimized, maximized) in [(2, true, false), (3, false, true), (5, false, true)] {
            shell.application_decoded_key(manager, 0xa5); // Alt+W
            shell.applications.iter_mut().find(|a| a.window_id == manager).unwrap().manager.menu_focus = item;
            shell.application_decoded_key(manager, 13);
            let window = shell.taskbar.windows.iter().find(|w| w.id == note).unwrap();
            assert_eq!((window.minimized, window.maximized), (minimized, maximized));
        }
        assert_eq!(shell.selected_window, Some(note));
        for item in [0, 1, 4] {
            shell.application_decoded_key(manager, 0xa5);
            shell.applications.iter_mut().find(|a| a.window_id == manager).unwrap().manager.menu_focus = item;
            shell.application_decoded_key(manager, 13);
            let window = shell.taskbar.windows.iter().find(|w| w.id == note).unwrap();
            assert!(!window.minimized && !window.maximized);
            assert!(window.width > 0 && window.height > 0);
            assert!(window.restore_geometry.is_some());
        }
        shell.application_decoded_key(manager, input::ALT_FILE);
        shell.application_decoded_key(manager, input::DOWN);
        shell.application_decoded_key(manager, 13);
        assert!(shell.application(manager).is_none());
        assert!(shell.application(note).is_some());
    }
    #[test]
    fn task_manager_submenu_receives_mouse_outside_owner_and_dismisses_on_activation() {
        use super::super::{explorer::input, task_manager};
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.launch_application(AppId::TaskManager);
        shell.application_decoded_key(id, input::ALT_VIEW);
        shell.application_decoded_key(id, input::DOWN);
        shell.application_decoded_key(id, input::RIGHT);
        let body = shell.application_body(id).unwrap();
        let env = shell.application_environment();
        let paused = task_manager::buttons(shell.application(id).unwrap(),body,&env)
            .into_iter().find(|b|b.command == super::super::view::Command::TaskManager(task_manager::Command::Item(14))).unwrap().bounds;
        let point = (paused.x + paused.width as i32 - 8, paused.y + 8);
        assert!(!body.contains(point.0, point.1));
        assert_eq!(shell.window_hit_test(point.0,point.1),Some(id));
        shell.application_pointer(id,point.0,point.1,true,true,false);
        assert!(shell.application(id).unwrap().manager.paused);
        assert!(shell.application(id).unwrap().manager.menu.is_none());
        shell.application_decoded_key(id,input::ALT_VIEW);
        let other=shell.launch_application(AppId::Calculator);
        shell.select_window(other);
        assert!(shell.application(id).unwrap().manager.menu.is_none());
    }
    #[test]
    fn shared_modals_capture_shell_input_and_prevent_owner_close_until_dismissed() {
        use super::super::common_dialogs::{self, ColorTarget, Dialog};
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.launch_application(AppId::Paint);
        let app = shell.applications.iter_mut().find(|app| app.window_id == id).unwrap();
        common_dialogs::open_color(app, ColorTarget::Paint);
        let original = app.state.clone();
        let body = shell.application_body(id).unwrap();
        let r = common_dialogs::bounds(shell.application(id).unwrap().common_dialog.as_ref().unwrap(), body);
        shell.application_pointer(id, r.x + 286, r.y + 58, true, true, false);
        assert_eq!(shell.captured_pointer_window(), Some(id));
        shell.application_pointer(id, r.x + 360, r.y + 58, false, true, false);
        shell.application_pointer(id, r.x + 360, r.y + 58, false, false, false);
        assert_eq!(shell.captured_pointer_window(), None);
        shell.request_application_close(id);
        assert!(shell.application(id).is_some());
        assert_eq!(shell.application(id).unwrap().state, original);
        shell.application_decoded_key(id, 27);
        assert!(shell.application(id).unwrap().common_dialog.is_none());
        let editor = shell.launch_application(AppId::Notepad);
        shell.application_key(editor, b'a');
        let app = shell.applications.iter_mut().find(|app| app.window_id == editor).unwrap();
        common_dialogs::open_font(app);
        shell.application_key(editor, b'z');
        let AppState::Document(doc) = &shell.application(editor).unwrap().state else { panic!("doc"); };
        assert_eq!(doc.editor.text, "a");
        assert!(matches!(shell.application(editor).unwrap().common_dialog, Some(Dialog::Font(_))));
        shell.application_decoded_key(editor, 27);
        shell.application_key(editor, b'b');
        let AppState::Document(doc) = &shell.application(editor).unwrap().state else { panic!("doc"); };
        assert_eq!(doc.editor.text, "ab");
    }
    #[test]
    fn run_launch_save_and_task_manager_end_task_are_real_actions() {
        let mut shell = ShellModel::new(Theme::aero());
        let calc = shell.launch_application(AppId::Calculator);
        let task = shell.launch_application(AppId::TaskManager);
        shell.apply_application_action(task, AppAction::EndTask(calc));
        assert!(shell.application(calc).is_none());
        let editor = shell.launch_application(AppId::Notepad);
        for key in b"hello" {
            shell.application_key(editor, *key);
        }
        shell.application_key(editor, 19);
        assert!(shell.application(editor).unwrap().dialog.is_some());
        for key in b"hello.txt\r" {
            shell.application_key(editor, *key);
        }
        let file = shell.files.resolve("C:\\hello.txt").unwrap();
        assert_eq!(shell.files.node(file).unwrap().contents, b"hello");
        let run = shell.launch_application(AppId::Run);
        for key in b"calc.exe\r" {
            shell.application_key(run, *key);
        }
        assert!(shell
            .applications
            .iter()
            .any(|app| app.id == AppId::Calculator));
    }
}

#[cfg(test)]
mod file_open_tests {
    use super::*;
    use crate::theme::Theme;
    #[test]
    fn explorer_reopens_rtf_in_wordpad_and_saves_to_the_same_vfs_node() {
        let mut shell = ShellModel::new(Theme::aero());
        let mut original = super::super::document::Document::new("Document.rtf");
        original.insert("Hello 中😀");
        original.bold = true;
        let file = shell.files.touch(1, "Document.RTF", &original.rtf());
        let id = shell.open_application_file(file).unwrap();
        let app = shell.application(id).unwrap();
        assert_eq!(app.id, AppId::WordPad);
        let AppState::Document(doc) = &app.state else {
            panic!("document");
        };
        assert_eq!(doc.editor.text, "Hello 中😀");
        assert!(doc.bold);
        assert_eq!(doc.file_id, Some(file));
        shell.application_key(id, b'!');
        shell.application_key(id, 19);
        let mut saved = super::super::document::Document::new("Document.rtf");
        saved
            .load(&shell.files.node(file).unwrap().contents, true)
            .unwrap();
        assert_eq!(saved.editor.text, "Hello 中😀!");
        assert!(
            !shell
                .files
                .children(1)
                .iter()
                .any(|node| node.name == "Documents"),
            "saving an open document must not depend on or create a default Documents folder"
        );
    }
    #[test]
    fn unsupported_binary_is_rejected_and_bad_picture_reports_an_error() {
        let mut shell = ShellModel::new(Theme::aero());
        let file = shell.files.touch(1,"ntdll.dll",b"MZ\0binary");
        assert_eq!(shell.open_application_file(file),None);
        assert!(shell.applications.is_empty());
        let file = shell.files.touch(1,"wrong.bmp",b"invalid");
        let id=shell.open_application_file(file).unwrap();
        let AppState::PhotoViewer(v)=&shell.application(id).unwrap().state else {panic!()};
        assert!(v.image.is_none());assert!(!v.message.is_empty());
    }
    #[test]
    fn leaving_a_window_clears_button_and_explorer_hover() {
        let mut shell = ShellModel::new(Theme::aero());
        let calc = shell.launch_application(AppId::Calculator);
        shell
            .applications
            .iter_mut()
            .find(|app| app.window_id == calc)
            .unwrap()
            .hot = Some(Rect::new(0, 0, 10, 10));
        assert!(shell.application_pointer(calc, -1, -1, false, false, false));
        assert!(shell.application(calc).unwrap().hot.is_none());
        assert!(!shell.clear_application_hover(calc));
        let explorer = shell.launch_application(AppId::Explorer);
        shell
            .explorers
            .iter_mut()
            .find(|e| e.window_id == explorer)
            .unwrap()
            .hot = Some(explorer::Hit::Back);
        assert!(shell.clear_application_hover(explorer));
        assert!(shell
            .explorers
            .iter()
            .find(|e| e.window_id == explorer)
            .unwrap()
            .hot
            .is_none());
    }
}

#[cfg(test)]
mod accessibility_tests {
    use super::*;
    use crate::theme::Theme;
    #[test]
    fn control_panel_accessibility_tools_launch_real_sessions() {
        let mut shell = ShellModel::new(Theme::aero());
        let center = shell.launch_application(AppId::EaseOfAccessCenter);
        let body = shell.application_body(center).unwrap();
        let env = shell.application_environment();
        let buttons = super::super::view::buttons(shell.application(center).unwrap(), body, &env);
        for target in [AppId::Magnifier, AppId::OnScreenKeyboard] {
            let bounds = buttons
                .iter()
                .find(|b| b.command == super::super::view::Command::Launch(target))
                .unwrap()
                .bounds;
            assert!(shell.application_pointer(
                center,
                bounds.x + 4,
                bounds.y + 4,
                true,
                true,
                false
            ));
            assert!(shell.applications.iter().any(|app| app.id == target));
        }
    }
}

#[cfg(test)]
mod document_workflow_tests {
    use super::*;
    use crate::theme::Theme;
    fn type_keys(s: &mut ShellModel, id: u32, text: &[u8]) {
        for key in text {
            s.application_key(id, *key);
        }
    }
    fn escape(s: &mut ShellModel, id: u32) {
        s.application_key(id, 27);
        s.flush_application_input(200);
    }
    #[test]
    fn save_as_cancel_and_overwrite_preserve_the_right_file() {
        let mut s = ShellModel::new(Theme::aero());
        let source = s.files.touch(1, "source.txt", b"original");
        let target = s.files.touch(1, "target.txt", b"keep");
        let id = s.open_application_file(source).unwrap();
        s.application_key(id, super::super::explorer::input::CTRL_END);
        type_keys(&mut s, id, b" edited");
        s.apply_application_action(id, AppAction::SaveAs);
        type_keys(&mut s, id, b"target.txt\r");
        assert_eq!(
            s.application(id)
                .unwrap()
                .dialog
                .as_ref()
                .unwrap()
                .overwrite,
            Some(target)
        );
        assert_eq!(s.files.node(target).unwrap().contents, b"keep");
        type_keys(&mut s, id, b"n");
        escape(&mut s, id);
        assert!(s.application(id).unwrap().dialog.is_none());
        assert!(s.application(id).unwrap().dirty());
        assert_eq!(s.application(id).unwrap().current_file(), Some(source));
        s.apply_application_action(id, AppAction::SaveAs);
        type_keys(&mut s, id, b"target.txt\ry");
        assert_eq!(s.files.node(source).unwrap().contents, b"original");
        assert_eq!(s.files.node(target).unwrap().contents, b"original edited");
        assert_eq!(s.application(id).unwrap().current_file(), Some(target));
        assert!(!s.application(id).unwrap().dirty());
        type_keys(&mut s, id, b"!\x13");
        assert_eq!(s.files.node(target).unwrap().contents, b"original edited!");
    }
    #[test]
    fn dirty_close_save_discard_and_cancel_are_distinct() {
        let mut s = ShellModel::new(Theme::aero());
        let id = s.launch_application(AppId::Notepad);
        type_keys(&mut s, id, b"unsaved");
        s.request_application_close(id);
        assert!(s.application(id).unwrap().confirm);
        escape(&mut s, id);
        assert!(!s.application(id).unwrap().confirm);
        assert!(s.application(id).unwrap().pending.is_none());
        s.request_application_close(id);
        type_keys(&mut s, id, b"s");
        assert!(s.application(id).unwrap().dialog.is_some());
        type_keys(&mut s, id, b"closed.txt\r");
        assert!(s.application(id).is_none());
        let closed = s.files.resolve("C:\\closed.txt").unwrap();
        assert_eq!(s.files.node(closed).unwrap().contents, b"unsaved");
        let id = s.launch_application(AppId::Notepad);
        type_keys(&mut s, id, b"discard");
        s.request_application_close(id);
        type_keys(&mut s, id, b"n");
        assert!(s.application(id).is_none());
    }
    #[test]
    fn new_and_open_share_dirty_prompt_and_failed_open_is_atomic() {
        let mut s = ShellModel::new(Theme::aero());
        let bad = s.files.touch(1, "bad.txt", b"binary\0");
        let good = s.files.touch(1, "good.txt", b"loaded\r\ntext");
        let id = s.launch_application(AppId::Notepad);
        type_keys(&mut s, id, b"keep\x0f");
        assert!(s.application(id).unwrap().confirm);
        type_keys(&mut s, id, b"n");
        type_keys(&mut s, id, b"bad.txt\r");
        assert!(s.application(id).unwrap().dialog.is_some());
        let AppState::Document(d) = &s.application(id).unwrap().state else {
            panic!()
        };
        assert_eq!(d.editor.text, "keep");
        assert!(d.dirty);
        assert!(s.load_into_application(id, bad).is_err());
        assert!(s.load_into_application(id, good).is_ok());
        // Close the still-open failed picker, then create a new clean document.
        escape(&mut s, id);
        type_keys(&mut s, id, b"\x0e");
        let AppState::Document(d) = &s.application(id).unwrap().state else {
            panic!()
        };
        assert_eq!(d.editor.text, "");
        assert!(d.file_id.is_none());
        assert!(!d.dirty);
    }
    #[test]
    fn paint_open_keeps_full_canvas_and_save_as_round_trips() {
        let mut s = ShellModel::new(Theme::aero());
        let mut p = super::super::paint::Paint::new(700, 500);
        p.color = 0xffed1c24;
        p.begin(680, 480);
        p.end();
        let file = s
            .files
            .touch(1, "large.bmp", &super::super::paint::encode_bmp(&p.canvas));
        let id = s.launch_application(AppId::Paint);
        s.load_into_application_encoding(id,file,None).unwrap();
        let AppState::Paint(p) = &s.application(id).unwrap().state else {
            panic!()
        };
        assert_eq!(p.canvas.width(), 700);
        assert_eq!(p.canvas.pixel(680, 480), Some(0xffed1c24));
        s.apply_application_action(id, AppAction::SaveAs);
        type_keys(&mut s, id, b"copy\r");
        let copy = s.files.resolve("C:\\copy.bmp").unwrap();
        let image = crate::bmp::decode_bmp(&s.files.node(copy).unwrap().contents).unwrap();
        assert_eq!((image.width, image.height), (700, 500));
    }
}

#[cfg(test)]
mod notepad_workflow_tests {
    use super::super::{
        document::Encoding,
        notepad::{self, Command},
    };
    use super::*;
    use crate::theme::Theme;
    #[test]
    fn unicode_save_as_cancel_and_commit_preserve_document_and_encoding() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.launch_application(AppId::Notepad);
        shell
            .applications
            .iter_mut()
            .find(|a| a.window_id == id)
            .unwrap()
            .paste("中文 €");
        shell.application_key(id, 19);
        let dialog = shell.application(id).unwrap().dialog.as_ref().unwrap();
        assert_eq!(dialog.encoding, Some(Encoding::Utf8Bom));
        shell.application_key(id, 27);
        shell.flush_application_input(200);
        let AppState::Document(d) = &shell.application(id).unwrap().state else {
            panic!()
        };
        assert_eq!(d.editor.text, "中文 €");
        assert_eq!(d.encoding, Encoding::Ansi);
        assert!(d.dirty);
        shell.application_key(id, 19);
        for b in b"unicode.txt\r" {
            shell.application_key(id, *b);
        }
        let file = shell.files.resolve("C:\\unicode.txt").unwrap();
        assert_eq!(
            shell.files.node(file).unwrap().contents,
            [b"\xef\xbb\xbf".as_slice(), "中文 €".as_bytes()].concat()
        );
        let AppState::Document(d) = &shell.application(id).unwrap().state else {
            panic!()
        };
        assert_eq!(d.encoding, Encoding::Utf8Bom);
        assert!(!d.dirty);
    }
    #[test]
    fn force_ansi_decode_is_not_reinterpreted_and_failed_open_is_atomic() {
        let mut d = super::super::document::Document::new("text.txt");
        d.load_encoding(b"\xc3\xa9\nsecond", Encoding::Ansi)
            .unwrap();
        assert_eq!(d.editor.text, "Ã©\nsecond");
        assert!(!d.crlf);
        let before = d.clone();
        assert!(d.load_encoding(b"\xff", Encoding::Utf8).is_err());
        assert_eq!(d, before);
        assert_eq!(d.encoded(), b"\xc3\xa9\nsecond");
    }
    #[test]
    fn new_preserves_format_and_real_shortcuts_open_the_right_dialog() {
        let mut shell = ShellModel::new(Theme::aero());
        let id = shell.launch_application(AppId::Notepad);
        let app = shell
            .applications
            .iter_mut()
            .find(|a| a.window_id == id)
            .unwrap();
        notepad::execute(app, Command::Wrap);
        app.notepad.font.points = 14;
        shell.application_key(id, 14);
        let app = shell.application(id).unwrap();
        let AppState::Document(d) = &app.state else {
            panic!()
        };
        assert!(d.word_wrap);
        assert_eq!(app.notepad.font.points, 14);
        for b in b"text" {
            shell.application_key(id, *b);
        }
        shell.application_key(id, 0x8e);
        assert!(shell.application(id).unwrap().notepad.search.replace);
        shell.application_key(id, 27);
        shell.flush_application_input(200);
        shell.application_key(id, 8);
        let AppState::Document(d) = &shell.application(id).unwrap().state else {
            panic!()
        };
        assert_eq!(d.editor.text, "tex");
        for b in b"\x1bof" {
            shell.application_key(id, *b);
        }
        assert!(matches!(
            shell.application(id).unwrap().common_dialog,
            Some(super::super::common_dialogs::Dialog::Font(_))
        ));
    }
}

#[cfg(test)]
mod media_workflow_tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn saved_windows_paths_and_relative_wpl_sources_reopen() {
        let mut shell = ShellModel::new(Theme::aero());
        let directory = shell.files.mkdir(1, "Music");
        let mut wave = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
        wave.extend_from_slice(&8000u32.to_le_bytes());
        wave.extend_from_slice(&16000u32.to_le_bytes());
        wave.extend_from_slice(b"\x02\0\x10\0data\x02\0\0\0\0\0");
        let track = shell.files.touch(directory, "A & B.wav", &wave);
        let id = shell.launch_application(AppId::MediaPlayer);
        let absolute = shell.files.path(track).unwrap();
        for (index, source) in [absolute.as_str(), "A & B.wav"].into_iter().enumerate() {
            let bytes = super::super::media_player::playlist::encode("Test", [source].into_iter());
            let playlist = shell.files.touch(directory, &format!("saved{index}.wpl"), &bytes);
            shell.load_media(id, playlist).unwrap();
            let AppState::MediaPlayer(player) = &shell.application(id).unwrap().state else { panic!() };
            assert_eq!(player.current, Some(track));
            assert_eq!(player.playlist_file, Some(playlist));
            assert_eq!(player.playlist.len(), 1);
            assert!(player.stream.is_some());
        }
    }
}

#[cfg(test)]
mod wordpad_workflow_tests {
    use super::super::{
        rich_text::{self, Format},
        wordpad::{self, Command},
    };
    use super::*;
    use crate::theme::Theme;
    fn type_keys(s: &mut ShellModel, id: u32, text: &[u8]) {
        for key in text {
            s.application_key(id, *key);
        }
    }
    #[test]
    fn save_as_docx_and_rtf_reopen_through_shell_associations() {
        let mut s = ShellModel::new(Theme::aero());
        let id = s.launch_application(AppId::WordPad);
        type_keys(&mut s, id, b"Mixed text\x01");
        wordpad::execute(
            s.applications
                .iter_mut()
                .find(|a| a.window_id == id)
                .unwrap(),
            Command::Bold,
        );
        for name in ["mixed.docx", "mixed.rtf"] {
            s.apply_application_action(id, AppAction::SaveAs);
            type_keys(&mut s, id, name.as_bytes());
            type_keys(&mut s, id, b"\r");
            assert!(s.application(id).unwrap().dialog.is_none());
            let file = s.files.resolve(&format!("C:\\{name}")).unwrap();
            let bytes = &s.files.node(file).unwrap().contents;
            assert!(if name.ends_with("docx") {
                bytes.starts_with(b"PK")
            } else {
                bytes.starts_with(b"{\\rtf")
            });
            let reopened = s.open_application_file(file).unwrap();
            let app = s.application(reopened).unwrap();
            assert_eq!(app.id, AppId::WordPad);
            let AppState::Document(d) = &app.state else {
                panic!()
            };
            assert_eq!(d.editor.text, "Mixed text");
            assert!(d.rich.as_ref().unwrap().style(0).bold);
            s.close_window(reopened);
        }
    }
    #[test]
    fn legacy_doc_is_imported_and_ctrl_s_never_replaces_original() {
        let mut s = ShellModel::new(Theme::aero());
        let bytes = include_bytes!("../../tests/fixtures/wordpad/word97.doc");
        let file = s.files.touch(1, "OLD.DOC", bytes);
        let id = s.open_application_file(file).unwrap();
        type_keys(&mut s, id, b"!\x13");
        let app = s.application(id).unwrap();
        let dialog = app.dialog.as_ref().unwrap();
        assert!(dialog.name.ends_with(".rtf"));
        assert!(!dialog.name.eq_ignore_ascii_case("OLD.DOC"));
        assert_eq!(s.files.node(file).unwrap().contents, bytes);
        type_keys(&mut s, id, b"imported.docx\r");
        let copy = s.files.resolve("C:\\imported.docx").unwrap();
        let loaded = rich_text::decode(&s.files.node(copy).unwrap().contents).unwrap();
        assert_eq!(loaded.rich.format, Format::Docx);
        assert!(loaded.text.ends_with('!'));
        assert_eq!(s.files.node(file).unwrap().contents, bytes);
    }
    #[test]
    fn failed_open_and_invalid_save_extension_preserve_existing_document() {
        let mut s = ShellModel::new(Theme::aero());
        let id = s.launch_application(AppId::WordPad);
        type_keys(&mut s, id, b"keep\x13");
        type_keys(&mut s, id, b"bad.doc\r");
        assert!(s.application(id).unwrap().dialog.is_some());
        assert!(s.files.resolve("C:\\bad.doc").is_none());
        let bad = s.files.touch(1, "bad.docx", b"PK\x03\x04broken");
        assert!(s.load_into_application(id, bad).is_err());
        let AppState::Document(d) = &s.application(id).unwrap().state else {
            panic!()
        };
        assert_eq!(d.editor.text, "keep");
        assert!(d.dirty);
    }
}

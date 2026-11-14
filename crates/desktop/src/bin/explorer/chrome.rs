//! Explorer chrome. All interactive rectangles come from the shared model layout.
use super::{arch, png, Shell};
use nt61_desktop::shell::layout::ShellLayout;
use nt61_desktop::{
    compositor::aero,
    graphics::{Rect, Surface},
    shell::*,
};

const TEXT: u32 = 0xFF182534;
const BLUE: u32 = 0xFF1764A0;

fn inset(b: Rect, pad: u32) -> Rect {
    Rect::new(
        b.x + pad as i32,
        b.y + pad as i32,
        b.width.saturating_sub(pad * 2),
        b.height.saturating_sub(pad * 2),
    )
}
fn image(frame: &mut Surface, source: &png::Image, b: Rect) {
    let stride = source.stride();
    for y in 0..b.height {
        for x in 0..b.width {
            let i = (y * source.height / b.height) as usize * stride
                + (x * source.width / b.width) as usize * 4;
            let p = &source.pixels[i..i + 4];
            frame.blend_pixel(
                (b.x + x as i32) as u32,
                (b.y + y as i32) as u32,
                ((p[3] as u32) << 24) | ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32,
            );
        }
    }
}

impl Shell {
    fn item(&self, frame: &mut Surface, b: Rect, id: u32, selected: bool) {
        if selected
            || self.model.controls.hot() == id
            || self.model.controls.pressed() == id
            || self.model.controls.focus() == id
        {
            aero::shadow(frame, b, 2);
            aero::menu_item(frame, b, true);
            if self.model.controls.pressed() == id { aero::blend_rounded_rect(frame, inset(b, 1), 2, 0x304896CC); }
        }
    }
    fn label(&self, frame: &mut Surface, text: &str, b: Rect, color: u32) {
        self.fonts.draw_fit(frame, text, b.x, b.y, b.width, color);
    }
    fn centered(&self, frame: &mut Surface, text: &str, b: Rect, color: u32) {
        let x = b.x + (b.width.saturating_sub(self.fonts.measure(text)) / 2) as i32;
        self.fonts.draw_fit(frame, text, x, b.y, b.width, color);
    }
    fn program_icon(&self, frame: &mut Surface, program: StartProgram, b: Rect) {
        match program {
            StartProgram::Folder(_) => image(frame,&self.folder,b),
            StartProgram::Application(id) => nt61_desktop::applications::icons::draw(frame,id,b),            StartProgram::Notepad => image(frame, &self.document, b),
            StartProgram::Computer => image(frame, &self.computer, b),
            StartProgram::Explorer => image(frame, &self.folder, b),
            StartProgram::CommandPrompt => {
                aero::blend_rounded_rect(frame, b, 2, 0xFF536373);
                frame.fill_rect(b.x + 2, b.y + 3, b.width - 4, b.height - 5, 0xFF0B0C0F);
                frame.fill_rect(b.x + 2, b.y + 2, b.width - 4, 3, 0xFFDAE5F1);
                self.fonts
                    .draw_color(frame, ">_", b.x + 4, b.y + 10, 0xFFF5F5F5);
            }
        }
    }
    pub(super) fn draw_namespace_window(&self, frame: &mut Surface, body: Rect, kind: WindowKind) {
        frame.fill_rect(body.x,body.y,body.width,body.height,0xFFFFFFFF);
        let heading = match kind {
            WindowKind::System => "View basic information about your computer",
            WindowKind::ControlPanel => "Adjust your computer's settings",
            WindowKind::ComputerManagement => "Computer Management (Local)",
            WindowKind::RecycleBin => "Recycle Bin",
            WindowKind::RecycleBinProperties => "Recycle Bin Properties",
            WindowKind::DisplaySettings => "Change the appearance of your display",
            WindowKind::Personalization => "Change the visuals and sounds on your computer",
            _ => "Delete Items",
        };
        self.label(frame,heading,Rect::new(body.x+16,body.y+14,body.width.saturating_sub(32),20),BLUE);
        let links = ShellModel::namespace_links(kind);
        for (index,(title,_)) in links.iter().enumerate() {
            let row = Rect::new(body.x+12,body.y+56+index as i32*36,body.width.saturating_sub(24),32);
            aero::blend_rounded_rect(frame,row,3,0xFFF0F7FC);
            self.label(frame,title,Rect::new(row.x+12,row.y+8,row.width.saturating_sub(24),18),BLUE);
        }
        if !links.is_empty() { return; }
        let lines: alloc::vec::Vec<alloc::string::String> = match kind {
            WindowKind::System => alloc::vec![
                "NT 6.1".into(),
                alloc::format!("System type: {}", if cfg!(target_arch="x86_64") {"x86-64"} else if cfg!(target_arch="aarch64") {"ARM64"} else if cfg!(target_arch="riscv64") {"RISC-V 64"} else {"LoongArch64"}),
                "Version 6.1.7601".into(),
                alloc::format!("Network adapters detected: {}",self.model.network_adapters),
                alloc::format!("Audio device available: {}",self.model.audio_available),
            ],
            WindowKind::DisplaySettings => alloc::vec![
                "Display: Primary monitor".into(),
                alloc::format!("Resolution: {} x {}",frame.width(),frame.height()),
                "Colors: True Color (32 bit)".into(),
                "Orientation: Landscape".into(),
            ],
            WindowKind::Personalization => alloc::vec![
                "Current theme: Windows 7 Aero".into(),
                "Window color: Sky blue glass".into(),
                "Desktop background: Windows".into(),
                "Glass blur, reflections, selection and menu shadows enabled.".into(),
            ],
            WindowKind::RecycleBinProperties => alloc::vec![
                alloc::format!("Recycled items: {}", self.model.files.recycle_bin().len()),
                "Location: Desktop".into(),
                "Empty Recycle Bin requires confirmation.".into(),
                "Use Restore all items in the Recycle Bin to recover items.".into(),
            ],
            WindowKind::ConfirmEmptyRecycleBin => {
                self.label(frame,"Permanently delete all items in the Recycle Bin?",Rect::new(body.x+16,body.y+50,body.width-32,20),TEXT);
                for (offset,title) in [(194,"Yes"),(102,"No")] {
                    let b=Rect::new(body.x+body.width as i32-offset,body.y+body.height as i32-36,80,26);
                    aero::menu_item(frame,b,true);
                    self.centered(frame,title,Rect::new(b.x,b.y+5,b.width,18),TEXT);
                }
                return;
            }
            WindowKind::RecycleBin => {
                let restore=Rect::new(body.x+12,body.y+40,140,26);
                aero::menu_item(frame,restore,true);
                self.label(frame,"Restore all items",Rect::new(restore.x+6,restore.y+5,128,18),BLUE);
                let items=self.model.files.recycle_bin();
                if items.is_empty() { alloc::vec!["The Recycle Bin is empty.".into()] }
                else { items.iter().map(|n|alloc::format!("{}    {}",n.name,n.original_path.as_deref().unwrap_or(""))).collect() }
            }
            _ => alloc::vec![],
        };
        let top = if kind == WindowKind::RecycleBin { 84 } else { 56 };
        for (index,line) in lines.iter().enumerate() {
            if top + index as u32 * 30 + 18 > body.height { break; }
            self.label(frame,line,Rect::new(body.x+18,body.y+top as i32+index as i32*30,body.width.saturating_sub(36),20),TEXT);
        }
    }

    pub(super) fn draw_taskbar(&self, frame: &mut Surface, fb: &arch::DesktopFramebufferInfo) {
        let l = ShellLayout::new(fb.width, fb.height);
        let top = l.taskbar_top();
        aero::taskbar_chrome(
            frame,
            Rect::new(0, top, fb.width, TASKBAR_HEIGHT),
            &self.glass,
        );
        let b = l.pinned();
        if self.model.controls.hot() == CONTROL_PIN_EXPLORER {
            aero::superbar_button(frame, b, false);
        }
        image(frame, &self.folder, Rect::new(b.x + 10, b.y + 3, 32, 32));
        for (i, w) in self.model.taskbar.windows.iter().enumerate() {
            let b = l.window_button(i, self.model.taskbar.windows.len());
            let active = self.model.selected_window == Some(w.id) && !w.minimized;
            aero::superbar_button(frame, b, active);
            if self.model.controls.hot() == CONTROL_TASKBAR_BASE + i as u32 {
                aero::blend_rounded_rect(frame, inset(b, 1), 3, 0x40FFFFFF);
            }
            let program = match w.kind {
                WindowKind::Application(id) => StartProgram::Application(id),
                WindowKind::Computer => StartProgram::Computer,
                WindowKind::Notepad => StartProgram::Notepad,
                WindowKind::CommandPrompt => StartProgram::CommandPrompt,
                _ => StartProgram::Explorer,
            };
            if b.width >= 28 {
                self.program_icon(frame, program, Rect::new(b.x + 4, top + 8, 24, 24));
            }
            self.label(
                frame,
                &w.title,
                Rect::new(b.x + 39, top + 14, b.width.saturating_sub(45), 16),
                0xFFFFFFFF,
            );
        }
        let active = self.model.start_menu.open || self.model.controls.pressed() == CONTROL_START;
        let orb = if active {
            &self.start_button_pressed
        } else if self.model.controls.hot() == CONTROL_START {
            &self.start_button_hover
        } else {
            &self.start_button_normal
        };
        image(frame, orb, l.start_orb());
        self.draw_notification_area(frame, fb);
    }
    pub(super) fn draw_notification_area(
        &self,
        frame: &mut Surface,
        fb: &arch::DesktopFramebufferInfo,
    ) {
        let l = ShellLayout::new(fb.width, fb.height);
        for (index, id, kind) in [
            (0, CONTROL_OVERFLOW, Flyout::Overflow),
            (1, CONTROL_TRAY_ACTION_CENTER, Flyout::ActionCenter),
            (2, CONTROL_TRAY_NETWORK, Flyout::Network),
            (3, CONTROL_TRAY_VOLUME, Flyout::Volume),
        ] {
            let b = l.tray(index);
            if self.model.controls.hot() == id || self.model.flyout == Some(kind) {
                aero::blend_rounded_rect(frame, inset(b, 1), 3, 0x50FFFFFF);
            }
            self.tray_glyph(frame, kind, Rect::new(b.x + 4, b.y + 11, 16, 16));
        }
        let c = l.clock();
        if self.model.controls.hot() == CONTROL_CLOCK || self.model.flyout == Some(Flyout::Clock) {
            aero::blend_rect(frame, c, 0x30FFFFFF);
        }
        self.centered(
            frame,
            &self.model.taskbar.clock,
            Rect::new(c.x, c.y + 5, c.width, 14),
            0xFFFFFFFF,
        );
        let date = self
            .model
            .taskbar
            .date
            .map(|d| alloc::format!("{}/{}/{:02}", d.month, d.day, d.year % 100));
        self.centered(
            frame,
            date.as_deref().unwrap_or("No RTC"),
            Rect::new(c.x, c.y + 22, c.width, 14),
            0xFFFFFFFF,
        );
        let p = l.peek();
        aero::blend_rect(
            frame,
            p,
            if self.model.controls.hot() == CONTROL_SHOW_DESKTOP {
                0x90FFFFFF
            } else {
                0x30FFFFFF
            },
        );
        frame.fill_rect(p.x, p.y, 1, p.height, 0xC0FFFFFF);
        frame.fill_rect(p.x + 1, p.y + 1, 1, p.height - 2, 0x50000000);
    }
    fn tray_glyph(&self, frame: &mut Surface, kind: Flyout, b: Rect) {
        let white = 0xFFF6FAFF;
        let dark = 0xFF304459;
        match kind {
            Flyout::Overflow => {
                for n in 0..4 {
                    frame.fill_rect(b.x + 4 + n, b.y + 8 - n, 2, 2, white);
                    frame.fill_rect(b.x + 8 + n, b.y + 5 + n, 2, 2, white);
                }
            }
            Flyout::ActionCenter => {
                frame.fill_rect(b.x + 3, b.y + 1, 2, 15, dark);
                frame.fill_rect(b.x + 4, b.y + 1, 1, 15, white);
                frame.fill_rect(b.x + 5, b.y + 2, 9, 7, white);
                frame.fill_rect(b.x + 10, b.y + 8, 4, 3, white);
            }
            Flyout::Network => {
                frame.fill_rect(b.x + 1, b.y + 2, 14, 10, dark);
                frame.fill_rect(b.x + 2, b.y + 3, 12, 8, white);
                frame.fill_rect(b.x + 3, b.y + 4, 10, 6, 0xFF498DC1);
                frame.fill_rect(b.x + 7, b.y + 12, 2, 2, white);
                frame.fill_rect(b.x + 4, b.y + 14, 8, 1, white);
            }
            Flyout::Volume => {
                frame.fill_rect(b.x + 1, b.y + 6, 4, 6, white);
                for n in 0..4 {
                    frame.fill_rect(b.x + 4 + n, b.y + 6 - n, 1, (6 + n * 2) as u32, white);
                }
                if self.model.muted {
                    self.fonts.draw_color(frame, "x", b.x + 9, b.y + 3, white);
                } else {
                    frame.fill_rect(b.x + 10, b.y + 5, 1, 7, white);
                    frame.fill_rect(b.x + 13, b.y + 3, 1, 11, white);
                }
            }
            _ => {}
        }
    }
    pub(super) fn draw_start_menu(&self, frame: &mut Surface, fb: &arch::DesktopFramebufferInfo) {
        let l = ShellLayout::new(fb.width, fb.height);
        let b = l.start();
        let left = l.left_width();
        aero::shadow(frame, b, 8);
        aero::blend_rounded_rect(frame, b, 8, 0xFF1B3148);
        aero::blend_rounded_rect(frame, inset(b, 1), 7, 0xA0759DBA);
        frame.copy_region(&self.glass, inset(b,2), b.x+2, b.y+2);
        aero::blend_rounded_rect(frame, inset(b, 2), 6, 0xA023405C);
        // Soft reflection across the upper glass and a bright inner rim.
        aero::blend_rect(frame, Rect::new(b.x+3,b.y+3,b.width-6,1),0xA0FFFFFF);
        // White application column, blue glass places column and a separate footer.
        aero::blend_rounded_rect(
            frame,
            Rect::new(b.x + 5, b.y + 5, left - 5, b.height - 10),
            4,
            0xFFFFFFFF,
        );
        aero::blend_rect(
            frame,
            Rect::new(
                b.x + left as i32 + 1,
                b.y + 4,
                b.width - left - 6,
                b.height / 2,
            ),
            0x184FA7E4,
        );
        let results = self.model.start_menu.visible_programs();
        let offset=self.model.start_menu.program_offset();
        for (i, program) in results.iter().enumerate().skip(offset).take(7) {
            let row = l.program(i-offset);
            self.item(
                frame,
                row,
                CONTROL_MENU_BASE + i as u32,
                self.model.start_menu.highlighted == i,
            );
            let size = 32.min(row.height.saturating_sub(4));
            self.program_icon(frame, *program, Rect::new(row.x + 4, row.y + 2, size, size));
            self.label(
                frame,
                program.title(),
                Rect::new(row.x + 42, row.y + 12, row.width.saturating_sub(48), 16),
                TEXT,
            );
        }
        if results.is_empty() {
            self.label(
                frame,
                if self.model.start_menu.query.is_empty() {"(Empty)"}else{"No items match your search."},
                Rect::new(b.x + 18, b.y + 24, left - 30, 18),
                TEXT,
            );
        }
        if results.len()>7 {
            for (next,label,id) in [(false,"Previous",CONTROL_PROGRAM_PREV),(true,"More programs",CONTROL_PROGRAM_NEXT)] {
                let row=l.program_page(next);self.item(frame,row,id,false);
                self.label(frame,label,Rect::new(row.x+5,row.y+6,row.width.saturating_sub(10),18),TEXT);
            }
        }
        let a = l.all_programs();
        frame.fill_rect(a.x, a.y - 3, a.width, 1, 0xFFD6DCE2);
        self.item(frame, a, CONTROL_ALL_PROGRAMS, false);
        let arrow = Rect::new(a.x + 9, a.y + 9, 7, 10);
        if self.model.start_menu.all_programs_view {
            aero::arrow_left(frame, arrow, TEXT);
        } else {
            aero::arrow_right(frame, arrow, TEXT);
        }
        self.label(
            frame,
            if self.model.start_menu.all_programs_view {
                "Back"
            } else {
                "All Programs"
            },
            Rect::new(a.x + 27, a.y + 8, a.width - 32, 16),
            TEXT,
        );
        let search = l.search();
        frame.fill_rect(
            search.x - 1,
            search.y - 1,
            search.width + 2,
            search.height + 2,
            if self.model.controls.focus() == CONTROL_SEARCH || self.model.controls.hot() == CONTROL_SEARCH { 0xFF3C7FB1 } else { 0xFF829DB3 },
        );
        if self.model.controls.focus() == CONTROL_SEARCH {
            aero::shadow(frame, Rect::new(search.x-1,search.y-1,search.width+2,search.height+2), 2);
        }
        frame.fill_rect(search.x, search.y, search.width, search.height, 0xFFFFFFFF);
        let text = if self.model.start_menu.query.is_empty() {
            "Search programs and files"
        } else {
            &self.model.start_menu.query
        };
        self.label(
            frame,
            text,
            Rect::new(
                search.x + 5,
                search.y + 5,
                search.width.saturating_sub(26),
                16,
            ),
            if self.model.start_menu.query.is_empty() {
                0xFF707070
            } else {
                TEXT
            },
        );
        if self.model.controls.focus() == CONTROL_SEARCH {
            let advance = self.fonts.measure(&self.model.start_menu.query);
            let caret_x = search.x + 5 + advance.min(search.width.saturating_sub(30)) as i32;
            frame.fill_rect(caret_x, search.y + 4, 1, 16, TEXT);
        }
        // Search magnifier stays inside its own box.
        aero::blend_rounded_rect(
            frame,
            Rect::new(search.x + search.width as i32 - 17, search.y + 6, 8, 8),
            4,
            0xFF728897,
        );
        aero::blend_rounded_rect(
            frame,
            Rect::new(search.x + search.width as i32 - 16, search.y + 7, 6, 6),
            3,
            0xFFFFFFFF,
        );
        frame.fill_rect(
            search.x + search.width as i32 - 10,
            search.y + 13,
            4,
            2,
            0xFF728897,
        );
        let avatar_x = b.x + left as i32 + ((b.width - left) / 2) as i32 - 26;
        let avatar = Rect::new(avatar_x, b.y + 10, 52, 52);
        aero::shadow(frame, avatar, 3);
        aero::blend_rounded_rect(frame, avatar, 5, 0xFFAABAC5);
        aero::blend_rounded_rect(frame, inset(avatar, 1), 4, 0xFFF0F6FC);
        image(frame, &self.avatar, inset(avatar, 4));
        aero::blend_rect(
            frame,
            Rect::new(avatar.x + 4, avatar.y + 4, 44, 15),
            0x30FFFFFF,
        );
        self.centered(
            frame,
            "Administrator",
            Rect::new(b.x + left as i32, b.y + 65, b.width - left, 16),
            0xFFFFFFFF,
        );
        for (i, title) in [
            "Programs",
            "Documents",
            "Pictures",
            "Music",
            "Computer",
            "Control Panel",
            "Devices and Printers",
        ]
        .iter()
        .enumerate()
        {
            let row = l.link(i);
            if self.model.controls.hot() == CONTROL_START_LINK_BASE + i as u32 {
                aero::blend_rounded_rect(frame, row, 3, 0x607DAACB);
                aero::blend_rounded_rect(frame, inset(row, 1), 2, 0x406EACDF);
            }
            self.label(
                frame,
                title,
                Rect::new(row.x + 8, row.y + 12, row.width.saturating_sub(27), 16),
                0xFFFFFFFF,
            );
            if i < 4 {
                aero::arrow_right(
                    frame,
                    Rect::new(row.x + row.width as i32 - 13, row.y + 14, 6, 9),
                    0xFFFFFFFF,
                );
            }
            if i == 3 {
                frame.fill_rect(
                    row.x + 6,
                    row.y + row.height as i32 - 1,
                    row.width - 12,
                    1,
                    0x607D98B0,
                );
            }
        }
        for (button, id, title) in [
            (l.shutdown(), CONTROL_SHUTDOWN, "Shut down"),
            (l.power_arrow(), CONTROL_POWER_ARROW, ""),
        ] {
            aero::blend_rounded_rect(frame, button, 3, 0xFF162F43);
            aero::blend_rounded_rect(frame, inset(button, 1), 2, 0xFF52728B);
            if self.model.controls.hot() == id {
                aero::blend_rounded_rect(frame, inset(button, 1), 2, 0x408FCEFA);
            }
            self.centered(
                frame,
                title,
                Rect::new(button.x, button.y + 5, button.width, 16),
                0xFFFFFFFF,
            );
        }
        let arrow = l.power_arrow();
        aero::arrow_right(frame, Rect::new(arrow.x + 8, arrow.y + 8, 6, 9), 0xFFFFFFFF);
        if self.model.start_menu.submenu.is_some() {
            if let Some(bounds) = self.model.context_submenu_bounds(fb.width, fb.height) {
                aero::menu_surface(frame, bounds);
                let group = self.model.start_menu.submenu.unwrap().group;
                for (i, title) in ShellModel::submenu_items(group).iter().enumerate() {
                    let row = Rect::new(
                        bounds.x + 4,
                        bounds.y + 4 + i as i32 * 28,
                        bounds.width - 8,
                        28,
                    );
                    self.item(frame, row, CONTROL_SUBMENU_BASE + i as u32, false);
                    self.label(
                        frame,
                        title,
                        Rect::new(row.x + 8, row.y + 7, row.width - 16, 16),
                        TEXT,
                    );
                }
            }
        }
    }
    pub(super) fn draw_flyout(&self, frame: &mut Surface, fb: &arch::DesktopFramebufferInfo) {
        let Some(kind) = self.model.flyout else {
            return;
        };
        let l = ShellLayout::new(fb.width, fb.height);
        let b = l.flyout(kind);
        aero::shadow(frame, b, 6);
        aero::blend_rounded_rect(frame, b, 5, 0xFF7C9CB2);
        aero::blend_rounded_rect(frame, inset(b, 1), 4, 0xFFEAF3FA);
        frame.fill_rect(b.x + 7, b.y + 7, b.width - 14, b.height - 14, 0xFFFFFFFF);
        let title = match kind {
            Flyout::Overflow => "Notification Area",
            Flyout::Volume => {
                if self.model.audio_available {
                    "Speakers"
                } else {
                    "No audio device"
                }
            }
            Flyout::Network => "Network",
            Flyout::ActionCenter => "Action Center",
            Flyout::Clock => "Date and Time",
            Flyout::Power => "Power options",
        };
        let footer = l.flyout_footer(kind);
        if kind != Flyout::Power {
            self.centered(
                frame,
                title,
                Rect::new(b.x + 12, b.y + 16, b.width - 24, 18),
                TEXT,
            );
            frame.fill_rect(b.x + 8, footer.y - 5, b.width - 16, 1, 0xFFD7E1E9);
            self.item(frame, footer, CONTROL_FLYOUT_LINK, false);
        }
        match kind {
            Flyout::Volume => {
                let track = l.volume_track();
                frame.fill_rect(track.x + 8, track.y, 4, track.height, 0xFF9CAEB9);
                let filled = track.height * self.model.volume as u32 / 100;
                frame.fill_rect(
                    track.x + 9,
                    track.y + (track.height - filled) as i32,
                    2,
                    filled,
                    0xFF56AC41,
                );
                let thumb = Rect::new(
                    track.x + 1,
                    track.y + (track.height - filled) as i32 - 4,
                    18,
                    9,
                );
                aero::blend_rounded_rect(frame, thumb, 2, 0xFF63859F);
                aero::blend_rounded_rect(frame, inset(thumb, 1), 1, 0xFFE0EFFA);
                self.centered(
                    frame,
                    &alloc::format!("{}%", self.model.volume),
                    Rect::new(b.x + 10, b.y + 34, b.width - 20, 14),
                    TEXT,
                );
                let mute = Rect::new(b.x + 54, b.y + b.height as i32 - 70, 52, 30);
                self.item(frame, mute, CONTROL_MUTE, self.model.muted);
                // The glyph's white speaker sits on a contrasting blue tile.
                frame.fill_rect(mute.x + 12, mute.y + 3, 28, 24, 0xFF507A9C);
                self.tray_glyph(
                    frame,
                    Flyout::Volume,
                    Rect::new(mute.x + 18, mute.y + 7, 16, 16),
                );
                self.centered(
                    frame,
                    "Mixer",
                    Rect::new(footer.x, footer.y + 6, footer.width, 16),
                    BLUE,
                );
            }
            Flyout::Network => {
                image(frame, &self.network, Rect::new(b.x + 24, b.y + 58, 40, 40));
                self.label(
                    frame,
                    if self.model.network_adapters == 0 {
                        "No network adapters detected"
                    } else {
                        "Network adapter available"
                    },
                    Rect::new(b.x + 78, b.y + 60, b.width - 92, 18),
                    TEXT,
                );
                self.label(
                    frame,
                    "Connection status is not reported.",
                    Rect::new(b.x + 18, b.y + 120, b.width - 36, 18),
                    0xFF677888,
                );
                self.centered(
                    frame,
                    "Open Network and Sharing Center",
                    Rect::new(footer.x, footer.y + 6, footer.width, 16),
                    BLUE,
                );
            }
            Flyout::ActionCenter => {
                if self.model.taskbar.notifications.is_empty() {
                    self.label(
                        frame,
                        "No new messages",
                        Rect::new(b.x + 24, b.y + 65, b.width - 48, 18),
                        TEXT,
                    );
                } else {
                    for (i, n) in self.model.taskbar.notifications.iter().take(3).enumerate() {
                        self.label(
                            frame,
                            &n.text,
                            Rect::new(b.x + 24, b.y + 56 + i as i32 * 28, b.width - 48, 18),
                            TEXT,
                        );
                    }
                }
                self.centered(
                    frame,
                    "Open Action Center",
                    Rect::new(footer.x, footer.y + 6, footer.width, 16),
                    BLUE,
                );
            }
            Flyout::Overflow => {
                self.centered(
                    frame,
                    if self.model.taskbar.notifications.is_empty() {
                        "No hidden notification icons"
                    } else {
                        "Notifications are shown in Action Center"
                    },
                    Rect::new(b.x + 12, b.y + 64, b.width - 24, 18),
                    0xFF617484,
                );
                self.centered(
                    frame,
                    "Customize...",
                    Rect::new(footer.x, footer.y + 6, footer.width, 16),
                    BLUE,
                );
            }
            Flyout::Clock => {
                self.draw_calendar(frame, b);
                self.centered(
                    frame,
                    "Change date and time settings...",
                    Rect::new(footer.x, footer.y + 6, footer.width, 16),
                    BLUE,
                );
            }
            Flyout::Power => {
                for (i, title) in ["Lock", "Log off", "Restart", "Shut down"]
                    .iter()
                    .enumerate()
                {
                    let row = Rect::new(b.x + 4, b.y + 4 + i as i32 * 34, b.width - 8, 34);
                    self.item(frame, row, CONTROL_POWER_BASE + i as u32, false);
                    self.label(
                        frame,
                        title,
                        Rect::new(row.x + 12, row.y + 10, row.width - 24, 16),
                        TEXT,
                    );
                }
            }
        }
    }
    pub(super) fn draw_shell_settings(&self, frame: &mut Surface, b: Rect, kind: Flyout) {
        frame.fill_rect(b.x, b.y, b.width, b.height, 0xFFFFFFFF);
        let (heading, lines) = match kind {
            Flyout::Volume => (
                "Master volume",
                alloc::vec![
                    alloc::format!(
                        "Playback device: {}",
                        if self.model.audio_available {
                            "available"
                        } else {
                            "not detected"
                        }
                    ),
                    alloc::format!("Volume: {}%", self.model.volume),
                    alloc::format!("Mute: {}", if self.model.muted { "on" } else { "off" }),
                    alloc::string::String::from("Use the taskbar speaker slider to adjust volume."),
                    alloc::string::String::from("Per-application audio sessions are unavailable."),
                ],
            ),
            Flyout::Network => (
                "View your network information",
                alloc::vec![
                    alloc::format!("Network adapters detected: {}", self.model.network_adapters),
                    alloc::string::String::from(
                        "Link state and IP configuration are not reported."
                    ),
                    alloc::string::String::from(
                        "No connection status is assumed from device presence."
                    ),
                ],
            ),
            Flyout::ActionCenter => {
                let mut lines: alloc::vec::Vec<alloc::string::String> = self
                    .model
                    .taskbar
                    .notifications
                    .iter()
                    .map(|n| n.text.clone())
                    .collect();
                if lines.is_empty() {
                    lines.push(alloc::string::String::from("No new messages."));
                }
                ("Review recent messages", lines)
            }
            Flyout::Clock => (
                "System date and time",
                alloc::vec![
                    self.model
                        .taskbar
                        .date
                        .map(|date| alloc::format!(
                            "Date: {:04}-{:02}-{:02}",
                            date.year,
                            date.month,
                            date.day
                        ))
                        .unwrap_or_else(|| alloc::string::String::from(
                            "No platform real-time clock is available."
                        )),
                    alloc::format!("Time: {}", self.model.taskbar.clock),
                    alloc::string::String::from(
                        "The calendar arrows browse months without changing time."
                    ),
                    alloc::string::String::from("Changing the system clock is not available yet."),
                ],
            ),
            Flyout::Overflow => (
                "Icons in the notification area",
                alloc::vec![
                    alloc::string::String::from("Action Center       Always shown"),
                    alloc::string::String::from("Network               Always shown"),
                    alloc::string::String::from("Volume                Always shown"),
                    alloc::string::String::from(
                        "No application has registered a hidden tray icon."
                    ),
                ],
            ),
            Flyout::Power => return,
        };
        self.label(
            frame,
            heading,
            Rect::new(b.x + 20, b.y + 20, b.width.saturating_sub(40), 20),
            BLUE,
        );
        for (i, line) in lines.iter().take(7).enumerate() {
            self.label(
                frame,
                line,
                Rect::new(
                    b.x + 20,
                    b.y + 62 + i as i32 * 28,
                    b.width.saturating_sub(40),
                    18,
                ),
                TEXT,
            );
        }
    }
    fn draw_calendar(&self, frame: &mut Surface, b: Rect) {
        let Some(month) = self.model.calendar_month else {
            self.centered(
                frame,
                "The system clock is unavailable.",
                Rect::new(b.x + 12, b.y + 100, b.width - 24, 18),
                TEXT,
            );
            return;
        };
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        self.centered(
            frame,
            &alloc::format!("{} {}", MONTHS[month.month as usize - 1], month.year),
            Rect::new(b.x + 40, b.y + 48, b.width - 80, 18),
            TEXT,
        );
        for (id, x, label) in [
            (CONTROL_CALENDAR_PREV, b.x + 12, "<"),
            (CONTROL_CALENDAR_NEXT, b.x + b.width as i32 - 38, ">"),
        ] {
            self.item(frame, Rect::new(x, b.y + 42, 26, 26), id, false);
            self.centered(frame, label, Rect::new(x, b.y + 48, 26, 18), BLUE);
        }
        let cell = (b.width - 30) / 7;
        let left = b.x + 15;
        for (i, label) in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
            .iter()
            .enumerate()
        {
            self.centered(
                frame,
                label,
                Rect::new(left + i as i32 * cell as i32, b.y + 82, cell, 18),
                0xFF617484,
            );
        }
        for day in 1..=month.days_in_month() {
            let n = month.weekday() + day as usize - 1;
            let row = Rect::new(
                left + (n % 7) as i32 * cell as i32,
                b.y + 108 + (n / 7) as i32 * 24,
                cell,
                23,
            );
            if self.model.taskbar.date == Some(CalendarDate { day, ..month }) {
                aero::blend_rounded_rect(frame, inset(row, 2), 2, 0xFFCCE6FB);
            }
            self.centered(
                frame,
                &alloc::format!("{day}"),
                Rect::new(row.x, row.y + 4, row.width, 16),
                TEXT,
            );
        }
    }
}

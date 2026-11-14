//! Boot Manager Menu UI
//
//! Windows 7 style boot manager menu structures

use crate::bcd::{BcdStore, BootEntry};

/// Boot menu
pub struct BootMenu<'a> {
    pub(crate) bcd: &'a BcdStore,
    pub(crate) selected_index: usize,
    countdown: u32,
    automatic: bool,
}

impl<'a> BootMenu<'a> {
    pub fn new(bcd: &'a BcdStore) -> Self {
        Self {
            bcd,
            selected_index: bcd.default.and_then(|guid| bcd.entries[..bcd.entry_count].iter()
                .position(|entry| entry.as_ref().map(|e| e.guid) == Some(guid))).unwrap_or(0),
            countdown: bcd.timeout,
            automatic: true,
        }
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }
    pub fn countdown(&self) -> u32 {
        self.countdown
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max = self.bcd.entry_count.saturating_sub(1);
        if self.selected_index < max {
            self.selected_index += 1;
        }
    }

    /// Stop automatic boot until the user explicitly selects an entry.
    pub fn cancel_auto(&mut self) {
        self.countdown = 0;
        self.automatic = false;
    }

    /// True while the auto-boot countdown is still running.
    pub fn is_counting(&self) -> bool {
        self.automatic
    }

    pub fn select(&self) -> Option<&BootEntry> {
        self.bcd.get_entry(self.selected_index)
    }

    pub fn tick(&mut self) -> bool {
        if !self.automatic { return false; }
        if self.countdown > 0 {
            self.countdown -= 1;
            self.countdown == 0
        } else {
            true
        }
    }
}

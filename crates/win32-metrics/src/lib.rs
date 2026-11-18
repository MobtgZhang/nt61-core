//! Windows 7 SP1 single-monitor system metrics at 96 DPI, Basic theme.
//! Fixed values come from the original 7601.17514 guest capture in
//! docs/desktop/baselines/win7-sp1-window-placement/metrics.json.
//! Display geometry is supplied by the caller. Other DPI/theme configurations,
//! configurable input preferences and unmeasured metrics are not implemented.
#![no_std]

/// Original SP1 Basic theme default COLORREF values, not ARGB pixels. Runtime
/// theme changes and per-user overrides must supply their own mutable palette.
pub const BASIC_96_COLORS: [u32; 31] = [
    0x00c8c8c8, 0x00000000, 0x00d1b499, 0x00dbcdbf, 0x00f0f0f0, 0x00ffffff, 0x00646464, 0x00000000,
    0x00000000, 0x00000000, 0x00b4b4b4, 0x00fcf7f4, 0x00ababab, 0x00ff9933, 0x00ffffff, 0x00f0f0f0,
    0x00a0a0a0, 0x006d6d6d, 0x00000000, 0x00544e43, 0x00ffffff, 0x00696969, 0x00e3e3e3, 0x00000000,
    0x00e1ffff, 0x00000000, 0x00cc6600, 0x00ead1b9, 0x00f2e4d7, 0x00ff9933, 0x00f0f0f0,
];

pub mod sm {
    pub const SM_CXSCREEN: i32 = 0;
    pub const SM_CYSCREEN: i32 = 1;
    pub const SM_CXVSCROLL: i32 = 2;
    pub const SM_CYHSCROLL: i32 = 3;
    pub const SM_CYCAPTION: i32 = 4;
    pub const SM_CXBORDER: i32 = 5;
    pub const SM_CYBORDER: i32 = 6;
    pub const SM_CXDLGFRAME: i32 = 7;
    pub const SM_CYDLGFRAME: i32 = 8;
    pub const SM_CYVTHUMB: i32 = 9;
    pub const SM_CXHTHUMB: i32 = 10;
    pub const SM_CXICON: i32 = 11;
    pub const SM_CYICON: i32 = 12;
    pub const SM_CXCURSOR: i32 = 13;
    pub const SM_CYCURSOR: i32 = 14;
    pub const SM_CYMENU: i32 = 15;
    pub const SM_CXFULLSCREEN: i32 = 16;
    pub const SM_CYFULLSCREEN: i32 = 17;
    pub const SM_CYKANJIWINDOW: i32 = 18;
    pub const SM_MOUSEPRESENT: i32 = 19;
    pub const SM_CYVSCROLL: i32 = 20;
    pub const SM_CXHSCROLL: i32 = 21;
    pub const SM_DEBUG: i32 = 22;
    pub const SM_SWAPBUTTON: i32 = 23;
    pub const SM_RESERVED1: i32 = 24;
    pub const SM_RESERVED2: i32 = 25;
    pub const SM_RESERVED3: i32 = 26;
    pub const SM_RESERVED4: i32 = 27;
    pub const SM_CXMIN: i32 = 28;
    pub const SM_CYMIN: i32 = 29;
    pub const SM_CXSIZE: i32 = 30;
    pub const SM_CYSIZE: i32 = 31;
    pub const SM_CXFRAME: i32 = 32;
    pub const SM_CYFRAME: i32 = 33;
    pub const SM_CXMINTRACK: i32 = 34;
    pub const SM_CYMINTRACK: i32 = 35;
    pub const SM_CXDOUBLECLK: i32 = 36;
    pub const SM_CYDOUBLECLK: i32 = 37;
    pub const SM_CXICONSPACING: i32 = 38;
    pub const SM_CYICONSPACING: i32 = 39;
    pub const SM_MENUDROPALIGNMENT: i32 = 40;
    pub const SM_PENWINDOWS: i32 = 41;
    pub const SM_DBCSENABLED: i32 = 42;
    pub const SM_CMOUSEBUTTONS: i32 = 43;
    pub const SM_CXFIXEDFRAME: i32 = 7;
    pub const SM_CYFIXEDFRAME: i32 = 8;
    pub const SM_CXSIZEFRAME: i32 = 32;
    pub const SM_CYSIZEFRAME: i32 = 33;
    pub const SM_SECURE: i32 = 44;
    pub const SM_CXEDGE: i32 = 45;
    pub const SM_CYEDGE: i32 = 46;
    pub const SM_CXMINSPACING: i32 = 47;
    pub const SM_CYMINSPACING: i32 = 48;
    pub const SM_CXSMICON: i32 = 49;
    pub const SM_CYSMICON: i32 = 50;
    pub const SM_CYSMCAPTION: i32 = 51;
    pub const SM_CXSMSIZE: i32 = 52;
    pub const SM_CYSMSIZE: i32 = 53;
    pub const SM_CXMENUSIZE: i32 = 54;
    pub const SM_CYMENUSIZE: i32 = 55;
    pub const SM_ARRANGE: i32 = 56;
    pub const SM_CXMINIMIZED: i32 = 57;
    pub const SM_CYMINIMIZED: i32 = 58;
    pub const SM_CXMAXTRACK: i32 = 59;
    pub const SM_CYMAXTRACK: i32 = 60;
    pub const SM_CXMAXIMIZED: i32 = 61;
    pub const SM_CYMAXIMIZED: i32 = 62;
    pub const SM_NETWORK: i32 = 63;
    pub const SM_CLEANBOOT: i32 = 67;
    pub const SM_CXDRAG: i32 = 68;
    pub const SM_CYDRAG: i32 = 69;
    pub const SM_SHOWSOUNDS: i32 = 70;
    pub const SM_CXMENUCHECK: i32 = 71;
    pub const SM_CYMENUCHECK: i32 = 72;
    pub const SM_SLOWMACHINE: i32 = 73;
    pub const SM_MIDEASTENABLED: i32 = 74;
    pub const SM_MOUSEWHEELPRESENT: i32 = 75;
    pub const SM_XVIRTUALSCREEN: i32 = 76;
    pub const SM_YVIRTUALSCREEN: i32 = 77;
    pub const SM_CXVIRTUALSCREEN: i32 = 78;
    pub const SM_CYVIRTUALSCREEN: i32 = 79;
    pub const SM_CMONITORS: i32 = 80;
    pub const SM_SAMEDISPLAYFORMAT: i32 = 81;
    pub const SM_IMMENABLED: i32 = 82;
    pub const SM_CXFOCUSBORDER: i32 = 83;
    pub const SM_CYFOCUSBORDER: i32 = 84;
    pub const SM_TABLETPC: i32 = 86;
    pub const SM_MEDIACENTER: i32 = 87;
    pub const SM_STARTER: i32 = 88;
    pub const SM_SERVERR2: i32 = 89;
    pub const SM_MOUSEHORIZONTALWHEELPRESENT: i32 = 91;
    pub const SM_CXPADDEDBORDER: i32 = 92;
    pub const SM_DIGITIZER: i32 = 94;
    pub const SM_MAXIMUMTOUCHES: i32 = 95;
    pub const SM_CMETRICS: i32 = 97;
}

/// Active primary display and usable work-area dimensions, in pixels.
/// The caller owns shell reservations; this module does not guess taskbar size.
#[derive(Clone, Copy, Debug)]
pub struct Display {
    pub width: i32,
    pub height: i32,
    pub work_width: i32,
    pub work_height: i32,
    /// Original cold 800x600 capture: 20. After a display mode change: 12.
    /// The runtime caller must track this session transition explicitly.
    pub max_track_padding: i32,
}

impl Display {
    pub fn basic_96_metric(self, index: i32) -> i32 {
        use sm::*;
        let width = self.width.max(0);
        let height = self.height.max(0);
        let work_width = self.work_width.clamp(0, width);
        let work_height = self.work_height.clamp(0, height);
        match index {
            SM_CXSCREEN | SM_CXVIRTUALSCREEN => width,
            SM_CYSCREEN | SM_CYVIRTUALSCREEN => height,
            SM_CXVSCROLL | SM_CYHSCROLL | SM_CYVTHUMB | SM_CXHTHUMB | SM_CYVSCROLL
            | SM_CXHSCROLL => 17,
            SM_CYCAPTION => 22,
            SM_CXBORDER | SM_CYBORDER | SM_CXFOCUSBORDER | SM_CYFOCUSBORDER => 1,
            SM_CXDLGFRAME | SM_CYDLGFRAME => 3,
            SM_CXICON | SM_CYICON | SM_CXCURSOR | SM_CYCURSOR => 32,
            SM_CYMENU | SM_CYSMCAPTION => 20,
            SM_CXFULLSCREEN => work_width,
            SM_CYFULLSCREEN => work_height.saturating_sub(22).max(0),
            SM_MOUSEPRESENT => 1,
            SM_CXMIN | SM_CXMINTRACK => 132,
            SM_CYMIN | SM_CYMINTRACK => 38,
            SM_CXSIZE => 35,
            SM_CYSIZE => 21,
            SM_CXFRAME | SM_CYFRAME => 8,
            SM_CXDOUBLECLK | SM_CYDOUBLECLK | SM_CXDRAG | SM_CYDRAG => 4,
            SM_CXICONSPACING | SM_CYICONSPACING => 75,
            SM_CXSMICON | SM_CYSMICON => 16,
            SM_CXSMSIZE | SM_CYSMSIZE | SM_CXMENUSIZE | SM_CYMENUSIZE => 19,
            SM_CXMAXTRACK => width.saturating_add(self.max_track_padding.max(0)),
            SM_CYMAXTRACK => height.saturating_add(self.max_track_padding.max(0)),
            SM_CXMAXIMIZED => work_width.saturating_add(16),
            SM_CYMAXIMIZED => work_height.saturating_add(16),
            SM_CMONITORS | SM_SAMEDISPLAYFORMAT => i32::from(width > 0 && height > 0),
            _ => 0,
        }
    }
}

/// Observed Basic/96-DPI display transition state. The first valid geometry is
/// initialization, including when it differs from a shell's layout placeholder.
/// A return to an earlier mode does not restore cold-session tracking limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionMetrics {
    geometry: Option<(u32, u32)>,
    mode_changed: bool,
}

impl SessionMetrics {
    pub fn observe_display(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if let Some(previous) = self.geometry {
            if previous != (width, height) {
                self.mode_changed = true;
            }
        }
        self.geometry = Some((width, height));
    }

    pub fn max_track_padding(self) -> i32 {
        if self.mode_changed {
            12
        } else {
            20
        }
    }
}

// The kernel's current display backend has one interactive display. Record the
// successful hardware transition there so even the first USER query after a
// mode change sees the new limit. Other sessions/DPI/themes need separate state.
static DISPLAY_MODE_CHANGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

pub fn record_display_mode_change() {
    DISPLAY_MODE_CHANGED.store(true, core::sync::atomic::Ordering::Release);
}

pub fn current_max_track_padding() -> i32 {
    if DISPLAY_MODE_CHANGED.load(core::sync::atomic::Ordering::Acquire) {
        12
    } else {
        20
    }
}

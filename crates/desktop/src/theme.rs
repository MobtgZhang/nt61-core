use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Aero,
    Basic,
    Classic,
    HighContrast,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub kind: ThemeKind,
    pub name: String,
    pub composition: bool,
    pub glass: bool,
    pub base: String,
    pub frame: String,
    pub text: String,
    pub taskbar: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
    pub window_background: u32,
    pub window_frame_active: u32,
    pub window_frame_inactive: u32,
    pub text_primary: u32,
    pub button_normal: u32,
    pub button_hover: u32,
    pub button_pressed: u32,
    pub button_disabled: u32,
    pub highlight: u32,
    pub titlebar_height: u32,
    pub window_corner_radius: u32,
    pub taskbar_height: u32,
}

impl ThemeTokens {
    pub fn windows7_basic() -> Self {
        Self {
            window_background: 0xFFF0F0F0,
            window_frame_active: 0xFF9AA5B1,
            window_frame_inactive: 0xFFB8C0C8,
            text_primary: 0xFF101418,
            button_normal: 0xFFF2F2F2,
            button_hover: 0xFFDCEAFB,
            button_pressed: 0xFFC2D9F5,
            button_disabled: 0xFFE5E8EA,
            highlight: 0xFF386CA6,
            titlebar_height: 30,
            window_corner_radius: 4,
            taskbar_height: 40,
        }
    }

    pub fn aero() -> Self {
        Self {
            window_frame_active: 0xFF4580C4,
            window_frame_inactive: 0xFF6D96C7,
            button_normal: 0xFFF2F2F2,
            ..Self::windows7_basic()
        }
    }

    pub fn classic() -> Self {
        Self {
            window_background: 0xFFD4D0C8,
            window_frame_active: 0xFF808080,
            window_frame_inactive: 0xFFA0A0A0,
            text_primary: 0xFF000000,
            button_normal: 0xFFD4D0C8,
            button_hover: 0xFFD4D0C8,
            button_pressed: 0xFFC0C0C0,
            button_disabled: 0xFFD4D0C8,
            highlight: 0xFF000080,
            titlebar_height: 18,
            window_corner_radius: 0,
            taskbar_height: 28,
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            window_background: 0xFF000000,
            window_frame_active: 0xFFFFFFFF,
            window_frame_inactive: 0xFF808080,
            text_primary: 0xFFFFFFFF,
            button_normal: 0xFF000000,
            button_hover: 0xFF808080,
            button_pressed: 0xFF404040,
            button_disabled: 0xFF202020,
            highlight: 0xFFFFD700,
            titlebar_height: 30,
            window_corner_radius: 0,
            taskbar_height: 40,
        }
    }

    pub fn for_theme(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Aero => Self::aero(),
            ThemeKind::Basic => Self::windows7_basic(),
            ThemeKind::Classic => Self::classic(),
            ThemeKind::HighContrast => Self::high_contrast(),
        }
    }
}

impl Theme {
    pub fn for_theme(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Aero => Self::aero(),
            ThemeKind::Basic => Self::windows7_basic(),
            ThemeKind::Classic => Self::classic(),
            ThemeKind::HighContrast => Self::high_contrast(),
        }
    }

    pub fn windows7_basic() -> Self {
        Self {
            kind: ThemeKind::Basic,
            name: String::from("Windows 7 Basic"),
            composition: true,
            glass: false,
            base: String::from("#F0F4F8"),
            frame: String::from("#9AA5B1"),
            text: String::from("#101418"),
            taskbar: String::from("#202124"),
        }
    }

    pub fn aero() -> Self {
        Self {
            kind: ThemeKind::Aero,
            name: String::from("Aero"),
            composition: true,
            glass: true,
            base: String::from("#EAF2FB"),
            frame: String::from("#4580C4"),
            text: String::from("#101418"),
            taskbar: String::from("#1F2937"),
        }
    }

    pub fn classic() -> Self {
        Self {
            kind: ThemeKind::Classic,
            name: String::from("Windows Classic"),
            composition: false,
            glass: false,
            base: String::from("#D4D0C8"),
            frame: String::from("#808080"),
            text: String::from("#000000"),
            taskbar: String::from("#C0C0C0"),
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            kind: ThemeKind::HighContrast,
            name: String::from("High Contrast Black"),
            composition: false,
            glass: false,
            base: String::from("#000000"),
            frame: String::from("#FFFFFF"),
            text: String::from("#FFFFFF"),
            taskbar: String::from("#000000"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_win7_basic_not_win10() {
        let theme = Theme::windows7_basic();
        assert_eq!(theme.kind, ThemeKind::Basic);
        assert!(theme.composition);
        assert!(!theme.glass);
    }

    #[test]
    fn aero_is_composited_and_glass() {
        let theme = Theme::aero();
        assert!(theme.composition && theme.glass);
    }

    #[test]
    fn theme_tokens_cover_named_widget_styles_and_metrics() {
        let basic = ThemeTokens::for_theme(ThemeKind::Basic);
        assert_eq!(basic.titlebar_height, 30);
        assert_eq!(basic.taskbar_height, 40);
        assert_eq!(
            ThemeTokens::for_theme(ThemeKind::Aero).window_frame_active,
            ThemeTokens::aero().window_frame_active
        );
    }
}

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

impl Theme {
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
            frame: String::from("#8AB4F8"),
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
}

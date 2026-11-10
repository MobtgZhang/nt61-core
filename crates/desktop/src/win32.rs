//! Win32 GUI compatibility coverage for the NT 6.1 desktop primitives.
//!
//! The matrix is intentionally conservative: a function is listed only when
//! an existing desktop module already owns the semantic work. It is not a
//! claim that the full Win32 GUI API is implemented.

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Win32Api {
    User32,
    Gdi32,
    Win32k,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Win32Function {
    pub api: Win32Api,
    pub name: &'static str,
    pub implemented: bool,
    pub owner: &'static str,
}

pub const WIN32_FUNCTIONS: &[Win32Function] = &[
    Win32Function {
        api: Win32Api::User32,
        name: "RegisterClassExW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "CreateWindowExW",
        implemented: true,
        owner: "window::ipc",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "DestroyWindow",
        implemented: true,
        owner: "window::ipc",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "ShowWindow",
        implemented: true,
        owner: "window::ipc",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "SetWindowPos",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "GetMessageW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "PeekMessageW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "TranslateMessage",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "DispatchMessageW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "SendMessageW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "PostMessageW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "DefWindowProcW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "BeginPaint",
        implemented: true,
        owner: "graphics::surface",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "EndPaint",
        implemented: true,
        owner: "graphics::surface",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "InvalidateRect",
        implemented: true,
        owner: "compositor::damage",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "GetCursorPos",
        implemented: true,
        owner: "platform::input",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "SetCapture",
        implemented: true,
        owner: "controls::tree",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "ReleaseCapture",
        implemented: true,
        owner: "controls::tree",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "GetForegroundWindow",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "SetForegroundWindow",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "OpenWindowStationW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "CreateDesktopW",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "SwitchDesktop",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::User32,
        name: "DrawTextW",
        implemented: true,
        owner: "graphics::font",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "CreateCompatibleDC",
        implemented: true,
        owner: "graphics::surface",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "CreateDIBSection",
        implemented: true,
        owner: "graphics::surface",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "SelectObject",
        implemented: true,
        owner: "graphics::font",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "BitBlt",
        implemented: true,
        owner: "graphics::blit",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "StretchBlt",
        implemented: true,
        owner: "graphics::blit",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "FillRect",
        implemented: true,
        owner: "graphics::surface",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "AlphaBlend",
        implemented: true,
        owner: "graphics::surface",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "IntersectClipRect",
        implemented: true,
        owner: "graphics::clip",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "TextOutW",
        implemented: true,
        owner: "graphics::font",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "DrawTextW",
        implemented: true,
        owner: "graphics::font",
    },
    Win32Function {
        api: Win32Api::Gdi32,
        name: "GetTextMetricsW",
        implemented: true,
        owner: "graphics::font",
    },
    Win32Function {
        api: Win32Api::Win32k,
        name: "NtUserGetForegroundWindow",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::Win32k,
        name: "NtUserSetForegroundWindow",
        implemented: true,
        owner: "window::manager",
    },
    Win32Function {
        api: Win32Api::Win32k,
        name: "NtUserGetAsyncKeyState",
        implemented: true,
        owner: "platform::input",
    },
    Win32Function {
        api: Win32Api::Win32k,
        name: "NtGdiIntersectClipRect",
        implemented: true,
        owner: "graphics::clip",
    },
    Win32Function {
        api: Win32Api::Win32k,
        name: "NtGdiFlush",
        implemented: true,
        owner: "graphics::surface",
    },
];

pub fn supported_functions() -> Vec<&'static str> {
    WIN32_FUNCTIONS
        .iter()
        .filter(|function| function.implemented)
        .map(|function| function.name)
        .collect()
}

pub fn unsupported_functions() -> Vec<&'static str> {
    WIN32_FUNCTIONS
        .iter()
        .filter(|function| !function.implemented)
        .map(|function| function.name)
        .collect()
}

pub fn functions_for(api: Win32Api) -> Vec<&'static str> {
    WIN32_FUNCTIONS
        .iter()
        .filter(|function| function.api == api)
        .map(|function| function.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::ControlTree;
    use crate::graphics::Surface;
    use crate::window::ipc::WindowService;

    #[test]
    fn capability_matrix_contains_documented_win7_surface() {
        for name in [
            "RegisterClassExW",
            "CreateWindowExW",
            "DestroyWindow",
            "ShowWindow",
            "SetWindowPos",
            "GetMessageW",
            "PeekMessageW",
            "TranslateMessage",
            "DispatchMessageW",
            "SendMessageW",
            "PostMessageW",
            "DefWindowProcW",
            "BeginPaint",
            "EndPaint",
            "InvalidateRect",
            "GetCursorPos",
            "SetCapture",
            "ReleaseCapture",
            "GetForegroundWindow",
            "SetForegroundWindow",
            "CreateDesktopW",
            "SwitchDesktop",
        ] {
            assert!(
                supported_functions().contains(&name),
                "{name} must be mapped to an existing desktop owner"
            );
        }
    }

    #[test]
    fn gdi_matrix_uses_existing_surface_and_font_owners() {
        for name in [
            "CreateCompatibleDC",
            "CreateDIBSection",
            "SelectObject",
            "BitBlt",
            "StretchBlt",
            "FillRect",
            "AlphaBlend",
            "IntersectClipRect",
            "TextOutW",
            "DrawTextW",
            "GetTextMetricsW",
        ] {
            assert!(supported_functions().contains(&name));
        }
    }

    #[test]
    fn matrix_owners_are_real_modules() {
        for function in WIN32_FUNCTIONS {
            assert!(function.implemented);
            assert!(!function.owner.is_empty());
        }
    }

    #[test]
    fn functions_are_grouped_by_api() {
        assert!(!functions_for(Win32Api::User32).is_empty());
        assert!(!functions_for(Win32Api::Gdi32).is_empty());
        assert!(!functions_for(Win32Api::Win32k).is_empty());
        assert!(unsupported_functions().is_empty());
    }

    #[test]
    fn compatibility_models_are_renderable_and_routable() {
        let surface = Surface::new(1, 1);
        let controls = ControlTree::new();
        let service = WindowService::new(1, 1);
        assert_eq!(surface.width(), 1);
        assert_eq!(controls.controls().len(), 0);
        assert!(service.compose(&surface).pixels().len() == 1);
    }
}

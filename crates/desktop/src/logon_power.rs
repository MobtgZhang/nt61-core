//! AuthUX's glass shutdown split button, using the original state bitmaps.
use crate::{
    bmp,
    graphics::{
        blit::{blit_nine_slice, blit_rgba},
        Rect, Surface,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonState {
    #[default]
    Normal,
    Focus,
    Hot,
    FocusHot,
    Pressed,
}

impl ButtonState {
    pub fn from_input(hot: bool, focused: bool, pressed: bool) -> Self {
        match (hot, focused, pressed) {
            (_, _, true) => Self::Pressed,
            (true, true, _) => Self::FocusHot,
            (true, false, _) => Self::Hot,
            (false, true, _) => Self::Focus,
            _ => Self::Normal,
        }
    }
}

pub struct LogonPowerButtons {
    main: [bmp::Image; 5],
    arrow: [bmp::Image; 5],
    power_glyph: bmp::Image,
    arrow_glyph: bmp::Image,
}

impl LogonPowerButtons {
    pub fn load() -> Option<Self> {
        macro_rules! bitmap {
            ($name:literal) => {
                bmp::decode_bmp(include_bytes!(concat!(
                    "../assets/windows7/logon-buttons/",
                    $name,
                    ".bmp"
                )))
                .ok()?
            };
        }
        Some(Self {
            main: [
                bitmap!("power-normal"),
                bitmap!("power-focus"),
                bitmap!("power-hover"),
                bitmap!("power-focus-hover"),
                bitmap!("power-pressed"),
            ],
            arrow: [
                bitmap!("power-arrow-normal"),
                bitmap!("power-arrow-focus"),
                bitmap!("power-arrow-hover"),
                bitmap!("power-arrow-focus-hover"),
                bitmap!("power-arrow-pressed"),
            ],
            power_glyph: bitmap!("power"),
            arrow_glyph: bitmap!("power-arrow-glyph"),
        })
    }

    /// 96-DPI AuthUX metrics: 38x28 primary and 20x28 options, joined at the seam.
    pub fn bounds(width: u32, y: i32) -> (Rect, Rect) {
        let right = width.saturating_sub(24) as i32;
        (
            Rect::new(right - 58, y, 38, 28),
            Rect::new(right - 20, y, 20, 28),
        )
    }

    pub fn draw(
        &self,
        frame: &mut Surface,
        main: Rect,
        arrow: Rect,
        main_state: ButtonState,
        arrow_state: ButtonState,
    ) {
        for (bounds, image, glyph) in [
            (main, &self.main[main_state as usize], &self.power_glyph),
            (arrow, &self.arrow[arrow_state as usize], &self.arrow_glyph),
        ] {
            // Height is native, so only the center stretches horizontally.
            blit_nine_slice(frame, &image.pixels, (image.width, image.height), bounds, 4);
            blit_rgba(
                frame,
                &glyph.pixels,
                glyph.stride(),
                Rect::new(
                    bounds.x + (bounds.width as i32 - glyph.width as i32) / 2,
                    bounds.y + (bounds.height as i32 - glyph.height as i32) / 2,
                    glyph.width,
                    glyph.height,
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_glass_states_and_alpha_survive_decode_and_composition() {
        let buttons = LogonPowerButtons::load().unwrap();
        let (main, arrow) = LogonPowerButtons::bounds(120, 5);
        assert_eq!((main.width, arrow.width, main.height), (38, 20, 28));
        assert_eq!(main.x + main.width as i32, arrow.x);
        let mut states = alloc::vec::Vec::new();
        for state in [
            ButtonState::Normal,
            ButtonState::Focus,
            ButtonState::Hot,
            ButtonState::FocusHot,
            ButtonState::Pressed,
        ] {
            let mut frame = Surface::new(120, 40);
            frame.fill_rect(0, 0, 120, 40, 0xFF438BB9);
            buttons.draw(&mut frame, main, arrow, state, state);
            assert_eq!(frame.pixel(0, 0), Some(0xFF438BB9));
            assert!(frame
                .pixels()
                .iter()
                .any(|p| (p >> 16 & 255) > (p & 255) + 20));
            assert!(states.iter().all(|pixels| pixels != frame.pixels()));
            states.push(frame.pixels().to_vec());
        }
        assert!(buttons.main[0].pixels.chunks_exact(4).any(|p| p[3] == 0));
        assert_eq!(
            (buttons.arrow_glyph.width, buttons.arrow_glyph.height),
            (9, 6)
        );
    }
}

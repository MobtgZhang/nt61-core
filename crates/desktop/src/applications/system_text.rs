//! Wrap dialog prose before passing it to the shell's single-line font callback.
use crate::graphics::{Rect, Surface};
use alloc::string::String;

pub fn paragraph<F: Fn(&mut Surface, &str, Rect, u32)>(
    surface: &mut Surface,
    value: &str,
    bounds: Rect,
    color: u32,
    text: &F,
) {
    if bounds.height < 36 {
        text(surface, value, bounds, color);
        return;
    }
    let columns = (bounds.width / 8).max(1) as usize;
    let mut y = bounds.y;
    let bottom = bounds.y.saturating_add(bounds.height as i32);
    for line in value.split('\n') {
        let mut part = String::new();
        let mut count = 0;
        for word in line.split_whitespace() {
            let length = word.chars().count();
            if count != 0 && count + 1 + length > columns {
                if y + 18 > bottom {
                    return;
                }
                text(
                    surface,
                    &part,
                    Rect::new(bounds.x, y, bounds.width, 18),
                    color,
                );
                y += 18;
                part.clear();
                count = 0;
            }
            if count != 0 {
                part.push(' ');
                count += 1;
            }
            for ch in word.chars() {
                if count >= columns {
                    if y + 18 > bottom {
                        return;
                    }
                    text(
                        surface,
                        &part,
                        Rect::new(bounds.x, y, bounds.width, 18),
                        color,
                    );
                    y += 18;
                    part.clear();
                    count = 0;
                }
                part.push(ch);
                count += 1;
            }
        }
        if y + 18 > bottom {
            return;
        }
        text(
            surface,
            part.trim_end(),
            Rect::new(bounds.x, y, bounds.width, 18),
            color,
        );
        y += 18;
    }
}

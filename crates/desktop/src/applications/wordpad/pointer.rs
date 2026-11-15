//! Document selection and the accessory's scroll/zoom controls.
use super::*;

fn zoom_track(b: Rect) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 119,
        b.y + b.height as i32 - 23,
        86,
        22,
    )
}

fn scroll_track(b: Rect, r: Rect) -> Rect {
    Rect::new(
        b.x + b.width as i32 - 16,
        r.y + 20,
        15,
        r.height.saturating_sub(40),
    )
}

fn scroll_geometry(app: &ApplicationSession, d: &Document, b: Rect) -> (Rect, u32, u32) {
    let r = viewport(b, &app.wordpad);
    let p = page(r, app.wordpad.zoom);
    let width = p
        .width
        .saturating_sub(margin(p, app.wordpad.zoom) * 2)
        .max(1);
    with_lines(app, d, width, |lines| {
        let content = lines.last().map_or(0, |line| line.y + line.height);
        let maximum = content.saturating_sub(r.height);
        let track = scroll_track(b, r);
        let height = (track.height * r.height / content.max(r.height).max(1))
            .max(24)
            .min(track.height);
        let travel = track.height.saturating_sub(height);
        let current = top(app, d, lines, r.height).min(maximum);
        let offset = u64::from(current) * u64::from(travel) / u64::from(maximum.max(1));
        (
            Rect::new(track.x, track.y + offset as i32, track.width, height),
            maximum,
            current,
        )
    })
}

pub(super) fn draw_scroll_thumb(
    app: &ApplicationSession,
    d: &Document,
    frame: &mut Surface,
    b: Rect,
) {
    let (thumb, maximum, _) = scroll_geometry(app, d, b);
    if maximum == 0 {
        return;
    }
    crate::compositor::aero::menu_item(frame, thumb, app.wordpad.scroll_drag.is_some());
    let middle = thumb.y + thumb.height as i32 / 2;
    for offset in [-3, 0, 3] {
        frame.fill_rect(thumb.x + 4, middle + offset, 7, 1, 0xff8a9bab);
    }
}

pub fn pointer(
    app: &mut ApplicationSession,
    b: Rect,
    x: i32,
    y: i32,
    down: bool,
    held: bool,
) -> bool {
    let r = viewport(b, &app.wordpad);
    if !down && !held {
        let changed = app.wordpad.scroll_drag.take().is_some() || app.wordpad.zoom_drag;
        app.wordpad.zoom_drag = false;
        if let AppState::Document(d) = &mut app.state {
            d.drag_anchor = None;
        }
        return changed;
    }
    let zoom = zoom_track(b);
    if app.wordpad.zoom_drag || (down && app.wordpad.status && zoom.contains(x, y)) {
        app.wordpad.zoom_drag = true;
        app.wordpad.popup = None;
        let offset = (x - zoom.x).clamp(0, 82) as u32;
        app.wordpad.zoom = (25 + offset * 375 / 82) as u16;
        app.wordpad.manual_scroll = false;
        return true;
    }
    let bar = Rect::new(b.x + b.width as i32 - 17, r.y, 17, r.height);
    if app.wordpad.scroll_drag.is_some() || (down && bar.contains(x, y)) {
        let AppState::Document(d) = &app.state else {
            return false;
        };
        let (thumb, maximum, current) = scroll_geometry(app, d, b);
        let track = scroll_track(b, r);
        if let Some(grab) = app.wordpad.scroll_drag {
            let travel = track.height.saturating_sub(thumb.height);
            let offset = (y - track.y - grab).clamp(0, travel as i32) as u32;
            app.wordpad.top =
                (u64::from(offset) * u64::from(maximum) / u64::from(travel.max(1))) as u32;
        } else if thumb.contains(x, y) && maximum > 0 {
            app.wordpad.scroll_drag = Some(y - thumb.y);
            app.wordpad.top = current;
        } else {
            let delta = if y < track.y || y >= track.y + track.height as i32 {
                22
            } else {
                r.height.saturating_sub(22)
            };
            app.wordpad.top = if y < thumb.y {
                current.saturating_sub(delta)
            } else {
                current.saturating_add(delta).min(maximum)
            };
        }
        app.wordpad.manual_scroll = true;
        app.wordpad.popup = None;
        return true;
    }
    if !r.contains(x, y) {
        return false;
    }
    let p = page(r, app.wordpad.zoom);
    let m = margin(p, app.wordpad.zoom);
    let AppState::Document(d) = &app.state else {
        return false;
    };
    if !down && d.drag_anchor.is_none() {
        return false;
    }
    let target = with_lines(app, d, p.width.saturating_sub(m * 2).max(1), |lines| {
        let top = top(app, d, lines, r.height);
        let py = (y - r.y).max(0) as u32 + top;
        let row = lines.iter().rposition(|l| l.y <= py).unwrap_or(0);
        let line = &lines[row];
        let px = x - p.x - m as i32;
        line.cells
            .iter()
            .find(|c| px < (c.x + c.width / 2) as i32)
            .map_or(line.end, |c| c.index)
    });
    app.wordpad.popup = None;
    if let AppState::Document(d) = &mut app.state {
        if down {
            d.drag_anchor = Some(target);
            d.anchor = None;
        } else {
            d.anchor = d.drag_anchor;
        }
        d.editor.cursor = target;
        if let Some(r) = &mut d.rich {
            r.typing = r.style(target.saturating_sub(1));
        }
    }
    true
}

use super::manager::{Window, WindowHandle};
use super::manager::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT,
};

pub fn hit_test(window: &Window, handle: WindowHandle, x: i32, y: i32) -> u32 {
    let _ = handle;
    let border = 6i32;
    let relative_x = x - window.rect.x;
    let relative_y = y - window.rect.y;
    let on_left = relative_x < border;
    let on_right = relative_x >= window.rect.width as i32 - border;
    let on_top = relative_y < border;
    let on_bottom = relative_y >= window.rect.height as i32 - border;
    match (on_left, on_top, on_right, on_bottom) {
        (true, true, _, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, _, true, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, _, true, _) => HTRIGHT,
        (_, true, _, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        (false, false, false, false) if relative_y < 30 => HTCAPTION,
        _ => HTCLIENT,
    }
}

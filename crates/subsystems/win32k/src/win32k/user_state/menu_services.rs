//! Menu identity and highlight changes share the menu state used by SetMenu.
use super::{with_user_state, UserState, MF_BYPOSITION};
use crate::arch::common::syscall_frame::SyscallFrame;

pub extern "C" fn nt_user_get_menu_index(frame: &SyscallFrame) -> u64 {
    with_user_state(|state| state.menus.get(&frame.args[0])
        .and_then(|menu| menu.items.iter().position(|item| item.submenu == frame.args[1] && item.submenu != 0))
        .map_or(u32::MAX as u64, |index| index as u64))
}

pub extern "C" fn nt_user_set_menu_context_help_id(frame: &SyscallFrame) -> u64 {
    with_user_state(|state| {
        let Some(menu) = state.menus.get_mut(&frame.args[0]) else { return 0 };
        menu.context_help_id = frame.args[1] as u32;
        1
    })
}

impl UserState {
    fn highlight(&mut self, menu: u64, item: u32, flags: u32, depth: usize) -> bool {
        if depth >= 32 { return false; }
        let Some(entry) = self.menus.get_mut(&menu) else { return false };
        let index = if flags & MF_BYPOSITION != 0 {
            ((item as usize) < entry.items.len()).then_some(item as usize)
        } else {
            entry.items.iter().position(|candidate| candidate.id == item)
        };
        if let Some(index) = index {
            let candidate = &mut entry.items[index];
            candidate.flags = (candidate.flags & !0x80) | (flags & 0x80);
            return true;
        }
        if flags & MF_BYPOSITION != 0 { return false; }
        // Do not hold an entry borrow while walking submenus. The fixed bound
        // also terminates malformed cyclic menu graphs.
        let count = entry.items.len();
        for index in 0..count {
            let submenu = self.menus.get(&menu).unwrap().items[index].submenu;
            if submenu != 0 && self.highlight(submenu, item, flags, depth + 1) { return true; }
        }
        false
    }
}

pub extern "C" fn nt_user_hilite_menu_item(frame: &SyscallFrame) -> u64 {
    if crate::libs::win32k::window::WINDOW_MANAGER.lock().get_window(frame.args[0]).is_none() { return 0; }
    let changed = with_user_state(|state| state.highlight(frame.args[1], frame.args[2] as u32, frame.args[3] as u32, 0));
    if changed {
        crate::libs::win32k::window::invalidate_rect_internal(frame.args[0], None);
    }
    u64::from(changed)
}


/// The private Win7 call marks an existing menu as right-to-left; it has no
/// second Boolean argument. Items inherit MFT_RIGHTORDER from this menu state.
pub extern "C" fn nt_user_set_menu_flag_rto_l(frame: &SyscallFrame) -> u64 {
    with_user_state(|state| {
        let Some(menu) = state.menus.get_mut(&frame.args[0]) else { return 0; };
        if menu.process_id != super::super::window::input_context::caller_identity().0 { return 0; }
        menu.right_to_left = true;
        for item in &mut menu.items { item.flags |= 0x2000; }
        1
    })
}

//! Class registration and class/window extra bytes share the window manager
//! lock. Pointer-sized fields are never truncated by their LONG_PTR services.
use super::{WindowClass, MAX_WINDOW_CLASSES, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_from_user, read_user_object};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

#[repr(C)]
#[derive(Clone, Copy)]
struct ClassEx {
    size: u32,
    style: u32,
    procedure: u64,
    class_bytes: i32,
    window_bytes: i32,
    instance: u64,
    icon: u64,
    cursor: u64,
    background: u64,
    menu: u64,
    name: u64,
    small_icon: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UnicodeString {
    length: u16,
    capacity: u16,
    padding: u32,
    buffer: u64,
}

pub extern "C" fn nt_user_register_class_ex_wow(frame: &SyscallFrame) -> u64 {
    let Ok(info) = read_user_object::<ClassEx>(frame.args[0]) else {
        return 0;
    };
    if info.size != 80
        || !(0..=4096).contains(&info.class_bytes)
        || !(0..=4096).contains(&info.window_bytes)
        || info.procedure == 0
    {
        return 0;
    }
    let Ok(name) = read_user_object::<UnicodeString>(frame.args[1]) else {
        return 0;
    };
    if name.length == 0 || name.length > 126 || name.length > name.capacity || name.length & 1 != 0
    {
        return 0;
    }
    let mut raw = [0u8; 128];
    if copy_from_user(&mut raw[..name.length as usize], name.buffer).is_err() {
        return 0;
    }
    let mut class_name = [0u16; 64];
    for (unit, bytes) in class_name.iter_mut().zip(raw.chunks_exact(2)) {
        *unit = u16::from_le_bytes([bytes[0], bytes[1]]);
    }
    if class_name[..name.length as usize / 2].contains(&0) {
        return 0;
    }
    let mut extra = Vec::new();
    if extra.try_reserve_exact(info.class_bytes as usize).is_err() {
        return 0;
    }
    extra.resize(info.class_bytes as usize, 0);
    let mut manager = WINDOW_MANAGER.lock();
    if manager
        .classes
        .iter()
        .flatten()
        .any(|class| class.instance == info.instance && same_class_name(&class.name, &class_name))
    {
        return 0;
    }
    if manager.classes.iter().flatten().count() >= MAX_WINDOW_CLASSES {
        return 0;
    }
    if manager.classes.try_reserve(1).is_err() {
        return 0;
    }
    // USER class atoms occupy the integer atom range. Never wrap and alias a
    // live or formerly allocated class when this range has been exhausted.
    let next = manager.next_class_atom.load(Ordering::Relaxed);
    if next >= 0xc000 {
        return 0;
    }
    manager.next_class_atom.store(next + 1, Ordering::Relaxed);
    manager.classes.push(Some(WindowClass {
        name: class_name,
        instance: info.instance,
        style: info.style,
        wndproc: info.procedure,
        cls_extra: info.class_bytes,
        wnd_extra: info.window_bytes,
        h_icon: info.icon,
        h_cursor: info.cursor,
        h_brbackground: info.background,
        menu_name: info.menu,
        atom: next as u16,
        small_icon: info.small_icon,
        extra,
    }));
    next
}

fn set_class(frame: &SyscallFrame, width: usize) -> u64 {
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window(frame.args[0]) else {
        return 0;
    };
    let atom = window.class_atom;
    let Some(class) = manager
        .classes
        .iter_mut()
        .flatten()
        .find(|class| class.atom == atom)
    else {
        return 0;
    };
    let index = frame.args[1] as i32;
    let value = match width {
        2 => frame.args[2] as u16 as u64,
        4 => frame.args[2] as u32 as u64,
        _ => frame.args[2],
    };
    let previous = match index {
        -8 if width != 2 => core::mem::replace(&mut class.menu_name, value),
        -10 if width != 2 => core::mem::replace(&mut class.h_brbackground, value),
        -12 if width != 2 => core::mem::replace(&mut class.h_cursor, value),
        -14 if width != 2 => core::mem::replace(&mut class.h_icon, value),
        -16 if width != 2 => core::mem::replace(&mut class.instance, value),
        -24 if width != 2 => core::mem::replace(&mut class.wndproc, value),
        -26 if width != 2 => u64::from(core::mem::replace(&mut class.style, value as u32)),
        -34 if width != 2 => core::mem::replace(&mut class.small_icon, value),
        offset if offset >= 0 => {
            let start = offset as usize;
            let Some(end) = start.checked_add(width) else {
                return 0;
            };
            let Some(bytes) = class.extra.get_mut(start..end) else {
                return 0;
            };
            let mut old = [0u8; 8];
            old[..width].copy_from_slice(bytes);
            bytes.copy_from_slice(&value.to_le_bytes()[..width]);
            u64::from_le_bytes(old)
        }
        _ => return 0,
    };
    match width {
        2 => previous as u16 as u64,
        4 => previous as u32 as u64,
        _ => previous,
    }
}

pub extern "C" fn nt_user_set_class_long_ptr(frame: &SyscallFrame) -> u64 {
    set_class(frame, 8)
}
pub extern "C" fn nt_user_set_class_long(frame: &SyscallFrame) -> u64 {
    set_class(frame, 4)
}
pub extern "C" fn nt_user_set_class_word(frame: &SyscallFrame) -> u64 {
    set_class(frame, 2)
}

/// Class names use ordinal, case-insensitive UTF-16 comparison. Expansion
/// mappings (such as sharp-s to two letters) are not ordinal equivalents.
pub(super) fn same_class_name(left: &[u16; 64], right: &[u16; 64]) -> bool {
    fn uppercase(unit: u16) -> u16 {
        let Some(character) = char::from_u32(u32::from(unit)) else { return unit };
        let mut mapped = character.to_uppercase();
        let Some(first) = mapped.next() else { return unit };
        if mapped.next().is_some() || u32::from(first) > u16::MAX as u32 {
            unit
        } else {
            first as u16
        }
    }
    left.iter().zip(right).all(|(&a, &b)| a == b || uppercase(a) == uppercase(b))
}

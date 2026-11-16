//! USER input context object contracts. This manages HIMC ownership and window
//! associations; composition and candidate rendering remain in the IME client.
use super::{WindowManager, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_to_user, probe_user_write, write_user_object};
use alloc::vec::Vec;

pub(super) struct InputContext {
    handle: u64,
    process: u64,
    thread: u64,
    client_data: u64,
    pub(super) ime_window: u64,
    default: bool,
}

pub(crate) fn caller_identity() -> (u64, u64) {
    let thread = crate::ps::thread::get_current_ethread();
    if thread.is_null() { return (0, 0); }
    // SAFETY: the current ETHREAD remains alive while executing its syscall.
    unsafe { ((*thread).client_id.unique_process, (*thread).client_id.unique_thread) }
}

impl WindowManager {
    fn ime_disabled(&self, process: u64, thread: u64) -> bool {
        self.ime_disabled_processes.contains(&process)
            || self.ime_disabled_threads.contains(&(process, thread))
    }

    pub(crate) fn default_input_context(&mut self, process: u64, thread: u64) -> Option<u64> {
        if self.ime_disabled(process, thread) { return None; }
        if let Some(context) = self.contexts.iter().find(|c| c.process == process && c.thread == thread && c.default) {
            return Some(context.handle);
        }
        self.allocate_context(process, thread, 0, true)
    }

    fn allocate_context(&mut self, process: u64, thread: u64, client_data: u64, default: bool) -> Option<u64> {
        let next = self.next_context.checked_add(1)?;
        if next >= 0x6000_0000 { return None; }
        self.contexts.try_reserve(1).ok()?;
        let handle = u64::from(self.next_context);
        self.next_context = next;
        self.contexts.push(InputContext { handle, process, thread, client_data, ime_window: 0, default });
        Some(handle)
    }
}

pub extern "C" fn nt_user_create_input_context(frame: &SyscallFrame) -> u64 {
    if frame.args[0] == 0 { return 0; }
    let (process, thread) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    if manager.default_input_context(process, thread).is_none() { return 0; }
    manager.allocate_context(process, thread, frame.args[0], false).unwrap_or(0)
}

pub extern "C" fn nt_user_destroy_input_context(frame: &SyscallFrame) -> u64 {
    let (process, thread) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(index) = manager.contexts.iter().position(|c| c.handle == frame.args[0]
        && c.process == process && c.thread == thread && !c.default) else { return 0 };
    let default = manager.contexts.iter().find(|c| c.process == process && c.thread == thread && c.default)
        .map_or(0, |c| c.handle);
    manager.contexts.remove(index);
    for window in manager.windows.iter_mut().flatten() {
        if window.input_context == frame.args[0] { window.input_context = default; }
    }
    1
}

pub extern "C" fn nt_user_query_input_context(frame: &SyscallFrame) -> u64 {
    let manager = WINDOW_MANAGER.lock();
    let Some(context) = manager.contexts.iter().find(|c| c.handle == frame.args[0]) else { return 0 };
    match frame.args[1] as u32 {
        0 => context.process,
        1 => context.thread,
        2 => manager.contexts.iter().find(|c| c.thread == context.thread && c.process == context.process && c.default)
            .map_or(0, |c| c.ime_window),
        3 => manager.contexts.iter().find(|c| c.thread == context.thread && c.process == context.process && c.default)
            .map_or(0, |c| c.handle),
        _ => 0,
    }
}

pub extern "C" fn nt_user_update_input_context(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    if frame.args[1] as u32 == 1 && manager.get_window(frame.args[2]).is_none() { return 0; }
    let Some(context) = manager.contexts.iter_mut().find(|c| c.handle == frame.args[0] && c.process == process) else { return 0 };
    match frame.args[1] as u32 {
        0 if context.client_data == 0 => context.client_data = frame.args[2],
        1 => context.ime_window = frame.args[2],
        _ => return 0,
    }
    1
}

pub extern "C" fn nt_user_build_himc_list(frame: &SyscallFrame) -> u64 {
    const INVALID_PARAMETER: u64 = 0xc000000d;
    const NO_MEMORY: u64 = 0xc0000017;
    const ACCESS_VIOLATION: u64 = 0xc0000005;
    let (process, caller_thread) = caller_identity();
    let requested = frame.args[0] as u32;
    let capacity = frame.args[1] as u32 as usize;
    if capacity > 0x1fff_ffff { return INVALID_PARAMETER; }
    if probe_user_write(frame.args[3], 4).is_err() { return ACCESS_VIOLATION; }
    let manager = WINDOW_MANAGER.lock();
    let thread = if requested == 0 { caller_thread } else { u64::from(requested) };
    let matches = |c: &&InputContext| c.process == process && (requested == u32::MAX || c.thread == thread);
    let count = manager.contexts.iter().filter(matches).count() as u32;
    let mut bytes = Vec::new();
    let copied = capacity.min(count as usize);
    if bytes.try_reserve_exact(copied * 8).is_err() { return NO_MEMORY; }
    for context in manager.contexts.iter().filter(matches).take(copied) {
        bytes.extend_from_slice(&context.handle.to_le_bytes());
    }
    drop(manager);
    if !bytes.is_empty() && copy_to_user(frame.args[2], &bytes).is_err() { return ACCESS_VIOLATION; }
    if write_user_object(frame.args[3], &count).is_err() { return ACCESS_VIOLATION; }
    if capacity < count as usize { 0xc0000023 } else { 0 }
}

pub extern "C" fn nt_user_associate_input_context(frame: &SyscallFrame) -> u64 {
    let flags = frame.args[2] as u32;
    if flags & !0x31 != 0 { return 2; }
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window(frame.args[0]) else { return 2 };
    if window.process_id != process { return 2; }
    let thread = window.thread_id;
    let context = if flags & 0x10 != 0 {
        manager.contexts.iter().find(|c| c.process == process && c.thread == thread && c.default)
            .map_or(0, |c| c.handle)
    } else {
        if frame.args[1] != 0 && !manager.contexts.iter().any(|c| c.handle == frame.args[1]
            && c.process == process && c.thread == thread) { return 2; }
        frame.args[1]
    };
    let focus = manager.get_focus();
    let mut targets = Vec::new();
    if targets.try_reserve_exact(manager.windows.len()).is_err() { return 2; }
    targets.push(frame.args[0]);
    if flags & 1 != 0 {
        let mut index = 0;
        while index < targets.len() {
            let parent = targets[index];
            for window in manager.windows.iter().flatten() {
                if window.parent == Some(parent) { targets.push(window.hwnd); }
            }
            index += 1;
        }
    }
    let mut result = 0;
    for hwnd in targets {
        let Some(window) = manager.get_window_mut(hwnd) else { continue };
        if window.thread_id != thread || window.process_id != process
            || (flags & 0x20 != 0 && window.input_context == 0) || window.input_context == context { continue; }
        window.input_context = context;
        if hwnd == focus { result = 1; }
    }
    result
}

pub extern "C" fn nt_user_get_app_ime_level(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    WINDOW_MANAGER.lock().get_window(frame.args[0])
        .filter(|w| w.process_id == process).map_or(0, |w| u64::from(w.ime_level))
}

pub extern "C" fn nt_user_set_app_ime_level(frame: &SyscallFrame) -> u64 {
    let (process, _) = caller_identity();
    let mut manager = WINDOW_MANAGER.lock();
    let Some(window) = manager.get_window_mut(frame.args[0]).filter(|w| w.process_id == process) else { return 0 };
    window.ime_level = frame.args[1] as u32;
    1
}

pub extern "C" fn nt_user_disable_thread_ime(frame: &SyscallFrame) -> u64 {
    let (process, caller_thread) = caller_identity();
    let requested = frame.args[0] as u32;
    let thread = if requested == 0 { caller_thread } else { u64::from(requested) };
    let mut manager = WINDOW_MANAGER.lock();
    if requested == u32::MAX {
        if !manager.ime_disabled_processes.contains(&process) {
            if manager.ime_disabled_processes.try_reserve(1).is_err() { return 0; }
            manager.ime_disabled_processes.push(process);
        }
    } else {
        if thread != caller_thread && !manager.contexts.iter().any(|c| c.thread == thread && c.process == process) { return 0; }
        if !manager.ime_disabled_threads.contains(&(process, thread)) {
            if manager.ime_disabled_threads.try_reserve(1).is_err() { return 0; }
            manager.ime_disabled_threads.push((process, thread));
        }
    }
    for window in manager.windows.iter_mut().flatten() {
        if window.process_id == process && (requested == u32::MAX || window.thread_id == thread) {
            window.input_context = 0;
        }
    }
    1
}

//! Native message retrieval and posting share the hardware input queue.
//! Waiting yields outside USER locks; scheduler event-based sleeping and sent
//! message callbacks still require integration with the dispatcher.

use super::{with_current_queue, Msg};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{probe_user_write, write_user_object};
use alloc::vec::Vec;

const ERROR_INVALID_WINDOW_HANDLE: u32 = 1400;
const ERROR_INVALID_THREAD_ID: u32 = 1444;
const ERROR_NOT_ENOUGH_QUOTA: u32 = 1816;

struct MessageFilter {
    hwnd: u64,
    windows: Vec<u64>,
    min: u32,
    max: u32,
}

impl MessageFilter {
    fn capture(frame: &SyscallFrame) -> Result<Self, u32> {
        let mut filter = Self {
            hwnd: frame.args[1],
            windows: Vec::new(),
            min: frame.args[2] as u32,
            max: frame.args[3] as u32,
        };
        if !matches!(filter.hwnd, 0 | u64::MAX | 0xffff_ffff) {
            let manager = super::super::window::WINDOW_MANAGER.lock();
            let root = manager
                .get_window(filter.hwnd)
                .ok_or(ERROR_INVALID_WINDOW_HANDLE)?;
            filter.windows.try_reserve(1).map_err(|_| 8u32)?;
            filter.windows.push(root.hwnd);
            let mut index = 0;
            while index < filter.windows.len() {
                if let Some(window) = manager.get_window(filter.windows[index]) {
                    for child in &window.children {
                        if !filter.windows.contains(child) {
                            filter.windows.try_reserve(1).map_err(|_| 8u32)?;
                            filter.windows.push(*child);
                        }
                    }
                }
                index += 1;
            }
        }
        Ok(filter)
    }

    fn matches(&self, message: &Msg) -> bool {
        if message.message == 0x12 {
            return true;
        }
        let window_matches = match self.hwnd {
            0 => true,
            u64::MAX | 0xffff_ffff => message.hwnd == 0,
            _ => self.windows.contains(&message.hwnd),
        };
        window_matches
            && ((self.min == 0 && self.max == 0)
                || (self.min..=self.max).contains(&message.message))
    }
}

fn retrieve(frame: &SyscallFrame, blocking: bool) -> u64 {
    let failure = if blocking { u64::MAX } else { 0 };
    loop {
        // Probe before removal so a bad output pointer does not silently drop
        // a pending message or advance the queue's synchronous keyboard state.
        if probe_user_write(frame.args[0], core::mem::size_of::<Msg>()).is_err() {
            super::super::input_services::set_last_error(998);
            return failure;
        }
        let filter = match MessageFilter::capture(frame) {
            Ok(filter) => filter,
            Err(error) => {
                super::super::input_services::set_last_error(error);
                return failure;
            }
        };
        super::super::raw_input::drain();
        let remove = blocking || frame.args[4] & 1 != 0;
        let message = with_current_queue(|queue| {
            queue.peek_filtered(|message| filter.matches(message), remove)
        });
        if let Some(message) = message {
            if write_user_object(frame.args[0], &message).is_err() {
                super::super::input_services::set_last_error(998);
                return failure;
            }
            if remove && message.message == 0xff {
                super::super::raw_input::dequeued(message.lparam as u64);
            }
            return u64::from(!blocking || message.message != 0x12);
        }
        if !blocking {
            return 0;
        }
        // No queue/window lock may span a scheduling boundary. An empty queue
        // is a wait condition, not GetMessage's documented -1 error result.
        crate::ke::scheduler::yield_();
    }
}

pub extern "C" fn nt_user_get_message(frame: &SyscallFrame) -> u64 {
    retrieve(frame, true)
}

pub extern "C" fn nt_user_peek_message(frame: &SyscallFrame) -> u64 {
    retrieve(frame, false)
}

pub extern "C" fn nt_user_post_message(frame: &SyscallFrame) -> u64 {
    let hwnd = frame.args[0];
    if hwnd != 0
        && super::super::window::WINDOW_MANAGER
            .lock()
            .get_window(hwnd)
            .is_none()
    {
        super::super::input_services::set_last_error(ERROR_INVALID_WINDOW_HANDLE);
        return 0;
    }
    if super::post_message(
        hwnd,
        frame.args[1] as u32,
        frame.args[2],
        frame.args[3] as i64,
    ) {
        1
    } else {
        super::super::input_services::set_last_error(ERROR_NOT_ENOUGH_QUOTA);
        0
    }
}

pub extern "C" fn nt_user_post_thread_message(frame: &SyscallFrame) -> u64 {
    let thread = u64::from(frame.args[0] as u32);
    if thread == 0 {
        super::super::input_services::set_last_error(ERROR_INVALID_THREAD_ID);
        return 0;
    }
    if thread == super::current_thread_id() {
        // A thread posting to itself implicitly creates its USER queue.
        with_current_queue(|_| {});
    }
    let mut table = super::THREAD_QUEUES.lock();
    let Some(entry) = table.iter_mut().find(|entry| entry.thread_id == thread) else {
        super::super::input_services::set_last_error(ERROR_INVALID_THREAD_ID);
        return 0;
    };
    let message = Msg {
        hwnd: 0,
        message: frame.args[1] as u32,
        wparam: frame.args[2],
        lparam: frame.args[3] as i64,
        time: crate::ke::time::get_tick_count(),
        pt_x: 0,
        pt_y: 0,
    };
    if entry.queue.post_message(message) {
        1
    } else {
        super::super::input_services::set_last_error(ERROR_NOT_ENOUGH_QUOTA);
        0
    }
}

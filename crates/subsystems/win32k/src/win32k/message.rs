//! Message Queue and Event Dispatch for win32k.sys
//
//! Implements the USER message queue for window messages.
//! Messages are posted to a per-thread message queue and retrieved
//! by GetMessage/PeekMessage.

use super::input_services::keyboard::KeyboardSnapshot;

#[path = "message/services.rs"]
pub mod services;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// Maximum messages in a queue.
const MAX_QUEUE_MESSAGES: usize = 256;

/// Window message with parameters
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Msg {
    pub hwnd: u64,
    pub message: u32,
    pub wparam: u64,
    pub lparam: i64,
    pub time: u32,
    pub pt_x: i32,
    pub pt_y: i32,
}

/// A posted message has no input snapshot. Only class-driver messages may
/// update a queue's keyboard state when removed by GetMessage/PeekMessage.
#[derive(Clone, Copy)]
struct QueuedMessage {
    message: Msg,
    keyboard: Option<KeyboardSnapshot>,
}

/// Message queue for a thread
pub struct MessageQueue {
    messages: VecDeque<QueuedMessage>,
    keyboard: KeyboardSnapshot,
    quit_flag: AtomicBool,
    exit_code: AtomicI32,
    /// Paint rectangles pending WmPaint
    paint_rects: Vec<crate::libs::win32k::window::Rect>,
    /// Current message being processed
    current_msg: Option<Msg>,
    /// Event signaled when message is available
    has_messages: AtomicBool,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self {
            messages: VecDeque::with_capacity(MAX_QUEUE_MESSAGES),
            keyboard: KeyboardSnapshot::empty(),
            quit_flag: AtomicBool::new(false),
            exit_code: AtomicI32::new(0),
            paint_rects: Vec::new(),
            current_msg: None,
            has_messages: AtomicBool::new(false),
        }
    }

    /// Post a message to the queue.
    pub fn post_message(&mut self, msg: Msg) -> bool {
        self.enqueue(msg, None)
    }

    pub fn keyboard_state(&self) -> KeyboardSnapshot {
        self.keyboard
    }

    pub fn set_keyboard_state(&mut self, state: KeyboardSnapshot) {
        self.keyboard = state;
    }

    fn enqueue(&mut self, msg: Msg, keyboard: Option<KeyboardSnapshot>) -> bool {
        if self.messages.len() >= MAX_QUEUE_MESSAGES {
            // kprintln!("[msgq] PostMessage: queue full");
            return false;
        }
        self.messages.push_back(QueuedMessage {
            message: msg,
            keyboard,
        });
        // Signal that a message is available
        self.has_messages.store(true, Ordering::SeqCst);
        // kprintln!("[msgq] PostMessage: msg={:#x}, hwnd={:#x}, count={}",
        //             msg.message, msg.hwnd, self.messages.len());
        true
    }

    /// Post a quit message.
    pub fn post_quit(&mut self, exit_code: i32) {
        self.quit_flag.store(true, Ordering::SeqCst);
        self.exit_code.store(exit_code, Ordering::SeqCst);
        // Signal that a message (WmQuit) is available
        self.has_messages.store(true, Ordering::SeqCst);
    }

    /// Check if there are messages in the queue.
    pub fn peek_has_messages(&self) -> bool {
        !self.messages.is_empty() || self.quit_flag.load(Ordering::SeqCst)
    }

    /// Get message count.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Check if quit has been posted.
    pub fn is_quit(&self) -> bool {
        self.quit_flag.load(Ordering::SeqCst)
    }

    /// Get exit code.
    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::SeqCst)
    }

    /// Check if message event is signaled.
    pub fn is_message_available(&self) -> bool {
        self.has_messages.load(Ordering::SeqCst)
    }

    /// Clear the message available flag.
    pub fn clear_message_flag(&mut self) {
        if self.messages.is_empty() && !self.quit_flag.load(Ordering::SeqCst) {
            self.has_messages.store(false, Ordering::SeqCst);
        }
    }

    /// HWND(-1) restricts retrieval to thread messages. WM_QUIT always bypasses
    /// both the window and numeric filters, including an explicitly posted quit.
    fn matches_filter(msg: &Msg, hwnd: u64, min: u32, max: u32) -> bool {
        if msg.message == 0x12 {
            return true;
        }
        let window_matches = match hwnd {
            0 => true,
            u64::MAX | 0xffff_ffff => msg.hwnd == 0,
            _ => msg.hwnd == hwnd,
        };
        window_matches && ((min == 0 && max == 0) || (min..=max).contains(&msg.message))
    }

    /// Retrieve a matching message immediately. Waiting must happen outside
    /// the thread queue lock, so producers can enqueue the awaited message.
    pub fn get_message(&mut self, hwnd: u64, min: u32, max: u32, _timeout_ms: u32) -> Option<Msg> {
        self.peek_message(hwnd, min, max, true)
    }

    pub fn peek_message(&mut self, hwnd: u64, min: u32, max: u32, remove: bool) -> Option<Msg> {
        self.peek_filtered(
            |message| Self::matches_filter(message, hwnd, min, max),
            remove,
        )
    }

    /// Apply a previously captured window hierarchy without acquiring the
    /// window lock while holding the queue lock. Window-manager posts acquire
    /// those locks in the opposite order.
    pub(super) fn peek_filtered(
        &mut self,
        mut matches: impl FnMut(&Msg) -> bool,
        remove: bool,
    ) -> Option<Msg> {
        if let Some(index) = self
            .messages
            .iter()
            .position(|entry| matches(&entry.message))
        {
            let entry = self.messages[index];
            if remove {
                self.messages.remove(index);
                self.current_msg = Some(entry.message);
                if let Some(state) = entry.keyboard {
                    self.keyboard = state;
                }
                self.clear_message_flag();
            }
            return Some(entry.message);
        }
        // PostQuitMessage is a coalesced low-priority request. Posted messages
        // take precedence; PM_NOREMOVE leaves the quit request pending.
        if self.quit_flag.load(Ordering::SeqCst) {
            let message = Msg {
                hwnd: 0,
                message: 0x12,
                wparam: self.exit_code.load(Ordering::SeqCst) as u32 as u64,
                lparam: 0,
                time: crate::ke::time::get_tick_count(),
                pt_x: 0,
                pt_y: 0,
            };
            if remove {
                self.quit_flag.store(false, Ordering::SeqCst);
                self.current_msg = Some(message);
                self.clear_message_flag();
            }
            return Some(message);
        }
        None
    }

    /// Send a message directly (no queue). The kernel cannot invoke a
    /// user-mode WndProc directly; the window manager queues a
    /// `KeUserModeCallback`-style request and returns the callback
    /// placeholder result to the kernel-side caller.
    pub fn send_message(&self, msg: &Msg) -> i64 {
        let mut wm = crate::libs::win32k::window::WINDOW_MANAGER.lock();
        wm.send_message(msg.hwnd, msg.message, msg.wparam, msg.lparam)
    }

    /// Translate against this queue's synchronous state. The scan byte in
    /// lParam describes the physical key; it is never a Shift-state bitfield.
    pub fn translate_keyboard_message(&self, message: &Msg) -> Option<Msg> {
        if !matches!(message.message, 0x100 | 0x104) {
            return None;
        }
        let value = if message.wparam as u32 == 0xe7 {
            // KEYEVENTF_UNICODE delivers VK_PACKET with a UTF-16 scan unit.
            (message.lparam as u64 >> 16) as u16
        } else {
            super::input_services::layout::character(message.wparam as u32, &self.keyboard.bytes())?
        };
        Some(Msg {
            message: if message.message == 0x104 {
                0x106
            } else {
                0x102
            },
            wparam: u64::from(value),
            ..*message
        })
    }

    /// Translate using the calling thread's state at its last removed message.
    pub fn translate_message(message: &Msg) -> Option<Msg> {
        with_current_queue(|queue| queue.translate_keyboard_message(message))
    }

    /// Dispatch a message to a window procedure.
    pub fn dispatch_message(&self, msg: &Msg) -> i64 {
        if msg.hwnd == 0 {
            return 0;
        }
        let mut wm = crate::libs::win32k::window::WINDOW_MANAGER.lock();
        wm.send_message(msg.hwnd, msg.message, msg.wparam, msg.lparam)
    }

    /// Add a paint rectangle.
    pub fn add_paint_rect(&mut self, rect: crate::libs::win32k::window::Rect) {
        self.paint_rects.push(rect);
    }

    /// Get and clear paint rectangles.
    pub fn get_paint_rects(&mut self) -> Vec<crate::libs::win32k::window::Rect> {
        let rects: Vec<_> = core::mem::take(&mut self.paint_rects);
        rects
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.paint_rects.clear();
        self.current_msg = None;
        self.quit_flag.store(false, Ordering::SeqCst);
        self.exit_code.store(0, Ordering::SeqCst);
        self.has_messages.store(false, Ordering::SeqCst);
    }
}

impl Default for MessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Thread-Local Message Queue
// =============================================================================

/// One thread's message queue, keyed by the thread's client ID.
struct ThreadQueueEntry {
    thread_id: u64,
    queue: MessageQueue,
}

/// Bound queues created by cross-thread posts. Existing live queues must
/// never be evicted: only thread teardown discards their pending messages.
const THREAD_QUEUE_MAX: usize = 64;

/// Per-thread message queues, serialized by a spinlock. Every message-queue
/// operation holds the lock only for the duration of the operation, so
/// concurrent threads never share a queue.
static THREAD_QUEUES: crate::ke::sync::Spinlock<Vec<ThreadQueueEntry>> =
    crate::ke::sync::Spinlock::new(Vec::new());

/// Client ID of the current thread, or 0 when the thread subsystem has not
/// started yet (early callers then share one fallback queue keyed by 0).
fn current_thread_id() -> u64 {
    let ethread = crate::ps::thread::get_current_ethread();
    if ethread.is_null() {
        return 0;
    }
    unsafe { (*ethread).client_id.unique_thread }
}

/// Run `f` with exclusive access to the current thread's message queue,
/// creating the queue on first use. This replaces the old global-singleton
/// queue: every thread now gets its own queue, so two threads no longer
/// steal each other's messages.
pub fn with_current_queue<R>(f: impl FnOnce(&mut MessageQueue) -> R) -> R {
    let tid = current_thread_id();
    let mut table = THREAD_QUEUES.lock();
    let slot = match table.iter_mut().find(|e| e.thread_id == tid) {
        Some(entry) => entry,
        None => {
            table.push(ThreadQueueEntry {
                thread_id: tid,
                queue: MessageQueue::new(),
            });
            table.last_mut().unwrap()
        }
    };
    f(&mut slot.queue)
}

/// Post directly to the window owner's queue. Callers holding WINDOW_MANAGER
/// use this entry point with the captured TID, avoiding recursive USER locks.
pub fn post_thread_message(
    thread_id: u64,
    hwnd: u64,
    message: u32,
    wparam: u64,
    lparam: i64,
) -> bool {
    enqueue_thread_message(
        thread_id,
        Msg {
            hwnd,
            message,
            wparam,
            lparam,
            time: crate::ke::time::get_tick_count(),
            pt_x: 0,
            pt_y: 0,
        },
        None,
    )
}

/// Hardware transport only: callers capture the state at event generation,
/// rather than sampling newer hardware state when a thread eventually wakes.
pub(super) fn post_keyboard_input(thread_id: u64, message: Msg, state: KeyboardSnapshot) -> bool {
    enqueue_thread_message(thread_id, message, Some(state))
}

fn enqueue_thread_message(
    thread_id: u64,
    message: Msg,
    keyboard: Option<KeyboardSnapshot>,
) -> bool {
    let mut table = THREAD_QUEUES.lock();
    let index = if let Some(index) = table.iter().position(|entry| entry.thread_id == thread_id) {
        index
    } else {
        if table.len() >= THREAD_QUEUE_MAX || table.try_reserve(1).is_err() {
            return false;
        }
        table.push(ThreadQueueEntry {
            thread_id,
            queue: MessageQueue::new(),
        });
        table.len() - 1
    };
    table[index].queue.enqueue(message, keyboard)
}

pub fn release_thread(thread_id: u64) {
    THREAD_QUEUES
        .lock()
        .retain(|entry| entry.thread_id != thread_id);
}

pub fn remove_raw_input(thread_id: u64, handle: u64) {
    if let Some(entry) = THREAD_QUEUES
        .lock()
        .iter_mut()
        .find(|entry| entry.thread_id == thread_id)
    {
        entry
            .queue
            .messages
            .retain(|entry| entry.message.message != 0xff || entry.message.lparam as u64 != handle);
    }
}

/// Resolve HWND ownership before posting; HWND zero targets the calling thread.
pub fn post_message(hwnd: u64, message: u32, wparam: u64, lparam: i64) -> bool {
    let thread = if hwnd == 0 {
        current_thread_id()
    } else {
        let manager = super::window::WINDOW_MANAGER.lock();
        let Some(window) = manager.get_window(hwnd) else {
            return false;
        };
        window.thread_id
    };
    post_thread_message(thread, hwnd, message, wparam, lparam)
}

/// Post a quit message.
pub fn post_quit_message(exit_code: i32) {
    with_current_queue(|queue| queue.post_quit(exit_code))
}

/// Check if quit has been posted.
pub fn is_quit_posted() -> bool {
    with_current_queue(|queue| queue.is_quit())
}

/// Get exit code.
pub fn get_exit_code() -> i32 {
    with_current_queue(|queue| queue.exit_code())
}

// =============================================================================
// Default Window Procedure
// =============================================================================

/// Default window procedure handler.
/// Processes messages that most windows handle by default.
pub fn default_wndproc(hwnd: u64, msg: u32, wparam: u64, lparam: i64) -> i64 {
    let _ = (hwnd, wparam, lparam);

    match msg {
        0x00ff => {
            super::raw_input::release(lparam as u64);
            0
        }
        0x0001 => {
            // WmCreate
            // kprintln!("[wndproc] WmCreate hwnd={:#x}", hwnd);
            0 // Continue creation
        }
        0x0002 => {
            // WmDestroy
            // kprintln!("[wndproc] WmDestroy hwnd={:#x}", hwnd);
            post_quit_message(0);
            0
        }
        0x000F => {
            // WmPaint
            // kprintln!("[wndproc] WmPaint hwnd={:#x}", hwnd);
            // invalidate_rect was already called, so just validate
            0
        }
        0x0014 => {
            // WmErasebkgnd
            // Return 1 to indicate background will be erased
            1
        }
        0x0005 => {
            // WmSize
            let width = lparam as u32;
            let _ = &width;
            let _ = &width;
            let height = (lparam >> 32) as u32;
            let _ = &height;
            let _ = &height;
            // kprintln!("[wndproc] WmSize hwnd={:#x} {}x{}", hwnd, width, height);
            0
        }
        0x0003 => {
            // WmMove
            let x = lparam as u32;
            let _ = &x;
            let _ = &x;
            let y = (lparam >> 32) as u32;
            let _ = &y;
            let _ = &y;
            // kprintln!("[wndproc] WmMove hwnd={:#x} ({},{})", hwnd, x, y);
            0
        }
        0x0010 => {
            // WmClose
            // kprintln!("[wndproc] WmClose hwnd={:#x}", hwnd);
            // Default: destroy the window
            crate::libs::win32k::window::destroy_window_internal(hwnd);
            0
        }
        0x0100 => {
            // WmKeydown
            // kprintln!("[wndproc] WmKeydown hwnd={:#x} vk={:#x}", hwnd, wparam);
            0
        }
        0x0101 => {
            // WmKeyup
            // kprintln!("[wndproc] WmKeyup hwnd={:#x} vk={:#x}", hwnd, wparam);
            0
        }
        0x0200 => {
            // WmMousemove
            let x = (lparam as i32) & 0xFFFF;
            let _ = &x;
            let _ = &x;
            let y = ((lparam >> 16) as i32) & 0xFFFF;
            let _ = &y;
            let _ = &y;
            // kprintln!("[wndproc] WmMousemove hwnd={:#x} ({},{})", hwnd, x, y);
            0
        }
        0x0201 => {
            // WmLbuttondown
            // kprintln!("[wndproc] WmLbuttondown hwnd={:#x}", hwnd);
            0
        }
        0x0202 => {
            // WmLbuttonup
            // kprintln!("[wndproc] WmLbuttonup hwnd={:#x}", hwnd);
            0
        }
        _ => {
            // Return 0 for unhandled messages (DefWindowProc behavior)
            // kprintln!("[wndproc] unhandled msg={:#x} hwnd={:#x}", msg, hwnd);
            0
        }
    }
}

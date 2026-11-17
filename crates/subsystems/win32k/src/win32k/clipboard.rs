//! Clipboard Management
//
//! Implements clipboard operations for win32k.sys
//! Reference: Windows SDK, ReactOS win32ss/user

#[path = "clipboard/services.rs"]
pub mod services;

use crate::ke::sync::Spinlock;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Clipboard format types
pub const CF_TEXT: u32 = 1;
pub const CF_BITMAP: u32 = 2;
pub const CF_METAFILEPICT: u32 = 3;
pub const CF_SYLK: u32 = 4;
pub const CF_DIF: u32 = 5;
pub const CF_TIFF: u32 = 6;
pub const CF_OEMTEXT: u32 = 7;
pub const CF_DIB: u32 = 8;
pub const CF_PALETTE: u32 = 9;
pub const CF_UNICODETEXT: u32 = 13;
pub const CF_ENHMETAFILE: u32 = 14;

/// Maximum size of a single clipboard object, per format.
const MAX_CLIPBOARD_DATA: usize = 1024 * 1024; // 1MB

/// Maximum number of distinct formats tracked simultaneously. A
/// poster can offer several representations (e.g. CF_UNICODETEXT and
/// CF_DIB) of the same user object at once.
const MAX_CLIPBOARD_FORMATS: usize = 8;

/// Inner clipboard data protected by a lock: one owned byte buffer
/// per format. Readers get an owned copy, so no one aliases
/// lock-protected storage and a later update can never mutate a
/// buffer someone still holds.
pub struct ClipboardDataInner {
    /// (format, bytes) pairs in insertion order.
    entries: Vec<(u32, Vec<u8>)>,
    opener: u64,
    modified: bool,
    listeners: Vec<u64>,
    viewer: u64,
}

impl ClipboardDataInner {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            opener: 0,
            modified: false,
            listeners: Vec::new(),
            viewer: 0,
        }
    }
}

impl Default for ClipboardDataInner {
    fn default() -> Self {
        Self::new()
    }
}

/// Clipboard state (protected by a spinlock)
pub struct ClipboardState {
    /// Whether clipboard is currently open
    pub is_open: AtomicBool,
    /// owner window handle
    pub owner: AtomicU64,
    /// Sequence number for clipboard changes
    pub sequence: AtomicU64,
    /// Current clipboard data
    pub data: Spinlock<ClipboardDataInner>,
    /// Whether clipboard has data
    pub has_data: AtomicBool,
}

impl ClipboardState {
    pub const fn new() -> Self {
        Self {
            is_open: AtomicBool::new(false),
            owner: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            data: Spinlock::new(ClipboardDataInner::new()),
            has_data: AtomicBool::new(false),
        }
    }
}

impl Default for ClipboardState {
    fn default() -> Self {
        Self::new()
    }
}

/// Global clipboard state
static CLIPBOARD: ClipboardState = ClipboardState::new();

#[cfg(test)]
pub(crate) static TEST_LOCK: Spinlock<()> = Spinlock::new(());

/// Open the clipboard for a window
pub fn open_clipboard(owner: u64) -> bool {
    let mut inner = CLIPBOARD.data.lock();
    if CLIPBOARD.is_open.load(Ordering::Acquire) { return false; }
    inner.opener = owner;
    inner.modified = false;
    CLIPBOARD.is_open.store(true, Ordering::Release);
    true
}

/// Notify once on close if data changed. A read-only open/close must neither
/// change persistent ownership nor manufacture a new clipboard sequence.
pub fn close_clipboard() -> bool {
    let (listeners, viewer) = {
        let mut inner = CLIPBOARD.data.lock();
        if !CLIPBOARD.is_open.load(Ordering::Acquire) { return false; }
        let mut listeners = Vec::new();
        if inner.modified {
            if listeners.try_reserve_exact(inner.listeners.len()).is_err() { return false; }
            listeners.extend_from_slice(&inner.listeners);
        }
        let viewer = if inner.modified { inner.viewer } else { 0 };
        inner.opener = 0;
        inner.modified = false;
        CLIPBOARD.is_open.store(false, Ordering::Release);
        (listeners, viewer)
    };
    for hwnd in listeners {
        crate::libs::win32k::message::post_message(hwnd, 0x031d, 0, 0);
    }
    if viewer != 0 { crate::libs::win32k::message::post_message(viewer, 0x0308, 0, 0); }
    true
}

pub fn get_open_window() -> u64 {
    let inner = CLIPBOARD.data.lock();
    if is_open() { inner.opener } else { 0 }
}

/// Called while the window manager lock is held, matching listener registration
/// lock order. Never acquire the window manager from inside the clipboard lock.
pub(super) fn detach_window(hwnd: u64) {
    let mut inner = CLIPBOARD.data.lock();
    inner.listeners.retain(|listener| *listener != hwnd);
    if inner.viewer == hwnd { inner.viewer = 0; }
    if inner.opener == hwnd { inner.opener = 0; }
    if get_owner() == hwnd { CLIPBOARD.owner.store(0, Ordering::Release); }
}

/// Check if clipboard is open
pub fn is_open() -> bool {
    CLIPBOARD.is_open.load(Ordering::SeqCst)
}

/// Get the clipboard owner
pub fn get_owner() -> u64 {
    CLIPBOARD.owner.load(Ordering::SeqCst)
}

/// Get the current sequence number
pub fn get_sequence_number() -> u64 {
    CLIPBOARD.sequence.load(Ordering::SeqCst)
}

/// Set clipboard data for one format.
///
/// Replaces any previous object registered for the same format and
/// preserves the other formats, so one owner can offer several
/// representations of the same user object. An empty object or one
/// larger than MAX_CLIPBOARD_DATA is rejected.
pub fn set_clipboard_data(format: u32, data: &[u8]) -> bool {
    if format == 0 || data.is_empty() || data.len() > MAX_CLIPBOARD_DATA { return false; }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(data.len()).is_err() { return false; }
    bytes.extend_from_slice(data);
    let mut inner = CLIPBOARD.data.lock();
    if !is_open() { return false; }
    match inner.entries.iter_mut().find(|(f, _)| *f == format) {
        Some((_, existing)) => *existing = bytes,
        None => {
            if inner.entries.len() >= MAX_CLIPBOARD_FORMATS || inner.entries.try_reserve(1).is_err() { return false; }
            inner.entries.push((format, bytes));
        }
    }
    inner.modified = true;
    CLIPBOARD.has_data.store(true, Ordering::Release);
    CLIPBOARD.sequence.fetch_add(1, Ordering::AcqRel);
    true
}

/// Get a copy of the clipboard object for 'format'.
///
/// 'format == 0' selects the first (preferred) format, matching the
/// previous single-format behaviour. The returned buffer is an owned
/// copy: the caller never aliases the lock-protected storage and a
/// subsequent clipboard update cannot change it under them.
pub fn get_clipboard_data(format: u32) -> Option<Vec<u8>> {
    if !CLIPBOARD.has_data.load(Ordering::SeqCst) {
        return None;
    }

    let inner = CLIPBOARD.data.lock();
    if format == 0 {
        return inner.entries.first().map(|(_, bytes)| bytes.clone());
    }
    inner
        .entries
        .iter()
        .find(|(f, _)| *f == format)
        .map(|(_, bytes)| bytes.clone())
}

/// Get the preferred (first registered) clipboard format, or 0 if the
/// clipboard is empty.
pub fn get_clipboard_format() -> u32 {
    if !CLIPBOARD.has_data.load(Ordering::SeqCst) {
        return 0;
    }
    let inner = CLIPBOARD.data.lock();
    inner.entries.first().map(|(f, _)| *f).unwrap_or(0)
}

/// Snapshot available formats in their enumeration order. All query services
/// use this lock-protected data rather than a separate cached availability bit.
pub fn formats() -> Vec<u32> {
    CLIPBOARD.data.lock().entries.iter().map(|(format, _)| *format).collect()
}

/// Check if a format is available ('0' means "any format").
pub fn is_format_available(format: u32) -> bool {
    if !CLIPBOARD.has_data.load(Ordering::SeqCst) {
        return false;
    }

    let inner = CLIPBOARD.data.lock();
    format == 0 || inner.entries.iter().any(|(f, _)| *f == format)
}

/// Empty the clipboard
pub fn empty_clipboard() -> bool {
    let mut inner = CLIPBOARD.data.lock();
    if !is_open() { return false; }
    inner.entries.clear();
    inner.modified = true;
    CLIPBOARD.owner.store(inner.opener, Ordering::Release);
    CLIPBOARD.has_data.store(false, Ordering::Release);
    CLIPBOARD.sequence.fetch_add(1, Ordering::AcqRel);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// One combined test: the clipboard is a global static, so parallel
    /// test threads would interfere with each other.
    #[test]
    fn clipboard_multi_format_lifecycle() {
        let _guard = TEST_LOCK.lock();
        if is_open() {
            close_clipboard();
        }

        assert!(open_clipboard(0x1111));
        assert!(empty_clipboard());
        assert_eq!(get_owner(), 0x1111);

        // Two formats coexist; format 0 reads the preferred one.
        assert!(set_clipboard_data(CF_UNICODETEXT, b"hello"));
        assert!(set_clipboard_data(CF_DIB, &[1, 2, 3]));
        assert_eq!(
            get_clipboard_data(CF_UNICODETEXT).as_deref(),
            Some(&b"hello"[..])
        );
        assert_eq!(
            get_clipboard_data(CF_DIB).as_deref(),
            Some(&[1u8, 2, 3][..])
        );
        assert_eq!(get_clipboard_data(0).as_deref(), Some(&b"hello"[..]));
        assert_eq!(get_clipboard_format(), CF_UNICODETEXT);
        assert!(is_format_available(CF_DIB));
        assert!(is_format_available(0));
        assert!(!is_format_available(CF_TIFF));

        // Replacing one format preserves the other.
        assert!(set_clipboard_data(CF_UNICODETEXT, b"world!"));
        assert_eq!(
            get_clipboard_data(CF_UNICODETEXT).as_deref(),
            Some(&b"world!"[..])
        );
        assert_eq!(
            get_clipboard_data(CF_DIB).as_deref(),
            Some(&[1u8, 2, 3][..])
        );

        // Over-sized and empty objects are rejected.
        let big = vec![0u8; MAX_CLIPBOARD_DATA + 1];
        assert!(!set_clipboard_data(CF_TEXT, &big));
        assert!(!set_clipboard_data(CF_TEXT, b""));

        // The format table is bounded.
        for format in [CF_TEXT, CF_OEMTEXT, CF_SYLK, CF_DIF, CF_TIFF, CF_PALETTE] {
            assert!(set_clipboard_data(format, b"x"));
        }
        assert!(!set_clipboard_data(CF_BITMAP, b"x"));

        // Emptying clears every format and bumps the sequence.
        let seq = get_sequence_number();
        assert!(empty_clipboard());
        assert!(get_clipboard_data(0).is_none());
        assert!(!is_format_available(0));
        assert_eq!(get_clipboard_format(), 0);
        assert_eq!(get_sequence_number(), seq + 1);

        // Closing publishes notifications and disables updates.
        let seq = get_sequence_number();
        assert!(close_clipboard());
        assert_eq!(get_sequence_number(), seq);
        assert!(!set_clipboard_data(CF_TEXT, b"nope"));
        assert!(!empty_clipboard());

        if open_clipboard(0x2222) {
            close_clipboard();
        }
    }
}

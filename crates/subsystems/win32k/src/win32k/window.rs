//! Window Management for win32k.sys
//
//! Implements window objects, window classes, and the USER subsystem.
//! This module provides the core windowing infrastructure for GDI.
//
//! ## Window Architecture
//
//! In NT win32k, windows are managed through:
//! - Window objects: Describe each window's properties
//! - Window classes: Define window behavior via WndProc
//! - Window messages: Communication between windows and the system
//! - Desktop/WindowStation: Organize windows in a hierarchy

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[path = "window/services.rs"]
pub mod services;
#[path = "window/placement.rs"]
pub mod placement;
#[path = "window/attributes.rs"]
pub mod attributes;
#[path = "window/input_context.rs"]
pub(crate) mod input_context;
#[path = "window/coordinates.rs"]
pub mod coordinates;
#[path = "window/classes.rs"]
pub mod class_services;

#[path = "window/native_state.rs"]
pub mod native_state;
#[path = "window/text.rs"]
pub mod text;

/// Maximum number of windows in the system.
pub const MAX_WINDOWS: usize = 4096;

/// Maximum number of window classes.
pub const MAX_WINDOW_CLASSES: usize = 256;

/// Window style flags as a bitmask wrapper.
/// These are bitmask values matching Windows WS_* constants.
/// Use the `bits()` and `from_bits()` methods for value access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct WindowStyle(u32);

impl WindowStyle {
    pub const BORDER: u32 = 0x00800000;
    pub const CAPTION: u32 = 0x00C00000;
    pub const CHILD: u32 = 0x40000000;
    pub const CLIP_CHILDREN: u32 = 0x02000000;
    pub const CLIP_SIBLINGS: u32 = 0x04000000;
    pub const DISABLED: u32 = 0x08000000;
    pub const GROUP: u32 = 0x00020000;
    pub const HORIZONTAL_SCROLL: u32 = 0x00100000;
    pub const VERTICAL_SCROLL: u32 = 0x00200000;
    pub const ICONIC: u32 = 0x20000000;
    pub const MAXIMIZE: u32 = 0x01000000;
    pub const MAXIMIZE_BOX: u32 = 0x00010000;
    pub const MINIMIZE: u32 = 0x20000000;
    pub const POPUP: u32 = 0x80000000;
    pub const SIZE_BOX: u32 = 0x00040000;
    pub const SYS_MENU: u32 = 0x00080000;
    pub const TAB_STOP: u32 = 0x00010000;
    pub const VISIBLE: u32 = 0x10000000;

    pub const fn new(bits: u32) -> Self {
        Self(bits)
    }
    pub const fn bits(&self) -> u32 {
        self.0
    }
    pub fn set(&mut self, flag: u32) {
        self.0 |= flag;
    }
    pub fn contains(&self, flag: u32) -> bool {
        (self.0 & flag) == flag
    }
}

/// Extended window style flags as a bitmask wrapper.
/// Matches Windows WS_EX_* constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct WindowStyleEx(u32);

impl WindowStyleEx {
    pub const APP_WINDOW: u32 = 0x00040000;
    pub const CLIENT_EDGE: u32 = 0x00000200;
    pub const COMBINE_RECT: u32 = 0x00000020;
    pub const CONTEXT_HELP: u32 = 0x00000400;
    pub const DLG_MODAL_FRAME: u32 = 0x00000001;
    pub const LEFT: u32 = 0x00000000;
    pub const RIGHT: u32 = 0x00001000;
    pub const LTR_READING: u32 = 0x00000000;
    pub const NO_ACTIVATE: u32 = 0x08000000;
    pub const NO_INHERIT_LAYOUT: u32 = 0x00100000;
    pub const OVERLAPPED_WINDOW: u32 = 0x00000300;
    pub const RIGHT_SCROLLBAR: u32 = 0x00000000;
    pub const RTL_READING: u32 = 0x00002000;
    pub const STATIC_EDGE: u32 = 0x00020000;
    pub const TOOL_WINDOW: u32 = 0x00000080;
    pub const TOPMOST: u32 = 0x00000008;
    pub const TRANSPARENT: u32 = 0x00000020;
    pub const WINDOW_EDGE: u32 = 0x00000100;

    pub const fn new(bits: u32) -> Self {
        Self(bits)
    }
    pub const fn bits(&self) -> u32 {
        self.0
    }
    pub fn set(&mut self, flag: u32) {
        self.0 |= flag;
    }
    pub fn contains(&self, flag: u32) -> bool {
        (self.0 & flag) == flag
    }
}

/// Window message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum WindowMessage {
    WmCreate = 0x0001,
    WmDestroy = 0x0002,
    WmMove = 0x0003,
    WmSize = 0x0005,
    WmActivate = 0x0006,
    WmSetfocus = 0x0007,
    WmKillfocus = 0x0008,
    WmEnable = 0x000A,
    WmSetredraw = 0x000B,
    WmSettext = 0x000C,
    WmGettext = 0x000D,
    WmGettextlength = 0x000E,
    WmPaint = 0x000F,
    WmClose = 0x0010,
    WmQueryendsession = 0x0011,
    WmQuit = 0x0012,
    WmQueryopen = 0x0013,
    WmErasebkgnd = 0x0014,
    WmSyscolorchange = 0x0015,
    WmShowwindow = 0x0018,
    WmWininichange = 0x001A,
    WmSetcursor = 0x0020,
    WmMouseactivate = 0x0021,
    WmChildactivate = 0x0022,
    WmQueuesync = 0x0023,
    WmGetminmaxinfo = 0x0024,
    WmPainticon = 0x0026,
    WmIconerasebkgnd = 0x0027,
    WmSpoolerStatus = 0x002A,
    WmDrawitem = 0x002B,
    WmMeasureitem = 0x002C,
    WmDeleteitem = 0x002D,
    WmVkeytoitem = 0x002E,
    WmChartoitem = 0x002F,
    WmSetfont = 0x0030,
    WmGetfont = 0x0031,
    WmSethotkey = 0x0032,
    WmGethotkey = 0x0033,
    WmQuerydragicon = 0x0037,
    WmCompareitem = 0x0039,
    WmGetobject = 0x003D,
    WmCompacting = 0x0041,
    WmWindowposchanging = 0x0046,
    WmWindowposchanged = 0x0047,
    WmCopydata = 0x004A,
    WmCanceljournal = 0x004B,
    WmCancelmode = 0x001F,
    WmNotify = 0x004E,
    WmInputlangchangerequest = 0x0050,
    WmInputlangchange = 0x0051,
    WmTcard = 0x0052,
    WmHelp = 0x0053,
    WmUserchanged = 0x0054,
    WmNotifyformat = 0x0055,
    WmContextmenu = 0x007B,
    WmStylechanging = 0x007C,
    WmStylechanged = 0x007D,
    WmDisplaychange = 0x007E,
    WmGeticon = 0x007F,
    WmSeticon = 0x0080,
    WmNccreate = 0x0081,
    WmNcdestroy = 0x0082,
    WmNccalcsize = 0x0083,
    WmNchittest = 0x0084,
    WmNcpaint = 0x0085,
    WmNcactivate = 0x0086,
    WmGetdlgcode = 0x0087,
    WmSyncpaint = 0x0088,
    WmNcmousemove = 0x00A0,
    WmNclbuttondown = 0x00A1,
    WmNclbuttonup = 0x00A2,
    WmNclbuttondblclk = 0x00A3,
    WmNcrbuttondown = 0x00A4,
    WmNcrbuttonup = 0x00A5,
    WmNcrbuttondblclk = 0x00A6,
    WmNcmbuttondown = 0x00A7,
    WmNcmbuttonup = 0x00A8,
    WmNcmbuttondblclk = 0x00A9,
    WmKeydown = 0x0100,
    WmKeyup = 0x0101,
    WmChar = 0x0102,
    WmDeadchar = 0x0103,
    WmSyskeydown = 0x0104,
    WmSyskeyup = 0x0105,
    WmSyschar = 0x0106,
    WmSysdeadchar = 0x0107,
    WmKeylast = 0x0108,
    WmInitdialog = 0x0110,
    WmCommand = 0x0111,
    WmSyscommand = 0x0112,
    WmTimer = 0x0113,
    WmHscroll = 0x0114,
    WmVscroll = 0x0115,
    WmInitmenu = 0x0116,
    WmInitmenupopup = 0x0117,
    WmMenuselect = 0x011F,
    WmMenuchar = 0x0120,
    WmEnteridle = 0x0121,
    WmMenurbuttonup = 0x0122,
    WmMenudrag = 0x0123,
    WmMenugetobject = 0x0124,
    WmUninitmenupopup = 0x0125,
    WmMenucommand = 0x0126,
    WmChangeuistate = 0x0127,
    WmUpdateuistate = 0x0128,
    WmQueryuistate = 0x0129,
    WmCtlcolormsgbox = 0x0132,
    WmCtlcoloredit = 0x0133,
    WmCtlcolorlistbox = 0x0134,
    WmCtlcolorbtn = 0x0135,
    WmCtlcolordlg = 0x0136,
    WmCtlcolorscrollbar = 0x0137,
    WmCtlcolorstatic = 0x0138,
    WmMousemove = 0x0200,
    WmLbuttondown = 0x0201,
    WmLbuttonup = 0x0202,
    WmLbuttondblclk = 0x0203,
    WmRbuttondown = 0x0204,
    WmRbuttonup = 0x0205,
    WmRbuttondblclk = 0x0206,
    WmMbuttondown = 0x0207,
    WmMbuttonup = 0x0208,
    WmMbuttondblclk = 0x0209,
    WmMousewheel = 0x020A,
    WmXbuttondown = 0x020B,
    WmXbuttonup = 0x020C,
    WmXbuttondblclk = 0x020D,
    WmMousehover = 0x02A1,
    WmMouseleave = 0x02A3,
    WmMousehwheel = 0x020E,
}

/// Window rectangle - uses shared Rect from objects.rs
pub use crate::libs::win32k::objects::Rect;

/// Window class structure
#[derive(Debug, Clone)]
pub struct WindowClass {
    pub name: [u16; 64],
    pub instance: u64,
    pub style: u32,
    pub wndproc: u64,
    pub cls_extra: i32,
    pub wnd_extra: i32,
    pub h_icon: u64,
    pub h_cursor: u64,
    pub h_brbackground: u64,
    pub menu_name: u64,
    pub atom: u16,
    pub small_icon: u64,
    pub extra: Vec<u8>,
}

/// Window object
#[derive(Debug, Clone)]
pub struct WindowObject {
    pub hwnd: u64,
    /// Stable class identity, independent of mutable GCLP_HMODULE.
    pub class_atom: u16,
    pub thread_id: u64,
    pub process_id: u64,
    pub input_context: u64,
    pub ime_level: u32,
    pub class_name: [u16; 64],
    pub title: Vec<u16>,
    pub style: u32,
    pub ex_style: u32,
    pub rect: Rect,
    pub client_rect: Rect,
    pub parent: Option<u64>,
    pub owner: Option<u64>,
    pub children: Vec<u64>,
    pub visible: bool,
    pub enabled: bool,
    pub foreground: bool,
    pub dirty: bool,
    pub dirty_rects: Vec<Rect>,
    pub wndproc: u64,
    pub user_data: u64,
    pub control_id: u32,
    /// Native USER properties are keyed by ATOM, not caller string addresses.
    /// Entries retain insertion order for BuildPropList enumeration.
    pub properties: Vec<(u16, u64)>,
    pub instance: u64,
    pub extra: Vec<u8>,
    pub normal_rect: Rect,
    pub minimize_point: (i32, i32),
    pub maximize_point: (i32, i32),
    pub placement_flags: u32,
    pub native: native_state::NativeWindowState,
}

impl WindowObject {
    pub fn new(hwnd: u64) -> Self {
        Self {
            hwnd,
            class_atom: 0,
            thread_id: 0,
            process_id: 0,
            input_context: 0,
            ime_level: 0,
            class_name: [0; 64],
            title: Vec::new(),
            style: 0,
            ex_style: 0,
            rect: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            client_rect: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            parent: None,
            owner: None,
            children: Vec::new(),
            visible: false,
            enabled: true,
            foreground: false,
            dirty: false,
            dirty_rects: Vec::new(),
            wndproc: 0,
            user_data: 0,
            control_id: 0,
            properties: Vec::new(),
            instance: 0,
            extra: Vec::new(),
            normal_rect: Rect::default(),
            minimize_point: (-1, -1),
            maximize_point: (-1, -1),
            placement_flags: 0,
            native: native_state::NativeWindowState::default(),
        }
    }

    pub fn set_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.rect.left = x;
        self.rect.top = y;
        self.rect.right = x.saturating_add(width.max(0));
        self.rect.bottom = y.saturating_add(height.max(0));
        self.dirty = true;
        self.update_client_rect();
    }

    pub fn update_client_rect(&mut self) {
        let caption = if self.style & WindowStyle::CAPTION != 0 { 20 } else { 0 };
        let border = if self.style & WindowStyle::BORDER != 0 { 2 } else { 0 };
        let width = self.rect.right.saturating_sub(self.rect.left).max(0);
        let height = self.rect.bottom.saturating_sub(self.rect.top).max(0);
        self.client_rect = Rect {
            left: border.min(width),
            top: (caption + border).min(height),
            right: width.saturating_sub(border).max(border.min(width)),
            bottom: height.saturating_sub(border).max((caption + border).min(height)),
        };
    }
}

/// Window manager state
pub struct WindowManager {
    windows: Vec<Option<WindowObject>>,
    contexts: Vec<input_context::InputContext>,
    next_context: u32,
    ime_disabled_threads: Vec<(u64, u64)>,
    ime_disabled_processes: Vec<u64>,
    z_order: Vec<u64>,
    dpi_aware_processes: Vec<u64>,
    shell_window: u64,
    shell_list_view: u64,
    /// Sorted hwnd -> slot-index map so HWND lookup is O(log n) in the
    /// number of live windows instead of a linear scan of the table.
    hwnd_index: BTreeMap<u64, usize>,
    next_hwnd: AtomicU64,
    window_count: AtomicUsize,
    pub classes: Vec<Option<WindowClass>>,
    pub next_class_atom: AtomicU64,
    foreground_window: AtomicU64,
    active_window: AtomicU64,
    capture_window: AtomicU64,
    focus_window: AtomicU64,
    pub desktop_window: AtomicU64,
}

/// Window procedures registered from user mode are stored as raw `u64`
/// addresses in `WindowObject::wndproc`. NT win32k does not call user-mode
/// code directly from kernel mode. Callbacks are represented as pending
/// requests that the owning thread delivers through the user-mode callback
/// pump and completes by token.
pub static WNDPROC_DELIVERY_MISSES: AtomicU64 = AtomicU64::new(0);

/// One user-mode window-procedure callback request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WndProcRequest {
    pub token: u64,
    pub wndproc: u64,
    pub hwnd: u64,
    pub message: u32,
    pub wparam: u64,
    pub lparam: i64,
}

static PENDING_WNDPROC_REQUESTS: Spinlock<Vec<WndProcRequest>> = Spinlock::new(Vec::new());
static NEXT_WNDPROC_REQUEST_TOKEN: AtomicU64 = AtomicU64::new(1);

fn call_wnd_proc(wndproc: u64, hwnd: u64, msg: u32, wparam: u64, lparam: i64) -> i64 {
    if wndproc == 0 {
        return 0;
    }

    // NT win32k does not call user WndProcs directly from kernel mode.
    // Queue the request and let the owning thread deliver it through the
    // user-mode callback pump, mirroring KeUserModeCallback semantics.
    let token = NEXT_WNDPROC_REQUEST_TOKEN.fetch_add(1, Ordering::Relaxed);
    PENDING_WNDPROC_REQUESTS.lock().push(WndProcRequest {
        token,
        wndproc,
        hwnd,
        message: msg,
        wparam,
        lparam,
    });
    0
}

/// Remove one pending WndProc request for delivery in user mode.
pub fn take_pending_wndproc_request() -> Option<WndProcRequest> {
    PENDING_WNDPROC_REQUESTS.lock().pop()
}

/// Complete a WndProc request and return its caller-visible result.
pub fn complete_pending_wndproc_request(token: u64, result: i64) -> Option<i64> {
    let mut requests = PENDING_WNDPROC_REQUESTS.lock();
    let index = requests.iter().position(|request| request.token == token)?;
    requests.remove(index);
    Some(result)
}

/// Number of WndProc requests awaiting user-mode delivery.
pub fn pending_wndproc_request_count() -> usize {
    PENDING_WNDPROC_REQUESTS.lock().len()
}

impl WindowManager {
    pub const fn new() -> Self {
        Self {
            windows: Vec::new(),
            contexts: Vec::new(),
            next_context: 0x5000_0000,
            ime_disabled_threads: Vec::new(),
            ime_disabled_processes: Vec::new(),
            z_order: Vec::new(),
            dpi_aware_processes: Vec::new(),
            shell_window: 0,
            shell_list_view: 0,
            hwnd_index: BTreeMap::new(),
            next_hwnd: AtomicU64::new(1), // HWND 0 is invalid
            window_count: AtomicUsize::new(0),
            classes: Vec::new(),
            next_class_atom: AtomicU64::new(1),
            foreground_window: AtomicU64::new(0),
            active_window: AtomicU64::new(0),
            capture_window: AtomicU64::new(0),
            focus_window: AtomicU64::new(0),
            desktop_window: AtomicU64::new(0),
        }
    }

    /// Allocate a new HWND.
    fn alloc_hwnd(&self) -> Option<u64> {
        let hwnd = self.next_hwnd.fetch_add(1, Ordering::SeqCst);
        let _ = &hwnd;
        let _ = &hwnd;
        if hwnd == 0 {
            None // Overflow
        } else {
            Some(hwnd)
        }
    }

    /// create a new window.
    pub fn create_window(
        &mut self,
        class_name: &[u16],
        title: &[u16],
        style: u32,
        ex_style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Option<u64>,
        wndproc: u64,
        control_id: u32,
    ) -> Option<u64> {
        if parent.is_some_and(|handle| self.get_window(handle).is_none()) {
            return None;
        }
        if style & WindowStyle::CHILD != 0 && parent.is_none() {
            return None;
        }
        if self.window_count.load(Ordering::Relaxed) >= MAX_WINDOWS {
            return None;
        }
        self.z_order.try_reserve(1).ok()?;
        let hwnd = self.alloc_hwnd()?;
        let _ = &hwnd;
        let _ = &hwnd;
        let mut window = WindowObject::new(hwnd);
        let (process, thread) = input_context::caller_identity();
        window.process_id = process;
        window.thread_id = thread;
        window.input_context = self.default_input_context(process, thread).unwrap_or(0);

        // Copy class name
        for (i, &c) in class_name.iter().take(63).enumerate() {
            window.class_name[i] = c;
        }

        window.title.try_reserve_exact(title.len()).ok()?;
        window.title.extend_from_slice(title);

        window.style = style;
        window.ex_style = ex_style;
        window.set_rect(x, y, width, height);
        window.normal_rect = window.rect;
        if let Some(class) = self.classes.iter().flatten().find(|class| class_services::same_class_name(&class.name, &window.class_name)) {
            window.extra.try_reserve_exact(class.wnd_extra as usize).ok()?;
            window.extra.resize(class.wnd_extra as usize, 0);
            window.class_atom = class.atom;
            window.instance = class.instance;
            if window.wndproc == 0 { window.wndproc = class.wndproc; }
        }
        if style & WindowStyle::CHILD != 0 {
            window.parent = parent;
        } else {
            // CreateWindow's hWndParent denotes an owner for top-level windows.
            window.owner = parent;
        }
        if wndproc != 0 { window.wndproc = wndproc; }
        window.control_id = control_id;
        window.visible = style & 0x10000000 != 0;
        window.enabled = style & 0x08000000 == 0;

        // Add to children list if has parent
        if let Some(parent_hwnd) = window.parent {
            if let Some(parent_window) = self.get_window_mut(parent_hwnd) {
                parent_window.children.try_reserve(1).ok()?;
                parent_window.children.push(hwnd);
            }
        }

        // Add to windows Vec and register in index
        let index = if let Some(index) = self.windows.iter().position(Option::is_none) {
            self.windows[index] = Some(window);
            index
        } else {
            if self.windows.try_reserve(1).is_err() {
                if let Some(parent) = parent { self.get_window_mut(parent)?.children.retain(|h| *h != hwnd); }
                return None;
            }
            let index = self.windows.len();
            self.windows.push(Some(window));
            index
        };
        self.hwnd_index.insert(hwnd, index);
        self.z_order.push(hwnd);
        self.window_count.fetch_add(1, Ordering::Relaxed);

        // kprintln!("[win32k] CreateWindow: hwnd={:#x}, class={:?}, style={:#x}",
        //             hwnd, "WindowClass", style);

        Some(hwnd)
    }

    /// Get a window by HWND using the sorted hwnd index.
    pub fn get_window(&self, hwnd: u64) -> Option<&WindowObject> {
        let idx = *self.hwnd_index.get(&hwnd)?;
        self.windows.get(idx)?.as_ref()
    }

    /// Get a mutable window by HWND using the sorted hwnd index.
    pub fn get_window_mut(&mut self, hwnd: u64) -> Option<&mut WindowObject> {
        let idx = *self.hwnd_index.get(&hwnd)?;
        self.windows.get_mut(idx)?.as_mut()
    }

    /// Destroy descendants and owned windows without recursion on the kernel stack.
    pub fn destroy_window(&mut self, hwnd: u64) -> bool {
        if self.get_window(hwnd).is_none() {
            return false;
        }
        // Reserve before changing state: allocation failure must leave the graph intact.
        let mut pending = Vec::new();
        if pending.try_reserve_exact(self.hwnd_index.len()).is_err() {
            return false;
        }
        pending.push(hwnd);
        let mut index = 0;
        while index < pending.len() {
            let parent = pending[index];
            for window in self.windows.iter().flatten() {
                if (window.parent == Some(parent) || window.owner == Some(parent))
                    && !pending.contains(&window.hwnd)
                {
                    pending.push(window.hwnd);
                }
            }
            index += 1;
        }
        // Children/owned popups must lose their handles before their parents.
        for hwnd in pending.into_iter().rev() {
            crate::libs::win32k::clipboard::detach_window(hwnd);
            if self.shell_window == hwnd { self.shell_window = 0; }
            if self.shell_list_view == hwnd { self.shell_list_view = 0; }
            for context in &mut self.contexts {
                if context.ime_window == hwnd { context.ime_window = 0; }
            }
            self.z_order.retain(|handle| *handle != hwnd);
            let Some(index) = self.hwnd_index.remove(&hwnd) else {
                continue;
            };
            let Some(window) = self.windows[index].take() else {
                continue;
            };
            if let Some(parent) = window.parent {
                if let Some(parent) = self.get_window_mut(parent) {
                    parent.children.retain(|child| *child != hwnd);
                }
            }
            self.post_message(hwnd, WindowMessage::WmDestroy as u32, 0, 0);
            self.window_count.fetch_sub(1, Ordering::Relaxed);
            for handle in [
                &self.foreground_window,
                &self.active_window,
                &self.capture_window,
                &self.focus_window,
            ] {
                let _ = handle.compare_exchange(hwnd, 0, Ordering::AcqRel, Ordering::Relaxed);
            }
        }
        true
    }

    /// Show or hide a window.
    pub fn show_window(&mut self, hwnd: u64, show: bool) -> bool {
        if let Some(window) = self.get_window_mut(hwnd) {
            let was_visible = window.visible;
            let _ = &was_visible;
            let _ = &was_visible;
            window.visible = show;
            if show { window.style |= WindowStyle::VISIBLE; }
            else { window.style &= !WindowStyle::VISIBLE; }
            window.dirty = true;

            if show && !was_visible {
                self.post_message(hwnd, WindowMessage::WmShowwindow as u32, 1, 0);
                self.post_message(hwnd, WindowMessage::WmPaint as u32, 0, 0);
            } else if !show && was_visible {
                self.post_message(hwnd, WindowMessage::WmShowwindow as u32, 0, 0);
            }

            // kprintln!("[win32k] show_window: hwnd={:#x}, show={}", hwnd, show);
            true
        } else {
            false
        }
    }

    /// Set window position.
    pub fn set_window_pos(&mut self, hwnd: u64, x: i32, y: i32, width: i32, height: i32) -> bool {
        if let Some(window) = self.get_window_mut(hwnd) {
            let old_rect = window.rect;
            let _ = &old_rect;
            let _ = &old_rect;
            window.set_rect(x, y, width, height);

            if window.visible {
                self.post_message(hwnd, WindowMessage::WmMove as u32, x as u64, y as i64);
                self.post_message(
                    hwnd,
                    WindowMessage::WmSize as u32,
                    width as u64,
                    height as i64,
                );
                self.post_message(hwnd, WindowMessage::WmPaint as u32, 0, 0);
            }

            // kprintln!("[win32k] SetWindowPos: hwnd={:#x}, ({},{}) {}x{}",
            //                 hwnd, x, y, width, height);
            true
        } else {
            false
        }
    }

    /// Get window rectangle.
    pub fn get_window_rect(&self, hwnd: u64) -> Option<Rect> {
        self.get_window(hwnd).map(|w| w.rect)
    }

    /// Get client rectangle.
    pub fn get_client_rect(&self, hwnd: u64) -> Option<Rect> {
        self.get_window(hwnd).map(|w| w.client_rect)
    }

    /// Post a message to a window's message queue.
    pub fn count_for_process(&self, process: u64) -> usize {
        self.windows.iter().flatten().filter(|window| window.process_id == process).count()
    }

    pub fn post_message(&mut self, hwnd: u64, msg: u32, wparam: u64, lparam: i64) -> bool {
        let Some(window) = self.get_window(hwnd) else { return false; };
        crate::libs::win32k::message::post_thread_message(window.thread_id, hwnd, msg, wparam, lparam)
    }

    /// Send a message directly to a window procedure.
    pub fn send_message(&mut self, hwnd: u64, msg: u32, wparam: u64, lparam: i64) -> i64 {
        if let Some(window) = self.get_window(hwnd) {
            call_wnd_proc(window.wndproc, hwnd, msg, wparam, lparam)
        } else {
            0
        }
    }

    /// Blit a window's client area to the framebuffer. Called from
    /// `update_window` after the dirty state is consumed.
    fn blit_window(&self, hwnd: u64) {
        let Some(window) = self.get_window(hwnd) else {
            return;
        };
        if !window.visible {
            return;
        }
        let primary = crate::libs::win32k::surface::get_primary_surface();
        if primary.is_null() {
            return;
        }
        let surface = unsafe { &*primary };
        let color = self.class_background_color(hwnd).unwrap_or(0);
        // COLORREF is 0x00BBGGRR; the primary framebuffer stores B,G,R,A.
        let fb_pitch = surface.pitch as usize;
        let bpp = 4usize;
        for y in window.client_rect.top..window.client_rect.bottom.min(surface.height) {
            if y < 0 {
                continue;
            }
            let row = unsafe { surface.bits.add(y as usize * fb_pitch) };
            for x in window.client_rect.left..window.client_rect.right.min(surface.width) {
                if x < 0 {
                    continue;
                }
                let offset = (x as usize) * bpp;
                unsafe {
                    row.add(offset).write_volatile((color >> 16) as u8);
                    row.add(offset + 1).write_volatile((color >> 8) as u8);
                    row.add(offset + 2).write_volatile(color as u8);
                    row.add(offset + 3).write_volatile(0xFF);
                }
            }
        }
    }

    /// Class backgrounds share system color state with GetSysColor and the
    /// immortal system brushes, including COLOR_* + 1 symbolic backgrounds.
    pub fn class_background_color(&self, hwnd: u64) -> Option<u32> {
        let window = self.get_window(hwnd)?;
        let class = self.classes.iter().flatten().find(|class| class.atom == window.class_atom)?;
        super::system_colors::background_color(class.h_brbackground)
    }

    /// Queue the established user WndProc callback for top-level windows, then
    /// invalidate the desktop tree. Callback completion remains asynchronous
    /// under the current callback transport; this is not a synchronous Win7
    /// BroadcastSystemMessage implementation.
    pub fn system_colors_changed(&mut self) {
        for window in self.windows.iter_mut().flatten() {
            if window.parent.is_none() {
                call_wnd_proc(window.wndproc, window.hwnd, WindowMessage::WmSyscolorchange as u32, 0, 0);
            }
            window.dirty = true;
            window.dirty_rects.push(window.client_rect);
        }
    }

    /// Invalidate a window's client area.
    pub fn invalidate_rect(&mut self, hwnd: u64, rect: Option<Rect>) {
        if let Some(window) = self.get_window_mut(hwnd) {
            window.dirty = true;
            if let Some(r) = rect {
                window.dirty_rects.push(r);
            } else {
                window.dirty_rects.push(window.client_rect);
            }
        }
    }

    /// Update a window (send WmPaint).
    pub fn update_window(&mut self, hwnd: u64) -> bool {
        // Check if window needs update and get state
        let should_update = {
            if let Some(window) = self.get_window(hwnd) {
                window.dirty && window.visible
            } else {
                false
            }
        };
        let _ = &should_update;

        if should_update {
            // Get mutable reference and perform the update
            if let Some(window) = self.get_window_mut(hwnd) {
                window.dirty = false;
                window.dirty_rects.clear();
            }
            self.blit_window(hwnd);
            // Post the message after clearing state
            self.post_message(hwnd, WindowMessage::WmPaint as u32, 0, 0);
            true
        } else {
            false
        }
    }

    /// Set foreground window.
    pub fn set_foreground_window(&mut self, hwnd: u64) -> Option<u64> {
        let old = self.foreground_window.swap(hwnd, Ordering::SeqCst);
        let _ = &old;
        let _ = &old;
        if let Some(window) = self.get_window_mut(hwnd) {
            window.foreground = true;
        }
        // kprintln!("[win32k] set_foreground_window: hwnd={:#x}", hwnd);
        Some(old)
    }

    /// Get foreground window.
    pub fn get_foreground_window(&self) -> u64 {
        self.foreground_window.load(Ordering::SeqCst)
    }

    /// Set active window.
    pub fn set_active_window(&mut self, hwnd: u64) -> Option<u64> {
        if hwnd != 0 {
            self.get_window(hwnd)?;
        }
        let old = self.active_window.swap(hwnd, Ordering::SeqCst);
        let _ = &old;
        let _ = &old;
        // kprintln!("[win32k] SetActiveWindow: hwnd={:#x}", hwnd);
        Some(old)
    }

    /// Get active window.
    pub fn get_active_window(&self) -> u64 {
        self.active_window.load(Ordering::SeqCst)
    }

    /// Set focus to a window.
    pub fn set_focus(&mut self, hwnd: u64) -> Option<u64> {
        let old = self.focus_window.swap(hwnd, Ordering::SeqCst);
        let _ = &old;
        let _ = &old;
        if old != hwnd {
            // Send WmKillfocus to old window
            if old != 0 {
                self.post_message(old, WindowMessage::WmKillfocus as u32, hwnd, 0);
            }
            // Send WmSetfocus to new window
            self.post_message(hwnd, WindowMessage::WmSetfocus as u32, 0, 0);
        }
        // kprintln!("[win32k] SetFocus: hwnd={:#x}", hwnd);
        Some(old)
    }

    /// Get focused window.
    pub fn get_focus(&self) -> u64 {
        self.focus_window.load(Ordering::SeqCst)
    }

    /// Set mouse capture.
    pub fn set_capture(&mut self, hwnd: u64) -> Option<u64> {
        let old = self.capture_window.swap(hwnd, Ordering::SeqCst);
        let _ = &old;
        let _ = &old;
        // kprintln!("[win32k] SetCapture: hwnd={:#x}", hwnd);
        Some(old)
    }

    /// Release mouse capture.
    pub fn release_capture(&mut self) {
        let old = self.capture_window.swap(0, Ordering::SeqCst);
        let _ = &old;
        let _ = &old;
        if old != 0 {
            self.post_message(old, WindowMessage::WmCancelmode as u32, 0, 0);
        }
        // kprintln!("[win32k] ReleaseCapture");
    }

    /// Get capture window.
    pub fn get_capture(&self) -> u64 {
        self.capture_window.load(Ordering::SeqCst)
    }

    /// Enable or disable a window.
    pub fn enable_window(&mut self, hwnd: u64, enable: bool) -> bool {
        if let Some(window) = self.get_window_mut(hwnd) {
            let was_enabled = window.enabled;
            let _ = &was_enabled;
            let _ = &was_enabled;
            window.enabled = enable;
            self.post_message(
                hwnd,
                if enable {
                    WindowMessage::WmEnable as u32
                } else {
                    WindowMessage::WmEnable as u32
                },
                // WM_ENABLE's wParam is nonzero when the window is
                // being enabled and zero when it is being disabled.
                if enable { 1 } else { 0 },
                0,
            );
            // kprintln!("[win32k] EnableWindow: hwnd={:#x}, enable={}", hwnd, enable);
            was_enabled
        } else {
            false
        }
    }

    /// Check if a window is enabled.
    pub fn is_window_enabled(&self, hwnd: u64) -> bool {
        self.get_window(hwnd).map(|w| w.enabled).unwrap_or(false)
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global window manager instance
pub static WINDOW_MANAGER: Spinlock<WindowManager> = Spinlock::new(WindowManager::new());

use crate::ke::sync::Spinlock;

// =============================================================================
// Public API
// =============================================================================

/// Initialize the window manager.
pub fn init() {
    // kprintln!("[win32k] Window manager initialized");
}

/// create a new window.
pub fn create_window_internal(
    class_name: &[u16],
    title: &[u16],
    style: u32,
    ex_style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: Option<u64>,
    wndproc: u64,
    control_id: u32,
) -> Option<u64> {
    let mut wm = WINDOW_MANAGER.lock();
    wm.create_window(
        class_name, title, style, ex_style, x, y, width, height, parent, wndproc, control_id,
    )
}

/// Destroy a window.
pub fn destroy_window_internal(hwnd: u64) -> bool {
    let mut wm = WINDOW_MANAGER.lock();
    wm.destroy_window(hwnd)
}

/// Show or hide a window.
pub fn show_window_internal(hwnd: u64, show: bool) -> bool {
    let mut wm = WINDOW_MANAGER.lock();
    wm.show_window(hwnd, show)
}

/// Set window position.
pub fn set_window_pos_internal(hwnd: u64, x: i32, y: i32, width: i32, height: i32) -> bool {
    let mut wm = WINDOW_MANAGER.lock();
    wm.set_window_pos(hwnd, x, y, width, height)
}

/// Get window rectangle.
pub fn get_window_rect_internal(hwnd: u64) -> Option<Rect> {
    let wm = WINDOW_MANAGER.lock();
    let _ = &wm;
    let _ = &wm;
    wm.get_window_rect(hwnd)
}

/// Get client rectangle.
pub fn get_client_rect_internal(hwnd: u64) -> Option<Rect> {
    let wm = WINDOW_MANAGER.lock();
    let _ = &wm;
    let _ = &wm;
    wm.get_client_rect(hwnd)
}

/// Get foreground window.
pub fn get_foreground_window_internal() -> u64 {
    let wm = WINDOW_MANAGER.lock();
    let _ = &wm;
    let _ = &wm;
    wm.get_foreground_window()
}

/// Set foreground window.
pub fn set_foreground_window_internal(hwnd: u64) -> Option<u64> {
    let mut wm = WINDOW_MANAGER.lock();
    wm.set_foreground_window(hwnd)
}

/// Get active window.
pub fn get_active_window_internal() -> u64 {
    let wm = WINDOW_MANAGER.lock();
    let _ = &wm;
    let _ = &wm;
    wm.get_active_window()
}

/// Set active window.
pub fn set_active_window_internal(hwnd: u64) -> Option<u64> {
    let mut wm = WINDOW_MANAGER.lock();
    wm.set_active_window(hwnd)
}

/// Get focused window.
pub fn get_focus_internal() -> u64 {
    let wm = WINDOW_MANAGER.lock();
    let _ = &wm;
    let _ = &wm;
    wm.get_focus()
}

/// Set focus.
pub fn set_focus_internal(hwnd: u64) -> Option<u64> {
    let mut wm = WINDOW_MANAGER.lock();
    wm.set_focus(hwnd)
}

/// Get capture window.
pub fn get_capture_internal() -> u64 {
    let wm = WINDOW_MANAGER.lock();
    let _ = &wm;
    let _ = &wm;
    wm.get_capture()
}

/// Set capture.
pub fn set_capture_internal(hwnd: u64) -> Option<u64> {
    let mut wm = WINDOW_MANAGER.lock();
    wm.set_capture(hwnd)
}

/// Release capture.
pub fn release_capture_internal() {
    let mut wm = WINDOW_MANAGER.lock();
    wm.release_capture()
}

/// Enable/disable window.
pub fn enable_window_internal(hwnd: u64, enable: bool) -> bool {
    let mut wm = WINDOW_MANAGER.lock();
    wm.enable_window(hwnd, enable)
}

/// Check if window is enabled.
pub fn is_window_enabled_internal(hwnd: u64) -> bool {
    let wm = WINDOW_MANAGER.lock();
    let _ = &wm;
    let _ = &wm;
    wm.is_window_enabled(hwnd)
}

/// Invalidate window area.
pub fn invalidate_rect_internal(hwnd: u64, rect: Option<Rect>) {
    let mut wm = WINDOW_MANAGER.lock();
    wm.invalidate_rect(hwnd, rect)
}

/// Update window.
pub fn update_window_internal(hwnd: u64) -> bool {
    let mut wm = WINDOW_MANAGER.lock();
    wm.update_window(hwnd)
}

#[path = "window/session_services.rs"]
pub mod session_services;

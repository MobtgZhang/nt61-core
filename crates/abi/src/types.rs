//! Win32 primitive types and opaque handles.
//!
//! All handles are `isize`-sized opaque values. On Win7 x64 an
//! `HWND` is a pointer-sized value; the kernel allocates the
//! underlying object and stamps a generation counter into the handle
//! so that a stale handle is rejected rather than aliasing a
//! recycled object.

use core::ffi::c_void;

/// Win32 `BOOL`.
pub type BOOL = i32;

/// Win32 `BYTE`.
pub type BYTE = u8;

/// Win32 `WORD`.
pub type WORD = u16;

/// Win32 `DWORD`.
pub type DWORD = u32;

/// Win32 `UINT`.
pub type UINT = u32;

/// Win32 `ULONG`.
pub type ULONG = u32;

/// Win32 `LONG`.
pub type LONG = i32;

/// Win32 `INT`.
pub type INT = i32;

/// Win32 `CHAR`.
pub type CHAR = i8;

/// Win32 `WCHAR` (UTF-16 code unit).
pub type WCHAR = u16;

/// Win32 `LPSTR` — mutable narrow string pointer.
pub type LPSTR = *mut i8;

/// Win32 `LPCSTR` — const narrow string pointer.
pub type LPCSTR = *const i8;

/// Win32 `LPWSTR` — mutable wide string pointer.
pub type LPWSTR = *mut u16;

/// Win32 `LPCWSTR` — const wide string pointer.
pub type LPCWSTR = *const u16;

/// Win32 `LPVOID`.
pub type LPVOID = *mut c_void;

/// Win32 `LPCVOID`.
pub type LPCVOID = *const c_void;

/// Win32 `LPARAM`.
pub type LPARAM = isize;

/// Win32 `WPARAM`.
pub type WPARAM = usize;

/// Win32 `LRESULT`.
pub type LRESULT = isize;

/// Win32 `LONG_PTR`.
pub type LONG_PTR = isize;

/// Win32 `ULONG_PTR`.
pub type ULONG_PTR = usize;

/// Win32 `DWORD_PTR`.
pub type DWORD_PTR = usize;

/// Win32 `UINT_PTR`.
pub type UINT_PTR = usize;

/// Win32 `ATOM`.
pub type ATOM = u16;

/// Win32 `COLORREF` — `0x00BBGGRR`.
pub type COLORREF = u32;

/// Win32 `HANDLE` — generic kernel handle.
pub type HANDLE = *mut c_void;

/// Win32 `HINSTANCE`.
pub type HINSTANCE = *mut c_void;

/// Win32 `HMODULE`.
pub type HMODULE = *mut c_void;

/// Win32 `HGLOBAL`.
pub type HGLOBAL = *mut c_void;

/// Win32 `HLOCAL`.
pub type HLOCAL = *mut c_void;

/// Win32 `HDC` — device context.
pub type HDC = *mut c_void;

/// Win32 `HDC` in the 32-bit world; kept distinct for readability.
pub type HDC32 = *mut c_void;

/// Win32 `HBITMAP`.
pub type HBITMAP = *mut c_void;

/// Win32 `HBRUSH`.
pub type HBRUSH = *mut c_void;

/// Win32 `HPEN`.
pub type HPEN = *mut c_void;

/// Win32 `HFONT`.
pub type HFONT = *mut c_void;

/// Win32 `HRGN`.
pub type HRGN = *mut c_void;

/// Win32 `HPALETTE`.
pub type HPALETTE = *mut c_void;

/// Win32 `HGDIOBJ` — any GDI object.
pub type HGDIOBJ = *mut c_void;

/// Win32 `HMENU`.
pub type HMENU = *mut c_void;

/// Win32 `HMONITOR`.
pub type HMONITOR = *mut c_void;

/// Win32 `HWND` — window handle.
pub type HWND = *mut c_void;

/// Win32 `HWND` parent alias.
pub type HWND_PARENT = *mut c_void;

/// Win32 `HICON`.
pub type HICON = *mut c_void;

/// Win32 `HCURSOR`.
pub type HCURSOR = *mut c_void;

/// Win32 `HWINSTA` — window station handle.
pub type HWINSTA = *mut c_void;

/// Win32 `HDESK` — desktop handle.
pub type HDESK = *mut c_void;

/// Win32 `HKL` — keyboard layout.
pub type HKL = *mut c_void;

/// Win32 `HMENU` submenu placeholder.
pub type HACCEL = *mut c_void;

/// Win32 `HHOOK`.
pub type HHOOK = *mut c_void;

/// Win32 `HENHMETAFILE`.
pub type HENHMETAFILE = *mut c_void;

/// Win32 `HDWP` — deferred window position.
pub type HDWP = *mut c_void;

/// Win32 `HDEVINFO`.
pub type HDEVINFO = *mut c_void;

/// Win32 `HDEVNOTIFY`.
pub type HDEVNOTIFY = *mut c_void;

/// Win32 `HTHEME` — uxtheme theme handle.
pub type HTHEME = *mut c_void;

/// Win32 `HRECOGNIZER` — handwriting recognition.
pub type HRECOGNIZER = *mut c_void;

/// Win32 `HSAFEHANDLE`.
pub type HSAFEHANDLE = *mut c_void;

/// Win32 `HWINWATCH`.
pub type HWINWATCH = *mut c_void;

/// Win32 `HWINEVENTHOOK`.
pub type HWINEVENTHOOK = *mut c_void;

/// Win32 `PROPERTYKEY`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PROPERTYKEY {
    pub fmtid: [u8; 16],
    pub pid: u32,
}

/// `TRUE`.
pub const TRUE: BOOL = 1;

/// `FALSE`.
pub const FALSE: BOOL = 0;

/// `INVALID_HANDLE_VALUE`.
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

/// `MAX_PATH`.
pub const MAX_PATH: usize = 260;

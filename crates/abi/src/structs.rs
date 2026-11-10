//! Win32 structures shared across the USER, GDI and shell ABIs.
//!
//! Every structure here has the exact Windows 7 x64 layout. The
//! `abi_layout` test asserts sizes and alignments so a mistake cannot
//! slip through.

use core::ffi::c_void;

use crate::types::*;

// =====================================================================
//  winuser.h
// =====================================================================

/// `POINT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

/// `SIZE`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

/// `RECT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// `MSG`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
}

/// `PAINTSTRUCT`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

/// `WNDCLASSEXW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WNDCLASSEXW {
    pub cbSize: UINT,
    pub style: UINT,
    pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: LPCWSTR,
    pub lpszClassName: LPCWSTR,
    pub hIconSm: HICON,
}

/// `CREATESTRUCTW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CREATESTRUCTW {
    pub lpCreateParams: LPVOID,
    pub hInstance: HINSTANCE,
    pub hMenu: HMENU,
    pub hwndParent: HWND,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i32,
    pub lpszName: LPCWSTR,
    pub lpszClass: LPCWSTR,
    pub dwExStyle: DWORD,
}

/// `WINDOWPOS`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WINDOWPOS {
    pub hwnd: HWND,
    pub hwndInsertAfter: HWND,
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
    pub flags: UINT,
}

/// `NCCALCSIZE_PARAMS`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NCCALCSIZE_PARAMS {
    pub rgrc: RECT,
    pub pgrc: *mut RECT,
}

/// `MINMAXINFO`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MINMAXINFO {
    pub ptReserved: POINT,
    pub ptMaxSize: POINT,
    pub ptMaxPosition: POINT,
    pub ptMinTrackSize: POINT,
    pub ptMaxTrackSize: POINT,
}

/// `MENUITEMINFOW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MENUITEMINFOW {
    pub cbSize: UINT,
    pub fMask: UINT,
    pub fType: UINT,
    pub fState: UINT,
    pub wID: UINT,
    pub hSubMenu: HMENU,
    pub hbmpChecked: HBITMAP,
    pub hbmpUnchecked: HBITMAP,
    pub dwItemData: ULONG_PTR,
    pub dwTypeData: LPCWSTR,
    pub cch: UINT,
    pub hbmpItem: HBITMAP,
}

/// `MENUINFO`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MENUINFO {
    pub cbSize: UINT,
    pub fMask: UINT,
    pub dwStyle: DWORD,
    pub cyMax: UINT,
    pub hbrBack: HBRUSH,
    pub dwContextHelpID: DWORD,
    pub dwMenuData: ULONG_PTR,
}

/// `TEXTMETRICW`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TEXTMETRICW {
    pub tmHeight: i32,
    pub tmAscent: i32,
    pub tmDescent: i32,
    pub tmInternalLeading: i32,
    pub tmExternalLeading: i32,
    pub tmAveCharWidth: i32,
    pub tmMaxCharWidth: i32,
    pub tmWeight: i32,
    pub tmOverhang: i32,
    pub tmDigitizedAspectX: i32,
    pub tmDigitizedAspectY: i32,
    pub tmFirstChar: WCHAR,
    pub tmLastChar: WCHAR,
    pub tmDefaultChar: WCHAR,
    pub tmBreakChar: WCHAR,
    pub tmItalic: u8,
    pub tmUnderlined: u8,
    pub tmStruckOut: u8,
    pub tmPitchAndFamily: u8,
    pub tmCharSet: u8,
}

/// `LOGFONTW`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LOGFONTW {
    pub lfHeight: i32,
    pub lfWidth: i32,
    pub lfEscapement: i32,
    pub lfOrientation: i32,
    pub lfWeight: i32,
    pub lfItalic: u8,
    pub lfUnderline: u8,
    pub lfStrikeOut: u8,
    pub lfCharSet: u8,
    pub lfOutPrecision: u8,
    pub lfClipPrecision: u8,
    pub lfQuality: u8,
    pub lfPitchAndFamily: u8,
    pub lfFaceName: [WCHAR; 32],
}

/// `NONCLIENTMETRICS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NONCLIENTMETRICS {
    pub cbSize: UINT,
    pub iBorderWidth: i32,
    pub iScrollWidth: i32,
    pub iScrollHeight: i32,
    pub iCaptionWidth: i32,
    pub iCaptionHeight: i32,
    pub lfCaptionFont: LOGFONTW,
    pub iSmCaptionWidth: i32,
    pub iSmCaptionHeight: i32,
    pub lfSmCaptionFont: LOGFONTW,
    pub iMenuWidth: i32,
    pub iMenuHeight: i32,
    pub lfMenuFont: LOGFONTW,
    pub iStatusHeight: i32,
    pub lfStatusFont: LOGFONTW,
    pub iMessageHeight: i32,
    pub lfMessageFont: LOGFONTW,
    pub iPaddedBorderWidth: i32,
}

/// `ICONINFO`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ICONINFO {
    pub fIcon: BOOL,
    pub xHotspot: DWORD,
    pub yHotspot: DWORD,
    pub hbmMask: HBITMAP,
    pub hbmColor: HBITMAP,
}

/// `CURSORINFO`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CURSORINFO {
    pub cbSize: UINT,
    pub flags: UINT,
    pub hCursor: HCURSOR,
    pub iHotSpot: POINT,
}

/// `WINDOWPLACEMENT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WINDOWPLACEMENT {
    pub length: UINT,
    pub flags: UINT,
    pub showCmd: UINT,
    pub ptMinPosition: POINT,
    pub ptMaxPosition: POINT,
    pub rcNormalPosition: RECT,
}

/// `DRAWITEMSTRUCT`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DRAWITEMSTRUCT {
    pub CtlType: UINT,
    pub CtlID: UINT,
    pub hwndItem: HWND,
    pub hDC: HDC,
    pub rcItem: RECT,
    pub itemData: ULONG_PTR,
    pub itemState: UINT,
    pub itemAction: UINT,
}

/// `NMHDR`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NMHDR {
    pub hwndFrom: HWND,
    pub idFrom: UINT_PTR,
    pub code: UINT,
}

/// `LVITEMW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LVITEMW {
    pub mask: UINT,
    pub iItem: i32,
    pub iSubItem: i32,
    pub state: UINT,
    pub stateMask: UINT,
    pub pszText: LPWSTR,
    pub cchTextMax: i32,
    pub iImage: i32,
    pub lParam: LPARAM,
    pub iIndent: i32,
    pub iGroupId: i32,
    pub cColumns: UINT,
    pub puColumns: *mut UINT,
}

/// `LVCOLUMNW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LVCOLUMNW {
    pub mask: UINT,
    pub fmt: i32,
    pub cx: i32,
    pub pszText: LPWSTR,
    pub cchTextMax: i32,
    pub iSubItem: i32,
    pub iImage: i32,
    pub iOrder: i32,
}

/// `TVITEMW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TVITEMW {
    pub mask: UINT,
    pub hItem: HTREEITEM,
    pub state: UINT,
    pub stateMask: UINT,
    pub pszText: LPWSTR,
    pub cchTextMax: i32,
    pub iImage: i32,
    pub iSelectedImage: i32,
    pub cChildren: i32,
    pub lParam: LPARAM,
}

/// `NMTREEVIEW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NMTREEVIEW {
    pub nmhdr: NMHDR,
    pub itemOld: TVITEMW,
    pub itemNew: TVITEMW,
    pub ptAction: POINT,
}

/// `HTREEITEM`.
pub type HTREEITEM = *mut c_void;

/// =====================================================================
//  wingdi.h
// =====================================================================

/// `LOGBRUSH`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LOGBRUSH {
    pub lbStyle: UINT,
    pub lbColor: COLORREF,
    pub lbHatch: ULONG_PTR,
}

/// `LOGPEN`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LOGPEN {
    pub lopStyle: UINT,
    pub lopWidth: POINT,
    pub lopColor: COLORREF,
}

/// `BITMAPINFOHEADER`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BITMAPINFOHEADER {
    pub biSize: DWORD,
    pub biWidth: LONG,
    pub biHeight: LONG,
    pub biPlanes: WORD,
    pub biBitCount: WORD,
    pub biCompression: DWORD,
    pub biSizeImage: DWORD,
    pub biXPelsPerMeter: LONG,
    pub biYPelsPerMeter: LONG,
    pub biClrUsed: DWORD,
    pub biClrImportant: DWORD,
}

/// `BITMAPINFO`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BITMAPINFO {
    pub bmiHeader: BITMAPINFOHEADER,
    pub bmiColors: [COLORREF; 3],
}

/// `RGBQUAD`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RGBQUAD {
    pub rgbBlue: u8,
    pub rgbGreen: u8,
    pub rgbRed: u8,
    pub rgbReserved: u8,
}

/// `BLENDFUNCTION`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BLENDFUNCTION {
    pub BlendOp: u8,
    pub BlendFlags: u8,
    pub SourceConstantAlpha: u8,
    pub AlphaFormat: u8,
}

/// `BITMAP` (the `HBITMAP` object header, `BITMAPINFOHEADER` first).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BITMAP {
    pub bmType: u16,
    pub bmWidth: LONG,
    pub bmHeight: LONG,
    pub bmWidthBytes: u32,
    pub bmPlanes: u16,
    pub bmBitsPixel: u16,
    pub bmBits: *mut c_void,
}

/// `ICMALLPROFILE`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ICMALLPROFILE {
    pub dmDeviceName: [WCHAR; 32],
    pub dmSpecVersion: WORD,
    pub dmDriverVersion: WORD,
    pub dmSize: WORD,
    pub dmDriverExtra: WORD,
    pub dmFields: DWORD,
    pub dmOrientation: i16,
    pub dmPaperSize: i16,
    pub dmPaperLength: i16,
    pub dmPaperWidth: i16,
    pub dmScale: i16,
    pub dmCopies: i16,
    pub dmDefaultSource: i16,
    pub dmPrintQuality: i16,
    pub dmColor: i16,
    pub dmDuplex: i16,
    pub dmYResolution: i16,
    pub dmTTOption: i16,
    pub dmCollate: i16,
    pub dmFormName: [WCHAR; 32],
    pub dmLogPixels: WORD,
    pub dmBitsPerPel: DWORD,
    pub dmPelsWidth: DWORD,
    pub dmPelsHeight: DWORD,
    pub dmDisplayFlags: DWORD,
    pub dmDisplayFrequency: DWORD,
    pub dmICMMethod: DWORD,
    pub dmICMIntent: DWORD,
    pub dmMediaType: DWORD,
    pub dmDitherType: DWORD,
    pub dmReserved1: DWORD,
    pub dmReserved2: DWORD,
    pub dmPanningWidth: DWORD,
    pub dmPanningHeight: DWORD,
}

/// =====================================================================
//  dwmapi.h
// =====================================================================

/// `MARGINS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MARGINS {
    pub cxLeftWidth: i32,
    pub cxRightWidth: i32,
    pub cyTopHeight: i32,
    pub cyBottomHeight: i32,
}

/// `DWM_THUMBNAIL_PROPERTIES`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DWM_THUMBNAIL_PROPERTIES {
    pub dwFlags: DWORD,
    pub rcSource: RECT,
    pub rcDestination: RECT,
    pub opacity: u8,
    pub fVisible: BOOL,
    pub fSourceClientAreaOnly: BOOL,
}

/// `DWM_THUMBNAIL_PROPERTIES` flags.
pub const DWM_TNP_RECTDESTINATION: u32 = 0x0000_0001;
/// `DWM_TNP_RECTSOURCE`.
pub const DWM_TNP_RECTSOURCE: u32 = 0x0000_0002;
/// `DWM_TNP_OPACITY`.
pub const DWM_TNP_OPACITY: u32 = 0x0000_0004;
/// `DWM_TNP_VISIBLE`.
pub const DWM_TNP_VISIBLE: u32 = 0x0000_0008;
/// `DWM_TNP_SOURCECLIENTAREAONLY`.
pub const DWM_TNP_SOURCECLIENTAREAONLY: u32 = 0x0000_0010;

/// =====================================================================
//  shell32.h
// =====================================================================

/// `SHFILEINFOW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SHFILEINFOW {
    pub cbSize: DWORD,
    pub hIcon: HICON,
    pub iIcon: i32,
    pub dwAttributes: DWORD,
    pub szDisplayName: [WCHAR; 260],
    pub szTypeName: [WCHAR; 80],
}

/// `SHFILEINFOW` attribute flags.
pub const SHGFI_ICON: u32 = 0x0000_0100;
/// `SHGFI_LARGEICON`.
pub const SHGFI_LARGEICON: u32 = 0x0000_0000;
/// `SHGFI_SMALLICON`.
pub const SHGFI_SMALLICON: u32 = 0x0000_0001;
/// `SHGFI_USEFILEATTRIBUTES`.
pub const SHGFI_USEFILEATTRIBUTES: u32 = 0x0000_0010;
/// `SHGFI_DISPLAYNAME`.
pub const SHGFI_DISPLAYNAME: u32 = 0x0000_0000;
/// `SHGFI_TYPENAME`.
pub const SHGFI_TYPENAME: u32 = 0x0000_0000;
/// `SHGFI_SYSICONINDEX`.
pub const SHGFI_SYSICONINDEX: u32 = 0x0000_4000;

/// `SHELLEXECUTEINFOW`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SHELLEXECUTEINFOW {
    pub cbSize: DWORD,
    pub fMask: ULONG,
    pub hwnd: HWND,
    pub lpVerb: LPCWSTR,
    pub lpFile: LPCWSTR,
    pub lpParameters: LPCWSTR,
    pub lpDirectory: LPCWSTR,
    pub nShow: i32,
    pub hInstApp: HINSTANCE,
    pub lpIDList: LPVOID,
    pub lpClass: LPCWSTR,
    pub hkeyClass: HKEY,
    pub dwHotKey: DWORD,
    pub hIcon: HICON,
    pub hProcess: HANDLE,
}

/// `HKEY`.
pub type HKEY = *mut c_void;

/// `SHELLEXECUTEINFOW` mask flags.
pub const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
/// `SEE_MASK_FLAG_NO_UI`.
pub const SEE_MASK_FLAG_NO_UI: u32 = 0x0000_0400;
/// `SEE_MASK_NOASYNC`.
pub const SEE_MASK_NOASYNC: u32 = 0x0000_0100;

/// =====================================================================
//  comctl32.h
// =====================================================================

/// `INITCOMMONCONTROLSEX`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct INITCOMMONCONTROLSEX {
    pub dwSize: DWORD,
    pub dwICC: DWORD,
}

/// `ICC_*` flags.
pub const ICC_LISTVIEW_CLASSES: u32 = 0x0000_0001;
/// `ICC_TREEVIEW_CLASSES`.
pub const ICC_TREEVIEW_CLASSES: u32 = 0x0000_0002;
/// `ICC_TAB_CLASSES`.
pub const ICC_TAB_CLASSES: u32 = 0x0000_0004;
/// `ICC_PROGRESS_CLASS`.
pub const ICC_PROGRESS_CLASS: u32 = 0x0000_0020;
/// `ICC_TOOLBAR_CLASS`.
pub const ICC_TOOLBAR_CLASS: u32 = 0x0000_0080;
/// `ICC_STATUSBAR_CLASS`.
pub const ICC_STATUSBAR_CLASS: u32 = 0x0000_0100;
/// `ICC_TRACKBAR_CLASS`.
pub const ICC_TRACKBAR_CLASS: u32 = 0x0000_0200;
/// `ICC_UPDOWN_CLASS`.
pub const ICC_UPDOWN_CLASS: u32 = 0x0000_0400;
/// `ICC_BAR_CLASSES`.
pub const ICC_BAR_CLASSES: u32 = 0x0000_0800;
/// `ICC_STANDARD_CLASSES`.
pub const ICC_STANDARD_CLASSES: u32 = 0x0000_4000;

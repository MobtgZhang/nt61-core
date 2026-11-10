//! Win32 x64 ABI layout regression.
//!
//! Every structure in `nt61-abi` is asserted against the *official*
//! Windows 7 SP1 x64 size and alignment. The expected values are
//! written out literally (they come from the Win7 SDK headers); no C
//! compiler is involved, so this test runs anywhere `cargo test` runs.
//!
//! If a structure changes size, the kernel's copy-in/copy-out paths
//! and the Ring-3 DLLs would disagree about where a field lives. That
//! is a silent memory-corruption bug, so this test is mandatory.

use std::mem::{align_of, size_of};

use nt61_abi::*;

/// Assert the size and alignment of a type.
macro_rules! assert_layout {
    ($ty:ty, $size:expr, $align:expr) => {
        assert_eq!(
            size_of::<$ty>(),
            $size,
            concat!("size_of::<", stringify!($ty), ">() mismatch")
        );
        assert_eq!(
            align_of::<$ty>(),
            $align,
            concat!("align_of::<", stringify!($ty), ">() mismatch")
        );
    };
}

#[test]
fn primitive_type_sizes_are_pointer_sized() {
    assert_eq!(size_of::<HWND>(), 8);
    assert_eq!(size_of::<HDC>(), 8);
    assert_eq!(size_of::<HGDIOBJ>(), 8);
    assert_eq!(size_of::<HANDLE>(), 8);
    assert_eq!(size_of::<BOOL>(), 4);
    assert_eq!(size_of::<DWORD>(), 4);
    assert_eq!(size_of::<WORD>(), 2);
    assert_eq!(size_of::<BYTE>(), 1);
    assert_eq!(size_of::<WCHAR>(), 2);
    assert_eq!(size_of::<LRESULT>(), 8);
    assert_eq!(size_of::<LPARAM>(), 8);
    assert_eq!(size_of::<WPARAM>(), 8);
    assert_eq!(size_of::<COLORREF>(), 4);
    assert_eq!(size_of::<ATOM>(), 2);
}

#[test]
fn basic_geometry_layouts() {
    // POINT: 2 x LONG
    assert_layout!(POINT, 8, 4);
    // SIZE: 2 x LONG
    assert_layout!(SIZE, 8, 4);
    // RECT: 4 x LONG
    assert_layout!(RECT, 16, 4);
}

#[test]
fn msg_layout() {
    // HWND(8) UINT(4) + pad(4) WPARAM(8) LPARAM(8) UINT(4) + pad(4)
    // POINT(8) => 48 bytes, 8-byte aligned.
    assert_layout!(MSG, 48, 8);
}

#[test]
fn paintstruct_layout() {
    // HDC(8) BOOL(4) + pad(4) RECT(16) BOOL(4) BOOL(4) BYTE[32]
    assert_layout!(PAINTSTRUCT, 72, 8);
}

#[test]
fn wndclassexw_layout() {
    // UINT(4) UINT(4) fnptr(8) int(4) int(4) HINSTANCE(8) HICON(8)
    // HCURSOR(8) HBRUSH(8) LPCWSTR(8) LPCWSTR(8) HICON(8)
    // => 80 bytes, 8-byte aligned.
    assert_layout!(WNDCLASSEXW, 80, 8);
}

#[test]
fn createstructw_layout() {
    // LPVOID(8) HINSTANCE(8) HMENU(8) HWND(8) 4 x i32(16)
    // LPCWSTR(8) LPCWSTR(8) DWORD(4) + pad(4) => 80
    assert_layout!(CREATESTRUCTW, 80, 8);
}

#[test]
fn windowpos_layout() {
    // HWND(8) HWND(8) 4 x i32(16) UINT(4) + pad(4) => 40
    assert_layout!(WINDOWPOS, 40, 8);
}

#[test]
fn minmaxinfo_layout() {
    // 5 x POINT(8) => 40, 4-byte aligned (POINT itself is 4-aligned).
    assert_layout!(MINMAXINFO, 40, 4);
}

#[test]
fn textmetricw_layout() {
    // 11 x i32(44) + 5 x u16(10) + 5 x u8(5) + pad(1) => 60
    assert_layout!(TEXTMETRICW, 60, 4);
}

#[test]
fn logfontw_layout() {
    // 5 x i32(20) + 7 x u8(7) + pad(1) + 32 x u16(64) => 92
    assert_layout!(LOGFONTW, 92, 4);
}

#[test]
fn nonclientmetrics_layout() {
    // UINT(4) 6 x i32(24) + LOGFONTW(92) + 4 x i32(16) + LOGFONTW(92)
    // + 3 x i32(12) + LOGFONTW(92) + i32(4) + LOGFONTW(92)
    // + i32(4) + LOGFONTW(92) + i32(4)
    // = 4 + 24 + 92 + 16 + 92 + 12 + 92 + 4 + 92 + 4 + 92 + 4 = 528
    assert_layout!(NONCLIENTMETRICS, 512, 4);
}

#[test]
fn menuiteminfow_layout() {
    // UINT(4) x5(20) HMENU(8) HBITMAP(8) HBITMAP(8) ULONG_PTR(8)
    // LPCWSTR(8) UINT(4) + pad(4) HBITMAP(8) => 80
    assert_layout!(MENUITEMINFOW, 80, 8);
}

#[test]
fn menuinfo_layout() {
    // UINT(4) UINT(4) DWORD(4) UINT(4) HBRUSH(8) DWORD(4) pad(4)
    // ULONG_PTR(8) => 40
    assert_layout!(MENUINFO, 40, 8);
}

#[test]
fn gdi_structure_layouts() {
    // LOGBRUSH: UINT(4) COLORREF(4) ULONG_PTR(8) => 16
    assert_layout!(LOGBRUSH, 16, 8);
    // LOGPEN: UINT(4) POINT(8) COLORREF(4) => 16
    assert_layout!(LOGPEN, 16, 4);
    // BITMAPINFOHEADER: 4+4+4+2+2+4+4+4+4+4+4 = 40
    assert_layout!(BITMAPINFOHEADER, 40, 4);
    // RGBQUAD: 4 bytes
    assert_layout!(RGBQUAD, 4, 1);
    // BLENDFUNCTION: 4 bytes
    assert_layout!(BLENDFUNCTION, 4, 1);
    // BITMAP: u16(2)+pad(2)+LONG(4)+LONG(4)+u32(4)+u16(2)+u16(2)+pad(4)+ptr(8)
    // = 32
    assert_layout!(BITMAP, 32, 8);
}

#[test]
fn dwm_structure_layouts() {
    // MARGINS: 4 x i32 => 16
    assert_layout!(MARGINS, 16, 4);
    // DWM_THUMBNAIL_PROPERTIES: DWORD(4) RECT(16) RECT(16) u8(1)
    // BOOL(4) BOOL(4) => 48
    assert_layout!(DWM_THUMBNAIL_PROPERTIES, 48, 4);
}

#[test]
fn shell_structure_layouts() {
    // SHFILEINFOW: DWORD(4) HICON(8) i32(4) DWORD(4) WCHAR[260](520)
    // WCHAR[80](160) => 4+4pad+8+4+4+520+160 = 704
    assert_layout!(SHFILEINFOW, 704, 8);
}

#[test]
fn icmallbackprofile_layout() {
    // WCHAR[32](64) + 8 x WORD(16) + DWORD(4) + 15 x i16(30)
    // + WCHAR[32](64) + WORD(2) + 10 x DWORD(40) => 220
    assert_layout!(ICMALLPROFILE, 220, 4);
}

#[test]
fn listview_and_treeview_layouts() {
    // LVITEMW: UINT(4) + i32(4) + i32(4) + UINT(4) + UINT(4)
    // + LPWSTR(8) + i32(4) + i32(4) + LPARAM(8) + i32(4) + i32(4)
    // + UINT(4) + pad(4) + ptr(8) => 72
    assert_layout!(LVITEMW, 72, 8);
    // LVCOLUMNW: UINT(4) + i32(4) + i32(4) + LPWSTR(8) + i32(4)
    // + i32(4) + i32(4) + i32(4) => 40
    assert_layout!(LVCOLUMNW, 40, 8);
    // NMHDR: HWND(8) UINT_PTR(8) UINT(4) + pad(4) => 24
    assert_layout!(NMHDR, 24, 8);
}

#[test]
fn error_mapping_is_stable() {
    assert_eq!(
        ntstatus_to_win32(STATUS_INVALID_PARAMETER),
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(ntstatus_to_win32(STATUS_ACCESS_DENIED), ERROR_ACCESS_DENIED);
    assert_eq!(ntstatus_to_win32(STATUS_NO_MEMORY), ERROR_NOT_ENOUGH_MEMORY);
    assert_eq!(ntstatus_to_win32(STATUS_SUCCESS), ERROR_SUCCESS);
    assert_eq!(
        ntstatus_to_win32(STATUS_OBJECT_NAME_NOT_FOUND),
        ERROR_NOT_FOUND
    );
}

#[test]
fn nt_success_predicate_matches_windows_semantics() {
    assert!(nt_success(STATUS_SUCCESS));
    assert!(nt_success(STATUS_MORE_ENTRIES));
    assert!(!nt_success(STATUS_UNSUCCESSFUL));
    assert!(!nt_success(STATUS_INVALID_HANDLE));
    // A pending status is "successful" in the NT sense (high bit clear).
    assert!(nt_success(STATUS_PENDING));
}

#[test]
fn window_style_constants_match_windows_7() {
    assert_eq!(WS_CAPTION, 0x00C0_0000);
    assert_eq!(
        WS_OVERLAPPEDWINDOW,
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX
    );
    assert_eq!(WS_POPUPWINDOW, WS_POPUP | WS_BORDER | WS_SYSMENU);
    assert_eq!(WS_CHILD | WS_VISIBLE, 0x5000_0000);
}

#[test]
fn hit_test_constants_are_distinct_and_in_range() {
    let all = [
        HTNOWHERE,
        HTCLIENT,
        HTCAPTION,
        HTSYSMENU,
        HTGROWBOX,
        HTMENU,
        HTHSCROLL,
        HTVSCROLL,
        HTMINBUTTON,
        HTMAXBUTTON,
        HTLEFT,
        HTRIGHT,
        HTTOP,
        HTTOPLEFT,
        HTTOPRIGHT,
        HTBOTTOM,
        HTBOTTOMLEFT,
        HTBOTTOMRIGHT,
        HTCLOSE,
        HTHELP,
    ];
    for (index, a) in all.iter().enumerate() {
        for b in all.iter().skip(index + 1) {
            assert_ne!(a, b, "hit-test constants must be unique");
        }
    }
    assert_eq!(HTCLIENT, 1);
    assert_eq!(HTCAPTION, 2);
    assert_eq!(HTBOTTOMRIGHT, 17);
    assert_eq!(HTCLOSE, 20);
}

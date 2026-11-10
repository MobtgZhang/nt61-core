//! Win32 constant blocks.
//!
//! Grouped by the header they come from in the Windows 7 SDK. Every
//! value is the Win7 x64 value; where Win32 has both a 32-bit and a
//! 64-bit variant of the same name, the x64 one is used.

// =====================================================================
//  winuser.h — window styles
// =====================================================================

/// `WS_OVERLAPPED`.
pub const WS_OVERLAPPED: u32 = 0x0000_0000;
/// `WS_POPUP`.
pub const WS_POPUP: u32 = 0x8000_0000;
/// `WS_CHILD`.
pub const WS_CHILD: u32 = 0x4000_0000;
/// `WS_MINIMIZE`.
pub const WS_MINIMIZE: u32 = 0x2000_0000;
/// `WS_VISIBLE`.
pub const WS_VISIBLE: u32 = 0x1000_0000;
/// `WS_DISABLED`.
pub const WS_DISABLED: u32 = 0x0800_0000;
/// `WS_CLIPSIBLINGS`.
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
/// `WS_CLIPCHILDREN`.
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
/// `WS_MAXIMIZE`.
pub const WS_MAXIMIZE: u32 = 0x0100_0000;
/// `WS_VSCROLL`.
pub const WS_VSCROLL: u32 = 0x0020_0000;
/// `WS_HSCROLL`.
pub const WS_HSCROLL: u32 = 0x0010_0000;
/// `WS_SYSMENU`.
pub const WS_SYSMENU: u32 = 0x0008_0000;
/// `WS_THICKFRAME`.
pub const WS_THICKFRAME: u32 = 0x0004_0000;
/// `WS_GROUP`.
pub const WS_GROUP: u32 = 0x0008_0000;
/// `WS_MINIMIZEBOX`.
pub const WS_MINIMIZEBOX: u32 = 0x0004_0000;
/// `WS_MAXIMIZEBOX`.
pub const WS_MAXIMIZEBOX: u32 = 0x0002_0000;
/// `WS_TABSTOP`.
pub const WS_TABSTOP: u32 = 0x0001_0000;
/// `WS_BORDER`.
pub const WS_BORDER: u32 = 0x0080_0000;
/// `WS_DLGFRAME`.
pub const WS_DLGFRAME: u32 = 0x0040_0000;
/// `WS_CAPTION`.
pub const WS_CAPTION: u32 = WS_BORDER | WS_DLGFRAME;
/// `WS_OVERLAPPEDWINDOW`.
pub const WS_OVERLAPPEDWINDOW: u32 =
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
/// `WS_POPUPWINDOW`.
pub const WS_POPUPWINDOW: u32 = WS_POPUP | WS_BORDER | WS_SYSMENU;

/// `WS_EX_TOPMOST`.
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
/// `WS_EX_ACCEPTFILES`.
pub const WS_EX_ACCEPTFILES: u32 = 0x0000_0010;
/// `WS_EX_TRANSPARENT`.
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
/// `WS_EX_MDICHILD`.
pub const WS_EX_MDICHILD: u32 = 0x0000_0040;
/// `WS_EX_TOOLWINDOW`.
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
/// `WS_EX_WINDOWEDGE`.
pub const WS_EX_WINDOWEDGE: u32 = 0x0000_0100;
/// `WS_EX_CLIENTEDGE`.
pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
/// `WS_EX_CONTEXTHELP`.
pub const WS_EX_CONTEXTHELP: u32 = 0x0000_0400;
/// `WS_EX_RIGHT`.
pub const WS_EX_RIGHT: u32 = 0x0000_1000;
/// `WS_EX_RTLREADING`.
pub const WS_EX_RTLREADING: u32 = 0x0000_2000;
/// `WS_EX_LEFTSCROLLBAR`.
pub const WS_EX_LEFTSCROLLBAR: u32 = 0x0000_4000;
/// `WS_EX_CONTROLPARENT`.
pub const WS_EX_CONTROLPARENT: u32 = 0x0001_0000;
/// `WS_EX_STATICEDGE`.
pub const WS_EX_STATICEDGE: u32 = 0x0002_0000;
/// `WS_EX_APPWINDOW`.
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;
/// `WS_EX_LAYERED`.
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
/// `WS_EX_NOINHERITLAYOUT`.
pub const WS_EX_NOINHERITLAYOUT: u32 = 0x0010_0000;
/// `WS_EX_LAYOUTRTL`.
pub const WS_EX_LAYOUTRTL: u32 = 0x0040_0000;
/// `WS_EX_COMPOSITED`.
pub const WS_EX_COMPOSITED: u32 = 0x0200_0000;
/// `WS_EX_NOACTIVATE`.
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
/// `WS_EX_NOREDIRECTIONBITMAP`.
pub const WS_EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;

// =====================================================================
//  winuser.h — class styles
// =====================================================================

/// `CS_VREDRAW`.
pub const CS_VREDRAW: u32 = 0x0001;
/// `CS_HREDRAW`.
pub const CS_HREDRAW: u32 = 0x0002;
/// `CS_OWNDC`.
pub const CS_OWNDC: u32 = 0x0020;
/// `CS_CLASSDC`.
pub const CS_CLASSDC: u32 = 0x0040;
/// `CS_PARENTDC`.
pub const CS_PARENTDC: u32 = 0x0080;
/// `CS_DBLCLKS`.
pub const CS_DBLCLKS: u32 = 0x0008;
/// `CS_NOCLOSE`.
pub const CS_NOCLOSE: u32 = 0x0200;
/// `CS_SAVEBITS`.
pub const CS_SAVEBITS: u32 = 0x0800;
/// `CS_BYTEALIGNCLIENT`.
pub const CS_BYTEALIGNCLIENT: u32 = 0x1000;
/// `CS_BYTEALIGNWINDOW`.
pub const CS_BYTEALIGNWINDOW: u32 = 0x2000;
/// `CS_GLOBALCLASS`.
pub const CS_GLOBALCLASS: u32 = 0x4000;
/// `CS_IME`.
pub const CS_IME: u32 = 0x0001_0000;

// =====================================================================
//  winuser.h — ShowWindow commands
// =====================================================================

/// `SW_HIDE`.
pub const SW_HIDE: i32 = 0;
/// `SW_SHOWNORMAL`.
pub const SW_SHOWNORMAL: i32 = 1;
/// `SW_SHOWMINIMIZED`.
pub const SW_SHOWMINIMIZED: i32 = 2;
/// `SW_SHOWMAXIMIZED`.
pub const SW_SHOWMAXIMIZED: i32 = 3;
/// `SW_MAXIMIZE`.
pub const SW_MAXIMIZE: i32 = 3;
/// `SW_SHOWNOACTIVATE`.
pub const SW_SHOWNOACTIVATE: i32 = 4;
/// `SW_SHOW`.
pub const SW_SHOW: i32 = 5;
/// `SW_MINIMIZE`.
pub const SW_MINIMIZE: i32 = 6;
/// `SW_SHOWMINNOACTIVE`.
pub const SW_SHOWMINNOACTIVE: i32 = 7;
/// `SW_SHOWNA`.
pub const SW_SHOWNA: i32 = 8;
/// `SW_RESTORE`.
pub const SW_RESTORE: i32 = 9;
/// `SW_SHOWDEFAULT`.
pub const SW_SHOWDEFAULT: i32 = 10;
/// `SW_FORCEMINIMIZE`.
pub const SW_FORCEMINIMIZE: i32 = 11;

// =====================================================================
//  winuser.h — messages
// =====================================================================

/// `WM_CREATE`.
pub const WM_CREATE: u32 = 0x0001;
/// `WM_DESTROY`.
pub const WM_DESTROY: u32 = 0x0002;
/// `WM_MOVE`.
pub const WM_MOVE: u32 = 0x0003;
/// `WM_SIZE`.
pub const WM_SIZE: u32 = 0x0005;
/// `WM_ACTIVATE`.
pub const WM_ACTIVATE: u32 = 0x0006;
/// `WM_SETFOCUS`.
pub const WM_SETFOCUS: u32 = 0x0007;
/// `WM_KILLFOCUS`.
pub const WM_KILLFOCUS: u32 = 0x0008;
/// `WM_ENABLE`.
pub const WM_ENABLE: u32 = 0x000A;
/// `WM_SETREDRAW`.
pub const WM_SETREDRAW: u32 = 0x000B;
/// `WM_SETTEXT`.
pub const WM_SETTEXT: u32 = 0x000C;
/// `WM_GETTEXT`.
pub const WM_GETTEXT: u32 = 0x000D;
/// `WM_GETTEXTLENGTH`.
pub const WM_GETTEXTLENGTH: u32 = 0x000E;
/// `WM_PAINT`.
pub const WM_PAINT: u32 = 0x000F;
/// `WM_CLOSE`.
pub const WM_CLOSE: u32 = 0x0010;
/// `WM_QUIT`.
pub const WM_QUIT: u32 = 0x0012;
/// `WM_ERASEBKGND`.
pub const WM_ERASEBKGND: u32 = 0x0014;
/// `WM_SHOWWINDOW`.
pub const WM_SHOWWINDOW: u32 = 0x0018;
/// `WM_ACTIVATEAPP`.
pub const WM_ACTIVATEAPP: u32 = 0x001C;
/// `WM_SETCURSOR`.
pub const WM_SETCURSOR: u32 = 0x0020;
/// `WM_GETCURSOR`.
pub const WM_GETCURSOR: u32 = 0x0021;
/// `WM_GETMINMAXINFO`.
pub const WM_GETMINMAXINFO: u32 = 0x0024;
/// `WM_WINDOWPOSCHANGING`.
pub const WM_WINDOWPOSCHANGING: u32 = 0x0046;
/// `WM_WINDOWPOSCHANGED`.
pub const WM_WINDOWPOSCHANGED: u32 = 0x0047;
/// `WM_NCCREATE`.
pub const WM_NCCREATE: u32 = 0x0081;
/// `WM_NCCALCSIZE`.
pub const WM_NCCALCSIZE: u32 = 0x0083;
/// `WM_NCHITTEST`.
pub const WM_NCHITTEST: u32 = 0x0084;
/// `WM_NCPAINT`.
pub const WM_NCPAINT: u32 = 0x0085;
/// `WM_NCACTIVATE`.
pub const WM_NCACTIVATE: u32 = 0x0086;
/// `WM_NCMOUSEMOVE`.
pub const WM_NCMOUSEMOVE: u32 = 0x00A0;
/// `WM_NCLBUTTONDOWN`.
pub const WM_NCLBUTTONDOWN: u32 = 0x00A1;
/// `WM_NCLBUTTONUP`.
pub const WM_NCLBUTTONUP: u32 = 0x00A2;
/// `WM_NCRBUTTONDOWN`.
pub const WM_NCRBUTTONDOWN: u32 = 0x00A4;
/// `WM_NCRBUTTONUP`.
pub const WM_NCRBUTTONUP: u32 = 0x00A5;
/// `WM_INITMENUPOPUP`.
pub const WM_INITMENUPOPUP: u32 = 0x00E7;
/// `WM_MENUSELECT`.
pub const WM_MENUSELECT: u32 = 0x00E9;
/// `WM_COMMAND`.
pub const WM_COMMAND: u32 = 0x0111;
/// `WM_SYSCOMMAND`.
pub const WM_SYSCOMMAND: u32 = 0x0112;
/// `WM_TIMER`.
pub const WM_TIMER: u32 = 0x0113;
/// `WM_KEYDOWN`.
pub const WM_KEYDOWN: u32 = 0x0100;
/// `WM_KEYUP`.
pub const WM_KEYUP: u32 = 0x0101;
/// `WM_CHAR`.
pub const WM_CHAR: u32 = 0x0102;
/// `WM_SYSKEYDOWN`.
pub const WM_SYSKEYDOWN: u32 = 0x0104;
/// `WM_SYSKEYUP`.
pub const WM_SYSKEYUP: u32 = 0x0105;
/// `WM_SYSCHAR`.
pub const WM_SYSCHAR: u32 = 0x0106;
/// `WM_MOUSEMOVE`.
pub const WM_MOUSEMOVE: u32 = 0x0200;
/// `WM_LBUTTONDOWN`.
pub const WM_LBUTTONDOWN: u32 = 0x0201;
/// `WM_LBUTTONUP`.
pub const WM_LBUTTONUP: u32 = 0x0202;
/// `WM_LBUTTONDBLCLK`.
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
/// `WM_RBUTTONDOWN`.
pub const WM_RBUTTONDOWN: u32 = 0x0204;
/// `WM_RBUTTONUP`.
pub const WM_RBUTTONUP: u32 = 0x0205;
/// `WM_RBUTTONDBLCLK`.
pub const WM_RBUTTONDBLCLK: u32 = 0x0206;
/// `WM_MBUTTONDOWN`.
pub const WM_MBUTTONDOWN: u32 = 0x0207;
/// `WM_MBUTTONUP`.
pub const WM_MBUTTONUP: u32 = 0x0208;
/// `WM_MBUTTONDBLCLK`.
pub const WM_MBUTTONDBLCLK: u32 = 0x0209;
/// `WM_MOUSEWHEEL`.
pub const WM_MOUSEWHEEL: u32 = 0x020A;
/// `WM_MOUSEHWHEEL`.
pub const WM_MOUSEHWHEEL: u32 = 0x020B;
/// `WM_MOUSEFIRST`.
pub const WM_MOUSEFIRST: u32 = 0x0200;
/// `WM_MOUSELAST`.
pub const WM_MOUSELAST: u32 = 0x020B;
/// `WM_NCMOUSEFIRST`.
pub const WM_NCMOUSEFIRST: u32 = 0x00A0;
/// `WM_NCMOUSELAST`.
pub const WM_NCMOUSELAST: u32 = 0x00A6;
/// `WM_COPYDATA`.
pub const WM_COPYDATA: u32 = 0x004A;
/// `WM_DROPFILES`.
pub const WM_DROPFILES: u32 = 0x0233;
/// `WM_APP`.
pub const WM_APP: u32 = 0x8000;
/// `WM_USER`.
pub const WM_USER: u32 = 0x0400;
/// `WM_NULL`.
pub const WM_NULL: u32 = 0x0000;
/// `WM_INPUT`.
pub const WM_INPUT: u32 = 0x00FF;
/// `WM_KEYFIRST`.
pub const WM_KEYFIRST: u32 = 0x0100;
/// `WM_KEYLAST`.
pub const WM_KEYLAST: u32 = 0x0108;
/// `WM_NOTIFY`.
pub const WM_NOTIFY: u32 = 0x004E;
/// `WM_CTLCOLORMSGBOX`.
pub const WM_CTLCOLORMSGBOX: u32 = 0x0130;
/// `WM_CTLCOLOREDIT`.
pub const WM_CTLCOLOREDIT: u32 = 0x0133;
/// `WM_CTLCOLORBTN`.
pub const WM_CTLCOLORBTN: u32 = 0x0135;
/// `WM_CTLCOLORSCROLLBAR`.
pub const WM_CTLCOLORSCROLLBAR: u32 = 0x0138;
/// `WM_CTLCOLORSTATIC`.
pub const WM_CTLCOLORSTATIC: u32 = 0x0137;

/// `WM_THEMECHANGED`.
pub const WM_THEMECHANGED: u32 = 0x031D;
/// `WM_SYSCOLORCHANGE`.
pub const WM_SYSCOLORCHANGE: u32 = 0x0315;
/// `WM_SETTINGCHANGE`.
pub const WM_SETTINGCHANGE: u32 = 0x001A;
/// `WM_DPICHANGED`.
pub const WM_DPICHANGED: u32 = 0x02E0;

// =====================================================================
//  winuser.h — hit test results
// =====================================================================

/// `HTNOWHERE`.
pub const HTNOWHERE: isize = 0;
/// `HTCLIENT`.
pub const HTCLIENT: isize = 1;
/// `HTCAPTION`.
pub const HTCAPTION: isize = 2;
/// `HTSYSMENU`.
pub const HTSYSMENU: isize = 3;
/// `HTGROWBOX`.
pub const HTGROWBOX: isize = 4;
/// `HTMENU`.
pub const HTMENU: isize = 5;
/// `HTHSCROLL`.
pub const HTHSCROLL: isize = 6;
/// `HTVSCROLL`.
pub const HTVSCROLL: isize = 7;
/// `HTMINBUTTON`.
pub const HTMINBUTTON: isize = 8;
/// `HTMAXBUTTON`.
pub const HTMAXBUTTON: isize = 9;
/// `HTLEFT`.
pub const HTLEFT: isize = 10;
/// `HTRIGHT`.
pub const HTRIGHT: isize = 11;
/// `HTTOP`.
pub const HTTOP: isize = 12;
/// `HTTOPLEFT`.
pub const HTTOPLEFT: isize = 13;
/// `HTTOPRIGHT`.
pub const HTTOPRIGHT: isize = 14;
/// `HTBOTTOM`.
pub const HTBOTTOM: isize = 15;
/// `HTBOTTOMLEFT`.
pub const HTBOTTOMLEFT: isize = 16;
/// `HTBOTTOMRIGHT`.
pub const HTBOTTOMRIGHT: isize = 17;
/// `HTCLOSE`.
pub const HTCLOSE: isize = 20;
/// `HTHELP`.
pub const HTHELP: isize = 21;

/// `WM_NCHITTEST` `lParam` low word.
pub const GET_X_LPARAM: u32 = 0xFFFF;

/// =====================================================================
//  winuser.h — GetSystemMetrics indices
// =====================================================================

/// `SM_CXSCREEN`.
pub const SM_CXSCREEN: i32 = 0;
/// `SM_CYSCREEN`.
pub const SM_CYSCREEN: i32 = 1;
/// `SM_CXVSCROLL`.
pub const SM_CXVSCROLL: i32 = 2;
/// `SM_CYHSCROLL`.
pub const SM_CYHSCROLL: i32 = 3;
/// `SM_CYCAPTION`.
pub const SM_CYCAPTION: i32 = 4;
/// `SM_CXCAPTION`.
pub const SM_CXCAPTION: i32 = 5;
/// `SM_CXMENU`.
pub const SM_CXMENU: i32 = 15;
/// `SM_CYMENU`.
pub const SM_CYMENU: i32 = 16;
/// `SM_CXSCROLLBAR`.
pub const SM_CXSCROLLBAR: i32 = 17;
/// `SM_CYSCROLLBAR`.
pub const SM_CYSCROLLBAR: i32 = 18;
/// `SM_CXSIZE`.
pub const SM_CXSIZE: i32 = 20;
/// `SM_CYSIZE`.
pub const SM_CYSIZE: i32 = 21;
/// `SM_CXMIN`.
pub const SM_CXMIN: i32 = 22;
/// `SM_CYMIN`.
pub const SM_CYMIN: i32 = 23;
/// `SM_CXMAX`.
pub const SM_CXMAX: i32 = 24;
/// `SM_CYMAX`.
pub const SM_CYMAX: i32 = 25;
/// `SM_CXFRAME`.
pub const SM_CXFRAME: i32 = 32;
/// `SM_CYFRAME`.
pub const SM_CYFRAME: i32 = 33;
/// `SM_CXICON`.
pub const SM_CXICON: i32 = 34;
/// `SM_CYICON`.
pub const SM_CYICON: i32 = 35;
/// `SM_CXDOUBLECLK`.
pub const SM_CXDOUBLECLK: i32 = 36;
/// `SM_CYDOUBLECLK`.
pub const SM_CYDOUBLECLK: i32 = 37;
/// `SM_CXICONSPACING`.
pub const SM_CXICONSPACING: i32 = 38;
/// `SM_CXBORDER`.
pub const SM_CXBORDER: i32 = 39;
/// `SM_CYBORDER`.
pub const SM_CYBORDER: i32 = 40;
/// `SM_CXBORDERWIDTH`.
pub const SM_CXBORDERWIDTH: i32 = 41;
/// `SM_CXSMICON`.
pub const SM_CXSMICON: i32 = 49;
/// `SM_CYSMICON`.
pub const SM_CYSMICON: i32 = 50;
/// `SM_CXMAXIMIZED`.
pub const SM_CXMAXIMIZED: i32 = 61;
/// `SM_CYMAXIMIZED`.
pub const SM_CYMAXIMIZED: i32 = 62;
/// `SM_MENUDROPALIGN`.
pub const SM_MENUDROPALIGN: i32 = 37;
/// `SM_CXSIZEFRAME` — same index as `SM_CXFRAME`.
pub const SM_CXSIZEFRAME: i32 = 32;
/// `SM_CYSIZEFRAME` — same index as `SM_CYFRAME`.
pub const SM_CYSIZEFRAME: i32 = 33;
/// `SM_CXCOMBOBOX`.
pub const SM_CXCOMBOBOX: i32 = 6;
/// `SM_CYCOMBOBOX`.
pub const SM_CYCOMBOBOX: i32 = 7;

// =====================================================================
//  winuser.h — GetSysColor indices
// =====================================================================

/// `COLOR_SCROLLBAR`.
pub const COLOR_SCROLLBAR: i32 = 0;
/// `COLOR_BACKGROUND`.
pub const COLOR_BACKGROUND: i32 = 1;
/// `COLOR_ACTIVECAPTION`.
pub const COLOR_ACTIVECAPTION: i32 = 2;
/// `COLOR_INACTIVECAPTION`.
pub const COLOR_INACTIVECAPTION: i32 = 3;
/// `COLOR_MENU`.
pub const COLOR_MENU: i32 = 4;
/// `COLOR_WINDOW`.
pub const COLOR_WINDOW: i32 = 5;
/// `COLOR_WINDOWFRAME`.
pub const COLOR_WINDOWFRAME: i32 = 6;
/// `COLOR_MENUTEXT`.
pub const COLOR_MENUTEXT: i32 = 7;
/// `COLOR_WINDOWTEXT`.
pub const COLOR_WINDOWTEXT: i32 = 8;
/// `COLOR_CAPTIONTEXT`.
pub const COLOR_CAPTIONTEXT: i32 = 9;
/// `COLOR_BORDER`.
pub const COLOR_BORDER: i32 = 10;
/// `COLOR_3DDKSHADOW`.
pub const COLOR_3DDKSHADOW: i32 = 21;
/// `COLOR_3DLIGHT`.
pub const COLOR_3DLIGHT: i32 = 22;
/// `COLOR_3DDKSHADOW_TEXT`.
pub const COLOR_3DDKSHADOW_TEXT: i32 = 21;

// =====================================================================
//  winuser.h — virtual keys
// =====================================================================

/// `VK_LBUTTON`.
pub const VK_LBUTTON: u32 = 0x01;
/// `VK_RBUTTON`.
pub const VK_RBUTTON: u32 = 0x02;
/// `VK_CANCEL`.
pub const VK_CANCEL: u32 = 0x03;
/// `VK_MBUTTON`.
pub const VK_MBUTTON: u32 = 0x04;
/// `VK_BACK`.
pub const VK_BACK: u32 = 0x08;
/// `VK_TAB`.
pub const VK_TAB: u32 = 0x09;
/// `VK_RETURN`.
pub const VK_RETURN: u32 = 0x0D;
/// `VK_SHIFT`.
pub const VK_SHIFT: u32 = 0x10;
/// `VK_CONTROL`.
pub const VK_CONTROL: u32 = 0x11;
/// `VK_MENU`.
pub const VK_MENU: u32 = 0x12;
/// `VK_PAUSE`.
pub const VK_PAUSE: u32 = 0x13;
/// `VK_CAPITAL`.
pub const VK_CAPITAL: u32 = 0x14;
/// `VK_ESCAPE`.
pub const VK_ESCAPE: u32 = 0x1B;
/// `VK_SPACE`.
pub const VK_SPACE: u32 = 0x20;
/// `VK_PRIOR`.
pub const VK_PRIOR: u32 = 0x21;
/// `VK_NEXT`.
pub const VK_NEXT: u32 = 0x22;
/// `VK_END`.
pub const VK_END: u32 = 0x23;
/// `VK_HOME`.
pub const VK_HOME: u32 = 0x24;
/// `VK_LEFT`.
pub const VK_LEFT: u32 = 0x25;
/// `VK_UP`.
pub const VK_UP: u32 = 0x26;
/// `VK_RIGHT`.
pub const VK_RIGHT: u32 = 0x27;
/// `VK_DOWN`.
pub const VK_DOWN: u32 = 0x28;
/// `VK_SELECT`.
pub const VK_SELECT: u32 = 0x29;
/// `VK_PRINT`.
pub const VK_PRINT: u32 = 0x2A;
/// `VK_EXECUTE`.
pub const VK_EXECUTE: u32 = 0x2B;
/// `VK_SNAPSHOT`.
pub const VK_SNAPSHOT: u32 = 0x2C;
/// `VK_INSERT`.
pub const VK_INSERT: u32 = 0x2D;
/// `VK_DELETE`.
pub const VK_DELETE: u32 = 0x2E;
/// `VK_HELP`.
pub const VK_HELP: u32 = 0x2F;
/// `VK_LWIN`.
pub const VK_LWIN: u32 = 0x5B;
/// `VK_RWIN`.
pub const VK_RWIN: u32 = 0x5C;
/// `VK_APPS`.
pub const VK_APPS: u32 = 0x5D;
/// `VK_NUMPAD0`.
pub const VK_NUMPAD0: u32 = 0x60;
/// `VK_NUMPAD1`.
pub const VK_NUMPAD1: u32 = 0x61;
/// `VK_NUMPAD2`.
pub const VK_NUMPAD2: u32 = 0x62;
/// `VK_NUMPAD3`.
pub const VK_NUMPAD3: u32 = 0x63;
/// `VK_NUMPAD4`.
pub const VK_NUMPAD4: u32 = 0x64;
/// `VK_NUMPAD5`.
pub const VK_NUMPAD5: u32 = 0x65;
/// `VK_NUMPAD6`.
pub const VK_NUMPAD6: u32 = 0x66;
/// `VK_NUMPAD7`.
pub const VK_NUMPAD7: u32 = 0x67;
/// `VK_NUMPAD8`.
pub const VK_NUMPAD8: u32 = 0x68;
/// `VK_NUMPAD9`.
pub const VK_NUMPAD9: u32 = 0x69;
/// `VK_MULTIPLY`.
pub const VK_MULTIPLY: u32 = 0x6A;
/// `VK_ADD`.
pub const VK_ADD: u32 = 0x6B;
/// `VK_SEPARATOR`.
pub const VK_SEPARATOR: u32 = 0x6C;
/// `VK_SUBTRACT`.
pub const VK_SUBTRACT: u32 = 0x6D;
/// `VK_DECIMAL`.
pub const VK_DECIMAL: u32 = 0x6E;
/// `VK_DIVIDE`.
pub const VK_DIVIDE: u32 = 0x6F;
/// `VK_F1`.
pub const VK_F1: u32 = 0x70;
/// `VK_F2`.
pub const VK_F2: u32 = 0x71;
/// `VK_F3`.
pub const VK_F3: u32 = 0x72;
/// `VK_F4`.
pub const VK_F4: u32 = 0x73;
/// `VK_F5`.
pub const VK_F5: u32 = 0x74;
/// `VK_F6`.
pub const VK_F6: u32 = 0x75;
/// `VK_F7`.
pub const VK_F7: u32 = 0x76;
/// `VK_F8`.
pub const VK_F8: u32 = 0x77;
/// `VK_F9`.
pub const VK_F9: u32 = 0x78;
/// `VK_F10`.
pub const VK_F10: u32 = 0x79;
/// `VK_F11`.
pub const VK_F11: u32 = 0x7A;
/// `VK_F12`.
pub const VK_F12: u32 = 0x7B;
/// `VK_NUMLOCK`.
pub const VK_NUMLOCK: u32 = 0x90;
/// `VK_SCROLL`.
pub const VK_SCROLL: u32 = 0x91;
/// `VK_LSHIFT`.
pub const VK_LSHIFT: u32 = 0xA0;
/// `VK_RSHIFT`.
pub const VK_RSHIFT: u32 = 0xA1;
/// `VK_LCONTROL`.
pub const VK_LCONTROL: u32 = 0xA2;
/// `VK_RCONTROL`.
pub const VK_RCONTROL: u32 = 0xA3;
/// `VK_LMENU`.
pub const VK_LMENU: u32 = 0xA4;
/// `VK_RMENU`.
pub const VK_RMENU: u32 = 0xA5;
/// `VK_OEM_1`.
pub const VK_OEM_1: u32 = 0xBA;
/// `VK_OEM_PLUS`.
pub const VK_OEM_PLUS: u32 = 0xBB;
/// `VK_OEM_COMMA`.
pub const VK_OEM_COMMA: u32 = 0xBC;
/// `VK_OEM_MINUS`.
pub const VK_OEM_MINUS: u32 = 0xBD;
/// `VK_OEM_PERIOD`.
pub const VK_OEM_PERIOD: u32 = 0xBE;
/// `VK_OEM_2`.
pub const VK_OEM_2: u32 = 0xBF;
/// `VK_OEM_3`.
pub const VK_OEM_3: u32 = 0xC0;
/// `VK_OEM_4`.
pub const VK_OEM_4: u32 = 0xDB;
/// `VK_OEM_5`.
pub const VK_OEM_5: u32 = 0xDC;
/// `VK_OEM_6`.
pub const VK_OEM_6: u32 = 0xDD;
/// `VK_OEM_7`.
pub const VK_OEM_7: u32 = 0xDE;

// =====================================================================
//  winuser.h — mouse button state
// =====================================================================

/// `MK_LBUTTON`.
pub const MK_LBUTTON: u32 = 0x0001;
/// `MK_RBUTTON`.
pub const MK_RBUTTON: u32 = 0x0002;
/// `MK_SHIFT`.
pub const MK_SHIFT: u32 = 0x0004;
/// `MK_CONTROL`.
pub const MK_CONTROL: u32 = 0x0008;
/// `MK_MBUTTON`.
pub const MK_MBUTTON: u32 = 0x0010;

// =====================================================================
//  winuser.h — window messages (WINDOWPOS)
// =====================================================================

/// `SWP_NOSIZE`.
pub const SWP_NOSIZE: u32 = 0x0001;
/// `SWP_NOMOVE`.
pub const SWP_NOMOVE: u32 = 0x0002;
/// `SWP_NOZORDER`.
pub const SWP_NOZORDER: u32 = 0x0004;
/// `SWP_NOREDRAW`.
pub const SWP_NOREDRAW: u32 = 0x0008;
/// `SWP_NOACTIVATE`.
pub const SWP_NOACTIVATE: u32 = 0x0010;
/// `SWP_FRAMECHANGED`.
pub const SWP_FRAMECHANGED: u32 = 0x0020;
/// `SWP_SHOWWINDOW`.
pub const SWP_SHOWWINDOW: u32 = 0x0040;
/// `SWP_HIDEWINDOW`.
pub const SWP_HIDEWINDOW: u32 = 0x0080;
/// `SWP_NOOWNERZORDER`.
pub const SWP_NOOWNERZORDER: u32 = 0x0200;
/// `SWP_NOSENDCHANGING`.
pub const SWP_NOSENDCHANGING: u32 = 0x0400;

// =====================================================================
//  GetWindowLongPtr indices
// =====================================================================

/// `GWL_WNDPROC`.
pub const GWL_WNDPROC: i32 = -4;
/// `GWL_HINSTANCE`.
pub const GWL_HINSTANCE: i32 = -6;
/// `GWL_HWNDPARENT`.
pub const GWL_HWNDPARENT: i32 = -8;
/// `GWL_STYLE`.
pub const GWL_STYLE: i32 = -16;
/// `GWL_EXSTYLE`.
pub const GWL_EXSTYLE: i32 = -20;
/// `GWL_USERDATA`.
pub const GWL_USERDATA: i32 = -21;
/// `GWL_ID`.
pub const GWL_ID: i32 = -12;

// =====================================================================
//  Menu flags
// =====================================================================

/// `MF_STRING`.
pub const MF_STRING: u32 = 0x0000_0000;
/// `MF_SEPARATOR`.
pub const MF_SEPARATOR: u32 = 0x0000_0800;
/// `MF_POPUP`.
pub const MF_POPUP: u32 = 0x0000_0010;
/// `MF_CHECKED`.
pub const MF_CHECKED: u32 = 0x0000_0008;
/// `MF_GRAYED`.
pub const MF_GRAYED: u32 = 0x0000_0001;
/// `MF_DISABLED`.
pub const MF_DISABLED: u32 = 0x0000_0002;
/// `MF_BITMAP`.
pub const MF_BITMAP: u32 = 0x0000_0004;
/// `MF_BYCOMMAND`.
pub const MF_BYCOMMAND: u32 = 0x0000_0000;
/// `MF_BYPOSITION`.
pub const MF_BYPOSITION: u32 = 0x0000_0400;
/// `MF_ENABLED`.
pub const MF_ENABLED: u32 = 0x0000_0000;

/// `TPM_LEFTBUTTON`.
pub const TPM_LEFTBUTTON: u32 = 0x0001;
/// `TPM_RIGHTBUTTON`.
pub const TPM_RIGHTBUTTON: u32 = 0x0002;
/// `TPM_CANCELALLCANCEL`.
pub const TPM_CANCELALLCANCEL: u32 = 0x0008;
/// `TPM_RETURNCMD`.
pub const TPM_RETURNCMD: u32 = 0x0100;
/// `TPM_NONOTIFY`.
pub const TPM_NONOTIFY: u32 = 0x0080;

// =====================================================================
//  SystemParametersInfo
// =====================================================================

/// `SPI_GETWORKAREA`.
pub const SPI_GETWORKAREA: u32 = 0x0030;
/// `SPI_SETWORKAREA`.
pub const SPI_SETWORKAREA: u32 = 0x002F;
/// `SPI_GETNONCLIENTMETRICS`.
pub const SPI_GETNONCLIENTMETRICS: u32 = 0x0029;
/// `SPI_GETKEYBOARDSIZE`.
pub const SPI_GETKEYBOARDSIZE: u32 = 0x000A;
/// `SPI_GETSCREENMETRICS`.
pub const SPI_GETSCREENMETRICS: u32 = 0x0070;
/// `SPI_GETSMALLICONMETRICS`.
pub const SPI_GETSMALLICONMETRICS: u32 = 0x0071;
/// `SPI_GETICONMETRICS`.
pub const SPI_GETICONMETRICS: u32 = 0x0072;
/// `SPI_SETWORKAREA`.
pub const SPI_SETWORKAREA_UI: u32 = 0x002F;

/// `SPIF_UPDATEINIFILE`.
pub const SPIF_UPDATEINIFILE: u32 = 0x0001;
/// `SPIF_SENDCHANGE`.
pub const SPIF_SENDCHANGE: u32 = 0x0002;

// =====================================================================
//  Win7 system colours (GetSysColor values)
// =====================================================================

/// Win7 default `COLOR_SCROLLBAR`.
pub const SYSCOLOR_SCROLLBAR: u32 = 0x00E1_E4E0;
/// Win7 default `COLOR_BACKGROUND`.
pub const SYSCOLOR_BACKGROUND: u32 = 0x00C8_C3B7;
/// Win7 default `COLOR_ACTIVECAPTION`.
pub const SYSCOLOR_ACTIVECAPTION: u32 = 0x0099_4E2E;
/// Win7 default `COLOR_INACTIVECAPTION`.
pub const SYSCOLOR_INACTIVECAPTION: u32 = 0x00BF_C8D3;
/// Win7 default `COLOR_MENU`.
pub const SYSCOLOR_MENU: u32 = 0x00F0_F0F0;
/// Win7 default `COLOR_WINDOW`.
pub const SYSCOLOR_WINDOW: u32 = 0x00F0_F0F0;
/// Win7 default `COLOR_WINDOWFRAME`.
pub const SYSCOLOR_WINDOWFRAME: u32 = 0x0064_6A6F;
/// Win7 default `COLOR_MENUTEXT`.
pub const SYSCOLOR_MENUTEXT: u32 = 0x0000_0000;
/// Win7 default `COLOR_WINDOWTEXT`.
pub const SYSCOLOR_WINDOWTEXT: u32 = 0x0000_0000;
/// Win7 default `COLOR_CAPTIONTEXT`.
pub const SYSCOLOR_CAPTIONTEXT: u32 = 0x0000_0000;
/// Win7 default `COLOR_BORDER`.
pub const SYSCOLOR_BORDER: u32 = 0x00C0_C0C0;

// =====================================================================
//  DWM attributes (dwmapi.h)
// =====================================================================

/// `DWMWA_USE_IMMERSIVE_DARK_MODE` (undocumented on Win7, but the
/// attribute id is stable and harmless on Win7).
pub const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
/// `DWMWA_NCRENDERING_POLICY`.
pub const DWMWA_NCRENDERING_POLICY: u32 = 2;
/// `DWMWA_TRANSITIONS_FORCEDISABLED`.
pub const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
/// `DWMWA_ALLOWED_NCPAINT`.
pub const DWMWA_ALLOWED_NCPAINT: u32 = 4;
/// `DWMWA_EXTENDED_FRAME_BOUNDS`.
pub const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;
/// `DWMWA_HAS_ICONIC_BITMAP`.
pub const DWMWA_HAS_ICONIC_BITMAP: u32 = 10;
/// `DWMWA_DISALLOW_PEEK`.
pub const DWMWA_DISALLOW_PEEK: u32 = 11;
/// `DWMWA_EXCLUDED_FROM_PEEK`.
pub const DWMWA_EXCLUDED_FROM_PEEK: u32 = 12;
/// `DWMWA_CLOAK`.
pub const DWMWA_CLOAK: u32 = 13;
/// `DWMWA_CLOAKED_FRAME`.
pub const DWMWA_CLOAKED_FRAME: u32 = 14;
/// `DWMWA_FREEZE_REPRESENTATION`.
pub const DWMWA_FREEZE_REPRESENTATION: u32 = 15;
/// `DWMWA_PASSIVE_UPDATE_MODE`.
pub const DWMWA_PASSIVE_UPDATE_MODE: u32 = 16;
/// `DWMWA_USE_HOSTBACKDROPBRUSH`.
pub const DWMWA_USE_HOSTBACKDROPBRUSH: u32 = 17;

/// `DWMNCRENDERINGPOLICY_DWMENABLED`.
pub const DWMNCRENDERINGPOLICY_DWMENABLED: i32 = 2;
/// `DWMNCRENDERINGPOLICY_WINDOWED`.
pub const DWMNCRENDERINGPOLICY_WINDOWED: i32 = 1;
/// `DWMNCRENDERINGPOLICY_DISABLED`.
pub const DWMNCRENDERINGPOLICY_DISABLED: i32 = 0;

//! Control-class specific constants (winuser.h / commctrl.h).
//!
//! These are grouped per control so a control implementation can pull
//! exactly the flags it needs.

// =====================================================================
//  BUTTON
// =====================================================================

/// `BS_PUSHBUTTON`.
pub const BS_PUSHBUTTON: u32 = 0x0000_0000;
/// `BS_DEFPUSHBUTTON`.
pub const BS_DEFPUSHBUTTON: u32 = 0x0000_0001;
/// `BS_CHECKBOX`.
pub const BS_CHECKBOX: u32 = 0x0000_0002;
/// `BS_AUTOCHECKBOX`.
pub const BS_AUTOCHECKBOX: u32 = 0x0000_0003;
/// `BS_RADIOBUTTON`.
pub const BS_RADIOBUTTON: u32 = 0x0000_0004;
/// `BS_AUTORADIOBUTTON`.
pub const BS_AUTORADIOBUTTON: u32 = 0x0000_0009;
/// `BS_GROUPBOX`.
pub const BS_GROUPBOX: u32 = 0x0000_0007;
/// `BS_AUTO2STATE`.
pub const BS_AUTO2STATE: u32 = 0x0000_0011;
/// `BS_3STATE`.
pub const BS_3STATE: u32 = 0x0000_0005;
/// `BS_AUTO3STATE`.
pub const BS_AUTO3STATE: u32 = 0x0000_0006;
/// `BS_OWNERDRAW`.
pub const BS_OWNERDRAW: u32 = 0x0000_000B;
/// `BS_LEFTTEXT`.
pub const BS_LEFTTEXT: u32 = 0x0000_0020;

/// `BST_UNCHECKED`.
pub const BST_UNCHECKED: u32 = 0x0000;
/// `BST_CHECKED`.
pub const BST_CHECKED: u32 = 0x0001;
/// `BST_INDETERMINATE`.
pub const BST_INDETERMINATE: u32 = 0x0002;
/// `BST_PUSHED`.
pub const BST_PUSHED: u32 = 0x0004;
/// `BST_FOCUS`.
pub const BST_FOCUS: u32 = 0x0008;
/// `BST_HOT`.
pub const BST_HOT: u32 = 0x2000;

// =====================================================================
//  EDIT
// =====================================================================

/// `ES_LEFT`.
pub const ES_LEFT: u32 = 0x0000;
/// `ES_CENTER`.
pub const ES_CENTER: u32 = 0x0001;
/// `ES_RIGHT`.
pub const ES_RIGHT: u32 = 0x0002;
/// `ES_MULTILINE`.
pub const ES_MULTILINE: u32 = 0x0004;
/// `ES_UPPERCASE`.
pub const ES_UPPERCASE: u32 = 0x0008;
/// `ES_LOWERCASE`.
pub const ES_LOWERCASE: u32 = 0x0010;
/// `ES_PASSWORD`.
pub const ES_PASSWORD: u32 = 0x0020;
/// `ES_AUTOVSCROLL`.
pub const ES_AUTOVSCROLL: u32 = 0x0040;
/// `ES_AUTOHSCROLL`.
pub const ES_AUTOHSCROLL: u32 = 0x0080;
/// `ES_NOHIDESEL`.
pub const ES_NOHIDESEL: u32 = 0x0100;
/// `ES_OEMCONVERT`.
pub const ES_OEMCONVERT: u32 = 0x0400;
/// `ES_READONLY`.
pub const ES_READONLY: u32 = 0x0800;
/// `ES_WANTRETURN`.
pub const ES_WANTRETURN: u32 = 0x1000;
/// `ES_AUTOWORDWRAP`.
pub const ES_AUTOWORDWRAP: u32 = 0x2000;

// =====================================================================
//  STATIC
// =====================================================================

/// `SS_LEFT`.
pub const SS_LEFT: u32 = 0x0000;
/// `SS_CENTER`.
pub const SS_CENTER: u32 = 0x0001;
/// `SS_RIGHT`.
pub const SS_RIGHT: u32 = 0x0002;
/// `SS_ICON`.
pub const SS_ICON: u32 = 0x0003;
/// `SS_BITMAP`.
pub const SS_BITMAP: u32 = 0x000E;
/// `SS_NOPREFIX`.
pub const SS_NOPREFIX: u32 = 0x0080;
/// `SS_NOTIFY`.
pub const SS_NOTIFY: u32 = 0x0100;
/// `SS_SUNKEN`.
pub const SS_SUNKEN: u32 = 0x1000;
/// `SS_EDITCONTROL`.
pub const SS_EDITCONTROL: u32 = 0x2000;
/// `SS_ENDELLIPSIS`.
pub const SS_ENDELLIPSIS: u32 = 0x4000;

// =====================================================================
//  LISTBOX
// =====================================================================

/// `LBS_NOTIFY`.
pub const LBS_NOTIFY: u32 = 0x0001;
/// `LBS_SORT`.
pub const LBS_SORT: u32 = 0x0002;
/// `LBS_HASSTRINGS`.
pub const LBS_HASSTRINGS: u32 = 0x0040;
/// `LBS_OWNERDRAWFIXED`.
pub const LBS_OWNERDRAWFIXED: u32 = 0x0010;
/// `LBS_NOINTEGRALHEIGHT`.
pub const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;
/// `LBS_MULTIPLESEL`.
pub const LBS_MULTIPLESEL: u32 = 0x0008;
/// `LBS_EXTENDEDSEL`.
pub const LBS_EXTENDEDSEL: u32 = 0x0800;
/// `LBS_USETABSTOPS`.
pub const LBS_USETABSTOPS: u32 = 0x0200;

/// `LBN_SELCHANGE`.
pub const LBN_SELCHANGE: i32 = 1;
/// `LBN_DBLCLK`.
pub const LBN_DBLCLK: i32 = 2;

// =====================================================================
//  COMBOBOX
// =====================================================================

/// `CBS_SIMPLE`.
pub const CBS_SIMPLE: u32 = 0x0001;
/// `CBS_DROPDOWN`.
pub const CBS_DROPDOWN: u32 = 0x0002;
/// `CBS_DROPDOWNLIST`.
pub const CBS_DROPDOWNLIST: u32 = 0x0003;
/// `CBS_AUTOHSCROLL`.
pub const CBS_AUTOHSCROLL: u32 = 0x0040;
/// `CBS_OWNERDRAWFIXED`.
pub const CBS_OWNERDRAWFIXED: u32 = 0x0010;
/// `CBS_HASSTRINGS`.
pub const CBS_HASSTRINGS: u32 = 0x0200;
/// `CBS_SORT`.
pub const CBS_SORT: u32 = 0x0004;

/// `CBN_SELCHANGE`.
pub const CBN_SELCHANGE: i32 = 1;
/// `CBN_SELENDOK`.
pub const CBN_SELENDOK: i32 = 5;
/// `CBN_CLOSEUP`.
pub const CBN_CLOSEUP: i32 = 8;

// =====================================================================
//  SysListView32
// =====================================================================

/// `LVS_ICON`.
pub const LVS_ICON: u32 = 0x0000;
/// `LVS_SMALLICON`.
pub const LVS_SMALLICON: u32 = 0x0002;
/// `LVS_LIST`.
pub const LVS_LIST: u32 = 0x0003;
/// `LVS_REPORT`.
pub const LVS_REPORT: u32 = 0x0001;
/// `LVS_TILE`.
pub const LVS_TILE: u32 = 0x0004;
/// `LVS_SINGLESEL`.
pub const LVS_SINGLESEL: u32 = 0x0000;
/// `LVS_MULTISELECT`.
pub const LVS_MULTISELECT: u32 = 0x0020;
/// `LVS_SHOWSELALWAYS`.
pub const LVS_SHOWSELALWAYS: u32 = 0x0008;
/// `LVS_SHOWSELNEVER`.
pub const LVS_SHOWSELNEVER: u32 = 0x0800;
/// `LVS_NOCOLUMNHEADER`.
pub const LVS_NOCOLUMNHEADER: u32 = 0x2000;
/// `LVS_OWNERDRAWFIXED`.
pub const LVS_OWNERDRAWFIXED: u32 = 0x1000;
/// `LVS_HASIMAGES`.
pub const LVS_HASIMAGES: u32 = 0x0200;

/// `LVIF_TEXT`.
pub const LVIF_TEXT: u32 = 0x0001;
/// `LVIF_IMAGE`.
pub const LVIF_IMAGE: u32 = 0x0002;
/// `LVIF_PARAM`.
pub const LVIF_PARAM: u32 = 0x0004;
/// `LVIF_STATE`.
pub const LVIF_STATE: u32 = 0x0008;
/// `LVIF_INDENT`.
pub const LVIF_INDENT: u32 = 0x0010;

/// `LVIS_SELECTED`.
pub const LVIS_SELECTED: u32 = 0x0002;
/// `LVIS_STATEIMAGEMASK`.
pub const LVIS_STATEIMAGEMASK: u32 = 0xF000;

/// `LVN_ITEMCHANGED`.
pub const LVN_ITEMCHANGED: i32 = -101;
/// `LVN_ITEMACTIVATE`.
pub const LVN_ITEMACTIVATE: i32 = -102;
/// `LVN_BEGINDRAG`.
pub const LVN_BEGINDRAG: i32 = -100;

// =====================================================================
//  SysTreeView32
// =====================================================================

/// `TVS_HASBUTTONS`.
pub const TVS_HASBUTTONS: u32 = 0x0001;
/// `TVS_HASLINES`.
pub const TVS_HASLINES: u32 = 0x0002;
/// `TVS_LINESATROOT`.
pub const TVS_LINESATROOT: u32 = 0x0004;
/// `TVS_SHOWSELALWAYS`.
pub const TVS_SHOWSELALWAYS: u32 = 0x0020;
/// `TVS_HASSELECTED`.
pub const TVS_HASSELECTED: u32 = 0x0020;
/// `TVS_FULLROWSELECT`.
pub const TVS_FULLROWSELECT: u32 = 0x0020;
/// `TVS_DISABLEDRAGDROP`.
pub const TVS_DISABLEDRAGDROP: u32 = 0x0010;
/// `TVS_NOTOOLTIPS`.
pub const TVS_NOTOOLTIPS: u32 = 0x0080;

/// `TVIF_TEXT`.
pub const TVIF_TEXT: u32 = 0x0001;
/// `TVIF_IMAGE`.
pub const TVIF_IMAGE: u32 = 0x0002;
/// `TVIF_PARAM`.
pub const TVIF_PARAM: u32 = 0x0004;
/// `TVIF_STATE`.
pub const TVIF_STATE: u32 = 0x0008;
/// `TVIF_CHILDREN`.
pub const TVIF_CHILDREN: u32 = 0x0040;
/// `TVIF_HANDLE`.
pub const TVIF_HANDLE: u32 = 0x0010;

/// `TVN_ITEMCHANGEDW`.
pub const TVN_ITEMCHANGEDW: i32 = -101;
/// `TVN_SELCHANGEDW`.
pub const TVN_SELCHANGEDW: i32 = -102;

// =====================================================================
//  ToolbarWindow32
// =====================================================================

/// `TBSTYLE_FLAT`.
pub const TBSTYLE_FLAT: u32 = 0x0000_0080;
/// `TBSTYLE_TRANSPARENT`.
pub const TBSTYLE_TRANSPARENT: u32 = 0x0000_0200;
/// `TBSTYLE_LIST`.
pub const TBSTYLE_LIST: u32 = 0x0000_1000;

/// `TB_GETBUTTON`.
pub const TB_GETBUTTON: u32 = 0x0417;
/// `TB_BUTTONCOUNT`.
pub const TB_BUTTONCOUNT: u32 = 0x0418;
/// `TB_SETBUTTON`.
pub const TB_SETBUTTON: u32 = 0x0419;
/// `TB_GETRECT`.
pub const TB_GETRECT: u32 = 0x0433;

// =====================================================================
//  Common controls — progress / status / trackbar / updown / tab
// =====================================================================

/// `PBS_SMOOTH`.
pub const PBS_SMOOTH: u32 = 0x0001;
/// `PBS_VERTICAL`.
pub const PBS_VERTICAL: u32 = 0x0004;

/// `SB_GETPARTS`.
pub const SB_GETPARTS: u32 = 0x0406;
/// `SB_SETPARTS`.
pub const SB_SETPARTS: u32 = 0x0405;

/// `TBM_GETPOS`.
pub const TBM_GETPOS: u32 = 0x0400;
/// `TBM_SETPOS`.
pub const TBM_SETPOS: u32 = 0x0405;
/// `TBM_GETRANGE`.
pub const TBM_GETRANGE: u32 = 0x0401;
/// `TBM_SETRANGE`.
pub const TBM_SETRANGE: u32 = 0x0406;

/// `UDM_GETPOS`.
pub const UDM_GETPOS: u32 = 0x0468;
/// `UDM_SETPOS`.
pub const UDM_SETPOS: u32 = 0x0467;
/// `UDM_SETRANGE`.
pub const UDM_SETRANGE: u32 = 0x0466;

/// `TCM_FIRST`.
pub const TCM_FIRST: u32 = 0x1300;
/// `TCM_GETITEMCOUNT`.
pub const TCM_GETITEMCOUNT: u32 = 0x1304;
/// `TCM_SETCURSEL`.
pub const TCM_SETCURSEL: u32 = 0x130C;
/// `TCM_GETCURSEL`.
pub const TCM_GETCURSEL: u32 = 0x130B;
/// `TCM_GETITEMW`.
pub const TCM_GETITEMW: u32 = 0x133A;

/// `TCS_SINGLE`.
pub const TCS_SINGLE: u32 = 0x0000;
/// `TCS_TABS`.
pub const TCS_TABS: u32 = 0x0000;
/// `TCS_RIGHTJUSTIFY`.
pub const TCS_RIGHTJUSTIFY: u32 = 0x0000;

/// `TTM_ADDTOOL`.
pub const TTM_ADDTOOL: u32 = 0x0408;
/// `TTM_SETTOOLINFO`.
pub const TTM_SETTOOLINFO: u32 = 0x0409;

// =====================================================================
//  DrawText
// =====================================================================

/// `DT_LEFT`.
pub const DT_LEFT: u32 = 0x0000_0000;
/// `DT_CENTER`.
pub const DT_CENTER: u32 = 0x0000_0001;
/// `DT_RIGHT`.
pub const DT_RIGHT: u32 = 0x0000_0002;
/// `DT_TOP`.
pub const DT_TOP: u32 = 0x0000_0000;
/// `DT_VCENTER`.
pub const DT_VCENTER: u32 = 0x0000_0004;
/// `DT_BOTTOM`.
pub const DT_BOTTOM: u32 = 0x0000_0008;
/// `DT_WORDBREAK`.
pub const DT_WORDBREAK: u32 = 0x0000_0010;
/// `DT_SINGLELINE`.
pub const DT_SINGLELINE: u32 = 0x0000_0020;
/// `DT_NOPREFIX`.
pub const DT_NOPREFIX: u32 = 0x0000_0800;
/// `DT_PATH_ELLIPSIS`.
pub const DT_PATH_ELLIPSIS: u32 = 0x0000_4000;
/// `DT_END_ELLIPSIS`.
pub const DT_END_ELLIPSIS: u32 = 0x0000_8000;
/// `DT_EDITCONTROL`.
pub const DT_EDITCONTROL: u32 = 0x0000_2000;

// =====================================================================
//  Clip / raster ops
// =====================================================================

/// `SRCCOPY`.
pub const SRCCOPY: u32 = 0x00CC_0020;
/// `SRCPAINT`.
pub const SRCPAINT: u32 = 0x00CC_00AA;
/// `SRCAND`.
pub const SRCAND: u32 = 0x00CC_0088;
/// `SRCINVERT`.
pub const SRCINVERT: u32 = 0x00CC_0066;
/// `MERGECOPY`.
pub const MERGECOPY: u32 = 0x00C0_00CC;
/// `MERGEPAINT`.
pub const MERGEPAINT: u32 = 0x00C0_00AA;
/// `PATCOPY`.
pub const PATCOPY: u32 = 0x00F0_0021;
/// `PATPAINT`.
pub const PATPAINT: u32 = 0x00F0_00A9;
/// `WHITENESS`.
pub const WHITENESS: u32 = 0x00FF_0062;
/// `BLACKNESS`.
pub const BLACKNESS: u32 = 0x0000_0082;

/// `TRANSPARENT`.
pub const TRANSPARENT: i32 = 1;
/// `OPAQUE`.
pub const OPAQUE: i32 = 2;

/// `AC_SRC_OVER`.
pub const AC_SRC_OVER: u8 = 0x00;
/// `AC_SRC_ALPHA`.
pub const AC_SRC_ALPHA: u8 = 0x01;

/// `PS_SOLID`.
pub const PS_SOLID: u32 = 0x0000;
/// `PS_DASH`.
pub const PS_DASH: u32 = 0x0001;
/// `PS_DOT`.
pub const PS_DOT: u32 = 0x0002;
/// `PS_ENDCAP_ROUND`.
pub const PS_ENDCAP_ROUND: u32 = 0x0000;
/// `PS_JOIN_ROUND`.
pub const PS_JOIN_ROUND: u32 = 0x0000;

/// `BS_SOLID`.
pub const BS_SOLID: u32 = 0x0000;
/// `BS_NULL`.
pub const BS_NULL: u32 = 0x0001;
/// `BS_HATCHED`.
pub const BS_HATCHED: u32 = 0x0002;

/// `HS_HORIZONTAL`.
pub const HS_HORIZONTAL: u32 = 0x0000;
/// `HS_VERTICAL`.
pub const HS_VERTICAL: u32 = 0x0001;
/// `HS_BDIAGONAL`.
pub const HS_BDIAGONAL: u32 = 0x0002;
/// `HS_FDIAGONAL`.
pub const HS_FDIAGONAL: u32 = 0x0003;
/// `HS_CROSS`.
pub const HS_CROSS: u32 = 0x0004;

/// `DEFAULT_GUI_FONT`.
pub const DEFAULT_GUI_FONT: u32 = 17;
/// `DEFAULT_PALETTE`.
pub const DEFAULT_PALETTE: u32 = 15;
/// `SYSTEM_FONT`.
pub const SYSTEM_FONT: u32 = 13;
/// `DEFAULT_HBRUSH`.
pub const DEFAULT_HBRUSH: u32 = 0;

// =====================================================================
//  Clipboard formats
// =====================================================================

/// `CF_TEXT`.
pub const CF_TEXT: u32 = 1;
/// `CF_BITMAP`.
pub const CF_BITMAP: u32 = 2;
/// `CF_METAFILEPICT`.
pub const CF_METAFILEPICT: u32 = 3;
/// `CF_SYLK`.
pub const CF_SYLK: u32 = 4;
/// `CF_DIB`.
pub const CF_DIB: u32 = 8;
/// `CF_UNICODETEXT`.
pub const CF_UNICODETEXT: u32 = 13;
/// `CF_HDROP`.
pub const CF_HDROP: u32 = 15;
/// `CF_LOCALE`.
pub const CF_LOCALE: u32 = 16;
/// `CF_DIBV5`.
pub const CF_DIBV5: u32 = 17;

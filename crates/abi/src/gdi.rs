//! GDI-specific constants (wingdi.h).

// =====================================================================
//  Stock objects
// =====================================================================

/// `WHITE_BRUSH`.
pub const WHITE_BRUSH: u32 = 0;
/// `LTGRAY_BRUSH`.
pub const LTGRAY_BRUSH: u32 = 1;
/// `GRAY_BRUSH`.
pub const GRAY_BRUSH: u32 = 2;
/// `DKGRAY_BRUSH`.
pub const DKGRAY_BRUSH: u32 = 3;
/// `BLACK_BRUSH`.
pub const BLACK_BRUSH: u32 = 4;
/// `NULL_BRUSH`.
pub const NULL_BRUSH: u32 = 5;
/// `WHITE_PEN`.
pub const WHITE_PEN: u32 = 6;
/// `BLACK_PEN`.
pub const BLACK_PEN: u32 = 7;
/// `NULL_PEN`.
pub const NULL_PEN: u32 = 8;
/// `SYSTEM_FONT`.
pub const STOCK_SYSTEM_FONT: u32 = 13;
/// `DEVICE_DEFAULT_FONT`.
pub const DEVICE_DEFAULT_FONT: u32 = 14;
/// `DEFAULT_PALETTE`.
pub const STOCK_DEFAULT_PALETTE: u32 = 15;
/// `SYSTEM_FIXED_FONT`.
pub const SYSTEM_FIXED_FONT: u32 = 16;
/// `DEFAULT_GUI_FONT`.
pub const STOCK_DEFAULT_GUI_FONT: u32 = 17;

// =====================================================================
//  DIB formats
// =====================================================================

/// `BI_RGB`.
pub const BI_RGB: u32 = 0;
/// `BI_RLE8`.
pub const BI_RLE8: u32 = 1;
/// `BI_RLE4`.
pub const BI_RLE4: u32 = 2;
/// `BI_BITFIELDS`.
pub const BI_BITFIELDS: u32 = 3;
/// `BI_ALPHABITFIELDS`.
pub const BI_ALPHABITFIELDS: u32 = 6;

/// `DIB_RGB_COLORS`.
pub const DIB_RGB_COLORS: u32 = 0;
/// `DIB_PAL_COLORS`.
pub const DIB_PAL_COLORS: u32 = 1;

// =====================================================================
//  Device capabilities
// =====================================================================

/// `HORZRES`.
pub const HORZRES: u32 = 8;
/// `VERTRES`.
pub const VERTRES: u32 = 10;
/// `HORZSIZE`.
pub const HORZSIZE: u32 = 4;
/// `VERTSIZE`.
pub const VERTSIZE: u32 = 6;
/// `LOGPIXELSX`.
pub const LOGPIXELSX: u32 = 88;
/// `LOGPIXELSY`.
pub const LOGPIXELSY: u32 = 90;
/// `BITSPIXEL`.
pub const BITSPIXEL: u32 = 12;
/// `PLANES`.
pub const PLANES: u32 = 14;
/// `COLORS`.
pub const COLORS: u32 = 16;
/// `RASTER_CAPABILITIES`.
pub const RASTER_CAPABILITIES: u32 = 38;
/// `TECHNOLOGY`.
pub const TECHNOLOGY: u32 = 2;
/// `VERTRES`.
pub const CAP_VERTRES: u32 = 10;

/// `DT_PLOTTER`.
pub const DT_PLOTTER: u32 = 0;
/// `DT_RASDISPLAY`.
pub const DT_RASDISPLAY: u32 = 1;
/// `DT_RASPRINTER`.
pub const DT_RASPRINTER: u32 = 2;

/// `RC_BITBLT`.
pub const RC_BITBLT: u32 = 0x0000_0002;
/// `RC_BANDING`.
pub const RC_BANDING: u32 = 0x0000_0004;
/// `RC_BITMAP64`.
pub const RC_BITMAP64: u32 = 0x0000_0100;
/// `RC_ALPHABLEND`.
pub const RC_ALPHABLEND: u32 = 0x0000_0200;

// =====================================================================
//  Mapping modes
// =====================================================================

/// `MM_TEXT`.
pub const MM_TEXT: i32 = 1;
/// `MM_ANISOTROPIC`.
pub const MM_ANISOTROPIC: i32 = 8;
/// `MM_HIMETRIC`.
pub const MM_HIMETRIC: i32 = 9;

// =====================================================================
//  Font weights / qualities / charsets
// =====================================================================

/// `FW_DONTCARE`.
pub const FW_DONTCARE: i32 = 0;
/// `FW_THIN`.
pub const FW_THIN: i32 = 100;
/// `FW_NORMAL`.
pub const FW_NORMAL: i32 = 400;
/// `FW_MEDIUM`.
pub const FW_MEDIUM: i32 = 500;
/// `FW_SEMIBOLD`.
pub const FW_SEMIBOLD: i32 = 600;
/// `FW_BOLD`.
pub const FW_BOLD: i32 = 700;
/// `FW_HEAVY`.
pub const FW_HEAVY: i32 = 900;

/// `DEFAULT_CHARSET`.
pub const DEFAULT_CHARSET: u8 = 1;
/// `ANSI_CHARSET`.
pub const ANSI_CHARSET: u8 = 0;
/// `SYMBOL_CHARSET`.
pub const SYMBOL_CHARSET: u8 = 2;
/// `GB2312_CHARSET`.
pub const GB2312_CHARSET: u8 = 134;
/// `CHINESEBIG5_CHARSET`.
pub const CHINESEBIG5_CHARSET: u8 = 136;
/// `DEFAULT_PITCH`.
pub const DEFAULT_PITCH: u8 = 0;
/// `FIXED_PITCH`.
pub const FIXED_PITCH: u8 = 1;
/// `VARIABLE_PITCH`.
pub const VARIABLE_PITCH: u8 = 2;

/// `CLEARTYPE_QUALITY`.
pub const CLEARTYPE_QUALITY: u8 = 5;
/// `ANTIALIASED_QUALITY`.
pub const ANTIALIASED_QUALITY: u8 = 4;
/// `NONANTIALIASED_QUALITY`.
pub const NONANTIALIASED_QUALITY: u8 = 3;

/// `OUT_TT_PRECIS`.
pub const OUT_TT_PRECIS: u8 = 4;
/// `CLIP_DEFAULT_PRECIS`.
pub const CLIP_DEFAULT_PRECIS: u8 = 0;
/// `CLIP_TT_ANALYTIC`.
pub const CLIP_TT_ANALYTIC: u8 = 16;

/// `TA_LEFT`.
pub const TA_LEFT: u32 = 0;
/// `TA_RIGHT`.
pub const TA_RIGHT: u32 = 2;
/// `TA_CENTER`.
pub const TA_CENTER: u32 = 6;
/// `TA_TOP`.
pub const TA_TOP: u32 = 0;
/// `TA_BOTTOM`.
pub const TA_BOTTOM: u32 = 8;
/// `TA_NOUPDATECP`.
pub const TA_NOUPDATECP: u32 = 0;

// =====================================================================
//  Region types
// =====================================================================

/// `NULLREGION`.
pub const NULLREGION: i32 = 0;
/// `SIMPLEREGION`.
pub const SIMPLEREGION: i32 = 1;
/// `COMPLEXREGION`.
pub const COMPLEXREGION: i32 = 2;

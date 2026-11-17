//! Native surface metadata and NT61-owned x86/x64 guest runtime DLL factory.
//!
//! Guest exports enter a checked runtime gateway; name/ABI availability is
//! distinct from handler support. ADVAPI32 named ordinals match the audited
//! Windows 7 SP1 reference; other DLL ordinals are NT61 assignments. Product
//! code and thunks are built from source; no Microsoft machine code is shipped.

use super::pe::{ImportSymbol, PeImageSpec, SectionSpec};

#[path = "../../../arch/src/arch/common/btl/win32/catalog.rs"]
pub mod catalog;
#[path = "../../../arch/src/arch/common/btl/win32/advapi/ordinals.rs"]
mod advapi_ordinals;
#[path = "../../../abi/src/syscall_metadata.rs"]
pub mod syscall_metadata;

// =====================================================================
//  Image bases
// =====================================================================

/// `ntdll.dll` — the lowest DLL, mapped first.
pub const NTDLL_BASE: u64 = 0x0000_1000_0000_0000;
/// `kernel32.dll`.
pub const KERNEL32_BASE: u64 = 0x0000_1000_1000_0000;
/// `user32.dll`.
pub const USER32_BASE: u64 = 0x0000_1000_2000_0000;
/// `gdi32.dll`.
pub const GDI32_BASE: u64 = 0x0000_1000_3000_0000;
/// `uxtheme.dll`.
pub const UXTHEME_BASE: u64 = 0x0000_1000_4000_0000;
/// `dwmapi.dll`.
pub const DWMAPI_BASE: u64 = 0x0000_1000_5000_0000;
/// `shell32.dll`.
pub const SHELL32_BASE: u64 = 0x0000_1000_6000_0000;
/// `ole32.dll`.
pub const OLE32_BASE: u64 = 0x0000_1000_7000_0000;
/// `oleaut32.dll`.
pub const OLEAUT32_BASE: u64 = 0x0000_1000_8000_0000;
/// `comctl32.dll`.
pub const COMCTL32_BASE: u64 = 0x0000_1000_9000_0000;
/// `comdlg32.dll`.
pub const COMDLG32_BASE: u64 = 0x0000_1000_c000_0000;

/// `IMAGE_FILE_MACHINE_AMD64`.
pub const MACHINE_AMD64: u16 = 0x8664;

/// The RVA of the first byte of `.text`.
pub const TEXT_RVA: u32 = 0x1000;

/// The stride between the stub entry points in `.text`.
///
/// Each guest runtime thunk fits in 16 bytes. Native placeholder surfaces
/// retain the same export spacing but do not implement the guest runtime.
pub const STUB_STRIDE: u32 = 0x10;

/// The RVA of the stub for the export at `index`.
pub fn stub_rva(index: u32) -> u32 {
    TEXT_RVA + index * STUB_STRIDE
}

// =====================================================================
//  DLL specifications
// =====================================================================

/// The specification of one system DLL.
#[derive(Clone, Debug)]
pub struct DllSpec {
    /// The file name, e.g. `"user32.dll"`.
    pub file_name: &'static str,
    /// The preferred load address.
    pub image_base: u64,
    /// The exported symbols, in ordinal order.
    pub exports: &'static [(&'static str, u32)],
    /// The imported modules and symbols.
    pub imports: &'static [(&'static str, &'static [ImportSymbol])],
}

/// The `ntdll.dll` surface.
///
/// `ntdll.dll` is the boundary between Ring 3 and the kernel. Every
/// `Nt*` entry is a syscall stub; the `Rtl*` entries are the
/// run-time library helpers that sit on top of the native calls.
pub const NTDLL_EXPORTS: &[(&str, u32)] = &[
    // --- Native system services -------------------------------------
    ("NtAllocateVirtualMemory", 0),
    ("NtClose", 1),
    ("NtCreateFile", 2),
    ("NtDeviceIoControlFile", 3),
    ("NtFreeVirtualMemory", 4),
    ("NtQuerySystemInformation", 5),
    ("NtReadFile", 6),
    ("NtWriteFile", 7),
    ("NtWaitForSingleObject", 8),
    // --- Run-time library --------------------------------------------
    ("RtlAllocateHeap", 9),
    ("RtlFreeHeap", 10),
    ("RtlUserThreadStart", 11),
    ("RtlInitUnicodeString", 12),
    ("LdrLoadDll", 13),
    ("LdrGetProcedureAddress", 14),
    // --- Win32k (USER) syscall stubs --------------------------------
    ("NtUserCreateWindowEx", 15),
    ("NtUserDestroyWindow", 16),
    ("NtUserGetMessage", 17),
    ("NtUserPeekMessage", 18),
    ("NtUserTranslateMessage", 19),
    ("NtUserDispatchMessage", 20),
    ("NtUserSendMessage", 21),
    ("NtUserPostMessage", 22),
    ("NtUserSetWindowPos", 23),
    ("NtUserShowWindow", 24),
    ("NtUserMoveWindow", 25),
    ("NtUserBeginPaint", 26),
    ("NtUserEndPaint", 27),
    ("NtUserGetDC", 28),
    ("NtUserReleaseDC", 29),
    ("NtUserInvalidateRect", 30),
    ("NtUserSetWindowText", 31),
    ("NtUserGetWindowText", 32),
    ("NtUserGetSystemMetrics", 33),
    ("NtUserGetCursorPos", 34),
    ("NtUserSetCapture", 35),
    ("NtUserGetCapture", 36),
    ("NtUserRegisterClassEx", 37),
    ("NtUserDefWindowProc", 38),
    ("NtUserCallWindowProc", 39),
    ("NtUserGetMessagePos", 40),
    ("NtUserBlockInput", 41),
    // --- Win32k (GDI) syscall stubs ---------------------------------
    ("NtGdiGetDC", 42),
    ("NtGdiCreateCompatibleDC", 43),
    ("NtGdiDeleteDC", 44),
    ("NtGdiSelectObject", 45),
    ("NtGdiDeleteObject", 46),
    ("NtGdiBitBlt", 47),
    ("NtGdiStretchBlt", 48),
    ("NtGdiPatBlt", 49),
    ("NtGdiFillRect", 50),
    ("NtGdiExtTextOutW", 51),
    ("NtGdiSelectFont", 52),
    ("NtGdiSetTextColor", 53),
    ("NtGdiSetBkMode", 54),
    ("NtGdiCreateRectRgn", 55),
    ("NtGdiIntersectClipRect", 56),
    ("NtGdiFlush", 57),
    ("NtGdiGetTextExtentPoint32W", 58),
];

/// The `kernel32.dll` surface.
pub const KERNEL32_EXPORTS: &[(&str, u32)] = &[
    ("CloseHandle", 0),
    ("CreateFileW", 1),
    ("GetLastError", 2),
    ("SetLastError", 3),
    ("GetProcAddress", 4),
    ("GetModuleHandleW", 5),
    ("LoadLibraryW", 6),
    ("ReadFile", 7),
    ("Sleep", 8),
    ("VirtualAlloc", 9),
    ("VirtualFree", 10),
    ("WriteFile", 11),
];

/// The `user32.dll` surface.
pub const USER32_EXPORTS: &[(&str, u32)] = &[
    ("BeginPaint", 0),
    ("CreateWindowExW", 1),
    ("DefWindowProcW", 2),
    ("DestroyWindow", 3),
    ("DispatchMessageW", 4),
    ("DrawTextW", 5),
    ("EndPaint", 6),
    ("GetClientRect", 7),
    ("GetDC", 8),
    ("GetMessageW", 9),
    ("GetSystemMetrics", 10),
    ("GetWindowTextW", 11),
    ("InvalidateRect", 12),
    ("MessageBoxW", 13),
    ("PeekMessageW", 14),
    ("RegisterClassExW", 15),
    ("ReleaseDC", 16),
    ("SendMessageW", 17),
    ("SetWindowPos", 18),
    ("SetWindowTextW", 19),
    ("ShowWindow", 20),
    ("TranslateMessage", 21),
    ("PostMessageW", 22),
    ("PostQuitMessage", 23),
    ("GetCursorPos", 24),
    ("SetCapture", 25),
    ("ReleaseCapture", 26),
    ("GetForegroundWindow", 27),
    ("SetForegroundWindow", 28),
    ("OpenWindowStationW", 29),
    ("CreateDesktopW", 30),
    ("SwitchDesktop", 31),
    ("GetWindowRect", 32),
];

/// The `gdi32.dll` surface.
pub const GDI32_EXPORTS: &[(&str, u32)] = &[
    ("BitBlt", 0),
    ("CreateCompatibleDC", 1),
    ("CreateDIBSection", 2),
    ("CreateFontIndirectW", 3),
    ("CreatePen", 4),
    ("CreateSolidBrush", 5),
    ("DeleteObject", 6),
    ("Ellipse", 7),
    ("ExtTextOutW", 8),
    ("FillRect", 9),
    ("GetStockObject", 10),
    ("GetTextExtentPoint32W", 11),
    ("GetTextMetricsW", 12),
    ("IntersectClipRect", 13),
    ("LineTo", 14),
    ("MoveToEx", 15),
    ("Polygon", 16),
    ("Rectangle", 17),
    ("RoundRect", 18),
    ("SelectObject", 19),
    ("SetBkMode", 20),
    ("SetTextColor", 21),
    ("StretchBlt", 22),
    ("TextOutW", 23),
    ("PatBlt", 24),
    ("AlphaBlend", 25),
    ("DrawEdge", 26),
    ("GdiFlush", 27),
    ("DrawTextW", 28),
    ("CreateRectRgn", 29),
];

/// The `uxtheme.dll` surface.
pub const UXTHEME_EXPORTS: &[(&str, u32)] = &[
    ("CloseThemeData", 0),
    ("DrawThemeBackground", 1),
    ("DrawThemeText", 2),
    ("EnableTheme", 3),
    ("GetCurrentThemeName", 4),
    ("GetThemeColor", 5),
    ("GetThemeMargins", 6),
    ("GetThemeMetrics", 7),
    ("GetThemePartSize", 8),
    ("IsThemeActive", 9),
    ("OpenThemeData", 10),
    ("SetWindowTheme", 11),
];

/// The `dwmapi.dll` surface.
pub const DWMAPI_EXPORTS: &[(&str, u32)] = &[
    ("DwmEnableComposition", 0),
    ("DwmExtendFrameIntoClientArea", 1),
    ("DwmFlush", 2),
    ("DwmGetWindowAttribute", 3),
    ("DwmIsCompositionEnabled", 4),
    ("DwmRegisterThumbnail", 5),
    ("DwmSetIconicLivePreviewBitmap", 6),
    ("DwmSetIconicThumbnail", 7),
    ("DwmSetWindowAttribute", 8),
    ("DwmUnregisterThumbnail", 9),
    ("DwmUpdateThumbnailProperties", 10),
];

/// The `shell32.dll` surface.
pub const SHELL32_EXPORTS: &[(&str, u32)] = &[
    ("ExtractIconExW", 0),
    ("SHChangeNotify", 1),
    ("SHGetFileInfoW", 2),
    ("SHGetFolderPathW", 3),
    ("SHGetKnownFolderPath", 4),
    ("SHParseDisplayName", 5),
    ("ShellExecuteExW", 6),
    ("Shell_NotifyIconW", 7),
];

/// The `ole32.dll` surface.
pub const OLE32_EXPORTS: &[(&str, u32)] = &[
    ("CLSIDFromString", 0),
    ("CoCreateInstance", 1),
    ("CoInitializeEx", 2),
    ("CoTaskMemAlloc", 3),
    ("CoTaskMemFree", 4),
    ("CoUninitialize", 5),
    ("StringFromCLSID", 6),
];

/// The `oleaut32.dll` surface.
pub const OLEAUT32_EXPORTS: &[(&str, u32)] = &[
    ("SafeArrayCreate", 0),
    ("SafeArrayDestroy", 1),
    ("SysAllocString", 2),
    ("SysFreeString", 3),
    ("VariantClear", 4),
    ("VariantInit", 5),
];

/// The Windows 7 common-dialog export surface.
pub const COMDLG32_EXPORTS: &[(&str, u32)] = &[
    ("CommDlgExtendedError", 0),
    ("GetOpenFileNameA", 1),
    ("GetOpenFileNameW", 2),
    ("GetSaveFileNameA", 3),
    ("GetSaveFileNameW", 4),
    ("ChooseFontA", 5),
    ("ChooseFontW", 6),
    ("ChooseColorA", 7),
    ("ChooseColorW", 8),
    ("PrintDlgA", 9),
    ("PrintDlgW", 10),
    ("PageSetupDlgA", 11),
    ("PageSetupDlgW", 12),
    ("GetFileTitleA", 13),
    ("GetFileTitleW", 14),
];

/// The `comctl32.dll` (v6) surface.
pub const COMCTL32_EXPORTS: &[(&str, u32)] = &[
    ("CreatePropertySheetPageW", 0),
    ("CreateStatusWindowW", 1),
    ("CreateToolbarEx", 2),
    ("DestroyPropertySheetPage", 3),
    ("ImageList_Add", 4),
    ("ImageList_AddIcon", 5),
    ("ImageList_Create", 6),
    ("ImageList_Destroy", 7),
    ("ImageList_GetIcon", 8),
    ("ImageList_SetIconSize", 9),
    ("InitCommonControlsEx", 10),
    ("InitMUILanguage", 11),
    ("PropertySheetW", 12),
    ("InitCommonControls", 13),
    ("GetMUILanguage", 14),
    ("DllGetVersion", 15),
    ("ImageList_GetImageCount", 16),
    ("ImageList_GetIconSize", 17),
    ("ImageList_SetImageCount", 18),
    ("ImageList_Remove", 19),
];

/// The `user32.dll` imports.
///
/// `user32.dll` talks to the kernel exclusively through `ntdll.dll`'s
/// `NtUser*` stubs, exactly as on Windows 7.
pub const USER32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtUserCreateWindowEx"),
        ImportSymbol::ByName("NtUserDestroyWindow"),
        ImportSymbol::ByName("NtUserGetMessage"),
        ImportSymbol::ByName("NtUserPeekMessage"),
        ImportSymbol::ByName("NtUserTranslateMessage"),
        ImportSymbol::ByName("NtUserDispatchMessage"),
        ImportSymbol::ByName("NtUserSendMessage"),
        ImportSymbol::ByName("NtUserPostMessage"),
        ImportSymbol::ByName("NtUserSetWindowPos"),
        ImportSymbol::ByName("NtUserShowWindow"),
        ImportSymbol::ByName("NtUserMoveWindow"),
        ImportSymbol::ByName("NtUserBeginPaint"),
        ImportSymbol::ByName("NtUserEndPaint"),
        ImportSymbol::ByName("NtUserGetDC"),
        ImportSymbol::ByName("NtUserReleaseDC"),
        ImportSymbol::ByName("NtUserInvalidateRect"),
        ImportSymbol::ByName("NtUserSetWindowText"),
        ImportSymbol::ByName("NtUserGetWindowText"),
        ImportSymbol::ByName("NtUserGetSystemMetrics"),
        ImportSymbol::ByName("NtUserGetCursorPos"),
        ImportSymbol::ByName("NtUserSetCapture"),
        ImportSymbol::ByName("NtUserGetCapture"),
        ImportSymbol::ByName("NtUserRegisterClassEx"),
        ImportSymbol::ByName("NtUserDefWindowProc"),
        ImportSymbol::ByName("NtUserCallWindowProc"),
        ImportSymbol::ByName("NtUserBlockInput"),
    ],
)];

/// The `gdi32.dll` imports.
pub const GDI32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtGdiCreateCompatibleDC"),
        ImportSymbol::ByName("NtGdiDeleteDC"),
        ImportSymbol::ByName("NtGdiSelectObject"),
        ImportSymbol::ByName("NtGdiDeleteObject"),
        ImportSymbol::ByName("NtGdiBitBlt"),
        ImportSymbol::ByName("NtGdiStretchBlt"),
        ImportSymbol::ByName("NtGdiPatBlt"),
        ImportSymbol::ByName("NtGdiFillRect"),
        ImportSymbol::ByName("NtGdiExtTextOutW"),
        ImportSymbol::ByName("NtGdiSelectFont"),
        ImportSymbol::ByName("NtGdiSetTextColor"),
        ImportSymbol::ByName("NtGdiSetBkMode"),
        ImportSymbol::ByName("NtGdiCreateRectRgn"),
        ImportSymbol::ByName("NtGdiIntersectClipRect"),
        ImportSymbol::ByName("NtGdiFlush"),
        ImportSymbol::ByName("NtGdiGetTextExtentPoint32W"),
    ],
)];

/// The `kernel32.dll` imports.
pub const KERNEL32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtAllocateVirtualMemory"),
        ImportSymbol::ByName("NtFreeVirtualMemory"),
        ImportSymbol::ByName("NtClose"),
        ImportSymbol::ByName("NtCreateFile"),
        ImportSymbol::ByName("NtReadFile"),
        ImportSymbol::ByName("NtWriteFile"),
        ImportSymbol::ByName("NtWaitForSingleObject"),
        ImportSymbol::ByName("NtQuerySystemInformation"),
        ImportSymbol::ByName("RtlAllocateHeap"),
        ImportSymbol::ByName("RtlFreeHeap"),
        ImportSymbol::ByName("RtlInitUnicodeString"),
        ImportSymbol::ByName("LdrLoadDll"),
        ImportSymbol::ByName("LdrGetProcedureAddress"),
    ],
)];

/// The `dwmapi.dll` imports.
pub const DWMAPI_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtAllocateVirtualMemory"),
        ImportSymbol::ByName("NtFreeVirtualMemory"),
    ],
)];

/// The `uxtheme.dll` imports.
pub const UXTHEME_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtAllocateVirtualMemory"),
        ImportSymbol::ByName("NtFreeVirtualMemory"),
    ],
)];

/// The `shell32.dll` imports.
pub const SHELL32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtAllocateVirtualMemory"),
        ImportSymbol::ByName("NtFreeVirtualMemory"),
    ],
)];

/// The `ole32.dll` imports.
pub const OLE32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtAllocateVirtualMemory"),
        ImportSymbol::ByName("NtFreeVirtualMemory"),
    ],
)];

/// The `oleaut32.dll` imports.
pub const OLEAUT32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtAllocateVirtualMemory"),
        ImportSymbol::ByName("NtFreeVirtualMemory"),
    ],
)];

/// The `comctl32.dll` imports.
pub const COMCTL32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("NtAllocateVirtualMemory"),
        ImportSymbol::ByName("NtFreeVirtualMemory"),
    ],
)];

/// Every system DLL, in load order.
pub const ALL_DLLS: &[DllSpec] = &[
    DllSpec {
        file_name: "ntdll.dll",
        image_base: NTDLL_BASE,
        exports: NTDLL_EXPORTS,
        imports: &[],
    },
    DllSpec {
        file_name: "kernel32.dll",
        image_base: KERNEL32_BASE,
        exports: KERNEL32_EXPORTS,
        imports: KERNEL32_IMPORTS,
    },
    DllSpec {
        file_name: "user32.dll",
        image_base: USER32_BASE,
        exports: USER32_EXPORTS,
        imports: USER32_IMPORTS,
    },
    DllSpec {
        file_name: "gdi32.dll",
        image_base: GDI32_BASE,
        exports: GDI32_EXPORTS,
        imports: GDI32_IMPORTS,
    },
    DllSpec {
        file_name: "uxtheme.dll",
        image_base: UXTHEME_BASE,
        exports: UXTHEME_EXPORTS,
        imports: UXTHEME_IMPORTS,
    },
    DllSpec {
        file_name: "dwmapi.dll",
        image_base: DWMAPI_BASE,
        exports: DWMAPI_EXPORTS,
        imports: DWMAPI_IMPORTS,
    },
    DllSpec {
        file_name: "shell32.dll",
        image_base: SHELL32_BASE,
        exports: SHELL32_EXPORTS,
        imports: SHELL32_IMPORTS,
    },
    DllSpec {
        file_name: "ole32.dll",
        image_base: OLE32_BASE,
        exports: OLE32_EXPORTS,
        imports: OLE32_IMPORTS,
    },
    DllSpec {
        file_name: "oleaut32.dll",
        image_base: OLEAUT32_BASE,
        exports: OLEAUT32_EXPORTS,
        imports: OLEAUT32_IMPORTS,
    },
    DllSpec {
        file_name: "comdlg32.dll",
        image_base: COMDLG32_BASE,
        exports: COMDLG32_EXPORTS,
        imports: &[],
    },
    DllSpec {
        file_name: "comctl32.dll",
        image_base: COMCTL32_BASE,
        exports: COMCTL32_EXPORTS,
        imports: COMCTL32_IMPORTS,
    },
];

/// Build the PE image for a system DLL.
///
/// The image contains:
///
/// * a `.text` section with one 16-byte stub per export;
/// * a `.rdata` section with the export directory, the import
///   directory and the string tables;
/// * a `.reloc` section with the base relocations of the stubs.
///
/// This legacy x64 entry point is retained for tools inspecting the x64 surface.
/// These exports remain placeholders; the native desktop links its runtime
/// separately. An import table does not provide the missing API implementations.
pub fn build_dll_image(spec: &DllSpec) -> Vec<u8> {
    build_dll_image_native(spec, MACHINE_AMD64, &[0x31, 0xc0, 0xc3])
}

fn build_dll_image_native(spec: &DllSpec, machine: u16, return_code: &[u8]) -> Vec<u8> {
    let mut image = PeImageSpec::new(
        spec.image_base,
        super::pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        true,
        machine,
    );
    image.module_name = spec.file_name.to_string();
    image.entry_point_rva = 0; // No DllMain; export zero is not an initializer.

    // The `.text` section: one stub per export.
    let mut text = vec![0u8; spec.exports.len() * STUB_STRIDE as usize];
    for (index, _) in spec.exports.iter().enumerate() {
        let at = index * STUB_STRIDE as usize;
        text[at..at + return_code.len()].copy_from_slice(return_code);
    }

    // The exports. The ordinal is 1-based, matching Windows.
    for (index, (name, _)) in spec.exports.iter().enumerate() {
        image.export_named(name, index as u16 + 1, stub_rva(index as u32));
    }

    // The imports.
    for (dll_name, symbols) in spec.imports {
        image.import(dll_name, symbols.to_vec());
    }

    // These position-independent instructions contain no absolute addresses.
    // Do not invent relocations that would corrupt instruction bytes.

    image.sections = vec![
        SectionSpec::code(text),
        SectionSpec::rdata(Vec::new()),
        SectionSpec::reloc(Vec::new()),
    ];

    let (bytes, _) = super::pe::emit(&image);
    bytes
}

/// DLLs backed by the isolated x86/x64 runtime gateway. Keeping this set
/// explicit avoids confusing native architecture placeholders with guest code.
pub const RUNTIME_DLLS: &[&str] = &["kernel32.dll", "ntdll.dll", "crypt32.dll", "advapi32.dll", "comdlg32.dll", "comctl32.dll", "gdi32.dll", "user32.dll", "bcrypt.dll"];

/// Exact API-set contracts imported by the supplied Firefox UCRT/VCRuntime
/// packages (both bitnesses). This is a bounded compatibility map, not a prefix
/// alias accepting arbitrary API-set names. Export names came from import
/// metadata; implementations are separately checked NT61 runtime handlers.
pub const APISET_CONTRACTS: &[(&str, &[&str])] = &[
    (
        "api-ms-win-core-console-l1-1-0.dll",
        &[
            "GetConsoleCP",
            "GetConsoleMode",
            "GetConsoleOutputCP",
            "GetNumberOfConsoleInputEvents",
            "PeekConsoleInputA",
            "ReadConsoleInputW",
            "ReadConsoleW",
            "SetConsoleCtrlHandler",
            "SetConsoleMode",
            "WriteConsoleW",
        ],
    ),
    (
        "api-ms-win-core-datetime-l1-1-0.dll",
        &["GetDateFormatW", "GetTimeFormatW"],
    ),
    (
        "api-ms-win-core-debug-l1-1-0.dll",
        &["IsDebuggerPresent", "OutputDebugStringW"],
    ),
    (
        "api-ms-win-core-errorhandling-l1-1-0.dll",
        &[
            "GetLastError",
            "RaiseException",
            "SetErrorMode",
            "SetLastError",
            "SetUnhandledExceptionFilter",
            "UnhandledExceptionFilter",
        ],
    ),
    (
        "api-ms-win-core-file-l1-1-0.dll",
        &[
            "CreateDirectoryW",
            "CreateFileW",
            "DeleteFileW",
            "FindClose",
            "FindFirstFileExW",
            "FindNextFileW",
            "FlushFileBuffers",
            "GetDiskFreeSpaceW",
            "GetDriveTypeW",
            "GetFileAttributesExW",
            "GetFileInformationByHandle",
            "GetFileSizeEx",
            "GetFileType",
            "GetFullPathNameW",
            "GetLogicalDrives",
            "LockFileEx",
            "ReadFile",
            "RemoveDirectoryW",
            "SetEndOfFile",
            "SetFileAttributesW",
            "SetFilePointerEx",
            "SetFileTime",
            "UnlockFileEx",
            "WriteFile",
        ],
    ),
    ("api-ms-win-core-file-l1-2-0.dll", &["GetTempPathW"]),
    ("api-ms-win-core-file-l2-1-0.dll", &["MoveFileExW"]),
    (
        "api-ms-win-core-handle-l1-1-0.dll",
        &["CloseHandle", "DuplicateHandle"],
    ),
    (
        "api-ms-win-core-heap-l1-1-0.dll",
        &[
            "GetProcessHeap",
            "HeapAlloc",
            "HeapCompact",
            "HeapFree",
            "HeapQueryInformation",
            "HeapReAlloc",
            "HeapSize",
            "HeapValidate",
            "HeapWalk",
        ],
    ),
    (
        "api-ms-win-core-interlocked-l1-1-0.dll",
        &["InterlockedFlushSList", "InterlockedPushEntrySList"],
    ),
    (
        "api-ms-win-core-libraryloader-l1-1-0.dll",
        &[
            "FreeLibrary",
            "FreeLibraryAndExitThread",
            "GetModuleFileNameW",
            "GetModuleHandleExW",
            "GetModuleHandleW",
            "GetProcAddress",
            "LoadLibraryExW",
        ],
    ),
    (
        "api-ms-win-core-localization-l1-2-0.dll",
        &[
            "EnumSystemLocalesW",
            "GetACP",
            "GetCPInfo",
            "GetLocaleInfoW",
            "GetOEMCP",
            "GetUserDefaultLCID",
            "IsValidCodePage",
            "IsValidLocale",
            "LCMapStringW",
        ],
    ),
    (
        "api-ms-win-core-memory-l1-1-0.dll",
        &["VirtualAlloc", "VirtualProtect", "VirtualQuery"],
    ),
    (
        "api-ms-win-core-namedpipe-l1-1-0.dll",
        &["CreatePipe", "PeekNamedPipe"],
    ),
    (
        "api-ms-win-core-processenvironment-l1-1-0.dll",
        &[
            "FreeEnvironmentStringsW",
            "GetCommandLineA",
            "GetCommandLineW",
            "GetCurrentDirectoryW",
            "GetEnvironmentStringsW",
            "GetStdHandle",
            "SetCurrentDirectoryW",
            "SetEnvironmentVariableW",
            "SetStdHandle",
        ],
    ),
    (
        "api-ms-win-core-processthreads-l1-1-0.dll",
        &[
            "CreateProcessW",
            "CreateThread",
            "ExitProcess",
            "ExitThread",
            "GetCurrentProcess",
            "GetCurrentProcessId",
            "GetCurrentThread",
            "GetCurrentThreadId",
            "GetExitCodeProcess",
            "GetStartupInfoW",
            "ResumeThread",
            "TerminateProcess",
            "TlsAlloc",
            "TlsFree",
            "TlsGetValue",
            "TlsSetValue",
        ],
    ),
    (
        "api-ms-win-core-processthreads-l1-1-1.dll",
        &["IsProcessorFeaturePresent"],
    ),
    (
        "api-ms-win-core-profile-l1-1-0.dll",
        &["QueryPerformanceCounter", "QueryPerformanceFrequency"],
    ),
    (
        "api-ms-win-core-rtlsupport-l1-1-0.dll",
        &[
            "RtlCaptureContext",
            "RtlLookupFunctionEntry",
            "RtlPcToFileHeader",
            "RtlUnwind",
            "RtlUnwindEx",
            "RtlVirtualUnwind",
        ],
    ),
    (
        "api-ms-win-core-string-l1-1-0.dll",
        &[
            "CompareStringW",
            "GetStringTypeW",
            "MultiByteToWideChar",
            "WideCharToMultiByte",
        ],
    ),
    (
        "api-ms-win-core-synch-l1-1-0.dll",
        &[
            "DeleteCriticalSection",
            "EnterCriticalSection",
            "InitializeCriticalSectionAndSpinCount",
            "LeaveCriticalSection",
            "WaitForSingleObject",
        ],
    ),
    ("api-ms-win-core-synch-l1-2-0.dll", &["Sleep"]),
    (
        "api-ms-win-core-sysinfo-l1-1-0.dll",
        &[
            "GetLocalTime",
            "GetSystemInfo",
            "GetSystemTimeAsFileTime",
            "SetLocalTime",
        ],
    ),
    (
        "api-ms-win-core-timezone-l1-1-0.dll",
        &[
            "FileTimeToSystemTime",
            "GetTimeZoneInformation",
            "SystemTimeToFileTime",
            "SystemTimeToTzSpecificLocalTime",
            "TzSpecificLocalTimeToSystemTime",
        ],
    ),
    (
        "api-ms-win-core-util-l1-1-0.dll",
        &["Beep", "DecodePointer", "EncodePointer"],
    ),
];

pub fn runtime_library_names() -> impl Iterator<Item = &'static str> {
    RUNTIME_DLLS
        .iter()
        .copied()
        .chain(APISET_CONTRACTS.iter().map(|entry| entry.0))
}

fn build_runtime_facade(file_name: &str, x64: bool) -> Option<Vec<u8>> {
    let (index, &(name, exports)) = APISET_CONTRACTS
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.0.eq_ignore_ascii_case(file_name))?;
    let base = if x64 {
        0x0000_1100_0000_0000 + index as u64 * 0x0010_0000
    } else {
        0x5000_0000 + index as u64 * 0x0010_0000
    };
    let mut image = PeImageSpec::new(
        base,
        super::pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        true,
        if x64 { MACHINE_AMD64 } else { 0x014c },
    );
    image.module_name = name.to_string();
    image.entry_point_rva = 0;
    for (index, name) in exports.iter().enumerate() {
        let target = if name.starts_with("Rtl") {
            "ntdll"
        } else {
            "kernel32"
        };
        assert!(
            catalog::find(&format!("{target}.dll"), name).is_some(),
            "unknown facade target"
        );
        image.export_forwarder(name, index as u16 + 1, &format!("{target}.{name}"));
    }
    image.sections = vec![SectionSpec::rdata(Vec::new())];
    Some(super::pe::emit(&image).0)
}

/// Kernel32 compatibility names whose implementation belongs to ntdll.
/// These are PE forwarders, so they retain the ntdll gateway and fail explicitly
/// until the real exception/unwind runtime supports the operation.
pub const KERNEL32_FORWARDERS: &[&str] = &[
    "RtlUnwind",
    "RtlUnwindEx",
    "RtlVirtualUnwind",
    "RtlCaptureContext",
    "RtlLookupFunctionEntry",
    "RtlPcToFileHeader",
];

fn ntdll_ordinal(name: &str, x64: bool, index: usize) -> u16 {
    syscall_metadata::NTDLL_CATALOG_ORDINALS.iter()
        .find(|entry| entry.0 == name)
        .map(|entry| if x64 { entry.1 } else { entry.2 })
        .unwrap_or(0x1000 + index as u16)
}

fn service_thunk(number: u32, arguments: u8, x64: bool) -> Vec<u8> {
    let mut code = vec![0xb8];
    code.extend_from_slice(&number.to_le_bytes());
    if x64 {
        code.extend_from_slice(&[0x4c, 0x8b, 0xd1, 0x0f, 0x05, 0xc3]);
    } else {
        code.extend_from_slice(&[0x8d, 0x54, 0x24, 0x04, 0xcd, 0x2e, 0xc2]);
        code.extend_from_slice(&(u16::from(arguments) * 4).to_le_bytes());
    }
    code
}

/// Emit NT61 guest machine code, not copied Windows code. These leaf thunks
/// enter the checked guest runtime and preserve each API's guest calling ABI.
/// Unsupported catalog entries terminate explicitly in the runtime handler.
pub fn build_runtime_dll(file_name: &str, x64: bool) -> Option<Vec<u8>> {
    let Some(index) = RUNTIME_DLLS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(file_name))
    else {
        return build_runtime_facade(file_name, x64);
    };
    // Preserve native System32's address plan. crypt32 has its own slot after
    // comctl32, avoiding collisions with user32/gdi32.
    let slot = [1u64, 0, 10, 11, 12, 9, 3, 2, 13][index];
    let base = if x64 {
        0x0000_1000_0000_0000 + slot * 0x1000_0000
    } else {
        0x6000_0000 + slot * 0x0100_0000
    };
    let mut image = PeImageSpec::new(
        base,
        super::pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        true,
        if x64 { MACHINE_AMD64 } else { 0x014c },
    );
    image.module_name = RUNTIME_DLLS[index].to_string();
    image.entry_point_rva = 0;
    let apis: Vec<_> = catalog::APIS
        .iter()
        .filter(|api| api.dll.eq_ignore_ascii_case(file_name))
        .collect();
    let mut text = vec![0xcc; apis.len() * STUB_STRIDE as usize];
    for (index, api) in apis.iter().enumerate() {
        let offset = index * STUB_STRIDE as usize;
        let mut code = vec![0xb8]; // mov eax, gateway service
        code.extend_from_slice(&(catalog::GATEWAY_BASE | api.id).to_le_bytes());
        if x64 {
            code.extend_from_slice(&[0x4c, 0x8b, 0xd1, 0x0f, 0x05, 0xc3]); // r10=rcx; syscall; ret
        } else {
            code.extend_from_slice(&[0x8d, 0x54, 0x24, 0x04, 0xcd, 0x2e]); // edx=&args; int 2e
            if api.cdecl {
                code.push(0xc3);
            } else {
                code.push(0xc2);
                code.extend_from_slice(&(u16::from(api.x86_arguments) * 4).to_le_bytes());
            }
        }
        assert!(code.len() <= STUB_STRIDE as usize);
        text[offset..offset + code.len()].copy_from_slice(&code);
        // ADVAPI32 keeps the audited named ordinals for ordinal imports.
        // Other DLLs retain their existing NT61 private ordinal numbering.
        let ordinal = if file_name.eq_ignore_ascii_case("advapi32.dll") {
            advapi_ordinals::ordinal(api.name, x64)
                .expect("ADVAPI32 export absent from the audited Windows 7 SP1 binary")
        } else if file_name.eq_ignore_ascii_case("ntdll.dll") {
            ntdll_ordinal(api.name, x64, index)
        } else {
            index as u16 + 1
        };
        image.export_named(api.name, ordinal, stub_rva(index as u32));
    }
    if file_name.eq_ignore_ascii_case("ntdll.dll") {
        // Preserve existing checked runtime implementations. Add every native
        // Nt/Zw identity with its original ordinal; unsupported services reach
        // the real dispatcher and fail explicitly instead of returning zero.
        for service in syscall_metadata::NATIVE_SERVICE_METADATA {
            let number = if x64 { service.number } else { service.x86_number };
            let zw = format!("Zw{}", &service.name[2..]);
            for (name, ordinal) in [
                (service.name, if x64 { service.x64_nt_ordinal } else { service.x86_nt_ordinal }),
                (zw.as_str(), if x64 { service.x64_zw_ordinal } else { service.x86_zw_ordinal }),
            ] {
                if apis.iter().any(|api| api.name == name) { continue; }
                let offset = text.len();
                text.resize(offset + STUB_STRIDE as usize, 0xcc);
                let code = if let Some(api) = apis.iter().find(|api| api.name == service.name) {
                    service_thunk(catalog::GATEWAY_BASE | api.id, service.arguments, x64)
                } else {
                    service_thunk(u32::from(number), service.arguments, x64)
                };
                text[offset..offset + code.len()].copy_from_slice(&code);
                image.export_named(name, ordinal, TEXT_RVA + offset as u32);
            }
        }
    }
    if file_name.eq_ignore_ascii_case("kernel32.dll") {
        for (index, name) in KERNEL32_FORWARDERS.iter().enumerate() {
            assert!(
                catalog::find("ntdll.dll", name).is_some(),
                "unknown kernel32 forward target"
            );
            image.export_forwarder(
                name,
                (apis.len() + index + 1) as u16,
                &format!("ntdll.{name}"),
            );
        }
    }
    image.sections = vec![SectionSpec::code(text), SectionSpec::rdata(Vec::new())];
    Some(super::pe::emit(&image).0)
}

/// Machine-readable ABI and support inventory. Forwarders inherit the target's
/// support state; host_service never promises a particular backend supports it.
pub fn runtime_manifest(x64: bool) -> String {
    fn quoted(value: &str) -> String {
        let mut out = String::from("\"");
        for c in value.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if c.is_control() => {
                    use core::fmt::Write;
                    let _ = write!(out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
    fn fields(dll: &str, name: &str) -> String {
        let support = catalog::support(dll, name);
        format!(
            "\"support\":{},\"scope\":{}",
            quoted(support.status.as_str()),
            quoted(support.scope)
        )
    }
    let mut text = format!("{{\"schema\":2,\"machine\":\"{}\",\"provenance\":\"NT61 source-generated guest thunks\",\"limitations\":{},\"exports\":[", if x64 { "AMD64" } else { "I386" }, quoted(catalog::RUNTIME_LIMITATIONS));
    for (index, api) in catalog::APIS.iter().enumerate() {
        if index != 0 {
            text.push(',');
        }
        let ordinal = if api.dll == "advapi32.dll" {
            advapi_ordinals::ordinal(api.name, x64).expect("audited ADVAPI32 ordinal")
        } else if api.dll == "ntdll.dll" {
            let index = catalog::APIS.iter().filter(|a| a.dll == api.dll)
                .position(|a| a.id == api.id).expect("catalog member");
            ntdll_ordinal(api.name, x64, index)
        } else {
            catalog::APIS.iter().filter(|a| a.dll == api.dll)
                .position(|a| a.id == api.id).expect("catalog member") as u16 + 1
        };
        text.push_str(&format!("{{\"dll\":\"{}\",\"name\":\"{}\",\"ordinal\":{},\"ordinal_provenance\":\"{}\",\"service\":{},\"arguments\":{},\"x86_stack_words\":{},\"cdecl\":{}, {}}}",
            api.dll, api.name, ordinal, if api.dll == "advapi32.dll" || (api.dll == "ntdll.dll" && ordinal < 0x1000) { "Windows 7 SP1 reference" } else { "NT61 private" }, catalog::GATEWAY_BASE | api.id, api.arguments, api.x86_arguments, api.cdecl, fields(api.dll, api.name)));
    }
    text.push_str("],\"native_services\":[");
    for (index, service) in syscall_metadata::NATIVE_SERVICE_METADATA.iter().enumerate() {
        if index != 0 { text.push(','); }
        text.push_str(&format!("{{\"name\":\"{}\",\"zw_alias\":\"Zw{}\",\"number\":{},\"arguments\":{},\"implementation_verified\":false}}",
            service.name, &service.name[2..], if x64 { service.number } else { service.x86_number }, service.arguments));
    }
    text.push_str("],\"forwarders\":[");
    for (i, name) in KERNEL32_FORWARDERS.iter().enumerate() {
        if i != 0 {
            text.push(',');
        }
        text.push_str(&format!(
            "{{\"dll\":\"kernel32.dll\",\"name\":\"{name}\",\"target\":\"ntdll.{name}\",{}}}",
            fields("ntdll.dll", name)
        ));
    }
    text.push_str("],\"contracts\":[");
    for (i, (contract, names)) in APISET_CONTRACTS.iter().enumerate() {
        if i != 0 {
            text.push(',');
        }
        text.push_str(&format!("{{\"dll\":\"{contract}\",\"forwarders\":["));
        for (j, name) in names.iter().enumerate() {
            if j != 0 {
                text.push(',');
            }
            let target = if name.starts_with("Rtl") {
                "ntdll"
            } else {
                "kernel32"
            };
            text.push_str(&format!(
                "{{\"name\":\"{name}\",\"target\":\"{target}.{name}\",{}}}",
                fields(&format!("{target}.dll"), name)
            ));
        }
        text.push_str("]}");
    }
    text.push_str("]}\n");
    text
}

/// Build every system DLL, keyed by file name.
pub fn build_all_dlls() -> Vec<(&'static str, Vec<u8>)> {
    ALL_DLLS
        .iter()
        .map(|spec| (spec.file_name, build_dll_image(spec)))
        .collect()
}

/// Emit the system DLL for the selected native architecture.
pub fn build_dll_image_for_arch(spec: &DllSpec, arch: &str) -> crate::error::Result<Vec<u8>> {
    let (machine, code): (u16, &[u8]) = match arch {
        "x86_64" | "x64" => (0x8664, &[0x31, 0xc0, 0xc3]), // xor eax,eax; ret
        "aarch64" | "arm64" => (0xaa64, &[0x00, 0x00, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6]), // mov w0,#0; ret
        "riscv64" => (0x5064, &[0x13, 0x05, 0x00, 0x00, 0x67, 0x80, 0x00, 0x00]), // li a0,0; ret
        "loongarch64" => (0x6264, &[0x04, 0x00, 0x80, 0x02, 0x20, 0x00, 0x00, 0x4c]), // addi.w a0,zero,0; jr ra
        _ => {
            return Err(crate::error::BuildError::InvalidParam(format!(
                "unsupported DLL architecture: {arch}"
            )))
        }
    };
    Ok(build_dll_image_native(spec, machine, code))
}

#[cfg(test)]
mod architecture_tests {
    use super::*;

    #[test]
    fn system_dll_headers_and_instructions_match_every_native_architecture() {
        let cases: &[(&str, u16, &[u8])] = &[
            ("x86_64", 0x8664, &[0x31, 0xc0, 0xc3]),
            (
                "aarch64",
                0xaa64,
                &[0x00, 0x00, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6],
            ),
            (
                "riscv64",
                0x5064,
                &[0x13, 0x05, 0x00, 0x00, 0x67, 0x80, 0x00, 0x00],
            ),
            (
                "loongarch64",
                0x6264,
                &[0x04, 0x00, 0x80, 0x02, 0x20, 0x00, 0x00, 0x4c],
            ),
        ];
        for &(arch, machine, code) in cases {
            for spec in ALL_DLLS {
                let bytes = build_dll_image_for_arch(spec, arch).unwrap();
                let image = super::super::pe_read::parse(&bytes).expect("valid PE");
                assert_eq!(image.machine, machine, "{arch}/{}", spec.file_name);
                assert_eq!(image.exports_by_name.len(), spec.exports.len());
                let text = image.sections.iter().find(|s| s.name == ".text").unwrap();
                for index in 0..spec.exports.len() {
                    let offset = text.pointer_to_raw_data as usize + index * STUB_STRIDE as usize;
                    assert_eq!(
                        &bytes[offset..offset + code.len()],
                        code,
                        "{arch}/{} export {index}",
                        spec.file_name
                    );
                }
            }
        }
        assert!(build_dll_image_for_arch(&ALL_DLLS[0], "=riscv64").is_err());
    }
}

//! The Win32 DLL export/import surface of NT61.
//!
//! This module is the *single source of truth* for what each system
//! DLL exports and what it imports. The build tool uses it to emit
//! the PE images, and the regression tests use it to verify that the
//! emitted images match the specification.
//!
//! The surface mirrors the Windows 7 SP1 x64 `System32` DLLs. The
//! **names and ordinals** are the publicly documented Win32 API and
//! are therefore reproduced exactly; the *implementations* are NT61's
//! own.
//!
//! # Why a table and not a pile of `build_*` functions
//!
//! The old `build_pe_image` path hard-coded one flat export list per
//! image in a Rust function. That made it impossible to say "this DLL
//! imports these three functions from `ntdll.dll`", and impossible to
//! write a test that compares the specification with the emitted
//! image. A table solves both.

use super::pe::{ImportSymbol, PeImageSpec, SectionSpec};

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

/// `IMAGE_FILE_MACHINE_AMD64`.
pub const MACHINE_AMD64: u16 = 0x8664;

/// The RVA of the first byte of `.text`.
pub const TEXT_RVA: u32 = 0x1000;

/// The stride between the stub entry points in `.text`.
///
/// Every exported function in these images is a 16-byte stub: a
/// `ret` followed by padding, with the real implementation reached
/// through the import address table. 16 bytes keeps the stubs
/// naturally aligned and matches the granularity the loader expects.
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
    image.entry_point_rva = TEXT_RVA;

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

    // Every stub that could hold an absolute address needs a base
    // relocation, so the image can be rebased.
    let mut relocations = Vec::new();
    for index in 0..spec.exports.len() as u32 {
        relocations.push(stub_rva(index) + 8);
    }
    image.relocations = relocations;

    image.sections = vec![
        SectionSpec::code(text),
        SectionSpec::rdata(Vec::new()),
        SectionSpec::reloc(Vec::new()),
    ];

    let (bytes, _) = super::pe::emit(&image);
    bytes
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
        _ => return Err(crate::error::BuildError::InvalidParam(format!("unsupported DLL architecture: {arch}"))),
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
            ("aarch64", 0xaa64, &[0x00, 0x00, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6]),
            ("riscv64", 0x5064, &[0x13, 0x05, 0x00, 0x00, 0x67, 0x80, 0x00, 0x00]),
            ("loongarch64", 0x6264, &[0x04, 0x00, 0x80, 0x02, 0x20, 0x00, 0x00, 0x4c]),
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
                    assert_eq!(&bytes[offset..offset + code.len()], code, "{arch}/{} export {index}", spec.file_name);
                }
            }
        }
        assert!(build_dll_image_for_arch(&ALL_DLLS[0], "=riscv64").is_err());
    }
}

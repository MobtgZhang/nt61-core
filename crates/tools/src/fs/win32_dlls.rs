//! Native surface metadata and NT61-owned x86/x64 guest runtime DLL factory.
//!
//! Guest exports enter a checked runtime gateway; name/ABI availability is
//! distinct from handler support. Guest runtime and foundation facade names
//! present in the audited Windows 7 SP1 reference use its ordinals.
//! Additional compatibility names use NT61 private assignments. Non-x86
//! ntdll exposes a subset of native kernel services independently. Product
//! code and thunks are built from source; no Microsoft machine code is shipped.

use super::pe::{ImportSymbol, PeImageSpec, SectionSpec};

#[path = "../../../arch/src/arch/common/btl/win32/catalog.rs"]
pub mod catalog;
#[path = "../../../arch/src/arch/common/btl/win32/advapi/ordinals.rs"]
mod advapi_ordinals;
#[path = "sp1_reference.rs"]
pub mod sp1_reference;
#[path = "../../../abi/src/syscall_metadata.rs"]
pub mod syscall_metadata;

// =====================================================================
//  Image bases
// =====================================================================

/// `ntdll.dll` — the lowest DLL, mapped first.
pub const NTDLL_BASE: u64 = 0x0000_0700_0000_0000;
/// `kernel32.dll`.
pub const KERNEL32_BASE: u64 = 0x0000_0700_1000_0000;
/// `user32.dll`.
pub const USER32_BASE: u64 = 0x0000_0700_2000_0000;
/// `gdi32.dll`.
pub const GDI32_BASE: u64 = 0x0000_0700_3000_0000;
/// `uxtheme.dll`.
pub const UXTHEME_BASE: u64 = 0x0000_0700_4000_0000;
/// `dwmapi.dll`.
pub const DWMAPI_BASE: u64 = 0x0000_0700_5000_0000;
/// `shell32.dll`.
pub const SHELL32_BASE: u64 = 0x0000_0700_6000_0000;
/// `ole32.dll`.
pub const OLE32_BASE: u64 = 0x0000_0700_7000_0000;
/// `oleaut32.dll`.
pub const OLEAUT32_BASE: u64 = 0x0000_0700_8000_0000;
/// `comctl32.dll`.
pub const COMCTL32_BASE: u64 = 0x0000_0700_9000_0000;
/// `comdlg32.dll`.
pub const COMDLG32_BASE: u64 = 0x0000_0700_c000_0000;

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
    /// Exported names and their original SP1 AMD64 ordinals; array order sets code RVAs.
    pub exports: &'static [(&'static str, u32)],
    /// The imported modules and symbols.
    pub imports: &'static [(&'static str, &'static [ImportSymbol])],
}

/// The `ntdll.dll` surface.
///
/// `ntdll.dll` is the boundary between Ring 3 and the kernel. Every
/// native service belongs here; win32k entry points belong inside user32/gdi32.
/// The Nt exports call the native kernel on the non-x86 hosts; the remaining
/// native runtime helpers are placeholders.
pub const NTDLL_EXPORTS: &[(&str, u32)] = &[
    // --- Native system services -------------------------------------
    ("NtAllocateVirtualMemory", 192),
    ("NtClose", 223),
    ("NtCreateFile", 239),
    ("NtDeviceIoControlFile", 280),
    ("NtFreeVirtualMemory", 304),
    ("NtQuerySystemInformation", 435),
    ("NtReadFile", 447),
    ("NtWriteFile", 570),
    ("NtWaitForSingleObject", 565),
    // --- Run-time library --------------------------------------------
    ("RtlAllocateHeap", 622),
    ("RtlFreeHeap", 852),
    ("RtlUserThreadStart", 1265),
    ("RtlInitUnicodeString", 940),
    ("LdrLoadDll", 132),
    ("LdrGetProcedureAddress", 125),

];

/// The `kernel32.dll` surface.
pub const KERNEL32_EXPORTS: &[(&str, u32)] = &[
    ("CloseHandle", 83),
    ("CreateFileW", 144),
    ("GetLastError", 519),
    ("SetLastError", 1152),
    ("GetProcAddress", 587),
    ("GetModuleHandleW", 541),
    ("LoadLibraryW", 835),
    ("ReadFile", 964),
    ("Sleep", 1218),
    ("VirtualAlloc", 1274),
    ("VirtualFree", 1277),
    ("WriteFile", 1334),
];

const NATIVE_KERNEL32_EXPORTS: &[(&str, u32)] = &[
    ("GetCurrentProcess", 455),
    ("GetCurrentThread", 459),
    ("Sleep", 1218),
];

/// The `user32.dll` surface.
pub const USER32_EXPORTS: &[(&str, u32)] = &[
    ("BeginPaint", 1516),
    ("CreateWindowExW", 1619),
    ("DefWindowProcW", 1665),
    ("DestroyWindow", 1675),
    ("DispatchMessageW", 1684),
    ("DrawTextW", 1717),
    ("EndPaint", 1729),
    ("GetClientRect", 1787),
    ("GetDC", 1800),
    ("GetMessageW", 1860),
    ("GetSystemMetrics", 1893),
    ("GetWindowTextW", 1932),
    ("InvalidateRect", 1959),
    ("MessageBoxW", 2050),
    ("PeekMessageW", 2080),
    ("RegisterClassExW", 2106),
    ("ReleaseDC", 2130),
    ("SendMessageW", 2153),
    ("SetWindowPos", 2231),
    ("SetWindowTextW", 2236),
    ("ShowWindow", 2256),
    ("TranslateMessage", 2285),
    ("PostMessageW", 2083),
    ("PostQuitMessage", 2084),
    ("GetCursorPos", 1799),
    ("SetCapture", 2157),
    ("ReleaseCapture", 2129),
    ("GetForegroundWindow", 1812),
    ("SetForegroundWindow", 2178),
    ("OpenWindowStationW", 2074),
    ("CreateDesktopW", 1603),
    ("SwitchDesktop", 2265),
    ("GetWindowRect", 1925),
    ("FillRect", 1755),
    ("DrawEdge", 1704),
];

/// The `gdi32.dll` surface.
pub const GDI32_EXPORTS: &[(&str, u32)] = &[
    ("BitBlt", 1031),
    ("CreateCompatibleDC", 1060),
    ("CreateDIBSection", 1065),
    ("CreateFontIndirectW", 1076),
    ("CreatePen", 1087),
    ("CreateSolidBrush", 1096),
    ("DeleteObject", 1242),
    ("Ellipse", 1249),
    ("EnumFontsW", 1308),
    ("ExtTextOutW", 1324),
    ("GetStockObject", 1537),
    ("GetTextExtentPoint32W", 1554),
    ("GetTextFaceW", 1560),
    ("GetTextMetricsW", 1562),
    ("IntersectClipRect", 1572),
    ("LineTo", 1578),
    ("MoveToEx", 1582),
    ("Polygon", 1610),
    ("Rectangle", 1619),
    ("RoundRect", 1630),
    ("SelectObject", 1643),
    ("SetBkMode", 1651),
    ("SetTextColor", 1690),
    ("StretchBlt", 1703),
    ("TextOutW", 1709),
    ("PatBlt", 1594),
    ("GdiAlphaBlend", 1343),
    ("GdiFlush", 1385),
    ("CreateRectRgn", 1091),
];

/// The `uxtheme.dll` surface.
pub const UXTHEME_EXPORTS: &[(&str, u32)] = &[
    ("CloseThemeData", 53),
    ("DrawThemeBackground", 54),
    ("DrawThemeText", 59),
    ("EnableTheming", 87),
    ("GetCurrentThemeName", 94),
    ("GetThemeColor", 101),
    ("GetThemeMargins", 108),
    ("GetThemeMetric", 109),
    ("GetThemePartSize", 110),
    ("IsThemeActive", 130),
    ("OpenThemeData", 134),
    ("SetWindowTheme", 136),
];

/// The `dwmapi.dll` surface.
pub const DWMAPI_EXPORTS: &[(&str, u32)] = &[
    ("DwmEnableComposition", 102),
    ("DwmExtendFrameIntoClientArea", 135),
    ("DwmFlush", 136),
    ("DwmGetWindowAttribute", 142),
    ("DwmIsCompositionEnabled", 144),
    ("DwmRegisterThumbnail", 147),
    ("DwmSetIconicLivePreviewBitmap", 149),
    ("DwmSetIconicThumbnail", 150),
    ("DwmSetWindowAttribute", 152),
    ("DwmUnregisterThumbnail", 153),
    ("DwmUpdateThumbnailProperties", 154),
];

/// The `shell32.dll` surface.
pub const SHELL32_EXPORTS: &[(&str, u32)] = &[
    ("ExtractIconExW", 293),
    ("SHBrowseForFolderW", 323),
    ("SHChangeNotify", 324),
    ("SHFileOperationW", 355),
    ("SHGetFileInfoW", 367),
    ("SHGetFolderPathW", 373),
    ("SHGetKnownFolderPath", 382),
    ("SHGetPathFromIDListW", 390),
    ("SHGetSpecialFolderLocation", 395),
    ("SHParseDisplayName", 408),
    ("ShellExecuteExW", 436),
    ("Shell_NotifyIconW", 444),
];

/// The `ole32.dll` surface.
pub const OLE32_EXPORTS: &[(&str, u32)] = &[
    ("CLSIDFromString", 22),
    ("CoCreateInstance", 30),
    ("CoInitializeEx", 77),
    ("CoTaskMemAlloc", 117),
    ("CoTaskMemFree", 118),
    ("CoUninitialize", 122),
    ("IIDFromString", 260),
    ("OleInitialize", 360),
    ("OleUninitialize", 383),
    ("StringFromCLSID", 437),
];

/// The `oleaut32.dll` surface.
pub const OLEAUT32_EXPORTS: &[(&str, u32)] = &[
    ("SafeArrayCreate", 15),
    ("SafeArrayDestroy", 16),
    ("SysAllocString", 2),
    ("SysAllocStringLen", 4),
    ("SysFreeString", 6),
    ("SysStringLen", 7),
    ("VariantClear", 9),
    ("VariantInit", 8),
];

/// The Windows 7 common-dialog export surface.
pub const COMDLG32_EXPORTS: &[(&str, u32)] = &[
    ("CommDlgExtendedError", 106),
    ("GetOpenFileNameA", 113),
    ("GetOpenFileNameW", 114),
    ("GetSaveFileNameA", 115),
    ("GetSaveFileNameW", 116),
    ("ChooseFontA", 104),
    ("ChooseFontW", 105),
    ("ChooseColorA", 102),
    ("ChooseColorW", 103),
    ("PrintDlgA", 120),
    ("PrintDlgW", 123),
    ("PageSetupDlgA", 118),
    ("PageSetupDlgW", 119),
    ("GetFileTitleA", 111),
    ("GetFileTitleW", 112),
];

/// The `comctl32.dll` (v6) surface.
pub const COMCTL32_EXPORTS: &[(&str, u32)] = &[
    ("CreatePropertySheetPageW", 19),
    ("CreateStatusWindowW", 21),
    ("CreateToolbarEx", 22),
    ("DestroyPropertySheetPage", 23),
    ("ImageList_Add", 39),
    ("ImageList_AddIcon", 40),
    ("ImageList_Create", 44),
    ("ImageList_Destroy", 45),
    ("ImageList_GetIcon", 58),
    ("ImageList_SetIconSize", 79),
    ("InitCommonControlsEx", 83),
    ("InitMUILanguage", 84),
    ("PropertySheetW", 88),
    ("InitCommonControls", 17),
    ("GetMUILanguage", 38),
    ("DllGetVersion", 24),
    ("ImageList_GetImageCount", 60),
    ("ImageList_GetIconSize", 59),
    ("ImageList_SetImageCount", 80),
    ("ImageList_Remove", 68),
];

/// Limited native imports for the legacy PE/loader scaffold.
///
/// All four names occur in the original SP1 user32/gdi32 import tables.
/// SP1 keeps its win32k syscall entry points internal to user32/gdi32;
/// neither DLL imports named NtUser/NtGdi entry points from ntdll.
/// This scaffold does not implement those entry points or the complete imports.
pub const USER32_IMPORTS: &[(&str, &[ImportSymbol])] = &[(
    "ntdll.dll",
    &[
        ImportSymbol::ByName("RtlAllocateHeap"),
        ImportSymbol::ByName("RtlFreeHeap"),
        ImportSymbol::ByName("RtlInitUnicodeString"),
        ImportSymbol::ByName("NtClose"),
    ],
)];

/// The same observed native subset for the legacy gdi32 scaffold.
pub const GDI32_IMPORTS: &[(&str, &[ImportSymbol])] = USER32_IMPORTS;

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
    let native_ntdll = machine != MACHINE_AMD64 && spec.file_name == "ntdll.dll";
    let native_kernel32 = machine != MACHINE_AMD64 && spec.file_name == "kernel32.dll";
    let image_base = match (machine, native_ntdll, native_kernel32) {
        (0x5064, true, _) => 0x0000_0030_0000_0000,
        (0x5064, _, true) => 0x0000_0030_1000_0000,
        _ => spec.image_base,
    };
    let mut image = PeImageSpec::new(
        image_base,
        super::pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        true,
        machine,
    );
    apply_reference_header(&mut image, spec.file_name, machine != 0x014c);
    image.module_name = spec.file_name.to_string();
    image.entry_point_rva = 0; // No DllMain; export zero is not an initializer.

    let stride = if native_kernel32 { 80 } else if native_ntdll { 64 } else { STUB_STRIDE as usize };
    let exports = if native_kernel32 { NATIVE_KERNEL32_EXPORTS } else { spec.exports };
    let exports = exports.iter().filter(|(name, _)| !native_ntdll || name.starts_with("Nt"));
    let mut text = vec![0u8; exports.clone().count() * stride];
    for (index, (name, ordinal)) in exports.enumerate() {
        let at = index * stride;
        if native_ntdll || native_kernel32 {
            let words = if native_ntdll {
                let service = syscall_metadata::NATIVE_SERVICE_METADATA.iter().find(|entry| entry.name == *name)
                    .expect("native ntdll service must exist in the SP1 table");
                assert_eq!(u32::from(service.x64_nt_ordinal), *ordinal);
                native_nt_thunk(machine, service.number, service.arguments)
            } else {
                native_kernel32_thunk(machine, name).to_vec()
            };
            assert!(words.len() * 4 <= stride);
            for (slot, instruction) in text[at..at + stride].chunks_exact_mut(4).zip(words) {
                slot.copy_from_slice(&instruction.to_le_bytes());
            }
        } else {
            text[at..at + return_code.len()].copy_from_slice(return_code);
        }
        image.export_named(name, u16::try_from(*ordinal).expect("SP1 export ordinal"), TEXT_RVA + at as u32);
    }

    if !native_kernel32 {
        for (dll_name, symbols) in spec.imports {
            image.import(dll_name, symbols.to_vec());
        }
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

fn native_kernel32_thunk(machine: u16, name: &str) -> &'static [u32] {
    match (machine, name) {
        (0xaa64, "GetCurrentProcess") => &[0x9280_0000, 0xd65f_03c0],
        (0xaa64, "GetCurrentThread") => &[0x9280_0020, 0xd65f_03c0],
        (0x5064, "GetCurrentProcess") => &[0xfff0_0513, 0x0000_8067],
        (0x5064, "GetCurrentThread") => &[0xffe0_0513, 0x0000_8067],
        (0x6264, "GetCurrentProcess") => &[0x02bf_fc04, 0x4c00_0020],
        (0x6264, "GetCurrentThread") => &[0x02bf_f804, 0x4c00_0020],
        (0xaa64, "Sleep") => &[
            0x5284_e209, 0x9ba9_7c08, 0xcb08_03e8, 0xd2f0_000a, 0x3100_041f, 0x9a88_0148,
            0xf81f_0fe8, 0x9100_03e1, 0x2a1f_03e0, 0x5280_0630, 0xd400_0001, 0x9100_43ff,
            0xd65f_03c0,
        ],
        (0x5064, "Sleep") => &[
            0x0005_031b, 0xfff0_0293, 0x0253_0063, 0x0205_1313, 0x0203_5313, 0x0000_22b7,
            0x7102_829b, 0x0253_02b3, 0x4050_02b3, 0x00c0_006f, 0xfff0_029b, 0x03f2_9293,
            0xff01_0113, 0x0051_3023, 0x0001_0593, 0x0000_0513, 0x0310_0893, 0x0000_0073,
            0x0101_0113, 0x0000_8067,
        ],
        (0x6264, "Sleep") => &[
            0x0040_808e, 0x02bf_fc0d, 0x5800_1dcd, 0x00df_008c, 0x1400_004d, 0x039c_41ad,
            0x001d_b58c, 0x0011_b00c, 0x5000_0800, 0x0320_000c, 0x02ff_c063, 0x29c0_006c,
            0x0015_0065, 0x0015_0004, 0x0280_c40b, 0x002b_0000, 0x02c0_4063, 0x4c00_0020,
        ],
        _ => unreachable!("unsupported native Kernel32 export: {machine:#06x}/{name}"),
    }
}

fn native_nt_thunk(machine: u16, service: u16, arguments: u8) -> Vec<u32> {
    let service = u32::from(service);
    assert!(service <= 0x7ff && arguments <= 11);
    if machine == 0xaa64 {
        return vec![0x5280_0010 | (service << 5), 0xd400_0001, 0xd65f_03c0];
    }
    let extra = arguments.saturating_sub(7) as usize;
    let (reserve, spill, copies, load_service, trap, extend_status, restore, ret) = match machine {
        0x5064 => (
            0xfe01_0113, 0x0111_3023,
            [0x0201_3283, 0x0051_3423, 0x0281_3283, 0x0051_3823, 0x0301_3283, 0x0051_3c23],
            0x0000_0893 | (service << 20), 0x0000_0073, 0x0005_051b, 0x0201_0113, 0x0000_8067,
        ),
        0x6264 => (
            0x02ff_8063, 0x29c0_006b,
            [0x28c0_806c, 0x29c0_206c, 0x28c0_a06c, 0x29c0_406c, 0x28c0_c06c, 0x29c0_606c],
            0x0280_000b | (service << 10), 0x002b_0000, 0x0010_0084, 0x02c0_8063, 0x4c00_0020,
        ),
        _ => unreachable!("no native NT syscall ABI for machine {machine:#06x}"),
    };
    let mut words = Vec::new();
    if extra != 0 {
        words.extend([reserve, spill]);
        words.extend_from_slice(&copies[..(extra - 1) * 2]);
    }
    words.extend([load_service, trap, extend_status]);
    if extra != 0 { words.push(restore); }
    words.push(ret);
    words
}

/// DLLs backed by the isolated x86/x64 runtime gateway. Keeping this set
/// explicit avoids confusing native architecture placeholders with guest code.
pub const RUNTIME_DLLS: &[&str] = &["kernel32.dll", "ntdll.dll", "crypt32.dll", "advapi32.dll", "comdlg32.dll", "comctl32.dll", "gdi32.dll", "user32.dll", "bcrypt.dll", "shell32.dll", "ole32.dll", "oleaut32.dll", "shlwapi.dll", "msvcrt.dll", "version.dll", "winspool.drv", "ws2_32.dll"];

/// A source-owned facade export backed by an existing checked runtime handler.
/// The ordinal belongs to the original image, independently for each bitness.
#[derive(Clone, Debug)]
pub struct FoundationExport {
    pub name: &'static str,
    pub ordinal: u16,
    pub target: &'static catalog::Api,
}

// Non-x86 scaffold ports use the AMD64 semantic reference only; no original
// ARM64/RISC-V/LoongArch SP1 binary or ABI equivalence is implied.
fn apply_reference_header(image: &mut PeImageSpec, name: &str, x64: bool) {
    if let Some(header) = sp1_reference::HEADERS.iter()
        .find(|header| header.x64 == x64 && header.name.eq_ignore_ascii_case(name)) {
        image.subsystem = header.subsystem;
        image.os_version = header.os_version;
        image.subsystem_version = header.subsystem_version;
    }
}

fn reference_module(name: &str, x64: bool) -> Option<&'static sp1_reference::Module> {
    sp1_reference::MODULES.iter().find(|m| m.x64 == x64 && m.name.eq_ignore_ascii_case(name))
}

fn foundation_target(dll: &str, name: &str, x64: bool, depth: usize, importer: Option<&str>) -> Option<&'static catalog::Api> {
    if depth > 8 { return None; }
    if let Some(api) = catalog::find(dll, name) { return Some(api); }
    let module = reference_module(dll, x64)?;
    let export = module.exports.iter().find(|e| e.name == name)?;
    if module.name == "wsock32.dll" && export.forwarder.is_none()
        && matches!(name, "getsockopt" | "recv" | "recvfrom" | "setsockopt")
    {
        return catalog::find("ws2_32.dll", name);
    }
    // A forwarded Kernel32 import uses the ISO's importer-specific host,
    // commonly KernelBase, rather than the contract's default Kernel32 host.
    // Otherwise valid SP1 forwarding chains loop back into their importer.
    if let Some(host) = module.host_for_importer(importer) {
        return foundation_target(host, name, x64, depth + 1, None);
    }
    if let Some(forwarder) = export.forwarder {
        let (host, symbol) = forwarder.rsplit_once('.')?;
        // Ordinal-only forwarders require separate ABI evidence; do not guess.
        if symbol.starts_with('#') { return None; }
        let host = format!("{}.dll", host.trim_end_matches(".dll").to_ascii_lowercase());
        // Some SP1 kernelbase entries forward to an ntdll identity for which
        // NT61 has no gateway yet. Preserve the explicit composition below
        // when the same checked Win32 ABI is already implemented in kernel32.
        if let Some(target) = foundation_target(&host, symbol, x64, depth + 1, Some(module.name)) {
            return Some(target);
        }
    }
    // Implementation composition is NT61-owned. These are deliberate module
    // boundaries, not a global same-name lookup or an arbitrary API-set alias.
    let targets: &[&str] = match module.name {
        "kernelbase.dll" => &["kernel32.dll", "advapi32.dll"],
        "sechost.dll" => &["advapi32.dll"],
        "kernel32.dll" if name.starts_with("Reg") => &["advapi32.dll"],
        "msvcrt.dll" if matches!(name, "memcmp" | "memcpy" | "memmove" | "memset"
            | "memchr" | "strstr" | "wcsstr" | "strpbrk" | "wcspbrk"
            | "strspn" | "wcsspn" | "strcspn" | "wcscspn"
            | "strlen" | "strnlen" | "strcmp" | "strncmp" | "strchr" | "strrchr"
            | "wcslen" | "wcsnlen" | "wcscmp" | "wcsncmp" | "wcschr" | "wcsrchr"
            | "strcpy" | "strncpy" | "strcat" | "strncat"
            | "wcscpy" | "wcsncpy" | "wcscat" | "wcsncat") => &["ntdll.dll"],
        _ => &[],
    };
    targets.iter().find_map(|target| catalog::find(target, name))
}

/// Original SP1 names for which NT61 has an explicit checked gateway identity.
/// Unsupported handlers retain their unsupported status; unknown ABI entries
/// are omitted, never replaced with zero-return stubs.
pub fn foundation_exports(name: &str, x64: bool) -> Vec<FoundationExport> {
    let Some(module) = reference_module(name, x64) else { return Vec::new(); };
    module.exports.iter().filter_map(|export| {
        foundation_target(module.name, export.name, x64, 0, None).map(|target| FoundationExport {
            name: export.name, ordinal: export.ordinal, target,
        })
    }).collect()
}

pub fn runtime_library_names_for_arch(x64: bool) -> impl Iterator<Item = &'static str> {
    let mut names = RUNTIME_DLLS.to_vec();
    for module in sp1_reference::MODULES.iter().filter(|module| module.x64 == x64) {
        if !names.contains(&module.name) && !foundation_exports(module.name, x64).is_empty() {
            names.push(module.name);
        }
    }
    names.into_iter()
}

pub fn runtime_library_names() -> impl Iterator<Item = &'static str> {
    runtime_library_names_for_arch(true)
}

fn kernel32_extra_forwarders(x64: bool) -> Vec<FoundationExport> {
    foundation_exports("kernel32.dll", x64).into_iter().filter(|e|
        catalog::find("kernel32.dll", e.name).is_none()
        && !KERNEL32_FORWARDERS.contains(&e.name)).collect()
}

fn msvcrt_extra_forwarders(x64: bool) -> impl Iterator<Item = FoundationExport> {
    foundation_exports("msvcrt.dll", x64).into_iter()
        .filter(|export| export.target.dll != "msvcrt.dll")
}

fn msvcrt_data_exports(x64: bool) -> impl Iterator<Item = (&'static str, u16, u32)> {
    let reference = reference_module("msvcrt.dll", x64).expect("audited MSVCRT reference");
    catalog::MSVCRT_DATA_EXPORTS.iter().map(move |&(name, offset)| {
        let ordinal = reference.exports.iter().find(|export| export.name == name)
            .expect("audited MSVCRT data export").ordinal;
        (name, ordinal, catalog::MSVCRT_DATA_RVA + offset)
    })
}

fn kernel32_ordinal(name: &str, x64: bool, private_index: usize) -> u16 {
    runtime_ordinal("kernel32.dll", name, x64, private_index)
}

fn runtime_ordinal(dll: &str, name: &str, x64: bool, private_index: usize) -> u16 {
    match (dll, name, x64) {
        ("oleaut32.dll", "SysAllocString", _) => return 2,
        ("oleaut32.dll", "SysAllocStringLen", _) => return 4,
        ("oleaut32.dll", "SysFreeString", _) => return 6,
        ("oleaut32.dll", "SysStringLen", _) => return 7,
        ("oleaut32.dll", "VariantInit", _) => return 8,
        ("oleaut32.dll", "VariantClear", _) => return 9,
        ("shlwapi.dll", "PathIsFileSpecW", false) => return 683,
        ("shlwapi.dll", "PathIsFileSpecW", true) => return 688,
        ("shlwapi.dll", "SHStrDupW", false) => return 823,
        ("shlwapi.dll", "SHStrDupW", true) => return 828,
        ("comctl32.dll", "TaskDialogIndirect", _) => return 345,
        ("version.dll", "GetFileVersionInfoExW", _) => return 3,
        ("version.dll", "GetFileVersionInfoSizeExW", _) => return 5,
        ("version.dll", "VerQueryValueW", _) => return 15,
        ("winspool.drv", "ClosePrinter", _) => return 146,
        ("winspool.drv", "GetPrinterDriverW", _) => return 283,
        ("winspool.drv", "OpenPrinterW", _) => return 293,
        _ => {}
    }
    reference_module(dll, x64)
        .and_then(|m| m.exports.iter().find(|e| e.name == name))
        .map(|e| e.ordinal)
        .unwrap_or(0x1000 + private_index as u16)
}

fn runtime_apis(x64: bool) -> impl Iterator<Item = &'static catalog::Api> {
    catalog::APIS.iter().filter(move |api| match api.name {
        "__C_specific_handler" => x64,
        "_except_handler4_common" => !x64,
        "InterlockedExchange" | "InterlockedCompareExchange" | "InterlockedExchangeAdd"
        | "InterlockedIncrement" | "InterlockedDecrement" => !x64,
        _ => true,
    })
}

fn build_runtime_facade(file_name: &str, x64: bool) -> Option<Vec<u8>> {
    if let Some(module) = reference_module(file_name, x64) {
        let exports = foundation_exports(module.name, x64);
        if exports.is_empty() { return None; }
        let index = sp1_reference::MODULES.iter().position(|m| core::ptr::eq(m, module))?;
        let base = if x64 { 0x0000_0720_0000_0000 } else { 0x4000_0000 }
            + index as u64 * 0x0010_0000;
        let mut image = PeImageSpec::new(base, super::pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
            true, if x64 { MACHINE_AMD64 } else { 0x014c });
        apply_reference_header(&mut image, module.name, x64);
        image.module_name = module.name.to_string();
        image.entry_point_rva = 0;
        let mut text = Vec::new();
        for export in exports {
            if module.name == "wsock32.dll"
                && matches!(export.name, "getsockopt" | "recv" | "recvfrom" | "setsockopt")
            {
                let rva = stub_rva((text.len() / STUB_STRIDE as usize) as u32);
                let thunk = service_thunk(catalog::GATEWAY_BASE | export.target.id,
                    export.target.x86_arguments, x64);
                text.extend_from_slice(&thunk);
                text.resize(text.len() + STUB_STRIDE as usize - thunk.len(), 0xcc);
                image.export_named(export.name, export.ordinal, rva);
            } else {
                let host = export.target.dll.trim_end_matches(".dll");
                image.export_forwarder(export.name, export.ordinal, &format!("{host}.{}", export.target.name));
            }
        }
        if !text.is_empty() {
            image.sections.push(SectionSpec::code(text));
        }
        image.sections.push(SectionSpec::rdata(Vec::new()));
        return Some(super::pe::emit(&image).0);
    }
    // No fallback table: a missing SP1 module is not an implicit contract alias.
    None
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
    build_runtime_image(file_name, x64, None)
}

/// Compose source-generated desktop resources with runtime exports. The input
/// is an NT61 resource PE, never a DLL copied from the Windows reference ISO.
pub fn build_runtime_dll_with_resources(file_name: &str, x64: bool, resources: &[u8]) -> Option<Vec<u8>> {
    build_runtime_image(file_name, x64, Some(resources))
}

fn build_runtime_image(file_name: &str, x64: bool, resources: Option<&[u8]>) -> Option<Vec<u8>> {
    let Some(index) = RUNTIME_DLLS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(file_name))
    else {
        return build_runtime_facade(file_name, x64);
    };
    // Preserve native System32's address plan. crypt32 has its own slot after
    // comctl32, avoiding collisions with user32/gdi32.
    let slot = [1u64, 0, 10, 11, 12, 9, 3, 2, 13, 4, 5, 8, 6, 14, 15, 16, 17][index];
    let base = if x64 {
        0x0000_0700_0000_0000 + slot * 0x1000_0000
    } else {
        0x6000_0000 + slot * 0x0100_0000
    };
    let mut image = PeImageSpec::new(
        base,
        super::pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        true,
        if x64 { MACHINE_AMD64 } else { 0x014c },
    );
    apply_reference_header(&mut image, RUNTIME_DLLS[index], x64);
    image.module_name = RUNTIME_DLLS[index].to_string();
    image.entry_point_rva = 0;
    let apis: Vec<_> = runtime_apis(x64)
        .filter(|api| api.dll.eq_ignore_ascii_case(file_name))
        .collect();
    let mut text = vec![0xcc; apis.len() * STUB_STRIDE as usize];
    for (index, api) in apis.iter().enumerate() {
        let offset = index * STUB_STRIDE as usize;
        let mut code = if api.name == "RtlCaptureContext" {
            // Save entry registers before the gateway overwrites EAX/EDX or
            // RAX/RCX/R10/R11. Keep the stable export slot, jumping to our
            // source-generated longer thunk; no Windows instructions copied.
            let target = text.len();
            let mut thunk = if x64 {
                vec![0x50, 0x41, 0x52, 0x41, 0x53]
            } else {
                vec![0x50, 0x51, 0x52]
            };
            thunk.push(0xb8);
            thunk.extend_from_slice(&(catalog::GATEWAY_BASE | api.id).to_le_bytes());
            if x64 {
                thunk.extend_from_slice(&[
                    0x4c, 0x8b, 0xd1, 0x0f, 0x05, // r10=rcx; syscall
                    0x49, 0x8b, 0xca, 0x41, 0x5b, 0x41, 0x5a, // restore rcx,r11,r10
                    0x48, 0x8d, 0x64, 0x24, 0x08, 0xc3, // discard saved rax; ret
                ]);
            } else {
                thunk.extend_from_slice(&[
                    0x8d, 0x54, 0x24, 0x10, 0xcd, 0x2e,
                    0x5a, 0x59, 0x8d, 0x64, 0x24, 0x04, 0xc2, 0x04, 0x00,
                ]);
            }
            text.extend_from_slice(&thunk);
            let mut jump = vec![0xe9];
            let displacement = i32::try_from(target as i64 - offset as i64 - 5)
                .expect("context thunk must fit in a rel32 jump");
            jump.extend_from_slice(&displacement.to_le_bytes());
            jump
        } else {
            vec![0xb8] // mov eax, gateway service
        };
        if api.name != "RtlCaptureContext" {
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
        }
        assert!(code.len() <= STUB_STRIDE as usize);
        text[offset..offset + code.len()].copy_from_slice(&code);
        // Audited modules keep their SP1 ordinals. Extra compatibility names
        // use a distinct private range; other DLLs retain NT61 numbering.
        let ordinal = if file_name.eq_ignore_ascii_case("advapi32.dll") {
            advapi_ordinals::ordinal(api.name, x64)
                .expect("ADVAPI32 export absent from the audited Windows 7 SP1 binary")
        } else {
            runtime_ordinal(api.dll, api.name, x64, index)
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
                kernel32_ordinal(name, x64, apis.len() + index),
                &format!("ntdll.{name}"),
            );
        }
        for export in kernel32_extra_forwarders(x64) {
            image.export_forwarder(export.name, export.ordinal,
                &format!("{}.{}", export.target.dll.trim_end_matches(".dll"), export.target.name));
        }
    }
    image.sections = vec![SectionSpec::code(text)];
    if file_name.eq_ignore_ascii_case("msvcrt.dll") {
        assert!(image.sections[0].data.len() <= (catalog::MSVCRT_DATA_RVA - TEXT_RVA) as usize);
        for (name, ordinal, rva) in msvcrt_data_exports(x64) {
            image.export_named(name, ordinal, rva);
        }
        for export in msvcrt_extra_forwarders(x64) {
            let host = export.target.dll.trim_end_matches(".dll");
            image.export_forwarder(export.name, export.ordinal, &format!("{host}.{}", export.target.name));
        }
        let mut data = vec![0; 16];
        data.extend_from_slice(&catalog::MSVCRT_DATA_TAG);
        image.sections.push(SectionSpec::data(data));
    }
    image.sections.push(SectionSpec::rdata(Vec::new()));
    let mut bytes = super::pe::emit(&image).0;
    if let Some(resources) = resources {
        let source = super::pe_read::parse(resources)?;
        if source.machine != image.machine { return None; }
        let (old_rva, size) = *source.data_directories.get(2)?;
        let section = source.sections.iter().find(|s| s.virtual_address == old_rva && size <= s.virtual_size)?;
        let start = section.pointer_to_raw_data as usize;
        let mut data = resources.get(start..start.checked_add(size as usize)?)?.to_vec();
        let new_rva = super::pe_read::parse(&bytes)?.size_of_image;
        // Directory offsets are relative to the resource root; only each
        // IMAGE_RESOURCE_DATA_ENTRY payload RVA changes when moving .rsrc.
        fn collect(data: &[u8], at: usize, depth: u8, leaves: &mut Vec<usize>) -> Option<()> {
            if depth > 2 { return None; }
            let header = data.get(at..at.checked_add(16)?)?;
            let count = usize::from(u16::from_le_bytes(header[12..14].try_into().ok()?))
                + usize::from(u16::from_le_bytes(header[14..16].try_into().ok()?));
            let entries = data.get(at+16..(at+16).checked_add(count.checked_mul(8)?)?)?;
            for entry in entries.chunks_exact(8) {
                let child = u32::from_le_bytes(entry[4..8].try_into().ok()?);
                if child & 0x80000000 != 0 {
                    collect(data, (child & 0x7fffffff) as usize, depth+1, leaves)?;
                } else {
                    if depth != 2 { return None; }
                    if !leaves.contains(&(child as usize)) { leaves.push(child as usize); }
                }
            }
            Some(())
        }
        let mut leaves = Vec::new();
        collect(&data, 0, 0, &mut leaves)?;
        for at in leaves {
            let entry = data.get(at..at.checked_add(16)?)?;
            let offset = u32::from_le_bytes(entry[..4].try_into().ok()?).checked_sub(old_rva)?;
            let length = u32::from_le_bytes(entry[4..8].try_into().ok()?);
            data.get(offset as usize..offset.checked_add(length)? as usize)?;
            data[at..at+4].copy_from_slice(&new_rva.checked_add(offset)?.to_le_bytes());
        }
        image.sections.push(SectionSpec { name: ".rsrc".into(), characteristics: 0x40000040, data });
        bytes = super::pe::emit(&image).0;
        let placed = super::pe_read::parse(&bytes)?;
        if placed.sections.last()?.virtual_address != new_rva { return None; }
        let pe = u32::from_le_bytes(bytes.get(0x3c..0x40)?.try_into().ok()?) as usize;
        let directory = pe + 24 + if x64 { 0x70 } else { 0x60 };
        bytes[directory+16..directory+20].copy_from_slice(&new_rva.to_le_bytes());
        bytes[directory+20..directory+24].copy_from_slice(&size.to_le_bytes());
    }
    Some(bytes)
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
    for (index, api) in runtime_apis(x64).enumerate() {
        if index != 0 {
            text.push(',');
        }
        let ordinal = if api.dll == "advapi32.dll" {
            advapi_ordinals::ordinal(api.name, x64).expect("audited ADVAPI32 ordinal")
        } else {
            let index = runtime_apis(x64).filter(|a| a.dll == api.dll)
                .position(|a| a.id == api.id).expect("catalog member");
            runtime_ordinal(api.dll, api.name, x64, index)
        };
        text.push_str(&format!("{{\"dll\":\"{}\",\"name\":\"{}\",\"ordinal\":{},\"ordinal_provenance\":\"{}\",\"service\":{},\"arguments\":{},\"x86_stack_words\":{},\"cdecl\":{}, {}}}",
            api.dll, api.name, ordinal, if ordinal < 0x1000 { "Windows 7 SP1 reference" } else { "NT61 private" }, catalog::GATEWAY_BASE | api.id, api.arguments, api.x86_arguments, api.cdecl, fields(api.dll, api.name)));
    }
    text.push_str("],\"data_exports\":[");
    for (index, (name, ordinal, rva)) in msvcrt_data_exports(x64).enumerate() {
        if index != 0 { text.push(','); }
        text.push_str(&format!("{{\"dll\":\"msvcrt.dll\",\"name\":\"{name}\",\"ordinal\":{ordinal},\"rva\":{rva},{}}}",
            fields("msvcrt.dll", name)));
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
        let index = runtime_apis(x64).filter(|api| api.dll == "kernel32.dll").count() + i;
        let ordinal = kernel32_ordinal(name, x64, index);
        text.push_str(&format!(
            "{{\"dll\":\"kernel32.dll\",\"name\":\"{name}\",\"ordinal\":{ordinal},\"ordinal_provenance\":\"{}\",\"target\":\"ntdll.{name}\",{}}}",
            if ordinal < 0x1000 { "Windows 7 SP1 reference" } else { "NT61 private" },
            fields("ntdll.dll", name)
        ));
    }
    for export in kernel32_extra_forwarders(x64) {
        text.push_str(&format!(",{{\"dll\":\"kernel32.dll\",\"name\":\"{}\",\"ordinal\":{},\"target\":\"{}.{}\",{}}}",
            export.name, export.ordinal, export.target.dll.trim_end_matches(".dll"), export.target.name,
            fields(export.target.dll, export.target.name)));
    }
    for export in msvcrt_extra_forwarders(x64) {
        text.push_str(&format!(",{{\"dll\":\"msvcrt.dll\",\"name\":\"{}\",\"ordinal\":{},\"target\":\"{}.{}\",{}}}",
            export.name, export.ordinal, export.target.dll.trim_end_matches(".dll"), export.target.name,
            fields(export.target.dll, export.target.name)));
    }
    text.push_str("],\"contracts\":[");
    let contracts: Vec<_> = sp1_reference::MODULES.iter()
        .filter(|m| m.x64 == x64 && !RUNTIME_DLLS.contains(&m.name)).map(|m| m.name).collect();
    for (i, contract) in contracts.iter().enumerate() {
        if i != 0 {
            text.push(',');
        }
        if let Some(module) = reference_module(contract, x64) {
            let exports = foundation_exports(contract, x64);
            text.push_str(&format!("{{\"dll\":\"{contract}\",\"baseline\":\"Windows 7 SP1 reference\",\"delivered\":{},\"original_export_count\":{},\"schema_default_host\":{},\"forwarders\":[",
                !exports.is_empty(), module.exports.len(), module.host.map(quoted).unwrap_or_else(|| "null".into())));
            for (j, export) in exports.iter().enumerate() {
                if j != 0 { text.push(','); }
                text.push_str(&format!("{{\"name\":\"{}\",\"ordinal\":{},\"target\":\"{}.{}\",{}}}",
                    export.name, export.ordinal, export.target.dll.trim_end_matches(".dll"), export.target.name,
                    fields(export.target.dll, export.target.name)));
            }
            text.push_str("],\"unavailable_exports\":[");
            let mut separator = "";
            for export in module.exports.iter().filter(|e| !exports.iter().any(|a| a.name == e.name)) {
                text.push_str(&format!("{separator}{{\"name\":\"{}\",\"ordinal\":{},\"reason\":\"No checked runtime ABI target\"}}", export.name, export.ordinal));
                separator = ",";
            }
            text.push_str("]}");
            continue;
        }

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
                let real_native_ntdll = machine != MACHINE_AMD64 && spec.file_name == "ntdll.dll";
                let real_native_kernel32 = machine != MACHINE_AMD64 && spec.file_name == "kernel32.dll";
                let exports = if real_native_kernel32 { NATIVE_KERNEL32_EXPORTS } else { spec.exports };
                if real_native_kernel32 {
                    assert!(image.imports.is_empty(), "{arch}/{}", spec.file_name);
                }
                let count = exports.iter().filter(|(name, _)| !real_native_ntdll || name.starts_with("Nt")).count();
                assert_eq!(image.exports_by_name.len(), count);
                if (real_native_ntdll || real_native_kernel32) && machine == 0x5064 {
                    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
                    let base = u64::from_le_bytes(bytes[pe + 0x30..pe + 0x38].try_into().unwrap());
                    assert!((0x1000..1 << 38).contains(&base));
                }
                let text = image.sections.iter().find(|s| s.name == ".text").unwrap();
                for &(name, ordinal) in exports {
                    if real_native_ntdll && !name.starts_with("Nt") {
                        assert!(!image.exports_by_name.contains_key(name));
                        assert!(!image.exports_by_ordinal.contains_key(&(ordinal as u16)));
                        continue;
                    }
                    let rva = image.exports_by_name[name];
                    assert_eq!(image.exports_by_ordinal[&(ordinal as u16)], rva);
                    let offset = text.pointer_to_raw_data as usize + (rva - text.virtual_address) as usize;
                    if real_native_ntdll || real_native_kernel32 {
                        assert_ne!(&bytes[offset..offset + code.len()], code, "{arch}/{name}");
                        continue;
                    }
                    assert_eq!(
                        &bytes[offset..offset + code.len()],
                        code,
                        "{arch}/{} export {name}",
                        spec.file_name
                    );
                }
            }
        }
        assert!(build_dll_image_for_arch(&ALL_DLLS[0], "=riscv64").is_err());
    }

    #[test]
    fn native_create_file_exports_bridge_all_eleven_arguments() {
        let cases: &[(&str, &[u32])] = &[
            ("aarch64", &[0x5280_0a50, 0xd400_0001, 0xd65f_03c0]),
            ("riscv64", &[0xfe01_0113, 0x0111_3023, 0x0201_3283, 0x0051_3423, 0x0281_3283, 0x0051_3823,
                0x0301_3283, 0x0051_3c23, 0x0520_0893, 0x0000_0073, 0x0005_051b, 0x0201_0113, 0x0000_8067]),
            ("loongarch64", &[0x02ff_8063, 0x29c0_006b, 0x28c0_806c, 0x29c0_206c, 0x28c0_a06c, 0x29c0_406c,
                0x28c0_c06c, 0x29c0_606c, 0x0281_480b, 0x002b_0000, 0x0010_0084, 0x02c0_8063, 0x4c00_0020]),
        ];
        let spec = ALL_DLLS.iter().find(|spec| spec.file_name == "ntdll.dll").unwrap();
        for &(arch, expected) in cases {
            let bytes = build_dll_image_for_arch(spec, arch).unwrap();
            let parsed = super::super::pe_read::parse(&bytes).unwrap();
            let text = parsed.sections.iter().find(|section| section.name == ".text").unwrap();
            let rva = parsed.exports_by_name["NtCreateFile"];
            let offset = (text.pointer_to_raw_data + rva - text.virtual_address) as usize;
            let expected: Vec<u8> = expected.iter().copied().flat_map(u32::to_le_bytes).collect();
            assert_eq!(&bytes[offset..offset + expected.len()], expected, "{arch}");
        }
    }

    #[test]
    fn profile_api_set_keeps_win7_performance_exports() {
        for x64 in [false, true] {
            let exports = foundation_exports("api-ms-win-core-profile-l1-1-0.dll", x64);
            assert!(exports.iter().any(|e| e.name == "QueryPerformanceFrequency"), "x64={x64}");
            let image = build_runtime_dll("api-ms-win-core-profile-l1-1-0.dll", x64).expect("profile facade");
            let parsed = super::super::pe_read::parse(&image).expect("valid profile PE");
            assert!(parsed.exports_by_name.contains_key("QueryPerformanceFrequency"));
        }
    }

    #[test]
    fn ole_exports_keep_sp1_ordinals() {
        for (x64, initialize, uninitialize, iid) in [(false, 308, 331, 208), (true, 360, 383, 260)] {
            let bytes = build_runtime_dll("ole32.dll", x64).unwrap();
            let library = super::super::pe_read::parse(&bytes).unwrap();
            for (name, ordinal) in [("OleInitialize", initialize), ("OleUninitialize", uninitialize), ("IIDFromString", iid)] {
                assert_eq!(library.exports_by_ordinal[&ordinal], library.exports_by_name[name]);
            }
        }
    }

    #[test]
    fn disk_io_and_wait_object_exports_keep_sp1_ordinals() {
        for (x64, ordinals) in [
            (false, [68, 568, 157, 160, 1018, 173, 176, 1022]),
            (true, [67, 574, 156, 159, 1022, 172, 175, 1026]),
        ] {
            let bytes = build_runtime_dll("kernel32.dll", x64).unwrap();
            let library = super::super::pe_read::parse(&bytes).unwrap();
            let code = library.sections.iter().find(|section| section.name == ".text").unwrap();
            for (name, ordinal) in ["CancelIo", "GetOverlappedResult", "CreateMutexA", "CreateMutexW", "ReleaseMutex",
                "CreateSemaphoreA", "CreateSemaphoreW", "ReleaseSemaphore"].into_iter().zip(ordinals) {
                let rva = library.exports_by_name[name];
                assert_eq!(library.exports_by_ordinal[&ordinal], rva);
                let offset = (code.pointer_to_raw_data + (rva - code.virtual_address)) as usize;
                let api = catalog::find("kernel32.dll", name).unwrap();
                assert_eq!(bytes[offset], 0xb8);
                assert_eq!(&bytes[offset + 1..offset + 5], &(catalog::GATEWAY_BASE | api.id).to_le_bytes());
            }
        }
    }

    #[test]
    fn winsock_legacy_ordinals_resolve_to_checked_runtime_code() {
        for x64 in [false, true] {
            let libraries: Vec<_> = runtime_library_names_for_arch(x64).collect();
            assert!(libraries.contains(&"wsock32.dll"));
            assert!(libraries.contains(&"ws2_32.dll"));
            let legacy_bytes = build_runtime_dll("wsock32.dll", x64).unwrap();
            let runtime_bytes = build_runtime_dll("ws2_32.dll", x64).unwrap();
            let legacy = super::super::pe_read::parse(&legacy_bytes).unwrap();
            let runtime = super::super::pe_read::parse(&runtime_bytes).unwrap();
            let original = reference_module("ws2_32.dll", x64).unwrap();
            let text = runtime.sections.iter().find(|section| section.name == ".text").unwrap();
            for (name, legacy_ordinal) in [
                ("accept", 1), ("bind", 2), ("connect", 4), ("getpeername", 5),
                ("getsockname", 6), ("getsockopt", 7), ("ioctlsocket", 12), ("listen", 13),
                ("recv", 16), ("recvfrom", 17), ("select", 18), ("send", 19),
                ("sendto", 20), ("setsockopt", 21), ("shutdown", 22),
                ("gethostbyaddr", 51), ("gethostbyname", 52), ("getprotobyname", 53),
                ("getprotobynumber", 54), ("gethostname", 57),
                ("closesocket", 3), ("socket", 23),
                ("htonl", 8), ("htons", 9), ("ntohl", 14), ("ntohs", 15),
                ("WSAGetLastError", 111), ("WSASetLastError", 112), ("WSAStartup", 115),
                ("WSACleanup", 116), ("__WSAFDIsSet", 151),
            ] {
                let export = original.exports.iter().find(|entry| entry.name == name).unwrap();
                let runtime_rva = runtime.exports_by_name[name];
                assert_eq!(runtime.exports_by_ordinal[&export.ordinal], runtime_rva);
                assert!(!runtime.forwarders_by_rva.contains_key(&runtime_rva));
                let section_offset = runtime_rva.checked_sub(text.virtual_address).unwrap();
                assert!(section_offset + 5 <= text.size_of_raw_data);
                let file_offset = (text.pointer_to_raw_data + section_offset) as usize;
                let service = catalog::GATEWAY_BASE | catalog::find("ws2_32.dll", name).unwrap().id;
                assert_eq!(runtime_bytes[file_offset], 0xb8);
                assert_eq!(&runtime_bytes[file_offset + 1..file_offset + 5], &service.to_le_bytes());
                assert_eq!(legacy.exports_by_ordinal[&legacy_ordinal], legacy.exports_by_name[name]);
                if let Some(forwarder) = reference_module("wsock32.dll", x64).unwrap().exports.iter()
                    .find(|entry| entry.name == name).unwrap().forwarder
                {
                    assert_eq!(legacy.forwarders[name], forwarder);
                } else {
                    let section = legacy.sections.iter().find(|section| section.name == ".text").unwrap();
                    let rva = legacy.exports_by_name[name];
                    assert!(!legacy.forwarders_by_rva.contains_key(&rva));
                    let offset = (section.pointer_to_raw_data + (rva - section.virtual_address)) as usize;
                    assert_eq!(&legacy_bytes[offset..offset + 5], &runtime_bytes[file_offset..file_offset + 5]);
                }
            }
            for name in ["WSAIoctl", "WSASocketW", "WSCEnumProtocols", "WSCGetProviderInfo",
                "WSCGetProviderPath", "freeaddrinfo", "getaddrinfo", "getnameinfo", "inet_ntop",
                "inet_pton", "inet_addr", "inet_ntoa"] {
                let export = original.exports.iter().find(|entry| entry.name == name).unwrap();
                assert_eq!(runtime.exports_by_ordinal.get(&export.ordinal), runtime.exports_by_name.get(name));
                assert!(runtime.exports_by_name.contains_key(name), "missing {name}, x64={x64}");
            }
        }
    }
}

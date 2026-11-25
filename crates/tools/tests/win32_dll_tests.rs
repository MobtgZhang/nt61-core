//! Legacy PE scaffold and guest runtime DLL regression.
//!
//! Scaffold tests validate PE/loader mechanics, not executable Win32 behavior.
//! Runtime tests below separately validate catalog-generated guest thunks.
//! This test verifies that each emitted scaffold image
//!
//!   * is a PE32+ DLL with the subsystem/version fields captured from SP1;
//!   * exports exactly the symbols the specification lists, with
//!     original SP1 AMD64 ordinals;
//!   * imports exactly the modules and symbols the specification
//!     lists;
//!   * does not fabricate relocations for position-independent code.
//!
//! A mistake here is not cosmetic: the loader resolves imports
//! through the import directory, and a wrong ordinal makes
//! `GetProcAddress` return the wrong function.

use std::collections::BTreeSet;

use nt61_tools::fs::pe;
use nt61_tools::fs::pe_read;
use nt61_tools::fs::win32_dlls;

/// The image that must be produced for every DLL.
fn build(name: &str) -> Vec<u8> {
    let spec = win32_dlls::ALL_DLLS
        .iter()
        .find(|spec| spec.file_name == name)
        .unwrap_or_else(|| panic!("{name} is not in the DLL specification"));
    win32_dlls::build_dll_image(spec)
}

fn assert_reference_header(name: &str, x64: bool, bytes: &[u8]) {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/desktop/baselines/win7-sp1-window-placement/pe-headers.json"
    )).expect("captured original headers");
    let expected = &reference["modules"][if x64 { "x64" } else { "x86" }][name];
    assert!(expected.is_object(), "missing original header evidence: {name}");
    let pe_offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    let opt = pe_offset + 24;
    let read = |offset| u16::from_le_bytes(bytes[opt + offset..opt + offset + 2].try_into().unwrap());
    assert_eq!(read(68) as u64, expected["subsystem"].as_u64().unwrap(), "{name}: subsystem");
    for (field, offset) in [("os_version", 40), ("subsystem_version", 48)] {
        for i in 0..2 {
            assert_eq!(read(offset + i * 2) as u64, expected[field][i].as_u64().unwrap(), "{name}: {field}[{i}]");
        }
    }
}

#[test]
fn guest_facades_require_an_original_sp1_module_for_the_requested_bitness() {
    for x64 in [false, true] {
        let names: BTreeSet<_> = win32_dlls::runtime_library_names_for_arch(x64).collect();
        for name in &names {
            assert!(win32_dlls::sp1_reference::HEADERS.iter()
                .any(|h| h.x64 == x64 && h.name.eq_ignore_ascii_case(name)),
                "runtime DLL has no original SP1 identity: {name}");
        }
        for absent in ["api-ms-win-core-file-l1-2-0.dll", "api-ms-win-core-file-l2-1-0.dll",
            "api-ms-win-core-localization-l1-2-0.dll", "api-ms-win-core-processthreads-l1-1-1.dll",
            "api-ms-win-core-synch-l1-2-0.dll", "api-ms-win-core-timezone-l1-1-0.dll",
            "api-ms-win-unknown-l1-1-0.dll"] {
            assert!(!names.contains(absent));
            assert!(win32_dlls::build_runtime_dll(absent, x64).is_none(), "{absent}");
            assert!(!win32_dlls::runtime_manifest(x64).contains(absent));
        }
        assert!(names.contains("api-ms-win-core-localization-l1-1-0.dll"));
    }
}

#[test]
fn guest_runtime_headers_match_original_per_bitness() {
    for x64 in [false, true] {
        for name in win32_dlls::runtime_library_names_for_arch(x64) {
            let bytes = win32_dlls::build_runtime_dll(name, x64).expect("runtime image");
            assert_reference_header(name, x64, &bytes);
        }
    }
}

#[test]
fn every_dll_preserves_original_sp1_subsystem_and_versions() {
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        let image = pe_read::parse(&bytes)
            .unwrap_or_else(|| panic!("{} must be a parsable PE", spec.file_name));

        assert_eq!(image.magic, 0x20B, "{}: PE32+ magic", spec.file_name);
        assert_eq!(image.machine, win32_dlls::MACHINE_AMD64);
        assert_ne!(
            image.characteristics & pe::IMAGE_FILE_DLL,
            0,
            "{}: must be marked as a DLL",
            spec.file_name
        );
        assert_reference_header(spec.file_name, true, &bytes);
        assert_ne!(
            image.subsystem,
            pe::IMAGE_SUBSYSTEM_EFI_APPLICATION,
            "{}: the EFI-application subsystem must never appear in a Win32 DLL",
            spec.file_name
        );
        assert_eq!(
            image.image_base, spec.image_base,
            "{}: image base",
            spec.file_name
        );
    }
}

#[test]
fn every_dll_exports_exactly_the_specified_symbols() {
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        let image = pe_read::parse(&bytes).expect("parse");

        let expected: BTreeSet<&str> = spec.exports.iter().map(|(name, _)| *name).collect();
        let actual: BTreeSet<&str> = image.exports_by_name.keys().map(String::as_str).collect();

        assert_eq!(
            actual, expected,
            "{}: the exported names do not match the specification",
            spec.file_name
        );
    }
}

#[test]
fn every_scaffold_export_preserves_original_sp1_ordinal() {
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        let image = pe_read::parse(&bytes).expect("parse");

        assert_eq!(
            image.exports_by_ordinal.values().filter(|rva| **rva != 0).count(),
            spec.exports.len(),
            "{}: the number of ordinal slots",
            spec.file_name
        );

        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/desktop/baselines/win7-sp1-window-placement/pe-scaffold-exports.json")).unwrap();
        let selected = &reference["modules"][spec.file_name]["selected_exports"];
        assert_eq!(selected.as_object().unwrap().len(), spec.exports.len());
        for (index, (name, specified)) in spec.exports.iter().enumerate() {
            let ordinal = selected[*name].as_u64().expect("original name") as u16;
            assert_eq!(*specified, u32::from(ordinal));
            let rva = image
                .exports_by_ordinal
                .get(&ordinal)
                .unwrap_or_else(|| panic!("{}: ordinal {ordinal} is missing", spec.file_name));
            assert_eq!(
                *rva,
                win32_dlls::stub_rva(index as u32),
                "{}: ordinal {ordinal} must point at its stub",
                spec.file_name
            );
            assert_eq!(
                image.exports_by_name.get(*name),
                Some(rva),
                "{}: {name} must map to ordinal {ordinal}",
                spec.file_name
            );
        }
    }
}

#[test]
fn every_dll_imports_exactly_the_specified_modules_and_symbols() {
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        let image = pe_read::parse(&bytes).expect("parse");

        assert_eq!(
            image.imports.len(),
            spec.imports.len(),
            "{}: the number of imported modules",
            spec.file_name
        );

        for (module_index, (dll_name, symbols)) in spec.imports.iter().enumerate() {
            let import = &image.imports[module_index];
            assert_eq!(
                &import.dll_name, dll_name,
                "{}: imported module name",
                spec.file_name
            );
            assert_eq!(
                import.symbols.len(),
                symbols.len(),
                "{}: the number of imported symbols from {dll_name}",
                spec.file_name
            );
            for (symbol_index, symbol) in symbols.iter().enumerate() {
                match symbol {
                    pe::ImportSymbol::ByName(expected) => assert_eq!(
                        import.symbols[symbol_index].as_deref(),
                        Some(*expected),
                        "{}: imported symbol {expected}",
                        spec.file_name
                    ),
                    pe::ImportSymbol::ByOrdinal(expected) => assert_eq!(
                        import.ordinals[symbol_index], *expected,
                        "{}: imported ordinal",
                        spec.file_name
                    ),
                }
            }
        }
    }
}

#[test]
fn position_independent_dll_code_has_no_fabricated_relocations() {
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        let image = pe_read::parse(&bytes).expect("parse");
        assert_eq!(image.data_directories[5], (0, 0));
        assert!(image.relocations.is_empty());
        assert_eq!(image.entry_point, 0, "an export is not DllMain");
    }
}

#[test]
fn the_user32_dll_exports_the_documented_win7_surface() {
    // The plan lists the first batch of `user32.dll` exports. This
    // test pins the ones that must be present for the Ring 3 stack to
    // be usable.
    let bytes = build("user32.dll");
    let image = pe_read::parse(&bytes).expect("parse");

    for name in [
        "RegisterClassExW",
        "CreateWindowExW",
        "DestroyWindow",
        "ShowWindow",
        "SetWindowPos",
        "GetMessageW",
        "PeekMessageW",
        "TranslateMessage",
        "DispatchMessageW",
        "SendMessageW",
        "PostMessageW",
        "DefWindowProcW",
        "GetDC",
        "ReleaseDC",
        "BeginPaint",
        "EndPaint",
        "InvalidateRect",
        "GetWindowTextW",
        "SetWindowTextW",
        "GetSystemMetrics",
        "GetCursorPos",
        "SetCapture",
        "MessageBoxW",
        "OpenWindowStationW",
        "CreateDesktopW",
        "SwitchDesktop",
    ] {
        assert!(
            image.exports_by_name.contains_key(name),
            "user32.dll must export {name}"
        );
    }
}

#[test]
fn the_gdi32_dll_exports_the_documented_win7_surface() {
    let bytes = build("gdi32.dll");
    let image = pe_read::parse(&bytes).expect("parse");

    for name in [
        "CreateCompatibleDC",
        "CreateDIBSection",
        "SelectObject",
        "DeleteObject",
        "BitBlt",
        "StretchBlt",
        "PatBlt",
        "GdiAlphaBlend",
        "MoveToEx",
        "LineTo",
        "Rectangle",
        "RoundRect",
        "Ellipse",
        "Polygon",
        "TextOutW",
        "ExtTextOutW",
        "GetTextExtentPoint32W",
        "GetTextMetricsW",
        "CreateSolidBrush",
        "CreatePen",
        "CreateFontIndirectW",
        "SetTextColor",
        "SetBkMode",
        "CreateRectRgn",
        "IntersectClipRect",
        "GdiFlush",
        "GetStockObject",
    ] {
        assert!(
            image.exports_by_name.contains_key(name),
            "gdi32.dll must export {name}"
        );
    }
}

#[test]
fn the_uxtheme_dwmapi_shell32_ole32_and_comctl32_surfaces_are_present() {
    let expectations: &[(&str, &[&str])] = &[
        (
            "uxtheme.dll",
            &[
                "IsThemeActive",
                "OpenThemeData",
                "CloseThemeData",
                "DrawThemeBackground",
                "DrawThemeText",
                "GetThemeColor",
                "GetThemePartSize",
                "GetThemeMargins",
                "SetWindowTheme",
                "GetThemeMetric",
                "EnableTheming",
                "GetCurrentThemeName",
            ],
        ),
        (
            "dwmapi.dll",
            &[
                "DwmIsCompositionEnabled",
                "DwmEnableComposition",
                "DwmExtendFrameIntoClientArea",
                "DwmSetWindowAttribute",
                "DwmGetWindowAttribute",
                "DwmFlush",
                "DwmRegisterThumbnail",
                "DwmUpdateThumbnailProperties",
                "DwmUnregisterThumbnail",
                "DwmSetIconicThumbnail",
                "DwmSetIconicLivePreviewBitmap",
            ],
        ),
        (
            "shell32.dll",
            &[
                "SHGetKnownFolderPath",
                "SHGetFolderPathW",
                "SHGetFileInfoW",
                "ShellExecuteExW",
                "SHParseDisplayName",
                "SHChangeNotify",
                "Shell_NotifyIconW",
                "ExtractIconExW",
            ],
        ),
        (
            "ole32.dll",
            &[
                "CoInitializeEx",
                "CoUninitialize",
                "CoCreateInstance",
                "CoTaskMemAlloc",
                "CoTaskMemFree",
                "CLSIDFromString",
                "StringFromCLSID",
            ],
        ),
        (
            "oleaut32.dll",
            &[
                "SysAllocString",
                "SysFreeString",
                "VariantInit",
                "VariantClear",
                "SafeArrayCreate",
            ],
        ),
        (
            "comctl32.dll",
            &[
                "InitCommonControlsEx",
                "ImageList_Create",
                "ImageList_Add",
                "ImageList_AddIcon",
                "ImageList_Destroy",
                "ImageList_GetIcon",
                "ImageList_SetIconSize",
                "CreatePropertySheetPageW",
                "DestroyPropertySheetPage",
                "PropertySheetW",
            ],
        ),
    ];

    for (dll, names) in expectations {
        let bytes = build(dll);
        let image = pe_read::parse(&bytes).expect("parse");
        for name in *names {
            assert!(
                image.exports_by_name.contains_key(*name),
                "{dll} must export {name}"
            );
        }
    }
}

#[test]
fn sp1_surfaces_do_not_invent_named_win32k_exports_or_ntdll_imports() {
    for dll in ["ntdll.dll", "user32.dll", "gdi32.dll"] {
        let bytes = build(dll);
        let image = pe_read::parse(&bytes).expect("parse");
        for name in image.exports_by_name.keys() {
            assert!(
                !name.starts_with("NtUser") && !name.starts_with("NtGdi"),
                "{dll}: SP1 has no named export {name}"
            );
        }
    }
    for imports in [win32_dlls::USER32_IMPORTS, win32_dlls::GDI32_IMPORTS] {
        for (module, symbols) in imports {
            for symbol in *symbols {
                if let pe::ImportSymbol::ByName(name) = symbol {
                    assert!(!name.starts_with("NtUser") && !name.starts_with("NtGdi"),
                        "{module}: fabricated win32k import {name}");
                }
            }
        }
    }
}

#[test]
fn the_loader_accepts_every_emitted_dll() {
    let mut db = nt61_loader::loader::ImageDatabase::new();
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        // `ntdll.dll` imports nothing, so it must load. Every other
        // DLL imports from `ntdll.dll`, which is not in the database
        // yet, so the loader must reject them rather than install a
        // bogus IAT.
        let loaded =
            nt61_loader::loader::load_image_full(spec.file_name, &bytes, &mut db, spec.image_base);
        if spec.imports.is_empty() {
            assert!(
                loaded.is_some(),
                "{}: an image without imports must load",
                spec.file_name
            );
        } else {
            assert!(
                loaded.is_none(),
                "{}: an image with unresolvable imports must be rejected",
                spec.file_name
            );
        }
    }
}

#[test]
fn the_loader_accepts_a_dll_once_its_imports_are_resolvable() {
    // Load `ntdll.dll` first (it imports nothing), register it, then
    // load `user32.dll`, whose imports are all satisfied by ntdll.
    let mut db = nt61_loader::loader::ImageDatabase::new();

    let ntdll = build("ntdll.dll");
    let ntdll_loaded =
        nt61_loader::loader::load_image_full("ntdll.dll", &ntdll, &mut db, win32_dlls::NTDLL_BASE)
            .expect("ntdll.dll must load");
    db.register(&ntdll_loaded, &ntdll);

    let user32 = build("user32.dll");
    let user32_loaded = nt61_loader::loader::load_image_full(
        "user32.dll",
        &user32,
        &mut db,
        win32_dlls::USER32_BASE,
    )
    .expect("user32.dll must load once ntdll.dll provides its imports");
    db.register(&user32_loaded, &user32);

    // The IAT of `user32.dll` must now hold the *resolved* addresses
    // of the ntdll stubs, not the hint/name RVAs.
    let address = db
        .lookup("ntdll.dll", "RtlAllocateHeap")
        .expect("ntdll must export the native heap helper");
    assert_eq!(
        address,
        win32_dlls::NTDLL_BASE + u64::from(win32_dlls::stub_rva(9))
    );
}

#[test]
fn guest_runtime_catalog_preserves_bitness_calling_convention_and_stable_services() {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for api in win32_dlls::catalog::APIS {
        assert!(ids.insert(api.id), "duplicate service ID {}", api.id);
        assert!(
            names.insert((api.dll, api.name)),
            "duplicate export {}!{}",
            api.dll,
            api.name
        );
        assert!(api.arguments <= 16 && api.x86_arguments <= 16);
    }
    for x64 in [false, true] {
        let user = pe_read::parse(&win32_dlls::build_runtime_dll("user32.dll", x64).unwrap()).unwrap();
        assert!(user.exports_by_name.contains_key("LoadStringW"));
        assert_eq!(user.exports_by_name["ChildWindowFromPoint"], user.exports_by_ordinal[&1576]);
        for (export, ordinal) in [
            ("GetSystemMenu", if x64 { 1892 } else { 1890 }),
            ("GetMenuState", if x64 { 1853 } else { 1851 }),
            ("EnableMenuItem", 1723),
            ("CheckMenuItem", 1572),
            ("LoadAcceleratorsW", if x64 { 1998 } else { 1994 }),
            ("LoadCursorW", if x64 { 2008 } else { 2003 }),
            ("LoadIconW", if x64 { 2010 } else { 2006 }),
            ("LoadImageW", if x64 { 2012 } else { 2008 }),
            ("SetCursor", if x64 { 2167 } else { 2161 }),
            ("TranslateAcceleratorW", if x64 { 2283 } else { 2275 }),
        ] {
            assert_eq!(user.exports_by_name[export], user.exports_by_ordinal[&ordinal]);
        }
        for name in win32_dlls::RUNTIME_DLLS {
            let bytes = win32_dlls::build_runtime_dll(name, x64).unwrap();
            let image = pe_read::parse(&bytes).unwrap();
            assert_eq!(image.magic, if x64 { 0x20b } else { 0x10b });
            assert_eq!(image.machine, if x64 { 0x8664 } else { 0x14c });
            assert_eq!(image.entry_point, 0);
            assert_eq!(image.data_directories[5], (0, 0));
            let section = image.sections.iter().find(|s| s.name == ".text").unwrap();
            for api in win32_dlls::catalog::APIS.iter().filter(|a| a.dll == *name) {
                if matches!((x64, api.name), (false, "__C_specific_handler") | (true, "_except_handler4_common")) {
                    assert!(!image.exports_by_name.contains_key(api.name));
                    continue;
                }
                if *name == "kernel32.dll" {
                    let ordinal = match api.name {
                        "InterlockedCompareExchange" => Some(748),
                        "InterlockedDecrement" => Some(749),
                        "InterlockedExchange" => Some(750),
                        "InterlockedExchangeAdd" => Some(751),
                        "InterlockedIncrement" => Some(753),
                        _ => None,
                    };
                    if let Some(ordinal) = ordinal {
                        if x64 {
                            assert!(!image.exports_by_name.contains_key(api.name));
                            continue;
                        }
                        assert_eq!(image.exports_by_name[api.name], image.exports_by_ordinal[&ordinal]);
                    }
                }
                let rva = *image.exports_by_name.get(api.name).unwrap();
                let offset = (rva - section.virtual_address + section.pointer_to_raw_data) as usize;
                let code = &bytes[offset..offset + 16];
                if api.name == "RtlCaptureContext" {
                    assert_eq!(code[0],0xe9);
                    let delta=i32::from_le_bytes(code[1..5].try_into().unwrap());
                    let target=(offset as i64+5+i64::from(delta)) as usize;
                    let prefix: &[u8]=if x64 {&[0x50,0x41,0x52,0x41,0x53,0xb8]} else {&[0x50,0x51,0x52,0xb8]};
                    assert_eq!(&bytes[target..target+prefix.len()],prefix);
                    assert_eq!(u32::from_le_bytes(bytes[target+prefix.len()..target+prefix.len()+4].try_into().unwrap()),
                        win32_dlls::catalog::GATEWAY_BASE | api.id);
                    continue;
                }
                assert_eq!(code[0], 0xb8);
                assert_eq!(
                    u32::from_le_bytes(code[1..5].try_into().unwrap()),
                    win32_dlls::catalog::GATEWAY_BASE | api.id
                );
                if x64 {
                    assert_eq!(&code[5..11], &[0x4c, 0x8b, 0xd1, 0x0f, 0x05, 0xc3]);
                } else {
                    assert_eq!(&code[5..11], &[0x8d, 0x54, 0x24, 0x04, 0xcd, 0x2e]);
                    if api.cdecl {
                        assert_eq!(code[11], 0xc3);
                    } else {
                        assert_eq!(code[11], 0xc2);
                        assert_eq!(
                            u16::from_le_bytes(code[12..14].try_into().unwrap()),
                            u16::from(api.x86_arguments) * 4
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn notepad_runtime_exports_preserve_reference_ordinals() {
    for x64 in [false, true] {
        for (library, exports) in [
            ("comctl32.dll", &[("TaskDialogIndirect", 345)][..]),
            ("version.dll", &[("GetFileVersionInfoExW", 3), ("GetFileVersionInfoSizeExW", 5), ("VerQueryValueW", 15)]),
            ("winspool.drv", &[("ClosePrinter", 146), ("GetPrinterDriverW", 283), ("OpenPrinterW", 293)]),
        ] {
            let bytes = win32_dlls::build_runtime_dll(library, x64).unwrap();
            let image = pe_read::parse(&bytes).unwrap();
            for &(name, ordinal) in exports {
                assert_eq!(image.exports_by_name[name], image.exports_by_ordinal[&ordinal]);
            }
        }
    }
}

#[test]
fn oleaut32_automation_exports_preserve_sp1_ordinals() {
    for x64 in [false, true] {
        let bytes = win32_dlls::build_runtime_dll("oleaut32.dll", x64).unwrap();
        let image = pe_read::parse(&bytes).unwrap();
        for (name, ordinal) in [("SysAllocStringLen", 4), ("SysStringLen", 7), ("VariantInit", 8), ("VariantClear", 9)] {
            assert_eq!(image.exports_by_name[name], image.exports_by_ordinal[&ordinal]);
        }
    }
}

#[test]
fn ntdll_exports_all_native_services_and_zw_aliases_with_audited_abi() {
    for x64 in [false, true] {
        let bytes = win32_dlls::build_runtime_dll("ntdll.dll", x64).unwrap();
        let image = pe_read::parse(&bytes).unwrap();
        let section = image.sections.iter().find(|s| s.name == ".text").unwrap();
        let mut ordinals = BTreeSet::new();
        for service in win32_dlls::syscall_metadata::NATIVE_SERVICE_METADATA {
            let zw_name = format!("Zw{}", &service.name[2..]);
            for (name, ordinal) in [
                (service.name, if x64 { service.x64_nt_ordinal } else { service.x86_nt_ordinal }),
                (zw_name.as_str(), if x64 { service.x64_zw_ordinal } else { service.x86_zw_ordinal }),
            ] {
                assert!(ordinals.insert(ordinal), "duplicate {name} ordinal");
                let rva = image.exports_by_name[name];
                assert_eq!(image.exports_by_ordinal[&ordinal], rva, "{name} ordinal");
                let offset = (rva - section.virtual_address + section.pointer_to_raw_data) as usize;
                let code = &bytes[offset..offset + 16];
                assert_eq!(code[0], 0xb8, "{name}");
                let expected = win32_dlls::catalog::find("ntdll.dll", service.name)
                    .map(|api| win32_dlls::catalog::GATEWAY_BASE | api.id)
                    .unwrap_or(u32::from(if x64 { service.number } else { service.x86_number }));
                assert_eq!(u32::from_le_bytes(code[1..5].try_into().unwrap()), expected, "{name}");
                if x64 {
                    assert_eq!(&code[5..11], &[0x4c, 0x8b, 0xd1, 0x0f, 0x05, 0xc3], "{name}");
                } else {
                    assert_eq!(&code[5..12], &[0x8d, 0x54, 0x24, 0x04, 0xcd, 0x2e, 0xc2], "{name}");
                    assert_eq!(u16::from_le_bytes(code[12..14].try_into().unwrap()), u16::from(service.arguments)*4, "{name}");
                }
            }
        }
        assert_eq!(ordinals.len(), 802);
    }
}

#[test]
fn common_dialog_and_control_runtime_dlls_export_both_guest_abis() {
    for x64 in [false, true] {
        let dialog = pe_read::parse(&win32_dlls::build_runtime_dll("comdlg32.dll", x64).unwrap()).unwrap();
        for name in ["CommDlgExtendedError", "GetOpenFileNameA", "GetOpenFileNameW", "GetSaveFileNameA", "GetSaveFileNameW", "ChooseFontA", "ChooseFontW", "ChooseColorA", "ChooseColorW", "PrintDlgA", "PrintDlgW", "PageSetupDlgA", "PageSetupDlgW", "GetFileTitleA", "GetFileTitleW"] {
            assert!(dialog.exports_by_name.contains_key(name), "missing {name}");
        }
        let controls = pe_read::parse(&win32_dlls::build_runtime_dll("comctl32.dll", x64).unwrap()).unwrap();
        assert_eq!(dialog.machine, controls.machine);
        assert_ne!(dialog.image_base, controls.image_base);
        for (name, _) in win32_dlls::COMCTL32_EXPORTS {
            assert!(controls.exports_by_name.contains_key(*name), "missing {name}");
        }
        assert_eq!(dialog.machine, if x64 { 0x8664 } else { 0x14c });
    }
}

#[test]
fn runtime_packaging_uses_each_requested_host_layout_in_one_process() {
    let temporary = tempfile::tempdir().unwrap();
    for arch in ["aarch64", "x86_64", "riscv64", "loongarch64"] {
        let root = temporary.path().join(arch);
        let native = root.join("Windows/System32");
        std::fs::create_dir_all(&native).unwrap();
        let marker = native.join("native-marker.txt");
        std::fs::write(&marker, b"preserve native files").unwrap();
        nt61_tools::fs::build::stage_guest_runtime(arch, &root).unwrap();
        assert_eq!(std::fs::read(marker).unwrap(), b"preserve native files");
        for (x64, directory) in [(false, root.join("Windows/SysWOW64")),
            (true, if arch == "x86_64" { native.clone() } else { root.join("Windows/SysX64") })] {
            for name in win32_dlls::runtime_library_names_for_arch(x64) {
                let bytes = std::fs::read(directory.join(name)).unwrap();
                let image = pe_read::parse(&bytes).unwrap();
                assert_eq!(image.machine, if x64 { 0x8664 } else { 0x14c });
                assert_eq!(image.magic, if x64 { 0x20b } else { 0x10b });
            }
        }
        if arch != "x86_64" { assert!(!native.join("ntdll.dll").exists()); }
    }
    let invalid = temporary.path().join("invalid");
    assert!(nt61_tools::fs::build::stage_guest_runtime("invalid", &invalid).is_err());
    assert!(!invalid.exists());
}

#[test]
fn runtime_packaging_rejects_stale_facades_before_overwriting_either_abi() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let x64 = root.join("Windows/System32");
    let x86 = root.join("Windows/SysWOW64");
    std::fs::create_dir_all(&x64).unwrap();
    std::fs::create_dir_all(&x86).unwrap();
    // Put the stale facade in x64, which is written after x86. Even x86's
    // existing runtime must survive this failed staging operation intact.
    let existing = x86.join("ntdll.dll");
    std::fs::write(&existing, b"preexisting runtime").unwrap();
    for name in ["api-ms-win-core-ums-l1-1-0.dll",
                 "api-ms-win-core-localization-l1-2-0.dll",
                 "API-MS-WIN-UNKNOWN-L1-1-0.DLL"] {
        let stale = x64.join(name);
        std::fs::write(&stale, b"preexisting facade").unwrap();
        let result = nt61_tools::fs::build::stage_guest_runtime("x86_64", root);
        assert!(result.is_err(), "{name}");
        assert_eq!(std::fs::read(&stale).unwrap(), b"preexisting facade");
        assert_eq!(std::fs::read(&existing).unwrap(), b"preexisting runtime");
        assert!(!x86.join("nt61-runtime-exports.json").exists());
        std::fs::remove_file(stale).unwrap();
    }
    nt61_tools::fs::build::stage_guest_runtime("x86_64", root).unwrap();
    assert!(x86.join("nt61-runtime-exports.json").exists());
}

#[test]
fn shell_runtime_preserves_resource_payloads_and_original_export_ordinals() {
    for x64 in [false, true] {
        // Three-level synthetic resource tree: type 3 / icon 1 / language 1033.
        let mut resource = vec![0u8; 92];
        for (header, id, child) in [(0usize,3u32,0x80000018u32),(24,1,0x80000030),(48,1033,72)] {
            resource[header+14..header+16].copy_from_slice(&1u16.to_le_bytes());
            resource[header+16..header+20].copy_from_slice(&id.to_le_bytes());
            resource[header+20..header+24].copy_from_slice(&child.to_le_bytes());
        }
        // Put .rsrc at RVA 0x2000 to ensure composition actually relocates it.
        resource[72..76].copy_from_slice(&0x2058u32.to_le_bytes());
        resource[76..80].copy_from_slice(&4u32.to_le_bytes());
        resource[88..92].copy_from_slice(b"ICON");
        let mut spec = pe::PeImageSpec::new(0x400000, 2, true, if x64 {0x8664} else {0x14c});
        spec.entry_point_rva = 0;
        spec.sections = vec![pe::SectionSpec::code(vec![0xc3]), pe::SectionSpec { name:".rsrc".into(), characteristics:0x40000040, data:resource }];
        let mut source = pe::emit(&spec).0;
        let dd = 0x98 + if x64 {0x70} else {0x60};
        source[dd+16..dd+20].copy_from_slice(&0x2000u32.to_le_bytes());
        source[dd+20..dd+24].copy_from_slice(&92u32.to_le_bytes());
        let bytes = win32_dlls::build_runtime_dll_with_resources("shell32.dll",x64,&source).unwrap();
        let parsed = pe_read::parse(&bytes).unwrap();
        let entry = parsed.exports_by_name["SHGetKnownFolderPath"];
        assert_eq!(parsed.exports_by_ordinal[&382], entry);
        let section = parsed.sections.iter().find(|s|s.name==".rsrc").unwrap();
        assert_eq!(parsed.data_directories[2],(section.virtual_address,92));
        let offset = section.pointer_to_raw_data as usize;
        assert_eq!(&bytes[offset+88..offset+92], b"ICON");
        assert_eq!(u32::from_le_bytes(bytes[offset+72..offset+76].try_into().unwrap()), section.virtual_address+88);
        let ole = pe_read::parse(&win32_dlls::build_runtime_dll("ole32.dll",x64).unwrap()).unwrap();
        for (name,ordinal) in [("CoTaskMemAlloc",if x64 {117}else{113}),("CoTaskMemFree",if x64 {118}else{114}),("CoTaskMemRealloc",if x64 {119}else{115})] {
            assert_eq!(ole.exports_by_name[name],ole.exports_by_ordinal[&ordinal]);
        }
    }
}

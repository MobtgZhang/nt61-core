//! The Win32 DLL surface regression.
//!
//! The DLLs that ship in `C:\Windows\System32` are the Ring 3
//! boundary of NT61. This test verifies that each emitted image
//!
//!   * is a PE32+ DLL with the **GUI subsystem** (2) — not the
//!     EFI-application subsystem (0x0A) the driver images use;
//!   * exports exactly the symbols the specification lists, with
//!     1-based ordinals;
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

#[test]
fn every_dll_is_a_gui_subsystem_pe32_plus_dll() {
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
        assert_eq!(
            image.subsystem,
            pe::IMAGE_SUBSYSTEM_WINDOWS_GUI,
            "{}: the subsystem must be 2 (Windows GUI), not {}",
            spec.file_name,
            image.subsystem
        );
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
fn every_dll_uses_one_based_ordinals_in_specification_order() {
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        let image = pe_read::parse(&bytes).expect("parse");

        assert_eq!(
            image.exports_by_ordinal.len(),
            spec.exports.len(),
            "{}: the number of ordinal slots",
            spec.file_name
        );

        for (index, (name, _)) in spec.exports.iter().enumerate() {
            let ordinal = index as u16 + 1;
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
        "AlphaBlend",
        "FillRect",
        "DrawEdge",
        "MoveToEx",
        "LineTo",
        "Rectangle",
        "RoundRect",
        "Ellipse",
        "Polygon",
        "TextOutW",
        "ExtTextOutW",
        "DrawTextW",
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
                "GetThemeMetrics",
                "EnableTheme",
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
fn the_ntdll_dll_exports_the_win32k_syscall_stubs() {
    let bytes = build("ntdll.dll");
    let image = pe_read::parse(&bytes).expect("parse");

    // The `NtUser*`/`NtGdi*` stubs are the Ring 3 → kernel boundary.
    let user_stubs = image
        .exports_by_name
        .keys()
        .filter(|name| name.starts_with("NtUser"))
        .count();
    let gdi_stubs = image
        .exports_by_name
        .keys()
        .filter(|name| name.starts_with("NtGdi"))
        .count();

    assert!(user_stubs >= 20, "ntdll.dll must expose the NtUser* stubs");
    assert!(gdi_stubs >= 10, "ntdll.dll must expose the NtGdi* stubs");
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
        .lookup("ntdll.dll", "NtUserCreateWindowEx")
        .expect("ntdll must export NtUserCreateWindowEx");
    assert_eq!(
        address,
        win32_dlls::NTDLL_BASE + u64::from(win32_dlls::stub_rva(15))
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
        for name in win32_dlls::RUNTIME_DLLS {
            let bytes = win32_dlls::build_runtime_dll(name, x64).unwrap();
            let image = pe_read::parse(&bytes).unwrap();
            assert_eq!(image.magic, if x64 { 0x20b } else { 0x10b });
            assert_eq!(image.machine, if x64 { 0x8664 } else { 0x14c });
            assert_eq!(image.entry_point, 0);
            assert_eq!(image.data_directories[5], (0, 0));
            let section = image.sections.iter().find(|s| s.name == ".text").unwrap();
            for api in win32_dlls::catalog::APIS.iter().filter(|a| a.dll == *name) {
                let rva = *image.exports_by_name.get(api.name).unwrap();
                let offset = (rva - section.virtual_address + section.pointer_to_raw_data) as usize;
                let code = &bytes[offset..offset + 16];
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

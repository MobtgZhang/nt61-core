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
//!   * carries a base relocation table, so it can be rebased.
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
fn every_dll_has_a_base_relocation_table() {
    for spec in win32_dlls::ALL_DLLS {
        let bytes = win32_dlls::build_dll_image(spec);
        let image = pe_read::parse(&bytes).expect("parse");

        let (rva, size) = image.data_directories[5];
        assert_ne!(
            rva, 0,
            "{}: the relocation directory must be present",
            spec.file_name
        );
        assert!(size > 0);
        assert_eq!(
            image.relocations.len(),
            spec.exports.len(),
            "{}: one relocation per stub",
            spec.file_name
        );
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

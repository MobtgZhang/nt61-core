# nt61-desktop

This crate owns the NT 6.1 desktop shell, Windows 7 theme metadata, the
Session 1 Explorer process, and the build-time resource generator.

## Layout

- `src/arch.rs` — desktop capability for `x86_64`, `aarch64`, `riscv64`, and
  `loongarch64`.
- `src/fs.rs` — boot/desktop compatibility for `ramfs`, `ext4`, `ntfs`,
  `refs`, `fat32`, `iso`, and `qcow2`.
- `src/resources.rs` — logical icon IDs used by the PE resource directory.
- `src/shell.rs` — desktop icons, selection, taskbar, and window state.
- `src/theme.rs` — Win7 Basic/Aero/Classic/High-Contrast definitions.
- `src/graphics` — bounded `Surface`, clipping, alpha blitting, and real font
  rasterization; text is never rendered by pre-baked label images.
- `src/compositor` — redirected surfaces, dirty damage tracking, and real Aero
  glass by blurring, saturating, and tinting the live backdrop.
- `src/controls` — renderer-independent control tree, focus, and pointer/keyboard
  event routing.
- `src/window` — Win32-shaped window/session manager plus the versioned
  multi-process window-service protocol with process ownership isolation.
- `src/win32.rs` — conservative user32/gdi32/win32k capability matrix mapped to
  the existing desktop owners.
- `src/widgets` — Button, Label, TextBox, CheckBox, Menu, ListView, ScrollBar,
  Panel, and Dialog state models.
- `src/bin/logonui` — keyboard- and mouse-driven LogonUI built on the control
  tree and font library.
- `src/bin/explorer` — Session 1 user-shell binary.
- `src/bin/notepad` — an independent Ring 3 Notepad application, not an internal
  Explorer-only rendering path.
- `src/resource_generator.rs` — generates `imageres.dll`, `shell32.dll`, and
  `aero.msstyles` at build time. These are project-local resource PEs, not
  Microsoft binaries.
- `src/bin/explorer` — the Session 1 user-shell binary. It is launched after
  SMSS/CSRSS/WinInit/Winlogon/Userinit and never directly from UEFI.

The window-control icon PNGs in `assets/7css/gui/icon` are derived from
[7.css](https://github.com/khang-nd/7.css) and are covered by the bundled MIT
license in `assets/7css/LICENSE`. The Explorer window chrome, Aero glass
gradients, title-bar controls, menus, and taskbar styling also follow the 7.css
visual specification. The default desktop wallpaper is a CC BY 4.0 asset from
[Project 7](https://github.com/maxik34/Project_7); see
`assets/desktop-wallpaper.license.txt` and `assets/desktop-wallpaper.license.md`.
Desktop icon payloads are packaged into resource PEs; LogonUI and Explorer paint
the Win7 background, panel, wallpaper, glass Superbar, and orb as procedural
gradients/rectangles and bitmap assets, not as pre-baked shell bitmaps.

There is no nested `crates/desktop/crates` layout. All desktop code remains in
this single crate; future extraction into `ui-core`, `ui-render`, and
`window-server` is intentionally deferred until their APIs are stable.

## Build and run

```sh
cargo test -p nt61-desktop --features resource-gen
make build
make run
```

Resource files are emitted only under
`build/<arch>/<format>/system/Windows/System32/resources/desktop`; no generated
`.dll` or `.msstyles` file is committed to the source tree.

The desktop boot path is:

`UEFI -> bootmgr -> winload.efi -> ntoskrnl -> smss.exe -> Session 0
(csrss.exe, wininit.exe, services.exe, lsass.exe, lsm.exe) -> Session 1
(csrss.exe, winlogon.exe, LogonUI, userinit.exe) -> explorer.exe`

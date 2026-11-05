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
- `src/resource_generator.rs` — generates `imageres.dll`, `shell32.dll`, and
  `aero.msstyles` at build time. These are project-local resource PEs, not
  Microsoft binaries.
- `src/bin/explorer` — the Session 1 user-shell binary. It is launched after
  SMSS/CSRSS/WinInit/Winlogon/Userinit and never directly from UEFI.

The window-control icon PNGs in `assets/7css/gui/icon` are derived from
[7.css](https://github.com/khang-nd/7.css) and are covered by the bundled MIT
license in `assets/7css/LICENSE`. Desktop icon payloads are generated as real
PNG resources with Win7 visual styling.

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

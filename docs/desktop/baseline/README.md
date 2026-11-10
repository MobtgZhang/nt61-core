# NT61 Desktop — Frozen Baseline

This file records the *factual* baseline that Stage A froze before any
Win7 graphics-stack work began. It is intentionally machine-readable
where possible so that later stages can diff against it.

## Source commit

| Field | Value |
|---|---|
| Commit | `a1e98ea1f5a3489b61e84756ed97b7f086acb1d6` |
| Subject | `ring3: userland library and system process updates` |
| Branch | `main` |

## Toolchain

| Tool | Version |
|---|---|
| `rustc` | 1.101.0-nightly (708c94ef0 2026-11-03) |
| `cargo` | 1.101.0-nightly (31d3246e0 2026-10-27) |
| `qemu-system-x86_64` | 10.2.1 (Debian 1:10.2.1+ds-1ubuntu3.3) |
| OVMF | `/usr/share/OVMF/OVMF_CODE_4M.fd` + `OVMF_VARS_4M.fd` (EDK2, 4 MiB) |
| Python | 3.x with PIL 12.3.0 (screenshot diffing) |

## Targets

| Target | Status |
|---|---|
| `x86_64-unknown-none` | usable — `staticlib` + `#[panic_handler]` produces `.a` |
| `x86_64-pc-windows-msvc` | **unusable** — no `link.exe`, no `.lib` import libraries |
| `x86_64-pc-windows-gnu` | **unusable** — no `x86_64-w64-mingw32-gcc` |
| `rust-lld` (GNU flavour) | `-shared` only; `-flavor gnu` rejects `-dll` / `-noentry` / `-def` |

**Consequence:** no external PE toolchain is introduced. The image
pipeline continues to use the repository's pure-Rust PE emitter
(`crates/tools/src/fs/build.rs::build_pe_image`) and the pure-Rust
ELF→PE converter (`crates/tools/src/fs/elf_to_pe.rs`).

## Network / crates

* crates.io direct: HTTP 403.
* `rsproxy.cn` mirror: HTTP 200.
* 490 crates cached locally, including `fontdue 0.9.5`, `ab_glyph 0.2.32`,
  `ttf-parser 0.25.1`, `png 0.17.16`.
* New dependencies must pin an exact version and commit the lockfile.

## Fonts available on the build host

| Path | License | Notes |
|---|---|---|
| `/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc` | OFL-1.1 | 19 MiB+ — must be subsetted |
| `/usr/share/fonts/truetype/arphic/ukai.ttc` | Arphic Public License | CJK fallback alternative |
| `resources/fonts/liberation-fonts/*` | SIL OFL 1.1 | Latin, already in-repo |
| `resources/fonts/open-sans/*` | SIL OFL 1.1 | Latin, already in-repo |
| `resources/fonts/dejavu-fonts-XXXX/*` | DejaVu Bitstream Vera | Latin, already in-repo |

## QEMU automation

* `-display gtk` and `-qmp unix:...` are already wired in the `Makefile`.
* `DISPLAY=:0` is available and `Xvfb` is installed, so headless screenshot
  regression is possible.

## Baseline architecture audit (commit `a1e98ea`)

The audit that motivated this plan. Each row was verified by reading the
source, not inferred.

| Area | Finding | Evidence |
|---|---|---|
| `explorer.exe` | Draws a pre-baked PNG into the framebuffer. No windows, no message loop, no events. | `crates/desktop/src/bin/explorer/main.rs` calls `arch::draw_rgba` with `wallpaper.png` / `taskbar.png` / `orb.png`. |
| Taskbar | A *static* PNG with the wallpaper already blurred and tinted into it. | `crates/desktop/tools/gen_assets.py::make_taskbar` crops + gaussian-blurs + tints the wallpaper and writes `taskbar.png`. |
| Fonts | Baked ASCII 32..127 bitmap atlas. | `crates/desktop/src/font_atlas_data.rs`: `FIRST_CHAR = 32`, `LAST_CHAR = 127`, `atlas_white.png` / `atlas_black.png`. |
| Logon UI | Baked glass panel image; no text input. | `crates/desktop/tools/gen_assets.py::make_logon_panel`. |
| Desktop ABI | Private syscall ABI, not Win32. | `crates/arch/src/arch/common/desktop_host.rs`: `SYS_DESKTOP_INFO = 0x0300` … `SYS_DESKTOP_POLL_KEY = 0x0304`. |
| `user32` / `gdi32` | Rust modules aggregated *into the kernel*, not loadable DLLs. | `crates/libs/mod.rs` re-exports `pub mod user32;`; `crates/libs/user32/src/user32/mod.rs` header says "This module is a stub". |
| `win32k` window manager | One global table, no Session/Desktop isolation. | `crates/subsystems/win32k/src/win32k/window.rs`: `WINDOW_MANAGER: Spinlock<WindowManager>`. |
| Text rendering | 8x8 VGA dot matrix. | `crates/subsystems/win32k/src/win32k/text.rs`: `FONT_DATA_8X8`. |
| Screen metrics | Hard-coded 1024x768. | `crates/libs/user32/src/user32/input.rs`: `SM_CXSCREEN => 1024`. |
| DWM / D2D / DWrite / DXGI / D3D | Absent. | `rg -l 'dwm\|dxgi\|d2d1\|dwrite'` hits only unrelated GPU-driver code. |
| uxtheme / comctl32 / shell32 / ole32 | Absent. | No such crates or modules exist. |
| Mouse | Relative deltas only; no absolute position, no button routing. | `crates/drivers/src/drivers/mouse.rs`: `post_mouse_event(dx, dy, dz, buttons)`. |

### Already-correct items (kept)

* `BootMode::Desktop` no longer shares the CMD path — `crates/ntoskrnl/src/kernel_main.rs`
  Phase 15 dispatches to `start_desktop_boot_sequence()`.
* A PE loader with import/export/relocation support already exists:
  `crates/loader/src/loader/mod.rs` (`resolve_imports`, `parse_exports`, `ImageDatabase`).
* A pure-Rust ELF→PE converter already exists:
  `crates/tools/src/fs/elf_to_pe.rs`.
* `fontdue` is already a dependency of `crates/fonts` (unused so far).

## Baseline verification

`make verify-strict` is the gate. It runs, in order:

1. `cargo check -p nt61-tools --all-targets` with `-D warnings`
2. `cargo test -p nt61-tools`
3. `cargo check -p cmd-stub-gen --all-targets`
4. kernel checks for `x86_64`, `aarch64`, `riscv64gc`, `loongarch64` on
   `*-unknown-none`
5. `nt61-boot` / `nt61-winload` on `x86_64-unknown-uefi`
6. an image-build smoke run for `fat32` / `ntfs` / `ext4` / `iso`

## What Stage A added

* `docs/desktop/win7-metrics.toml` — the machine-readable Win7 visual metric spec.
* `docs/desktop/api-matrix.md` — per-export status of the existing `user32` / `gdi32` / `win32k` surfaces.
* `docs/desktop/theme-license.md` — asset provenance and redistribution audit.
* `crates/tools/tests/win7_metrics_tests.rs` — schema + value validation of the metrics TOML.

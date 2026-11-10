# NT61 Desktop — Theme, Font and Icon Licence Audit

This file records the provenance and redistribution terms of every
visual asset the desktop uses.

**Hard rule.** No Microsoft asset may enter the repository. That means
no `aero.msstyles`, no `AeroGlass` bitmaps, no Windows wallpapers, no
Windows icons, no `Segoe UI`, no `Segoe UI Semibold`, no Windows
7 system sounds, and no `imageres.dll` fragments.

NT61 therefore ships:

1. **Original theme data** — colours, metrics, radii, gradients and
   timings, defined by this project in
   [`docs/desktop/win7-metrics.toml`](win7-metrics.toml) and
   [`crates/theme`](../crates/theme). The Win7 *look* is reproduced by
   **procedural drawing**, not by copying Microsoft's images.
2. **Original vector icons** — described as path data in
   `crates/shell/explorer` (and, later, `crates/theme`), rasterised at
   run time at 16/32/48 px.
3. **Open-licensed fonts**, subsetted where necessary.

---

## 1. Fonts

| Font | Path in repo | Licence | Redistribution | Use |
|---|---|---|---|---|
| Liberation Sans | `resources/fonts/liberation-fonts/` | SIL Open Font License 1.1 | Allowed, with the licence text and the Reserved Font Name caveat | Latin UI text (substitute for Segoe UI) |
| Open Sans | `resources/fonts/open-sans/` | SIL Open Font License 1.1 | Allowed | Latin UI text, secondary |
| DejaVu Sans / Sans Mono | `resources/fonts/dejavu-fonts-XXXX/` | DejaVu Fonts Licence (Bitstream Vera + Public domain additions) | Allowed | Fallback, terminal |
| DejaVuSans.ttf / DejaVuSansMono.ttf | `crates/fonts/assets/` | DejaVu Fonts Licence | Allowed | Boot-time rasterisation before the font service is up |
| JetBrains Mono | `resources/fonts/jetbrains-mono/` | SIL Open Font License 1.1 | Allowed | Monospace only |
| Source Code Pro | `resources/fonts/source-code-pro/` | SIL Open Font License 1.1 | Allowed | Monospace only |
| Noto Sans CJK (subset) | to be produced by the `crates/tools` subsetter | SIL Open Font License 1.1 | Allowed — the subset **must** keep the OFL notice and the "Reserved Font Name" clause unmodified | CJK fallback |
| Arphic UKai / UMing | `arphic-*.ttc` (host-only, optional) | Arphic Public License | Allowed with notice | Optional CJK fallback |

### CJK subsetting rules

* The subsetter **must** keep the OFL copyright notice, the licence URL
  and the Reserved Font Name statement inside the output font.
* The subset is generated at build time from a host font; the generated
  file is committed under `crates/fonts/assets/` with its licence file
  next to it.
* Target coverage: GB2312 level 1 + level 2 (6763 hanzi) plus CJK
  punctuation and full-width forms.

---

## 2. Icons

> **Icon style requirement.** Icons must be drawn as *smooth,
> anti-aliased vector artwork* in the Windows 7 idiom — soft gradients,
> rounded geometry, subtle depth and drop shadows. **No pixel-art /
> retro / 8-bit icons** and no nearest-neighbour upscaled bitmaps. The
> reference for the visual language is the community Win7 icon work
> (e.g. the [Visnalize/win7-icons](https://github.com/Visnalize/win7-icons)
> viewer and the [B00merang-Artwork/Windows-7](https://github.com/B00merang-Artwork/Windows-7)
> GTK theme), which reproduce the *style* of the Win7 icon set. Those
> repositories are **style references only** — their pixels are
> derivative of Microsoft's artwork and are therefore never vendored
> into this repository. Every icon shipped by NT61 is an original path
> authored here and rasterised at run time with the anti-aliased
> renderer in `crates/gfx`.

| Icon set | Source | Status |
|---|---|---|
| Win7 shell icons (Computer, Recycle Bin, Network, Control Panel, user folder) | **Original vector paths** authored in this repository | OK |
| Caption buttons (minimise / maximise / restore / close) | **Original vector paths**, drawn to match the Win7 *geometry* (not the artwork) | OK |
| Start Orb | **Original procedural rendering**: radial gradient + four rounded-rect panes + gloss + outer glow | OK |
| Taskbar / notification-area icons | **Original vector paths** | OK |
| Application icons (`notepad`, `cmd`, `calculator`, `paint`, …) | **Original vector paths** | OK |

> The pre-Stage-G repository contained baked PNGs under
> `crates/desktop/assets/` (`orb.png`, `taskbar.png`, `wallpaper.png`,
> `logon_panel.png`, `atlas_white.png`, `atlas_black.png`,
> `assets/7css/gui/icon/*.png`). These are **removed** as part of
> Stage G because they are exactly the "pixel collage" the plan
> forbids. The 7css-derived icons in particular trace back to a
> third-party re-skin of Windows 7 and must not be redistributed.

---

## 3. Theme data

`crates/theme` defines its own theme format:

* colour ramps (active / inactive / hot / pressed)
* metrics (taken from `win7-metrics.toml`)
* nine-patch geometry, corner radii, border widths
* shadow and glow parameters
* animation durations

It **does not** parse, embed or redistribute `aero.msstyles` or any
other Microsoft theme file.

---

## 4. Runtime-generated imagery

| Asset | How it is produced |
|---|---|
| Wallpaper | Supplied by the user, or generated procedurally as a gradient + noise field. |
| Taskbar glass | Sampled live from the composited desktop, blurred, saturated and tinted at run time. |
| Title-bar glass | Sampled live from the content behind the window, blurred, saturated and tinted at run time. |
| Window shadows / glows | Computed by the CPU rasteriser (`crates/gfx`). |
| Start Orb / caption buttons / shell icons | Rasterised from original vector paths at run time. |
| Cursors | Drawn by the software cursor layer in `crates/gfx`. |

None of the above is a pre-baked bitmap shipped in the repository.

---

## 5. Verification

`crates/tools/tests/theme_license_tests.rs` walks the tracked file list
and fails the build if a banned artefact is found:

* any file under `crates/desktop/assets/theme/` or
  `crates/desktop/assets/fonts/`
* any file whose path contains `7css`
* any `.msstyles` file
* any file whose name matches `Segoe`, `seguisb`, `segoeui`, `tahoma`,
  `arial`

The check is deliberately conservative: it looks at *paths*, not at
pixel content, so it can run in CI without a rasteriser.

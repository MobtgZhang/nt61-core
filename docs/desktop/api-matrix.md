# NT61 Desktop — Win32 API Matrix

Status legend:

| Status | Meaning |
|---|---|
| `STUB` | The symbol exists but performs no real work (or returns a hard-coded value). |
| `INTEGRATED` | The symbol is implemented against a real in-kernel object model, but the object model is not yet the Win32 authority (e.g. a user-mode shadow table instead of a kernel handle). |
| `TESTED` | Implemented **and** covered by an automated test that asserts observable behaviour. |
| `ABSENT` | The symbol does not exist. |

> "The function exists" is **not** completion. A row only leaves `STUB`
> once a behaviour test exists for it.

---

## 1. `user32` — kernel-aggregated module (baseline `a1e98ea`)

Source: `crates/libs/user32/src/user32/`.

| Export | Status | Notes |
|---|---|---|
| `CreateWindowExW` | `INTEGRATED` | Resolves class, allocates a handle, sends `WM_NCCREATE`/`WM_CREATE`. Handle table is a *user-mode shadow* (`WINDOW_OBJECT_TABLE`), not a kernel object. |
| `DestroyWindow` | `INTEGRATED` | Sends `WM_DESTROY`/`WM_NCDESTROY`. |
| `ShowWindow` | `INTEGRATED` | Toggles a boolean; `SW_*` semantics are partial. |
| `UpdateWindow` | `INTEGRATED` | Sends `WM_PAINT` directly instead of marking a paint region. |
| `GetWindowTextW` | `INTEGRATED` | Copies the stored UTF-16 text. |
| `GetWindowRectW` | `INTEGRATED` | Returns the stored rect. |
| `GetWindowLongPtrW` | `INTEGRATED` | Supports `GWL_WNDPROC/HINSTANCE/HWNDPARENT/STYLE/EXSTYLE`. |
| `GetParentW` | `INTEGRATED` | |
| `GetMenuW` | `INTEGRATED` | |
| `GetForegroundWindow` / `SetForegroundWindow` | `INTEGRATED` | Single global slot; no per-desktop activation order. |
| `DefWindowProcW` | `STUB` | Only acknowledges `WM_NCCREATE`; no `WM_NCHITTEST`, `WM_SIZE`, `WM_SETFOCUS`, … |
| `GetMessageW` / `PeekMessageW` | `INTEGRATED` | User-mode queue; **does not block** when empty. |
| `TranslateMessage` / `DispatchMessageW` | `INTEGRATED` | |
| `SendMessageW` / `PostMessageW` | `INTEGRATED` | |
| `PostQuitMessage` / `WaitMessage` | `INTEGRATED` | |
| `GetAsyncKeyState` / `GetKeyState` / `GetKeyboardState` | `STUB` | Reads a flat keyboard buffer; no per-thread or per-desktop focus semantics. |
| `GetCursorPos` | `STUB` | |
| `GetSystemMetrics` | `STUB` | **Hard-codes `SM_CXSCREEN => 1024`, `SM_CYSCREEN => 768`.** |
| `MessageBoxW` / `DialogBoxParamW` | `STUB` | |
| `RegisterClassExW` / `UnregisterClassW` | `INTEGRATED` | Kernel-aggregated class table. |
| `CreateMenu` / `TrackPopupMenu` / `DestroyMenu` | `STUB` | |
| `OpenWindowStationW` / `CreateDesktopW` / `SwitchDesktop` | `STUB` | Return *pseudo* handles; no object, no ACL. |
| `SetCapture` / `ReleaseCapture` | `ABSENT` | |
| `GetDC` / `ReleaseDC` | `INTEGRATED` | Via `gdi_link`. |
| `BeginPaint` / `EndPaint` / `InvalidateRect` | `INTEGRATED` | |

### `user32` absent but required by the plan

`SetWindowPos`, `MoveWindow`, `GetClientRect`, `GetWindowTextLengthW`,
`SetWindowTextW`, `IsWindow`, `IsWindowVisible`, `EnableWindow`,
`SetWindowLongPtrW`, `GetDCEx`, `RedrawWindow`, `GetDesktopWindow`,
`GetTopWindow`, `GetWindow`, `EnumWindows`, `FindWindowExW`,
`GetClassNameW`, `GetParent`, `SetParent`, `SetFocus`, `GetFocus`,
`GetActiveWindow`, `SetActiveWindow`, `GetKeyState`, `MapVirtualKeyW`,
`ToUnicodeEx`, `SendInput`, `GetMessagePos`, `WindowFromPoint`,
`TrackPopupMenuEx`, `CreatePopupMenu`, `AppendMenuW`, `InsertMenuItemW`,
`DrawTextW`, `SetTimer`, `KillTimer`, `GetSysColor`, `GetSysColorBrush`,
`SystemParametersInfoW`, `RegisterWindowMessageW`, `IsDialogMessageW`,
`CharUpperW`, `wsprintfW`, `LoadStringW`, `GetWindowThreadProcessId`,
`AttachThreadInput`, `SetWindowsHookExW`, `CallNextHookEx`,
`UnhookWindowsHookEx`, `GetClassInfoExW`, `GetMenuItemInfoW`.

---

## 2. `gdi32` — kernel-aggregated module (baseline `a1e98ea`)

Source: `crates/libs/gdi32/src/gdi32/`.

| Export | Status | Notes |
|---|---|---|
| `CreateCompatibleDC` / `DeleteDC` | `INTEGRATED` | |
| `SelectObject` / `DeleteObject` | `INTEGRATED` | Kernel object table (`GdiObject`). |
| `CreateSolidBrush` / `CreatePen` / `CreatePenIndirect` / `CreateFontIndirectW` | `INTEGRATED` | |
| `BitBlt` / `PatBlt` | `INTEGRATED` | `pat_blt_surface` exists; `BitBlt` is partial. |
| `FillRect` | `INTEGRATED` | |
| `TextOutW` / `ExtTextOutW` / `DrawTextW` | `STUB` | Renders with the **8x8 VGA dot matrix** (`crates/libs/gdi32/src/gdi32/text.rs`), not a real font. |
| `GetTextExtentPoint32W` / `GetTextMetricsW` | `STUB` | Returns hard-coded `tm_height = 16`. |
| `GetDeviceCaps` | `INTEGRATED` | |
| `SaveDC` / `RestoreDC` | `INTEGRATED` | |
| `CreateRectRgn` / `IntersectClipRect` | `INTEGRATED` | |
| `GetStockObject` | `INTEGRATED` | |
| `CreateDIBSection` | `INTEGRATED` | |
| `AlphaBlend` / `StretchBlt` / `DrawEdge` / `Polygon` / `RoundRect` / `Ellipse` / `LineTo` / `MoveToEx` | `ABSENT` | |

---

## 3. `win32k` — kernel subsystem (baseline `a1e98ea`)

Source: `crates/subsystems/win32k/src/win32k/`.

| Area | Status | Notes |
|---|---|---|
| Window table | `INTEGRATED` | One global `WINDOW_MANAGER`; **no Session / WindowStation / Desktop**. |
| Window creation order | `INTEGRATED` | `WM_NCCREATE` → `WM_CREATE`; **`WM_NCCALCSIZE` is missing**. |
| `WM_NCHITTEST` | `ABSENT` | Without it there is no dragging, resizing, or caption-button hit testing. |
| Message queue blocking | `STUB` | `GetMessage` returns immediately when the queue is empty. |
| Focus / capture / activation | `STUB` | Not modelled per desktop. |
| GDI objects | `INTEGRATED` | |
| Text rendering | `STUB` | `FONT_DATA_8X8` dot matrix. |
| Font stack | `ABSENT` | `crates/fonts` (`fontdue`) is **not** referenced by `win32k` or `gdi32`. |
| CJK fallback | `ABSENT` | |
| Surface / redirection | `STUB` | `surface.rs` exists but is not a DWM-capable redirection surface. |
| Clipboard | `INTEGRATED` | Single global clipboard. |
| Regions | `INTEGRATED` | |

---

## 4. Shadow SSDT syscall numbering (baseline `a1e98ea`)

The handlers are registered with the **real Windows 7 SP1 x64 win32k
index** (`0x1000`–`0x1FFF`), and `dispatch_shadow_service` masks the
number with `& 0xFFF` before indexing the table.

So the *numbering itself* is already Win7-correct. What is missing is
that the **`ntdll.dll` shipped in the image does not expose the
`NtUser*` / `NtGdi*` stubs**: `crates/tools/src/fs/build.rs::build_ntdll_pe`
exports only 11 ntoskrnl-level symbols (`NtCreateFile`, `NtReadFile`, …).
There is therefore no Ring-3 path into `win32k` at all.

Stage C closes this by emitting a real `ntdll.dll` with the Win7
`NtUser*` / `NtGdi*` export surface plus the `syscall` instruction stubs.

---

## 5. PE / image layer (baseline `a1e98ea`)

| Capability | Status | Notes |
|---|---|---|
| PE32+ emitter | `INTEGRATED` | `crates/tools/src/fs/build.rs::build_pe_image`. |
| Export table (by name) | `INTEGRATED` | |
| Export table (by ordinal) | `ABSENT` | |
| Export forwarders | `ABSENT` | |
| Import table | `ABSENT` | The emitter writes no `.idata`; the loader can *resolve* imports but nothing produces them. |
| `.reloc` section | `ABSENT` | `build_pe_base_relocation_table` exists but is not wired into the emitted images. |
| GUI subsystem | `ABSENT` | Every emitted image uses `IMAGE_SUBSYSTEM_EFI_APPLICATION (0x0A)`. Win7 GUI images use `IMAGE_SUBSYSTEM_WINDOWS_GUI (2)`. |
| `.rsrc` section | `ABSENT` | `crates/desktop/src/resource_generator.rs` has a simplified, non-generic version. |
| DLL image bases | `INTEGRATED` | Distinct bases per image so the loader does not map two images over each other. |

---

## 6. Ring-3 runtime (baseline `a1e98ea`)

| Item | Status | Notes |
|---|---|---|
| `ntdll` syscall stubs | `INTEGRATED` | `crates/userland/runtime/src/userspace/ntdll/syscalls.rs` has the numbers; `crates/arch/src/arch/x86_64/user_syscall.rs` has the `syscall` instruction wrappers. |
| `NtUser*` / `NtGdi*` stubs | `ABSENT` | The win32k service table is not reachable from Ring 3. |
| `GetLastError` thread-local | `ABSENT` | |
| NTSTATUS → Win32 error mapping | `ABSENT` | `crates/userland/runtime/src/userspace/ntdll/status.rs` has partial coverage. |
| PE loader for user images | `INTEGRATED` | `crates/loader/src/loader/mod.rs::load_image_full`. |
| `user32.dll` / `gdi32.dll` as *files* | `ABSENT` | Only kernel-aggregated Rust modules exist. |
| `dwm.exe` | `ABSENT` | |
| `explorer.exe` (real shell) | `ABSENT` | Only the PNG-drawing stub exists. |
| `LogonUI.exe` (real) | `ABSENT` | Only the baked-panel stub exists. |

---

## 7. Display / input (baseline `a1e98ea`)

| Item | Status | Notes |
|---|---|---|
| Framebuffer HAL | `INTEGRATED` | `crates/hal/src/hal/common/framebuffer_impl.rs`. |
| Pixel format single source of truth | `ABSENT` | `color32(r,g,b)` packs `0x00BBGGRR`; `desktop_host::draw_rgba` performs a *second* manual channel swap. |
| `DisplayBackend` trait | `ABSENT` | |
| CPU software rasteriser | `ABSENT` | No blur / shadow / gradient / rounded-rect / AA glyph path exists. |
| Mouse absolute position | `ABSENT` | `post_mouse_event(dx, dy, dz, buttons)` only. |
| Mouse button state machine / double click | `ABSENT` | |
| Software cursor layer | `ABSENT` | |
| Keyboard scancode → VK → char | `STUB` | `keyboard_input` stops at raw ASCII bytes. |

---

## 8. Target state after Stage J

| Surface | Target |
|---|---|
| `user32.dll` | Real PE32+ DLL in `C:\Windows\System32`, exports backed by real kernel objects. |
| `gdi32.dll` | Real PE32+ DLL, `TextOutW` backed by `fontdue`. |
| `ntdll.dll` | Real PE32+ DLL with the Win7 `NtUser*`/`NtGdi*` stub surface. |
| `uxtheme.dll` | Real PE32+ DLL, programmatic Aero non-client drawing. |
| `dwmapi.dll` | Real PE32+ DLL. |
| `shell32.dll` / `ole32.dll` / `oleaut32.dll` / `comctl32.dll` | Real PE32+ DLLs. |
| `d2d1.dll` / `dwrite.dll` / `dxgi.dll` / `d3d11.dll` | Real PE32+ DLLs with exact vtable ABIs over a CPU backend. |
| `dwm.exe` | Separate Ring-3 process, one instance per session. |
| `explorer.exe` | Real shell: `Progman` / `WorkerW` / `SHELLDLL_DefView` / `Shell_TrayWnd`. |
| `LogonUI.exe` | Real login UI with live input on the secure desktop. |

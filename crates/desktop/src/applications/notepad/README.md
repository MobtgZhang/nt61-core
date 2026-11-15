# Notepad

Windows 7 menu-based plain-text accessory. See `docs/desktop/notepad.md` for behavior and validation.

Modules: `mod.rs`, `commands.rs`, `menus.rs`, `pointer.rs`, `layout.rs`, `render.rs`, `dialogs.rs`, `ui.rs`.

Application-owned artwork lives in `resources/`, beside the application code. See `resources/README.md` for regeneration commands and licenses. Common VFS, document, font, compositor and runtime services are reused rather than duplicated.

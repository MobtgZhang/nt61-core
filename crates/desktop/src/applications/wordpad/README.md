# Wordpad

Windows 7 Home/View rich-text accessory. See `docs/desktop/wordpad.md` for formats, validation and remaining limits.

Modules: `mod.rs`, `commands.rs`, `ribbon.rs`, `pointer.rs`, `layout.rs`, `formats/`.

Application-owned artwork lives in `resources/`, beside the application code. See `resources/README.md` for regeneration commands and licenses. Common VFS, document, font, compositor and runtime services are reused rather than duplicated.

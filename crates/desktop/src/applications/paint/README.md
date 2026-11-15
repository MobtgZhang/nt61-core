# Paint

Windows 7-style lightweight bitmap accessory using the local surface, file dialog and application host.

Modules: `mod.rs` (model/commands), `raster.rs` (drawing), `bitmap.rs` (BMP I/O), `ui.rs` (ribbon/control rendering).

Application-owned artwork lives in `resources/`, beside the application code. See `resources/README.md` for regeneration commands and licenses. Common VFS, document, font, compositor and runtime services are reused rather than duplicated.

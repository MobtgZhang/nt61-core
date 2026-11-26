//! Dump the Win32 DLLs to a directory so that external tools
//! (`file`, `objdump`) can inspect them.
//!
//! Usage: `cargo run -p nt61-tools --bin dump_win32_dlls -- <out-dir> [arch]`

use nt61_tools::fs::win32_dlls::{ALL_DLLS, build_dll_image_for_arch};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir: PathBuf = args.next()
        .unwrap_or_else(|| "build/win32-dlls".to_string())
        .into();
    let arch = args.next().unwrap_or_else(|| "x86_64".to_string());

    if let Err(error) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create {out_dir:?}: {error}");
        std::process::exit(1);
    }

    for spec in ALL_DLLS {
        let bytes = build_dll_image_for_arch(spec, &arch).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
        let path = out_dir.join(spec.file_name);
        if let Err(error) = std::fs::write(&path, &bytes) {
            eprintln!("cannot write {path:?}: {error}");
            std::process::exit(1);
        }
        println!("{path:?} ({} bytes)", bytes.len());
    }
}

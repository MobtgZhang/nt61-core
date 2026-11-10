//! Dump the Win32 DLLs to a directory so that external tools
//! (`file`, `objdump`) can inspect them.
//!
//! Usage: `cargo run -p nt61-tools --bin dump-win32-dlls -- <out-dir>`

use std::path::PathBuf;

fn main() {
    let out_dir: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "build/win32-dlls".to_string())
        .into();

    if let Err(error) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create {out_dir:?}: {error}");
        std::process::exit(1);
    }

    for (name, bytes) in nt61_tools::fs::win32_dlls::build_all_dlls() {
        let path = out_dir.join(name);
        if let Err(error) = std::fs::write(&path, &bytes) {
            eprintln!("cannot write {path:?}: {error}");
            std::process::exit(1);
        }
        println!("{path:?} ({} bytes)", bytes.len());
    }
}

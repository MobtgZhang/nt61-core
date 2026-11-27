use std::path::Path;

fn main() {
    for input in [
        "src/fs/sp1_reference.rs",
        "../../docs/desktop/baselines/win7-sp1-window-placement/original-fingerprints.json",
    ] {
        println!("cargo:rerun-if-changed={input}");
        assert!(
            Path::new(input).is_file(),
            "Missing {input} (relative to crates/tools). Run `make prepare-build-inputs SP1_WIM=/path/to/install.wim` from the workspace root."
        );
    }
}

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

fn check_application_icons() {
    let applications = Path::new("src/applications");
    let mut missing = Vec::new();
    for app in fs::read_dir(applications).expect("desktop application directory") {
        let resources = app
            .expect("desktop application directory entry")
            .path()
            .join("resources");
        if !resources.join("icon.svg").is_file() {
            continue;
        }
        let rasters = [
            "icon-16.rgba",
            "icon-24.rgba",
            "icon-32.rgba",
            "icon-48.rgba",
            "icon-64.rgba",
            "icon-256.rgba",
            "icon.rgba",
        ];
        for raster in rasters {
            let path = resources.join(raster);
            println!("cargo:rerun-if-changed={}", path.display());
            if !path.is_file() {
                missing.push(path);
            }
        }
    }
    missing.sort();
    if let Some(first) = missing.first() {
        panic!(
            "Missing {} application icon rasters (first: {}). Run `python3 scripts/generate-application-icons.py --missing-only` from the workspace root.",
            missing.len(),
            first.display(),
        );
    }
}

fn main() {
    check_application_icons();
    let system_logo = "src/applications/explorer/resources/shell/system-logo.rgba";
    println!("cargo:rerun-if-changed={system_logo}");
    println!("cargo:rustc-check-cfg=cfg(nt61_system_logo)");
    if Path::new(system_logo).is_file() {
        let length = fs::metadata(system_logo)
            .expect("optional system logo")
            .len();
        assert_eq!(
            length,
            157 * 151 * 4,
            "Invalid RGBA length for {system_logo}"
        );
        println!("cargo:rustc-cfg=nt61_system_logo");
    }
    // lld defaults to a 64 KiB ELF base on these targets. The PE wrapper
    // needs a separate aligned header area below the first LOAD segment;
    // at that default it produces ImageBase=0, which the user mapper
    // correctly refuses. Choose the link address here so absolute data
    // pointers and the PE's preferred address stay in agreement.
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if matches!(target_arch.as_str(), "riscv64" | "loongarch64") {
        println!("cargo:rustc-link-arg=--image-base=0x200000");
    }
    if target_arch == "aarch64" && env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        for binary in ["explorer", "logonui", "application-host"] {
            println!("cargo:rustc-link-arg-bin={binary}=--image-base=0x400000000");
        }
    }
    let path = "../../docs/desktop/win7-metrics.toml";
    println!("cargo:rerun-if-changed={path}");
    let source = fs::read_to_string(path).expect("desktop metrics");
    let mut section = String::new();
    let mut values = BTreeMap::new();
    for line in source.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_owned();
        } else if let Some((key, value)) = line.split_once('=') {
            if let Ok(number) = value.trim().parse::<u32>() {
                values.insert(format!("{section}.{}", key.trim()), number);
            }
        }
    }
    let mut output = String::new();
    for (name, key) in [
        ("TASKBAR_HEIGHT", "screen.taskbar_height"),
        ("START_ORB_SIZE", "screen.orb_size"),
        ("START_ORB_GLOW_RADIUS", "screen.orb_glow_radius"),
        ("PROGRAM_BUTTON_WIDTH", "screen.taskbar_button_width"),
        ("TRAY_WIDTH", "screen.tray_width"),
        ("CLOCK_WIDTH", "screen.clock_width"),
        ("SHOW_DESKTOP_WIDTH", "screen.show_desktop_width"),
        ("UI_FONT_SIZE", "screen.clock_font_size"),
        ("START_MENU_WIDTH", "start_menu.width"),
        ("START_MENU_HEIGHT", "start_menu.height"),
        ("START_LEFT_WIDTH", "start_menu.left_pane_width"),
        ("SEARCH_HEIGHT", "start_menu.search_height"),
        ("POWER_BUTTON_WIDTH", "start_menu.power_button_width"),
        ("POWER_ARROW_WIDTH", "start_menu.power_arrow_width"),
        ("POWER_BUTTON_HEIGHT", "start_menu.power_button_height"),
        ("MAXIMIZED_FRAME_X", "reference_system_metrics.sm_cxframe"),
        ("MAXIMIZED_FRAME_Y", "reference_system_metrics.sm_cyframe"),
    ] {
        let value = values
            .get(key)
            .or_else(|| {
                key.strip_prefix("reference_system_metrics.")
                    .and_then(|metric| values.get(&format!("screen.{metric}")))
            })
            .unwrap_or_else(|| panic!("missing metric {key} in {path}"));
        output.push_str(&format!("pub const {name}: u32 = {value};\n"));
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("shell_metrics.rs"),
        output,
    )
    .expect("write shell metrics");
}

use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() {
    // lld defaults to a 64 KiB ELF base on these targets. The PE wrapper
    // needs a separate aligned header area below the first LOAD segment;
    // at that default it produces ImageBase=0, which the user mapper
    // correctly refuses. Choose the link address here so absolute data
    // pointers and the PE's preferred address stay in agreement.
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if matches!(target_arch.as_str(), "riscv64" | "loongarch64") {
        println!("cargo:rustc-link-arg=--image-base=0x200000");
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
    ] {
        let value = values
            .get(key)
            .unwrap_or_else(|| panic!("missing metric {key}"));
        output.push_str(&format!("pub const {name}: u32 = {value};\n"));
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("shell_metrics.rs"),
        output,
    )
    .expect("write shell metrics");
}

//! Desktop parameter schema checks and independently captured SP1 metrics.
//! Exact checks of legacy rendering parameters detect configuration drift only;
//! they do not establish Windows 7 pixel or timing conformance.

use std::path::PathBuf;

use toml::Value;

fn metrics_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/desktop/win7-metrics.toml")
}

fn load_metrics() -> Value {
    let path = metrics_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{} is not valid TOML: {e}", path.display()))
}

fn table<'a>(root: &'a Value, path: &str) -> &'a toml::map::Map<String, Value> {
    let mut current = root;
    for part in path.split('.') {
        current = current
            .get(part)
            .unwrap_or_else(|| panic!("missing section [{path}] in win7-metrics.toml"));
    }
    current
        .as_table()
        .unwrap_or_else(|| panic!("[{path}] is not a table"))
}

fn int_at(root: &Value, section: &str, key: &str) -> i64 {
    table(root, section)
        .get(key)
        .unwrap_or_else(|| panic!("missing key {section}.{key}"))
        .as_integer()
        .unwrap_or_else(|| panic!("{section}.{key} is not an integer"))
}

fn float_at(root: &Value, section: &str, key: &str) -> f64 {
    table(root, section)
        .get(key)
        .unwrap_or_else(|| panic!("missing key {section}.{key}"))
        .as_float()
        .unwrap_or_else(|| panic!("{section}.{key} is not a float"))
}

fn rgb_at(root: &Value, section: &str, key: &str) -> [i64; 3] {
    let value = table(root, section)
        .get(key)
        .unwrap_or_else(|| panic!("missing key {section}.{key}"));
    let array = value
        .as_array()
        .unwrap_or_else(|| panic!("{section}.{key} is not an array"));
    assert_eq!(array.len(), 3, "{section}.{key} must have 3 components");
    let mut out = [0i64; 3];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = array[index]
            .as_integer()
            .unwrap_or_else(|| panic!("{section}.{key}[{index}] is not an integer"));
        assert!(
            (0..=255).contains(slot),
            "{section}.{key}[{index}] = {slot} is outside 0..=255"
        );
    }
    out
}

#[test]
fn metrics_file_parses_and_has_required_sections() {
    let metrics = load_metrics();
    for section in [
        "meta",
        "screen",
        "window",
        "aero",
        "start_menu",
        "logon",
        "desktop",
        "font",
        "animation",
    ] {
        assert!(
            metrics.get(section).is_some(),
            "win7-metrics.toml is missing the [{section}] section"
        );
    }
}

#[test]
fn meta_declares_windows_7_sp1() {
    let metrics = load_metrics();
    assert_eq!(
        table(&metrics, "meta").get("target_os").unwrap().as_str(),
        Some("Windows 7 SP1")
    );
    assert_eq!(
        table(&metrics, "meta")
            .get("target_build")
            .unwrap()
            .as_integer(),
        Some(7601)
    );
    assert_eq!(int_at(&metrics, "meta", "dpi"), 96);
}

#[test]
fn system_metrics_match_original_sp1_capture() {
    let metrics = load_metrics();
    let path = metrics_path().parent().unwrap()
        .join("baselines/win7-sp1-window-placement/metrics.json");
    let capture: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(capture["dpi"], serde_json::json!([96, 96]));
    assert_eq!(capture["dwm_composition"], false);
    let original = capture["metrics"].as_object().unwrap();
    let recorded = table(&metrics, "reference_system_metrics");
    assert_eq!(recorded.len(), original.len());
    for (name, value) in original {
        assert_eq!(int_at(&metrics, "reference_system_metrics", &name.to_ascii_lowercase()),
            value.as_i64().unwrap(), "original SP1 {name}");
    }
    assert!(!table(&metrics, "screen").keys().any(|key| key.starts_with("sm_")),
        "unmeasured/invalid legacy SM aliases must not reappear as system metrics");
}

#[test]
fn legacy_taskbar_and_caption_parameters_remain_stable() {
    let metrics = load_metrics();
    assert_eq!(int_at(&metrics, "screen", "taskbar_height"), 40);
    assert_eq!(int_at(&metrics, "screen", "taskbar_button_width"), 160);
    assert_eq!(int_at(&metrics, "screen", "taskbar_button_height"), 40);
    assert_eq!(int_at(&metrics, "screen", "orb_size"), 36);
    assert_eq!(int_at(&metrics, "screen", "orb_margin"), 6);
    assert_eq!(int_at(&metrics, "screen", "tray_width"), 118);
    assert_eq!(int_at(&metrics, "screen", "show_desktop_width"), 10);
    assert_eq!(int_at(&metrics, "screen", "clock_width"), 68);
    assert_eq!(int_at(&metrics, "window", "caption_height"), 22);
    assert_eq!(int_at(&metrics, "window", "caption_button_width"), 29);
    assert_eq!(int_at(&metrics, "window", "caption_button_height"), 21);
    assert_eq!(int_at(&metrics, "window", "border_width"), 8);
    assert_eq!(int_at(&metrics, "window", "resize_grip"), 14);
    assert_eq!(int_at(&metrics, "window", "corner_radius"), 8);
    assert_eq!(int_at(&metrics, "window", "shadow_extent"), 12);
}

#[test]
fn legacy_aero_parameters_remain_stable_without_a_pixel_conformance_claim() {
    let metrics = load_metrics();
    assert_eq!(rgb_at(&metrics, "aero", "glass_tint"), [0x1E, 0x40, 0x7A]);
    assert_eq!(float_at(&metrics, "aero", "glass_tint_alpha"), 0.55);
    assert_eq!(float_at(&metrics, "aero", "glass_highlight"), 0.28);
    assert_eq!(int_at(&metrics, "aero", "glass_blur_radius"), 12);
    assert_eq!(float_at(&metrics, "aero", "glass_saturation"), 1.30);
    assert_eq!(
        rgb_at(&metrics, "aero", "active_caption_text"),
        [255, 255, 255]
    );
    assert_eq!(
        rgb_at(&metrics, "aero", "inactive_caption_text"),
        [0xE8, 0xE8, 0xE8]
    );
    assert_eq!(float_at(&metrics, "aero", "active_caption_glow"), 0.55);
    assert_eq!(float_at(&metrics, "aero", "inactive_caption_glow"), 0.12);
}

#[test]
fn start_menu_two_pane_layout_is_self_consistent() {
    let metrics = load_metrics();
    let width = int_at(&metrics, "start_menu", "width");
    let left = int_at(&metrics, "start_menu", "left_pane_width");
    let right = int_at(&metrics, "start_menu", "right_pane_width");
    assert_eq!(width, 440);
    assert_eq!(left, 260);
    assert_eq!(right, 180);
    assert!(
        left + right <= width,
        "start menu panes ({left} + {right}) must fit inside the menu width {width}"
    );
    assert_eq!(int_at(&metrics, "start_menu", "height"), 640);
    assert_eq!(int_at(&metrics, "start_menu", "item_height"), 32);
    assert_eq!(int_at(&metrics, "start_menu", "recent_items"), 17);
}

#[test]
fn logon_panel_matches_windows_7() {
    let metrics = load_metrics();
    assert_eq!(int_at(&metrics, "logon", "panel_width"), 320);
    assert_eq!(int_at(&metrics, "logon", "panel_height"), 300);
    assert_eq!(int_at(&metrics, "logon", "avatar_size"), 96);
    let field = table(&metrics, "logon")
        .get("password_field")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(field.len(), 4, "password_field is [x, y, w, h]");
    assert_eq!(field[2].as_integer(), Some(210));
    assert_eq!(field[3].as_integer(), Some(30));
}

#[test]
fn all_opacity_values_are_in_range() {
    let metrics = load_metrics();
    for section in ["aero", "start_menu", "logon", "desktop"] {
        for (key, value) in table(&metrics, section) {
            // Only keys that actually name an opacity are checked;
            // other floats in these tables are multipliers
            // (e.g. aero.glass_saturation is 1.30 by design).
            if !key.ends_with("alpha") {
                continue;
            }
            let alpha = value
                .as_float()
                .unwrap_or_else(|| panic!("{section}.{key} must be a float"));
            assert!(
                (0.0..=1.0).contains(&alpha),
                "{section}.{key} = {alpha} is outside 0.0..=1.0"
            );
        }
    }
}

#[test]
fn font_sizes_are_expressed_in_points() {
    let metrics = load_metrics();
    let font = table(&metrics, "font");
    for key in [
        "ui_size",
        "caption_size",
        "small_size",
        "title_size",
        "menu_size",
        "icon_label_size",
        "logon_name_size",
    ] {
        let value = font
            .get(key)
            .unwrap_or_else(|| panic!("missing font.{key}"))
            .as_integer()
            .unwrap_or_else(|| panic!("font.{key} is not an integer"));
        assert!(
            (6..=72).contains(&value),
            "font.{key} = {value} pt is outside the plausible 6..72 pt range"
        );
    }
    assert_eq!(font.get("ui_size").unwrap().as_integer(), Some(9));
}

#[test]
fn font_fallback_chain_lists_a_cjk_family() {
    let metrics = load_metrics();
    let families = table(&metrics, "font.fallback")
        .get("families")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(
        !families.is_empty(),
        "font.fallback.families must not be empty"
    );
    let cjk = table(&metrics, "font.fallback")
        .get("cjk_family")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(
        families.iter().any(|f| f.as_str() == Some(cjk)),
        "the CJK fallback family {cjk} must also appear in the fallback chain"
    );
}

#[test]
fn animation_durations_are_positive_and_bounded() {
    let metrics = load_metrics();
    for (key, value) in table(&metrics, "animation") {
        let ms = value
            .as_integer()
            .unwrap_or_else(|| panic!("animation.{key}"));
        assert!(
            (1..=2000).contains(&ms),
            "animation.{key} = {ms} ms is outside the plausible 1..=2000 ms range"
        );
    }
}

#[test]
fn no_microsoft_font_is_referenced_as_an_asset() {
    // The `ui_family` key names the Win7 metric reference face. It is
    // documentation only; the shipped faces are listed in
    // docs/desktop/theme-license.md. Guard against someone turning it
    // into an actual dependency.
    let metrics = load_metrics();
    let families = table(&metrics, "font.fallback")
        .get("families")
        .unwrap()
        .as_array()
        .unwrap();
    for family in families {
        let name = family.as_str().unwrap_or_default().to_ascii_lowercase();
        assert!(
            !name.contains("segoe") && !name.contains("tahoma") && !name.contains("arial"),
            "font.fallback.families must not reference a proprietary Microsoft face: {name}"
        );
    }
}

use nt61_win32_metrics::{sm, Display};
use serde_json::Value;
const INDICES: &[(i32, &str)] = &[
    (0, "SM_CXSCREEN"),
    (1, "SM_CYSCREEN"),
    (2, "SM_CXVSCROLL"),
    (3, "SM_CYHSCROLL"),
    (4, "SM_CYCAPTION"),
    (5, "SM_CXBORDER"),
    (6, "SM_CYBORDER"),
    (7, "SM_CXDLGFRAME"),
    (8, "SM_CYDLGFRAME"),
    (9, "SM_CYVTHUMB"),
    (10, "SM_CXHTHUMB"),
    (11, "SM_CXICON"),
    (12, "SM_CYICON"),
    (13, "SM_CXCURSOR"),
    (14, "SM_CYCURSOR"),
    (15, "SM_CYMENU"),
    (16, "SM_CXFULLSCREEN"),
    (17, "SM_CYFULLSCREEN"),
    (18, "SM_CYKANJIWINDOW"),
    (19, "SM_MOUSEPRESENT"),
    (20, "SM_CYVSCROLL"),
    (21, "SM_CXHSCROLL"),
    (22, "SM_DEBUG"),
    (23, "SM_SWAPBUTTON"),
    (28, "SM_CXMIN"),
    (29, "SM_CYMIN"),
    (30, "SM_CXSIZE"),
    (31, "SM_CYSIZE"),
    (32, "SM_CXFRAME"),
    (33, "SM_CYFRAME"),
    (34, "SM_CXMINTRACK"),
    (35, "SM_CYMINTRACK"),
    (36, "SM_CXDOUBLECLK"),
    (37, "SM_CYDOUBLECLK"),
    (38, "SM_CXICONSPACING"),
    (39, "SM_CYICONSPACING"),
    (49, "SM_CXSMICON"),
    (50, "SM_CYSMICON"),
    (51, "SM_CYSMCAPTION"),
    (52, "SM_CXSMSIZE"),
    (53, "SM_CYSMSIZE"),
    (54, "SM_CXMENUSIZE"),
    (55, "SM_CYMENUSIZE"),
    (59, "SM_CXMAXTRACK"),
    (60, "SM_CYMAXTRACK"),
    (61, "SM_CXMAXIMIZED"),
    (62, "SM_CYMAXIMIZED"),
    (68, "SM_CXDRAG"),
    (69, "SM_CYDRAG"),
    (83, "SM_CXFOCUSBORDER"),
    (84, "SM_CYFOCUSBORDER"),
    (92, "SM_CXPADDEDBORDER"),
];
#[test]
fn all_captured_basic_96_metrics_match_original_sp1() {
    let capture: Value = serde_json::from_str(include_str!(
        "../../../docs/desktop/baselines/win7-sp1-window-placement/metrics.json"
    ))
    .unwrap();
    assert_eq!(capture["metrics"].as_object().unwrap().len(), INDICES.len());
    let display = Display {
        width: 800,
        height: 600,
        work_width: 800,
        work_height: 560,
        max_track_padding: 20,
    };
    for &(index, name) in INDICES {
        assert_eq!(
            display.basic_96_metric(index) as i64,
            capture["metrics"][name].as_i64().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn default_colorrefs_match_original_basic_palette_without_channel_swapping() {
    let capture: Value = serde_json::from_str(include_str!(
        "../../../docs/desktop/baselines/win7-sp1-window-placement/metrics.json"
    ))
    .unwrap();
    for (index, color) in nt61_win32_metrics::BASIC_96_COLORS.iter().enumerate() {
        assert_eq!(
            *color as u64,
            capture["syscolors_colorref"][index.to_string()]
                .as_u64()
                .unwrap()
        );
    }
}
#[test]
fn invalid_indices_and_bounded_geometry() {
    let display = Display {
        width: i32::MAX,
        height: i32::MAX,
        work_width: i32::MAX,
        work_height: i32::MAX,
        max_track_padding: 20,
    };
    for index in [-1, i32::MIN, i32::MAX, 24, 96, 97] {
        assert_eq!(display.basic_96_metric(index), 0);
    }
    assert_eq!(display.basic_96_metric(sm::SM_CXMAXIMIZED), i32::MAX);
    assert_eq!(display.basic_96_metric(sm::SM_CYMAXTRACK), i32::MAX);
    let empty = Display {
        width: -1,
        height: -5,
        work_width: 800,
        work_height: 600,
        max_track_padding: 20,
    };
    assert_eq!(empty.basic_96_metric(sm::SM_CXSCREEN), 0);
    assert_eq!(empty.basic_96_metric(sm::SM_CYFULLSCREEN), 0);
}

#[test]
fn original_mode_changes_and_custom_work_area_match_all_captured_metrics() {
    let captures: Value = serde_json::from_str(include_str!(
        "../../../docs/desktop/baselines/win7-sp1-window-placement/metrics-geometry.json"
    ))
    .unwrap();
    for capture in captures.as_array().unwrap() {
        let metrics = &capture["metrics"];
        let display = Display {
            width: metrics["SM_CXSCREEN"].as_i64().unwrap() as i32,
            height: metrics["SM_CYSCREEN"].as_i64().unwrap() as i32,
            work_width: capture["work_area"][2].as_i64().unwrap() as i32,
            work_height: capture["work_area"][3].as_i64().unwrap() as i32,
            max_track_padding: 12,
        };
        for &(index, name) in INDICES {
            assert_eq!(
                display.basic_96_metric(index) as i64,
                metrics[name].as_i64().unwrap(),
                "{name} {display:?}"
            );
        }
        for index in 76..=81 {
            assert_eq!(
                display.basic_96_metric(index) as i64,
                metrics[format!("SM_{index}")].as_i64().unwrap()
            );
        }
    }
}

#[test]
fn session_geometry_initialization_and_mode_reversion_keep_original_limits() {
    let mut session = nt61_win32_metrics::SessionMetrics::default();
    session.observe_display(0, 0);
    session.observe_display(800, 600);
    assert_eq!(session.max_track_padding(), 20);
    session.observe_display(800, 600);
    assert_eq!(session.max_track_padding(), 20);
    session.observe_display(0, 768);
    assert_eq!(session.max_track_padding(), 20);
    session.observe_display(1024, 768);
    assert_eq!(session.max_track_padding(), 12);
    session.observe_display(800, 600);
    assert_eq!(session.max_track_padding(), 12);
}

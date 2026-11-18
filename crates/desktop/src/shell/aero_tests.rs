use super::*;

fn reference_shell() -> ShellModel {
    let mut shell = ShellModel::new(Theme::aero());
    shell.refresh_controls(800, 600);
    shell.open_window("SP1 placement", 100, 100, 400, 300);
    shell
}

#[test]
fn windows_arrow_shortcuts_follow_sp1_single_monitor_cycle() {
    use nt61_abi::session_bridge as key;
    for (direction, cycle) in [
        (key::WINDOW_LEFT, [Rect::new(0, 0, 400, 560), Rect::new(400, 0, 400, 560)]),
        (key::WINDOW_RIGHT, [Rect::new(400, 0, 400, 560), Rect::new(0, 0, 400, 560)]),
    ] {
        let mut shell = reference_shell();
        for expected in [cycle[0], cycle[1], Rect::new(100, 100, 400, 300)] {
            shell.session_hotkeys(direction);
            assert_eq!(shell.taskbar.windows[0].bounds(), expected);
        }
    }
}

#[test]
fn opposite_arrow_and_down_restore_the_original_placement() {
    use nt61_abi::session_bridge as key;
    for (first, second) in [(key::WINDOW_LEFT, key::WINDOW_RIGHT),
        (key::WINDOW_RIGHT, key::WINDOW_LEFT), (key::WINDOW_LEFT, key::WINDOW_DOWN)] {
        let mut shell = reference_shell();
        shell.session_hotkeys(first);
        assert_ne!(shell.taskbar.windows[0].bounds(), Rect::new(100, 100, 400, 300));
        shell.session_hotkeys(second);
        assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(100, 100, 400, 300));
        assert!(!shell.taskbar.windows[0].minimized);
    }
}

#[test]
fn snap_maximize_restore_minimize_preserves_the_normal_rectangle() {
    use nt61_abi::session_bridge as key;
    let mut shell = reference_shell();
    shell.session_hotkeys(key::WINDOW_LEFT);
    shell.session_hotkeys(key::WINDOW_UP);
    assert!(shell.taskbar.windows[0].maximized);
    assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(-8, -8, 816, 576));
    assert_eq!(shell.taskbar.windows[0].restore_geometry, Some((100, 100, 400, 300)));
    shell.session_hotkeys(key::WINDOW_DOWN);
    assert!(!shell.taskbar.windows[0].maximized);
    assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(100, 100, 400, 300));
    shell.session_hotkeys(key::WINDOW_DOWN);
    assert!(shell.taskbar.windows[0].minimized);
    assert_eq!(shell.selected_window, None);
    shell.session_hotkeys(key::WINDOW_UP);
    assert!(shell.taskbar.windows[0].minimized);
}

#[test]
fn dragging_to_an_edge_commits_only_on_release_and_preserves_normal_placement() {
    for (edge, expected) in [(0, Rect::new(0, 0, 400, 560)),
        (799, Rect::new(400, 0, 400, 560))] {
        let mut shell = reference_shell();
        assert!(shell.begin_window_drag(1, 200, 114));
        shell.drag_window_to(edge, 200);
        assert_eq!(shell.taskbar.windows[0].snapped, None);
        shell.release_window_drag();
        assert_eq!(shell.taskbar.windows[0].bounds(), expected);
        shell.window_shortcut(WindowShortcut::Down);
        assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(100, 100, 400, 300));
    }
}

#[test]
fn dragging_out_of_snap_restores_size_and_updates_the_normal_placement() {
    let mut shell = reference_shell();
    shell.snap_window(1, 800, SnapSide::Left);
    assert!(shell.begin_window_drag(1, 200, 14));
    assert!(!shell.drag_window_to(200, 14));
    assert_eq!(shell.taskbar.windows[0].snapped, Some(SnapSide::Left));
    shell.drag_window_to(300, 200);
    shell.release_window_drag();
    assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(100, 186, 400, 300));
    assert_eq!(shell.taskbar.windows[0].snapped, None);
    shell.snap_window(1, 800, SnapSide::Right);
    shell.window_shortcut(WindowShortcut::Down);
    assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(100, 186, 400, 300));
}

#[test]
fn resizing_at_an_edge_does_not_trigger_caption_snap() {
    let mut shell = reference_shell();
    assert!(shell.begin_window_drag(1, 100, 200));
    shell.drag_window_to(0, 200);
    shell.release_window_drag();
    assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(0, 100, 500, 300));
    assert_eq!(shell.taskbar.windows[0].snapped, None);
}

#[test]
fn window_shortcuts_cannot_move_windows_on_a_secure_screen() {
    use nt61_abi::session_bridge as key;
    let mut shell = reference_shell();
    shell.session.show(crate::session::Screen::Security);
    let before = shell.taskbar.windows.clone();
    for action in [key::WINDOW_LEFT, key::WINDOW_RIGHT, key::WINDOW_UP, key::WINDOW_DOWN] {
        shell.session_hotkeys(action);
        assert_eq!(shell.taskbar.windows, before);
    }
}

#[test]
fn snap_uses_the_display_height_and_covers_odd_widths() {
    for (width, height) in [(1024, 768), (1280, 720), (1280, 1024), (1025, 769)] {
        let mut shell = ShellModel::new(Theme::aero());
        shell.refresh_controls(width, height);
        let id = shell.open_window("Snap", 100, 100, 400, 300);
        shell.snap_window(id, width, SnapSide::Right);
        let window = &shell.taskbar.windows[0];
        assert_eq!(window.bounds(), Rect::new((width / 2) as i32, 0,
            width - width / 2, height - TASKBAR_HEIGHT));
    }
}

#[test]
fn maximized_caption_drag_matches_sp1_first_movement_anchor() {
    // Original maxdrag.json: 400px normal window, 800px screen; all three
    // input granularities were captured independently in original Notepad.
    for (steps, expected_xs) in [(1, [100, 100, 100, 100, 100, 100]),
        (4, [202, 165, 105, 100, 100, 100]),
        (10, [244, 199, 127, 100, 100, 84])] {
        for (start_x, expected_x) in [20, 70, 150, 200, 600, 660].into_iter().zip(expected_xs) {
            let mut shell = reference_shell();
            shell.window_shortcut(WindowShortcut::Up);
            assert!(shell.begin_window_drag(1, start_x, 14));
            for step in 1..=steps {
                shell.drag_window_to(start_x + (300 - start_x) * step / steps,
                    14 + 186 * step / steps);
            }
            shell.release_window_drag();
            assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(expected_x, 178, 400, 300),
                "steps={steps}, anchor={start_x}");
            assert!(!shell.taskbar.windows[0].maximized);
            shell.window_shortcut(WindowShortcut::Up);
            shell.window_shortcut(WindowShortcut::Down);
            assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(expected_x, 178, 400, 300));
        }
    }
    for (steps, expected_xs) in [(1, [60, 60, 60, 60, 60, 60]),
        (4, [202, 165, 105, 67, 60, 58]), (10, [244, 199, 127, 82, 58, 4])] {
        for (start_x, expected_x) in [20, 70, 150, 200, 600, 660].into_iter().zip(expected_xs) {
            let mut shell = reference_shell();
            shell.taskbar.windows[0].width = 480;
            shell.window_shortcut(WindowShortcut::Up);
            assert!(shell.begin_window_drag(1, start_x, 14));
            for step in 1..=steps {
                shell.drag_window_to(start_x + (300 - start_x) * step / steps,
                    14 + 186 * step / steps);
            }
            shell.release_window_drag();
            assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(expected_x, 178, 480, 300),
                "width=480, steps={steps}, anchor={start_x}");
        }
    }
}

#[test]
fn dragging_to_top_maximizes_and_restores_original_rectangle() {
    let mut shell = reference_shell();
    assert!(shell.begin_window_drag(1, 200, 114));
    shell.drag_window_to(300, 0);
    shell.release_window_drag();
    assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(-8, -8, 816, 576));
    shell.window_shortcut(WindowShortcut::Down);
    assert_eq!(shell.taskbar.windows[0].bounds(), Rect::new(100, 100, 400, 300));
}

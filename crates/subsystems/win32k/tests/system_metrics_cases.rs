//! Exercise native USER32 and the production metric handler after scanout changes.
use super::*;

#[test]
fn native_metrics_follow_active_scanout_and_successful_mode_transition() {
    use hal::common::framebuffer::TEST_DISPLAY;
    struct Restore;
    impl Drop for Restore { fn drop(&mut self) { TEST_DISPLAY.with(|display| display.set((0,0))); } }
    let _restore = Restore;
    TEST_DISPLAY.with(|display| display.set((800,600)));
    for (index,expected) in [(0,800),(1,600),(17,538),(59,820),(60,620),(61,816),(62,576),(68,4),(-1,0)] {
        assert_eq!(user32_input::get_system_metrics(index),expected,"index={index}");
    }
    // The test never creates or resizes a GDI primary surface: metrics must
    // consult the active scanout rather than retain a stale bitmap dimension.
    TEST_DISPLAY.with(|display| display.set((1024,768)));
    nt61_win32_metrics::record_display_mode_change();
    for (index,expected) in [(0,1024),(1,768),(17,706),(59,1036),(60,780),(61,1040),(62,744)] {
        assert_eq!(user32_input::get_system_metrics(index),expected,"index={index}");
    }
    TEST_DISPLAY.with(|display| display.set((800,600)));
    assert_eq!(user32_input::get_system_metrics(59),812);
    assert_eq!(user32_input::get_system_metrics(60),612);
}

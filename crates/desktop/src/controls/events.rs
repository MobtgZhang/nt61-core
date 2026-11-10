#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlEvent {
    HoverChanged(u32),
    Pressed(u32),
    Activated(u32),
    FocusChanged(u32),
    TextChanged(u32),
    Submit,
    Cancel,
    ContextMenu,
}

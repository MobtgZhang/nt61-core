#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Panel,
    Image,
    Label,
    Button,
    Edit,
    List,
    Tree,
    Progress,
    Toolbar,
    Status,
    CheckBox,
    ComboBox,
    ListView,
    RichEdit,
    Tabs,
    TrackBar,
    UpDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlVisualState {
    Normal,
    Hot,
    Pressed,
    Disabled,
    Focused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerAction {
    Move,
    Down,
    Up,
    Cancel,
}

pub const CONTROL_NONE: u32 = 0;

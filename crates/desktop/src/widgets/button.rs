use crate::controls::ControlVisualState;
use alloc::string::String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub id: u32,
    pub text: String,
    pub enabled: bool,
    pub state: ControlVisualState,
}

impl Button {
    pub fn new(id: u32, text: &str) -> Self {
        Self {
            id,
            text: String::from(text),
            enabled: true,
            state: ControlVisualState::Normal,
        }
    }

    pub fn set_state(&mut self, state: ControlVisualState) {
        if self.enabled {
            self.state = state;
        }
    }
}

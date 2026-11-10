use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckBoxState {
    Unchecked,
    Checked,
    Indeterminate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckBox {
    pub id: u32,
    pub text: String,
    pub state: CheckBoxState,
    pub visible: bool,
    pub enabled: bool,
}

impl CheckBox {
    pub fn new(id: u32, text: &str) -> Self {
        Self {
            id,
            text: String::from(text),
            state: CheckBoxState::Unchecked,
            visible: true,
            enabled: true,
        }
    }

    pub fn toggle(&mut self) {
        if self.enabled {
            self.state = match self.state {
                CheckBoxState::Unchecked => CheckBoxState::Checked,
                CheckBoxState::Checked => CheckBoxState::Unchecked,
                CheckBoxState::Indeterminate => CheckBoxState::Unchecked,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_box_toggles_between_states() {
        let mut check_box = CheckBox::new(1, "Use Aero glass");
        assert_eq!(check_box.state, CheckBoxState::Unchecked);
        check_box.toggle();
        assert_eq!(check_box.state, CheckBoxState::Checked);
        check_box.toggle();
        assert_eq!(check_box.state, CheckBoxState::Unchecked);
    }
}

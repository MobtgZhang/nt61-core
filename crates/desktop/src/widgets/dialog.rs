use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogResult {
    Ok,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub id: u32,
    pub title: String,
    pub visible: bool,
    pub modal: bool,
    pub result: Option<DialogResult>,
    pub buttons: Vec<DialogResult>,
}

impl Dialog {
    pub fn ok_cancel(id: u32, title: &str) -> Self {
        Self {
            id,
            title: String::from(title),
            visible: true,
            modal: true,
            result: None,
            buttons: alloc::vec![DialogResult::Ok, DialogResult::Cancel],
        }
    }

    pub fn finish(&mut self, result: DialogResult) -> bool {
        if !self.buttons.contains(&result) {
            return false;
        }
        self.result = Some(result);
        self.visible = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_dialog_accepts_only_offered_buttons() {
        let mut dialog = Dialog::ok_cancel(1, "Delete");
        assert!(dialog.modal);
        assert!(dialog.finish(DialogResult::Ok));
        assert_eq!(dialog.result, Some(DialogResult::Ok));
        assert!(!dialog.visible);

        let mut dialog = Dialog::ok_cancel(2, "Save");
        assert!(dialog.finish(DialogResult::Cancel));
        assert_eq!(dialog.result, Some(DialogResult::Cancel));
    }
}

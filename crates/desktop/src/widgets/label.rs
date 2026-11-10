use alloc::string::String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub id: u32,
    pub text: String,
    pub visible: bool,
}

impl Label {
    pub fn new(id: u32, text: &str) -> Self {
        Self {
            id,
            text: String::from(text),
            visible: true,
        }
    }
}

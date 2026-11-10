use super::types::ControlKind;
use crate::graphics::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlLayout {
    pub id: u32,
    pub kind: ControlKind,
    pub rect: Rect,
    pub visible: bool,
    pub enabled: bool,
}

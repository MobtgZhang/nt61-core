//! Renderer-agnostic control tree and Win32-compatible control model.
//!
//! The tree owns geometry, focus and event routing. Renderers map control
//! IDs to visual widgets, so applications do not need to duplicate hit
//! testing or keyboard focus logic.

mod common;
mod events;
mod layout;
mod list_view;
mod toolbar;
mod tree;
mod tree_view;
mod types;
mod v6;

pub use events::ControlEvent;
pub use layout::ControlLayout;
pub use tree::ControlTree;
pub use types::{ControlKind, ControlVisualState, PointerAction, CONTROL_NONE};
pub use v6::{CommonControl, Control, ControlNotification, ControlWindow};

pub use common::{CommonKey, CommonNotification, Modifiers};
pub use list_view::{ListColumn, ListRow, ListView};
pub use toolbar::{Toolbar, ToolbarButton, ToolbarKind};
pub use tree_view::{TreeItem, TreeView};

//! Logical desktop icon identities.
//!
//! Physical payloads are generated into the build output by
//! `nt61-desktop`'s `resource-gen` binary; this module deliberately
//! contains no generated PE/theme bytes in the source tree.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageResource {
    Computer,
    Folder,
    Document,
    Notepad,
    Cmd,
    RecycleBin,
    Network,
    ControlPanel,
    WindowClose,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
}

impl ImageResource {
    pub const ALL: [ImageResource; 12] = [
        Self::Computer,
        Self::Folder,
        Self::Document,
        Self::Notepad,
        Self::Cmd,
        Self::RecycleBin,
        Self::Network,
        Self::ControlPanel,
        Self::WindowClose,
        Self::WindowMinimize,
        Self::WindowMaximize,
        Self::WindowRestore,
    ];

    pub const fn resource_id(self) -> u16 {
        match self {
            Self::Computer => 1,
            Self::Folder => 2,
            Self::Document => 3,
            Self::Notepad => 4,
            Self::Cmd => 5,
            Self::RecycleBin => 6,
            Self::Network => 7,
            Self::ControlPanel => 8,
            Self::WindowClose => 9,
            Self::WindowMinimize => 10,
            Self::WindowMaximize => 11,
            Self::WindowRestore => 12,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_ids_are_unique() {
        let mut ids = ImageResource::ALL
            .iter()
            .map(|resource| resource.resource_id());
        let first = ids.next().unwrap();
        assert_eq!(ids.collect::<alloc::vec::Vec<_>>().len(), 11);
        assert_ne!(first, 0);
    }
}

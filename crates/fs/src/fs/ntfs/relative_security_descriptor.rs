use alloc::vec::Vec;

pub(crate) const HEADER_SIZE: usize = 20;

pub(crate) const COMPONENTS: [(u32, u16); 4] =
    [(0x1, 0x0001), (0x2, 0x0002), (0x8, 0x2a30), (0x4, 0x15cc)];

const SELF_RELATIVE: u16 = 0x8000;
pub(crate) const MAXIMUM_SIZE: usize = 131_072;

#[derive(Clone, Copy)]
pub(crate) struct RelativeDescriptor<'a> {
    pub(crate) resource_manager_control: u8,
    pub(crate) control: u16,
    pub(crate) components: [Option<&'a [u8]>; 4],
}

impl<'a> RelativeDescriptor<'a> {
    pub(crate) fn parse(descriptor: &'a [u8]) -> Option<Self> {
        let header = descriptor.get(..HEADER_SIZE)?;
        let control = u16::from_le_bytes([header[2], header[3]]);
        if header[0] != 1 || control & SELF_RELATIVE == 0 || descriptor.len() > MAXIMUM_SIZE {
            return None;
        }
        let mut components = [None; 4];
        for (index, component) in components.iter_mut().enumerate() {
            let field = 4 + index * 4;
            let offset = u32::from_le_bytes(header[field..field + 4].try_into().ok()?) as usize;
            if offset == 0 {
                continue;
            }
            if offset < HEADER_SIZE || !offset.is_multiple_of(4) {
                return None;
            }
            let tail = descriptor.get(offset..)?;
            *component = Some(if index < 2 {
                tail.get(..sid_length(tail)?)?
            } else {
                let present = if index == 2 { 0x10 } else { 4 };
                if control & present == 0 {
                    return None;
                }
                let acl = tail.get(..acl_length(tail)?)?;
                validate_acl(acl)?;
                acl
            });
        }
        Some(Self {
            resource_manager_control: header[1],
            control,
            components,
        })
    }

    pub(crate) fn select(self, information: u32) -> Self {
        let mut selected = Self {
            resource_manager_control: 0,
            control: SELF_RELATIVE,
            components: [None; 4],
        };
        for (index, (field, control)) in COMPONENTS.iter().copied().enumerate() {
            if information & field != 0 {
                selected.components[index] = self.components[index];
                selected.control |= self.control & control;
            }
        }
        selected
    }

    pub(crate) fn validate_discretionary_acl(&self) -> Option<()> {
        let Some(acl) = self.components[3] else {
            return Some(());
        };
        let mut entries = acl.get(8..)?;
        for _ in 0..u16::from_le_bytes([acl[4], acl[5]]) {
            let header = entries.get(..4)?;
            let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
            let entry = entries.get(..length)?;
            let body = entry.get(8..)?;
            if !matches!(header[0], 0 | 1) || sid_length(body)? != body.len() {
                return None;
            }
            entries = entries.get(length..)?;
        }
        Some(())
    }

    pub(crate) fn merge(mut self, replacement: Self, information: u32) -> Self {
        for (index, (field, control)) in COMPONENTS.iter().copied().enumerate() {
            if information & field != 0 {
                self.components[index] = replacement.components[index];
                self.control = (self.control & !control) | (replacement.control & control);
            }
        }
        self
    }

    pub(crate) fn encode(self) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        let size = HEADER_SIZE
            + self
                .components
                .iter()
                .flatten()
                .map(|value| value.len())
                .sum::<usize>();
        if size > MAXIMUM_SIZE {
            return None;
        }
        bytes.try_reserve_exact(size).ok()?;
        bytes.resize(HEADER_SIZE, 0);
        bytes[0] = 1;
        bytes[1] = self.resource_manager_control;
        bytes[2..4].copy_from_slice(&(self.control | SELF_RELATIVE).to_le_bytes());
        for (index, component) in self.components.iter().enumerate() {
            if let Some(component) = component {
                let offset = bytes.len() as u32;
                bytes[4 + index * 4..8 + index * 4].copy_from_slice(&offset.to_le_bytes());
                bytes.extend_from_slice(component);
            }
        }
        Some(bytes)
    }
}

pub(crate) fn capture(
    address: u64,
    pointer_width: usize,
    mut read: impl FnMut(&mut [u8], u64) -> Result<(), i32>,
    invalid_descriptor: i32,
    no_memory: i32,
) -> Result<Vec<u8>, i32> {
    if !matches!(pointer_width, 4 | 8) {
        return Err(invalid_descriptor);
    }
    let mut prefix = [0; 4];
    read(&mut prefix, address)?;
    let relative = prefix[3] & 0x80 != 0;
    let fields = if relative { 4 } else { pointer_width };
    let width = if relative { 4 } else { pointer_width };
    let mut header = [0; 40];
    read(&mut header[..fields + 4 * width], address)?;
    if header[0] != 1 || header[..4] != prefix {
        return Err(invalid_descriptor);
    }
    let control = u16::from_le_bytes([header[2], header[3]]);
    let mut parts = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut total = HEADER_SIZE;
    for (index, part) in parts.iter_mut().enumerate() {
        if index == 2 && control & 0x10 == 0 || index == 3 && control & 4 == 0 {
            continue;
        }
        let field = fields + index * width;
        let mut value = [0; 8];
        value[..width].copy_from_slice(&header[field..field + width]);
        let value = u64::from_le_bytes(value);
        if value == 0 {
            continue;
        }
        let location = if relative {
            if value < HEADER_SIZE as u64 || value % 4 != 0 || value > MAXIMUM_SIZE as u64 - 8 {
                return Err(invalid_descriptor);
            }
            address.checked_add(value).ok_or(invalid_descriptor)?
        } else {
            value
        };
        let mut prefix = [0; 8];
        read(&mut prefix, location)?;
        let length = if index < 2 { sid_length(&prefix) } else { acl_length(&prefix) }
            .ok_or(invalid_descriptor)?;
        total += length;
        if total > MAXIMUM_SIZE || relative && value + length as u64 > MAXIMUM_SIZE as u64 {
            return Err(invalid_descriptor);
        }
        part.try_reserve_exact(length).map_err(|_| no_memory)?;
        part.resize(length, 0);
        read(part, location)?;
        if part[..8] != prefix {
            return Err(invalid_descriptor);
        }
    }
    let descriptor = RelativeDescriptor {
        resource_manager_control: header[1],
        control,
        components: core::array::from_fn(|index| {
            let part = &parts[index];
            (!part.is_empty()).then_some(part.as_slice())
        }),
    };
    let bytes = descriptor.encode().ok_or(no_memory)?;
    RelativeDescriptor::parse(&bytes).ok_or(invalid_descriptor)?;
    Ok(bytes)
}

pub(crate) fn sid_length(header: &[u8]) -> Option<usize> {
    let header = header.get(..8)?;
    (header[0] == 1 && header[1] <= 15).then(|| 8 + usize::from(header[1]) * 4)
}

pub(crate) fn acl_length(header: &[u8]) -> Option<usize> {
    let header = header.get(..8)?;
    let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
    (matches!(header[0], 2..=4) && length >= 8 && length.is_multiple_of(4)).then_some(length)
}

fn validate_acl(acl: &[u8]) -> Option<()> {
    let mut entries = acl.get(8..)?;
    for _ in 0..u16::from_le_bytes([acl[4], acl[5]]) {
        let ace = entries.get(..4)?;
        let length = usize::from(u16::from_le_bytes([ace[2], ace[3]]));
        if length < 4 || !length.is_multiple_of(4) {
            return None;
        }
        entries = entries.get(length..)?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
    const EMPTY_ACL: &[u8] = &[2, 0, 8, 0, 0, 0, 0, 0];

    #[test]
    fn preserves_unselected_fields_and_distinguishes_empty_from_null_acl() {
        let original = RelativeDescriptor {
            resource_manager_control: 42,
            control: 0xe016,
            components: [Some(OWNER), Some(OWNER), Some(EMPTY_ACL), Some(EMPTY_ACL)],
        };
        let null_dacl = RelativeDescriptor {
            resource_manager_control: 0,
            control: 0x9004,
            components: [None; 4],
        };
        let bytes = original.merge(null_dacl, 4).encode().unwrap();
        let merged = RelativeDescriptor::parse(&bytes).unwrap();
        assert_eq!(merged.resource_manager_control, 42);
        assert_eq!(merged.control, 0xf016);
        assert_eq!(
            merged.components,
            [Some(OWNER), Some(OWNER), Some(EMPTY_ACL), None]
        );
        let projected = merged.select(4).encode().unwrap();
        assert_eq!(&projected[2..4], &[4, 0x90]);
        assert_eq!(projected.len(), HEADER_SIZE);
        let restored = merged.merge(original, 4).select(4).encode().unwrap();
        assert_eq!(&restored[HEADER_SIZE..], EMPTY_ACL);
    }

    #[test]
    fn rejects_truncated_and_out_of_bounds_objects() {
        let original = RelativeDescriptor {
            resource_manager_control: 0,
            control: 0x8004,
            components: [Some(OWNER), None, None, Some(EMPTY_ACL)],
        };
        let bytes = original.encode().unwrap();
        for length in 0..bytes.len() {
            assert!(RelativeDescriptor::parse(&bytes[..length]).is_none());
        }
        for offset in [16u32, 21, u32::MAX - 3] {
            let mut invalid = bytes.clone();
            invalid[4..8].copy_from_slice(&offset.to_le_bytes());
            assert!(RelativeDescriptor::parse(&invalid).is_none());
        }
        let mut invalid = bytes;
        invalid[HEADER_SIZE + OWNER.len() + 4] = 1;
        assert!(RelativeDescriptor::parse(&invalid).is_none());
    }
}

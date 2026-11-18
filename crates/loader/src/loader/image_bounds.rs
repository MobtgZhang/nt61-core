//! Separate virtual reservation limits from the initialized image allocation.
const MAX_VIRTUAL_IMAGE: usize = 512 * 1024 * 1024;
const MAX_INITIALIZED_IMAGE: usize = 128 * 1024 * 1024;

pub(super) fn initialized_extent(
    image_size: usize,
    headers: usize,
    sections: impl IntoIterator<Item = (usize, usize, usize)>,
) -> Option<usize> {
    if image_size == 0 || image_size > MAX_VIRTUAL_IMAGE || headers > image_size {
        return None;
    }
    let mut extent = headers;
    for (address, virtual_size, raw_size) in sections {
        if address.checked_add(virtual_size.max(raw_size))? > image_size {
            return None;
        }
        if raw_size != 0 {
            extent = extent.max(address.checked_add(raw_size)?);
        }
    }
    (extent <= MAX_INITIALIZED_IMAGE).then_some(extent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_zero_fill_reserves_virtual_space_without_allocating_it() {
        assert_eq!(
            initialized_extent(
                144 << 20,
                4096,
                [
                    (4096, 16 << 20, 16 << 20),
                    ((16 << 20) + 4096, 112 << 20, 0),
                ]
            ),
            Some((16 << 20) + 4096)
        );
    }

    #[test]
    fn malformed_and_excessive_layouts_are_rejected_before_allocation() {
        assert_eq!(initialized_extent(usize::MAX, 4096, []), None);
        assert_eq!(initialized_extent(4096, 8192, []), None);
        assert_eq!(
            initialized_extent(144 << 20, 4096, [(4096, 144 << 20, 0)]),
            None
        );
        assert_eq!(
            initialized_extent(144 << 20, 4096, [(4096, 0, 129 << 20)]),
            None
        );
        assert_eq!(initialized_extent(4096, 512, [(usize::MAX, 1, 0)]), None);
        assert_eq!(initialized_extent(4096, 512, [(0, usize::MAX, 1)]), None);
    }
}

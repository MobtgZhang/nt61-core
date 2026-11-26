pub(super) fn read(
    attribute: &[u8],
    cluster_size: u64,
    partition_start: u64,
    offset: u64,
    output: &mut [u8],
    mut read_sector: impl FnMut(u64, &mut [u8; 512]) -> bool,
) -> Option<()> {
    visit(
        attribute,
        cluster_size,
        partition_start,
        offset,
        output.len(),
        false,
        |physical, sector_offset, range| {
            let mut sector = [0; 512];
            if !read_sector(physical, &mut sector) {
                return None;
            }
            let end = sector_offset + range.len();
            output[range].copy_from_slice(&sector[sector_offset..end]);
            Some(())
        },
    )
}

pub(super) fn stage_write(
    attribute: &[u8],
    cluster_size: u64,
    partition_start: u64,
    offset: u64,
    input: &[u8],
    sectors: &mut alloc::collections::BTreeMap<u64, [u8; 512]>,
    mut read_sector: impl FnMut(u64, &mut [u8; 512]) -> bool,
) -> Option<()> {
    visit(
        attribute,
        cluster_size,
        partition_start,
        offset,
        input.len(),
        true,
        |physical, sector_offset, range| {
            use alloc::collections::btree_map::Entry;
            let sector = match sectors.entry(physical) {
                Entry::Occupied(found) => found.into_mut(),
                Entry::Vacant(slot) => {
                    let mut sector = [0; 512];
                    if !read_sector(physical, &mut sector) {
                        return None;
                    }
                    slot.insert(sector)
                }
            };
            let end = sector_offset + range.len();
            sector[sector_offset..end].copy_from_slice(&input[range]);
            Some(())
        },
    )
}

fn visit(
    attribute: &[u8],
    cluster_size: u64,
    partition_start: u64,
    offset: u64,
    length: usize,
    allow_uninitialized: bool,
    mut sector_chunk: impl FnMut(u64, usize, core::ops::Range<usize>) -> Option<()>,
) -> Option<()> {
    let sectors_per_cluster = cluster_size / 512;
    if !cluster_size.is_power_of_two()
        || sectors_per_cluster == 0
        || attribute.len() < 64
        || attribute[8] != 1
        || attribute[12..14] != [0, 0]
        || attribute[16..24] != [0; 8]
        || attribute[34..36] != [0; 2]
    {
        return None;
    }
    let runs_offset = u16::from_le_bytes(attribute[32..34].try_into().ok()?) as usize;
    let name_offset = u16::from_le_bytes(attribute[10..12].try_into().ok()?) as usize;
    if runs_offset < 64
        || (attribute[9] != 0
            && (name_offset < 64 || name_offset + 2 * usize::from(attribute[9]) > runs_offset))
    {
        return None;
    }
    let clusters = u64::from_le_bytes(attribute[24..32].try_into().ok()?).checked_add(1)?;
    let allocated = u64::from_le_bytes(attribute[40..48].try_into().ok()?);
    let size = u64::from_le_bytes(attribute[48..56].try_into().ok()?);
    let initialized = u64::from_le_bytes(attribute[56..64].try_into().ok()?);
    let end = offset.checked_add(length as u64)?;
    if allocated != clusters.checked_mul(cluster_size)?
        || size > allocated
        || initialized > size
        || end
            > if allow_uninitialized {
                allocated
            } else {
                initialized
            }
    {
        return None;
    }

    let mut mapping = attribute.get(runs_offset..)?;
    let mut next_vcn = 0u64;
    let mut lcn = 0u64;
    let mut copied = 0usize;
    loop {
        let (&sizes, rest) = mapping.split_first()?;
        if sizes == 0 {
            return (next_vcn == clusters && copied == length).then_some(());
        }
        let len_width = usize::from(sizes & 15);
        let lcn_width = usize::from(sizes >> 4);
        if !(1..=8).contains(&len_width) || !(1..=8).contains(&lcn_width) {
            return None;
        }
        let length_bytes = rest.get(..len_width)?;
        let delta_bytes = rest.get(len_width..len_width + lcn_width)?;
        mapping = &rest[len_width + lcn_width..];
        let mut length_word = [0; 8];
        length_word[..len_width].copy_from_slice(length_bytes);
        let length = u64::from_le_bytes(length_word);
        let mut delta_word = [if delta_bytes[lcn_width - 1] & 0x80 == 0 {
            0
        } else {
            0xFF
        }; 8];
        delta_word[..lcn_width].copy_from_slice(delta_bytes);
        lcn = lcn.checked_add_signed(i64::from_le_bytes(delta_word))?;
        let first_vcn = next_vcn;
        next_vcn = first_vcn.checked_add(length)?;
        if length == 0 || next_vcn > clusters || (allow_uninitialized && length != clusters) {
            return None;
        }
        lcn.checked_add(length)?.checked_mul(cluster_size)?;
        let run_start = first_vcn.checked_mul(cluster_size)?;
        let run_end = next_vcn.checked_mul(cluster_size)?;
        let mut position = offset.max(run_start);
        let limit = end.min(run_end);
        while position < limit {
            let physical_sector = lcn
                .checked_mul(sectors_per_cluster)?
                .checked_add((position - run_start) / 512)?
                .checked_add(partition_start)?;
            let sector_offset = (position % 512) as usize;
            let chunk = (limit - position).min((512 - sector_offset) as u64) as usize;
            sector_chunk(physical_sector, sector_offset, copied..copied + chunk)?;
            copied += chunk;
            position += chunk as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attribute(mapping: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 64];
        bytes[8] = 1;
        bytes[24..32].copy_from_slice(&1u64.to_le_bytes());
        bytes[32] = 64;
        for field in [40, 48, 56] {
            bytes[field..field + 8].copy_from_slice(&1024u64.to_le_bytes());
        }
        bytes.extend_from_slice(mapping);
        bytes
    }

    #[test]
    fn unaligned_range_crosses_fragment_with_signed_three_byte_delta() {
        let mapping = attribute(&[0x31, 1, 0, 0, 1, 0x31, 1, 1, 0, 0xff, 0]);
        let mut actual = [0; 5];
        let mut visited = Vec::new();
        assert_eq!(
            read(&mapping, 512, 9, 510, &mut actual, |sector, bytes| {
                visited.push(sector);
                bytes.fill(sector as u8);
                true
            }),
            Some(())
        );
        assert_eq!(visited, [65545, 10]);
        assert_eq!(actual, [9, 9, 10, 10, 10]);
    }

    #[test]
    fn rejects_incomplete_oversized_and_sparse_mapping_pairs() {
        for mapping in [
            &[0x11, 2, 3][..],
            &[0x91, 2, 3, 0],
            &[1, 2, 0],
            &[0x11, 1, 3, 0],
        ] {
            assert!(read(&attribute(mapping), 512, 0, 0, &mut [0; 1], |_, _| true).is_none());
        }
        assert!(read(
            &attribute(&[0x11, 2, 3, 0]),
            512,
            0,
            1023,
            &mut [0; 2],
            |_, _| true
        )
        .is_none());
    }
}

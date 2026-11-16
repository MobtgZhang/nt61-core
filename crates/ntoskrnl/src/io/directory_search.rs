//! Owned per-open directory search and NT/DOS wildcard matching.
//!
//! Embed `SearchState` in the file object and serialize access with the file's
//! request lock. Duplicated handles then share one cursor; a reused handle slot
//! cannot inherit a retired file's search. Source positions are internal vector
//! positions, never filesystem FileIndex tokens. Directory mutation/reordering
//! requires a filesystem-owned stable enumeration cookie before full equivalence.
use alloc::vec::Vec;
use crate::libs::ntdll::status::{STATUS_INSUFFICIENT_RESOURCES, STATUS_INVALID_PARAMETER};

fn bools(length: usize) -> Result<Vec<bool>, i32> {
    let mut row = Vec::new();
    row.try_reserve_exact(length).map_err(|_| STATUS_INSUFFICIENT_RESOURCES)?;
    row.resize(length, false);
    Ok(row)
}

/// Match NT expression tokens `* ? < > "` over UTF-16 code units.
///
/// The expression is already uppercased when case-insensitive matching is used,
/// just as FsRtlIsNameInExpression requires. `upcase` maps name code units only;
/// use the mounted volume's $UpCase mapping, or identity for case-sensitive calls.
/// No Unicode expansion, normalization or Win32 wildcard rewriting is performed.
/// Empty input follows the observed Win7 export's special early-return rule.
/// Rolling rows bound storage by the name size and avoid recursive backtracking.
pub fn matches_expression(
    expression: &[u16],
    name: &[u16],
    upcase: impl Fn(u16) -> u16,
) -> Result<bool, i32> {
    if expression.is_empty() || name.is_empty() {
        return Ok(expression.is_empty() && name.is_empty());
    }
    let length = name.len().checked_add(1).ok_or(STATUS_INVALID_PARAMETER)?;
    let mut next = bools(length)?;
    let mut current = bools(length)?;
    let mut after_qm = bools(length)?;
    next[name.len()] = true;
    let last_dot = name.iter().rposition(|&unit| unit == b'.' as u16);
    for index in (0..expression.len()).rev() {
        let token = expression[index];
        if token == b'>' as u16
            && (index + 1 == expression.len() || expression[index + 1] != b'>' as u16)
        {
            after_qm.copy_from_slice(&next);
        }
        for position in (0..=name.len()).rev() {
            let at_end = position == name.len();
            current[position] = match token {
                0x2a => next[position] || (!at_end && current[position + 1]),
                // DOS_STAR cannot remain active after consuming the final dot.
                0x3c => next[position] || (!at_end && if last_dot == Some(position) {
                    next[position + 1]
                } else {
                    current[position + 1]
                }),
                // DOS_QM skips the complete adjacent run at a dot or name end.
                // Win7 also accepts a final dot when that run ends the expression.
                0x3e if at_end || name[position] == b'.' as u16 => {
                    after_qm[position]
                        || (!at_end && position + 1 == name.len() && after_qm[name.len()])
                }
                0x3e => next[position + 1],
                0x22 => (at_end && next[position])
                    || (!at_end && name[position] == b'.' as u16 && next[position + 1]),
                0x3f => !at_end && next[position + 1],
                literal => !at_end && literal == upcase(name[position]) && next[position + 1],
            };
        }
        core::mem::swap(&mut current, &mut next);
    }
    Ok(next[0])
}

#[derive(Debug, Default)]
pub struct SearchState {
    expression: Option<Vec<u16>>,
    source_position: usize,
    returned_entry: bool,
}

impl SearchState {
    pub const fn new() -> Self {
        Self { expression: None, source_position: 0, returned_entry: false }
    }

    /// Capture the first search expression (None means `*`). Later caller
    /// expressions are ignored, including on restart. A restart rewinds the
    /// cursor. The enumeration owner chooses when to begin relative to its
    /// handle, information-class and buffer validation.
    pub fn begin(
        &mut self,
        expression: Option<&[u16]>,
        restart: bool,
        upcase: impl Fn(u16) -> u16,
    ) -> Result<(), i32> {
        if self.expression.is_none() {
            let input = expression.unwrap_or(&[b'*' as u16]);
            let mut captured = Vec::new();
            captured.try_reserve_exact(input.len()).map_err(|_| STATUS_INSUFFICIENT_RESOURCES)?;
            captured.extend(input.iter().map(|unit| upcase(*unit)));
            self.expression = Some(captured);
        }
        if restart {
            self.source_position = 0;
        }
        Ok(())
    }

    pub fn position(&self) -> usize { self.source_position }
    pub fn returned_entry(&self) -> bool { self.returned_entry }
    pub fn expression(&self) -> Option<&[u16]> { self.expression.as_deref() }

    pub fn matches(&self, name: &[u16], upcase: impl Fn(u16) -> u16) -> Result<bool, i32> {
        let expression = self.expression.as_deref().ok_or(STATUS_INVALID_PARAMETER)?;
        matches_expression(expression, name, upcase)
    }

    /// Publish the next source position after the owner has accepted output.
    /// A partial first name can leave this state unchanged. The caller supplies
    /// a position after the last emitted matching record, or the source end for
    /// an exhausted scan. This does not interpret a user's opaque FileIndex.
    pub fn commit(&mut self, next_source_position: usize, entries_written: usize) {
        self.source_position = next_source_position;
        self.returned_entry |= entries_written != 0;
    }
}

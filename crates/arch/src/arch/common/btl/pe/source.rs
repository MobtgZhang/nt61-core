//! Read only PE headers and section data, excluding an installer's archive or
//! Authenticode overlay. File APIs continue to expose the complete source file.
use super::{u16_at, u32_at, zeroed, LoadError, MAX_IMAGE};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageReadError {
    Read(u32),
    Image(LoadError),
}
impl From<LoadError> for ImageReadError {
    fn from(error: LoadError) -> Self {
        Self::Image(error)
    }
}

/// Read a bounded PE file through the mounted filesystem's range reader.
/// No request exceeds 64 KiB and a short successful read is retried until EOF.
pub fn read_image_file(
    mut read_at: impl FnMut(u64, usize) -> Result<Vec<u8>, u32>,
) -> Result<Vec<u8>, ImageReadError> {
    let dos = read_exact(&mut read_at, 0, 64)?;
    if dos.get(..2) != Some(b"MZ") {
        return Err(LoadError::InvalidImage.into());
    }
    let pe = u32_at(&dos, 0x3c)? as usize;
    if !(64..=1024 * 1024).contains(&pe) {
        return Err(LoadError::InvalidImage.into());
    }
    let nt = read_exact(&mut read_at, pe, 24)?;
    if nt.get(..4) != Some(b"PE\0\0") {
        return Err(LoadError::InvalidImage.into());
    }
    let count = u16_at(&nt, 6)? as usize;
    let optional = u16_at(&nt, 20)? as usize;
    if !(1..=96).contains(&count) || !(64..=4096).contains(&optional) {
        return Err(LoadError::InvalidImage.into());
    }
    let tables = read_exact(&mut read_at, pe + 24, optional + count * 40)?;
    let headers = u32_at(&tables, 60)? as usize;
    if headers < pe + 24 + tables.len() || headers > 1024 * 1024 {
        return Err(LoadError::InvalidImage.into());
    }
    let mut length = headers;
    for section in tables[optional..].chunks_exact(40) {
        let size = u32_at(section, 16)? as usize;
        if size == 0 {
            continue;
        } // BSS has no on-disk data.
        let offset = u32_at(section, 20)? as usize;
        length = length.max(offset.checked_add(size).ok_or(LoadError::InvalidImage)?);
    }
    if length > MAX_IMAGE + 1024 * 1024 {
        return Err(LoadError::ImageTooLarge.into());
    }
    read_exact(&mut read_at, 0, length)
}

fn read_exact(
    read_at: &mut impl FnMut(u64, usize) -> Result<Vec<u8>, u32>,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, ImageReadError> {
    let mut bytes = zeroed(length)?;
    let mut done = 0;
    while done < length {
        let count = (length - done).min(65536);
        let chunk = read_at((offset + done) as u64, count).map_err(ImageReadError::Read)?;
        if chunk.is_empty() || chunk.len() > count {
            return Err(LoadError::InvalidImage.into());
        }
        bytes[done..done + chunk.len()].copy_from_slice(&chunk);
        done += chunk.len();
    }
    Ok(bytes)
}

//! Checked IsTextUnicode gateway. This process currently uses the non-DBCS
//! code-page mode; a future DBCS implementation must supply its lead-byte table.
//! https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-istextunicode

use super::super::{put, read, word};
use crate::arch::common::btl::{memory::GuestMemory, pe::GuestImage};

#[path = "text_algorithm.rs"]
pub mod algorithm;

pub fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Option<Result<u64, u32>> {
    if name != "IsTextUnicode" {
        return None;
    }
    Some((|| {
        let mask = if a[2] == 0 {
            None
        } else {
            if !image.memory.probe_write(a[2], 4) {
                return Err(998);
            }
            Some(word(&mut image.memory, a[2], 4)? as u32)
        };
        let size = a[1] as u32;
        let length = (size as usize & !1).min(512);
        let bytes = if length == 0 {
            alloc::vec::Vec::new()
        } else {
            read(&mut image.memory, a[0], length)?
        };
        let (result, flags) = algorithm::classify(&bytes, size, mask);
        if a[2] != 0 {
            put(&mut image.memory, a[2], u64::from(flags), 4)?;
        }
        Ok(u64::from(result))
    })())
}

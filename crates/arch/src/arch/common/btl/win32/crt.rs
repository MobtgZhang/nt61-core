//! Bounded byte/UTF-16 CRT primitives shared by NTDLL and MSVCRT facades.
//! C byte strings are not ANSI text: every nonzero byte is accepted. No guest
//! pointer is dereferenced by the host. Invalid memory faults, rather than
//! returning a fabricated C result or changing Win32 LastError.
use super::{ApiError, GuestImage, GuestMemory, MAX_BUFFER};
use alloc::{vec, vec::Vec};

pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "strlen"
            | "memchr"
            | "strstr"
            | "wcsstr"
            | "strpbrk"
            | "wcspbrk"
            | "strspn"
            | "wcsspn"
            | "strcspn"
            | "wcscspn"
            | "strnlen"
            | "strcmp"
            | "strncmp"
            | "strchr"
            | "strrchr"
            | "wcslen"
            | "wcsnlen"
            | "wcscmp"
            | "wcsncmp"
            | "wcschr"
            | "wcsrchr"
            | "strcpy"
            | "strncpy"
            | "strcat"
            | "strncat"
            | "wcscpy"
            | "wcsncpy"
            | "wcscat"
            | "wcsncat"
    )
}

fn search_string(image: &mut GuestImage, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    if name == "memchr" {
        let count = usize::try_from(args[2]).map_err(|_| ApiError::Unsupported)?;
        if count > MAX_BUFFER {
            return Err(ApiError::Unsupported);
        }
        for index in 0..count {
            if unit(&mut image.memory, args[0], index, false)? as u8 == args[1] as u8 {
                return args[0]
                    .checked_add(index as u64)
                    .ok_or(ApiError::MemoryFault);
            }
        }
        return Ok(0);
    }
    let wide = name.starts_with("wcs");
    let width = if wide { 2 } else { 1 };
    let limit = MAX_BUFFER / width;
    if name.ends_with("str") {
        // KMP keeps repeated-prefix needles linear in the bounded input size.
        // Read only through the match, so success at a page boundary does not
        // require an unrelated suffix of the haystack to be mapped.
        let mut needle = Vec::new();
        for index in 0..limit {
            let value = unit(&mut image.memory, args[1], index, wide)?;
            if value == 0 {
                break;
            }
            needle.push(value);
        }
        if needle.len() == limit {
            return Err(ApiError::Unsupported);
        }
        if needle.is_empty() {
            return Ok(args[0]);
        }
        let mut prefix = vec![0usize; needle.len()];
        let mut matched = 0;
        for index in 1..needle.len() {
            while matched != 0 && needle[index] != needle[matched] {
                matched = prefix[matched - 1];
            }
            if needle[index] == needle[matched] {
                matched += 1;
            }
            prefix[index] = matched;
        }
        matched = 0;
        for index in 0..limit {
            let value = unit(&mut image.memory, args[0], index, wide)?;
            if value == 0 {
                return Ok(0);
            }
            while matched != 0 && value != needle[matched] {
                matched = prefix[matched - 1];
            }
            if value == needle[matched] {
                matched += 1;
                if matched == needle.len() {
                    return args[0]
                        .checked_add(((index + 1 - matched) * width) as u64)
                        .ok_or(ApiError::MemoryFault);
                }
            }
        }
        return Err(ApiError::Unsupported);
    }
    // Byte/UTF-16 membership is locale-independent and linear even for large
    // duplicate sets. NUL terminates the set and is never a member.
    let mut members = vec![0u64; if wide { 1024 } else { 4 }];
    let mut terminated = false;
    for index in 0..limit {
        let value = unit(&mut image.memory, args[1], index, wide)? as usize;
        if value == 0 {
            terminated = true;
            break;
        }
        members[value / 64] |= 1u64 << (value % 64);
    }
    if !terminated {
        return Err(ApiError::Unsupported);
    }
    let find_member = name.ends_with("pbrk");
    let complement = name.ends_with("cspn");
    for index in 0..limit {
        let value = unit(&mut image.memory, args[0], index, wide)? as usize;
        if value == 0 {
            return Ok(if find_member { 0 } else { index as u64 });
        }
        let member = members[value / 64] & (1u64 << (value % 64)) != 0;
        if find_member && member {
            return args[0]
                .checked_add((index * width) as u64)
                .ok_or(ApiError::MemoryFault);
        }
        if !find_member && member == complement {
            return Ok(index as u64);
        }
    }
    Err(ApiError::Unsupported)
}

fn copy_string(image: &mut GuestImage, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    let wide = name.starts_with("wcs");
    let width = if wide { 2 } else { 1 };
    let bounded = matches!(name, "strncpy" | "strncat" | "wcsncpy" | "wcsncat");
    let append = name.ends_with("cat");
    let count = if bounded { args[2] } else { u64::MAX };
    let limit = MAX_BUFFER / width;
    if bounded && count > limit as u64 {
        return Err(ApiError::Unsupported);
    }
    // strncpy(..., 0) does not access either pointer. Concatenation still
    // locates the destination terminator and appends a terminator.
    if bounded && !append && count == 0 {
        return Ok(args[0]);
    }
    let mut offset = 0;
    if append {
        loop {
            if offset >= limit {
                return Err(ApiError::Unsupported);
            }
            if unit(&mut image.memory, args[0], offset, wide)? == 0 {
                break;
            }
            offset += 1;
        }
    }
    let mut bytes = Vec::new();
    let mut terminated = false;
    for i in 0..count.min(limit as u64) as usize {
        let value = unit(&mut image.memory, args[1], i, wide)?;
        bytes.extend_from_slice(&value.to_le_bytes()[..width]);
        if value == 0 {
            terminated = true;
            break;
        }
    }
    if !terminated && !bounded {
        return Err(ApiError::Unsupported);
    }
    if bounded && !append {
        bytes.resize(count as usize * width, 0);
    } else if !terminated {
        bytes.resize(
            bytes
                .len()
                .checked_add(width)
                .ok_or(ApiError::Unsupported)?,
            0,
        );
    }
    if offset * width + bytes.len() > MAX_BUFFER {
        return Err(ApiError::Unsupported);
    }
    let destination = args[0]
        .checked_add((offset * width) as u64)
        .filter(|_| args[0] != 0)
        .ok_or(ApiError::MemoryFault)?;
    if !image.memory.probe_write(destination, bytes.len())
        || !image.memory.write_virt(destination, &bytes)
    {
        return Err(ApiError::MemoryFault);
    }
    Ok(args[0])
}

fn unit(
    memory: &mut dyn GuestMemory,
    base: u64,
    index: usize,
    wide: bool,
) -> Result<u16, ApiError> {
    let width = if wide { 2 } else { 1 };
    let offset = index.checked_mul(width).ok_or(ApiError::MemoryFault)?;
    let address = base
        .checked_add(offset as u64)
        .filter(|_| base != 0)
        .ok_or(ApiError::MemoryFault)?;
    let mut bytes = [0; 2];
    if memory.read_virt(address, &mut bytes[..width]) != width {
        return Err(ApiError::MemoryFault);
    }
    Ok(u16::from_le_bytes(bytes))
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, args: &[u64]) -> Result<u64, ApiError> {
    if matches!(
        name,
        "memchr"
            | "strstr"
            | "wcsstr"
            | "strpbrk"
            | "wcspbrk"
            | "strspn"
            | "wcsspn"
            | "strcspn"
            | "wcscspn"
    ) {
        return search_string(image, name, args);
    }
    if matches!(
        name,
        "strcpy" | "strncpy" | "strcat" | "strncat" | "wcscpy" | "wcsncpy" | "wcscat" | "wcsncat"
    ) {
        return copy_string(image, name, args);
    }
    let wide = name.starts_with("wcs");
    let bounded = matches!(name, "strnlen" | "wcsnlen" | "strncmp" | "wcsncmp");
    let compare = name.ends_with("cmp");
    let search = name.ends_with("chr");
    let reverse = matches!(name, "strrchr" | "wcsrchr");
    let bound = if bounded {
        args[if compare { 2 } else { 1 }]
    } else {
        u64::MAX
    };
    let limit = (MAX_BUFFER / if wide { 2 } else { 1 }) as u64;
    let target = if wide {
        args[1] as u16
    } else {
        args[1] as u8 as u16
    };
    let mut found = 0;
    for i in 0..bound.min(limit) as usize {
        let left = unit(&mut image.memory, args[0], i, wide)?;
        if compare {
            let right = unit(&mut image.memory, args[1], i, wide)?;
            if left != right {
                // Original SP1 MSVCRT differs by function and guest bitness:
                // strcmp/wcscmp return a sign; x86 strncmp and both wcsncmp
                // return the unsigned-unit difference. x64 strncmp is signed.
                let difference = i32::from(left) - i32::from(right);
                let sign_only = name == "strcmp"
                    || name == "wcscmp"
                    || name == "strncmp" && image.thread.context.mode == super::DecodeMode::X64;
                return Ok(if sign_only {
                    difference.signum()
                } else {
                    difference
                } as u32 as u64);
            }
        }
        if search && left == target {
            found = args[0]
                .checked_add((i * if wide { 2 } else { 1 }) as u64)
                .ok_or(ApiError::MemoryFault)?;
            if !reverse {
                return Ok(found);
            }
        }
        if left == 0 {
            return Ok(if search {
                found
            } else if compare {
                0
            } else {
                i as u64
            });
        }
    }
    if bound <= limit {
        Ok(if compare { 0 } else { bound })
    } else {
        Err(ApiError::Unsupported)
    }
}

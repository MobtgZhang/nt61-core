//! Desktop values in a profile's NTUSER.DAT (HKCU\Control Panel\Desktop).
use super::{
    hive::{Hive, KeyNode, ValueType},
    hive_writer::{HiveKey, HiveValue, HiveWriter},
};
use alloc::{string::String, vec::Vec};

pub fn read(bytes: &[u8]) -> Option<(String, String, String)> {
    let hive = Hive::parse(bytes).ok()?;
    let defaults = || ("C:\\Windows\\Web\\Wallpaper\\Windows\\img0.jpg".into(), "10".into(), "0".into());
    let Some(panel) = hive.find_subkey(&hive.root().ok()?, "Control Panel").ok()? else {
        return Some(defaults());
    };
    let Some(desktop) = hive.find_subkey(&panel, "Desktop").ok()? else {
        return Some(defaults());
    };
    let string = |name, fallback: &str| match hive.find_value(&desktop, name).ok()? {
        Some(value) if value.value_type == ValueType::String => value.as_utf16_string(),
        Some(_) => None,
        None => Some(fallback.into()),
    };
    Some((
        string("Wallpaper", "C:\\Windows\\Web\\Wallpaper\\Windows\\img0.jpg")?,
        string("WallpaperStyle", "10")?,
        string("TileWallpaper", "0")?,
    ))
}

fn copy(hive: &Hive<'_>, node: &KeyNode, depth: usize, budget: &mut usize) -> Option<HiveKey> {
    if depth > 64 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    let mut key = HiveKey::new(&node.name);
    key.flags = node.flags;
    for v in hive.values(node).ok()? {
        key.values.push(HiveValue {
            name: v.name,
            value_type: v.value_type,
            data: v.data,
        });
    }
    for child in hive.subkeys(node).ok()? {
        key.subkeys.push(copy(hive, &child, depth + 1, budget)?);
    }
    Some(key)
}

fn child<'a>(key: &'a mut HiveKey, name: &str) -> &'a mut HiveKey {
    let index = match key
        .subkeys
        .iter()
        .position(|k| k.name.eq_ignore_ascii_case(name))
    {
        Some(i) => i,
        None => {
            key.subkeys.push(HiveKey::new(name));
            key.subkeys.len() - 1
        }
    };
    &mut key.subkeys[index]
}

/// Preserve the existing key/value tree. Serialization shares the project's
/// REGF writer; security cells and original key timestamps are not preserved.
pub fn update(previous: Option<&[u8]>, path: &str, style: &str, tile: &str) -> Option<Vec<u8>> {
    let mut root = if let Some(bytes) = previous {
        let hive = Hive::parse(bytes).ok()?;
        copy(&hive, &hive.root().ok()?, 0, &mut 8192)?
    } else {
        HiveKey::new("")
    };
    let desktop = child(child(&mut root, "Control Panel"), "Desktop");
    for (name, text) in [
        ("Wallpaper", path),
        ("WallpaperStyle", style),
        ("TileWallpaper", tile),
    ] {
        desktop
            .values
            .retain(|v| !v.name.eq_ignore_ascii_case(name));
        desktop.values.push(HiveValue {
            name: name.into(),
            value_type: ValueType::String,
            data: text
                .encode_utf16()
                .chain(core::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        });
    }
    HiveWriter::new().serialise(&root, 0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_values_roundtrip_without_removing_other_values() {
        let mut root = HiveKey::new("");
        child(&mut root, "Software").values.push(HiveValue {
            name: "ExistingBinary".into(), value_type: ValueType::Binary,
            data: alloc::vec![0, 255, 1, 128],
        });
        child(child(&mut root, "Control Panel"), "Desktop").values.push(HiveValue {
            name: "ExistingDword".into(), value_type: ValueType::DWord,
            data: 73u32.to_le_bytes().to_vec(),
        });
        let original = HiveWriter::new().serialise(&root, 0).unwrap();
        let first = update(
            Some(&original),
            "C:\\Windows\\Web\\Wallpaper\\Windows\\img0.jpg",
            "10",
            "0",
        )
        .unwrap();
        assert_eq!(read(&first).unwrap().1, "10");
        let next = update(Some(&first), "C:\\Pictures\\test.jpg", "0", "1").unwrap();
        assert_eq!(
            read(&next).unwrap(),
            ("C:\\Pictures\\test.jpg".into(), "0".into(), "1".into())
        );
        let hive = Hive::parse(&next).unwrap();
        let root = hive.root().unwrap();
        let software = hive.find_subkey(&root, "Software").unwrap().unwrap();
        assert_eq!(hive.find_value(&software, "ExistingBinary").unwrap().unwrap().data, [0, 255, 1, 128]);
        let panel = hive.find_subkey(&root, "Control Panel").unwrap().unwrap();
        let desktop = hive.find_subkey(&panel, "Desktop").unwrap().unwrap();
        assert_eq!(hive.find_value(&desktop, "ExistingDword").unwrap().unwrap().data, 73u32.to_le_bytes());
        assert!(update(Some(b"invalid hive"), "x", "0", "0").is_none());
    }
    #[test]
    fn missing_settings_default_but_corrupt_hives_are_rejected() {
        let bytes = HiveWriter::new().serialise(&HiveKey::new(""), 0).unwrap();
        let expected = ("C:\\Windows\\Web\\Wallpaper\\Windows\\img0.jpg".into(), "10".into(), "0".into());
        assert_eq!(read(&bytes), Some(expected));
        assert!(read(b"invalid hive").is_none());
    }
}

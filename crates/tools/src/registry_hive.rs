//! Persist a NEW offline registry fragment and verify all keys and values with
//! the kernel's production REGF reader before publication. Existing hives are
//! never replaced or imported: their ACLs/unknown cells cannot be round-tripped
//! by this restricted writer. This does not authenticate or install a package.
use crate::regf::{HiveBuilder, Node, Value};
use nt61_driver_package::registry_apply::RegistryImage;
use std::{
    io::{self, Write},
    path::Path,
};

// The parser is shared source, exactly as used by the existing system_registry
// tests. It has no kernel dependencies. Keep one format implementation.
extern crate alloc;
#[allow(dead_code)]
#[path = "../../registry/src/registry/hive.rs"]
mod reader;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Encode an image using the production REGF writer and read back EVERY key,
/// value, registry type and byte. Large cells/indexes not supported by the
/// current writer are rejected before it runs.
pub fn encode_registry_hive(image: &RegistryImage) -> io::Result<Vec<u8>> {
    let mut root = Node::new("INF_REGISTRY_FRAGMENT").root();
    for (path, values) in image.keys() {
        let parts: Vec<_> = path.split('\\').filter(|p| !p.is_empty()).collect();
        if parts.len() > 64 {
            return Err(invalid("registry hive depth exceeds 64"));
        }
        let mut node = &mut root;
        for part in parts {
            let index = if let Some(index) = node.subkeys.iter().position(|k| k.name == part) {
                index
            } else {
                // One lh index must fit in the writer's 4096-byte HBIN.
                if node.subkeys.len() >= 506 {
                    return Err(invalid(
                        "REGF writer does not support this many sibling keys",
                    ));
                }
                node.subkeys.push(Node::new(part));
                node.subkeys.len() - 1
            };
            node = &mut node.subkeys[index];
        }
        if values.len() > 1015 {
            return Err(invalid(
                "REGF writer does not support this many values in one key",
            ));
        }
        for (name, value) in values {
            if name.len() > 2000 || value.data.len() > 4060 || value.kind > 11 {
                return Err(invalid(
                    "unsupported REGF value name, data size or registry type",
                ));
            }
            node.values.push(Value {
                name: name.clone(),
                data_type: value.kind,
                data: value.data.clone(),
            });
        }
    }
    let bytes = HiveBuilder::new(root)
        .with_name("INF registry fragment")
        .finish();
    verify_registry_hive(image, &bytes)?;
    Ok(bytes)
}

/// A parser accepting the header alone is insufficient; recursively enumerate
/// the actual persisted tree and compare it against the entire expected image.
pub fn verify_registry_hive(image: &RegistryImage, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > 128 * 1024 * 1024 {
        return Err(invalid("excessive REGF size"));
    }
    let hive = reader::Hive::parse(bytes).map_err(|e| invalid(e.to_string()))?;
    let root = hive.root().map_err(|e| invalid(e.to_string()))?;
    let mut pending = vec![(String::new(), root, 0usize)];
    let expected: std::collections::BTreeMap<_, _> = image.keys().collect();
    let mut visited = std::collections::BTreeSet::new();
    while let Some((path, key, depth)) = pending.pop() {
        if depth > 64 || !visited.insert(path.clone()) {
            return Err(invalid("duplicate or excessively deep registry key"));
        }
        let values = expected
            .get(path.as_str())
            .ok_or_else(|| invalid("unexpected REGF key"))?;
        // This verifier accepts the restricted writer's lh indexes only.
        // Check counts BEFORE the reader allocates or follows any index; in
        // particular an untrusted recursive ri index must never reach it.
        let field = |offset: usize| -> io::Result<u32> {
            let data = bytes
                .get(key.offset as usize + offset..key.offset as usize + offset + 4)
                .ok_or_else(|| invalid("truncated REGF key"))?;
            Ok(u32::from_le_bytes(
                data.try_into().map_err(|_| invalid("truncated field"))?,
            ))
        };
        let children = field(0x18)? as usize;
        if children > 506 || field(0x28)? as usize != values.len() {
            return Err(invalid("unsupported REGF counts"));
        }
        if children != 0 {
            let offset = 4096usize + field(0x20)? as usize;
            if bytes.get(offset + 4..offset + 6) != Some(b"lh") {
                return Err(invalid("unsupported REGF subkey index"));
            }
            let count = bytes
                .get(offset + 6..offset + 8)
                .ok_or_else(|| invalid("truncated REGF index"))?;
            if usize::from(u16::from_le_bytes([count[0], count[1]])) != children {
                return Err(invalid("REGF subkey counts differ"));
            }
        }
        let actual = hive.values(&key).map_err(|e| invalid(e.to_string()))?;
        if actual.len() != values.len() {
            return Err(invalid("REGF value count differs"));
        }
        let mut names = std::collections::BTreeSet::new();
        for value in actual {
            if !names.insert(value.name.clone()) {
                return Err(invalid("duplicate REGF value"));
            }
            let wanted = values
                .get(&value.name)
                .ok_or_else(|| invalid("unexpected REGF value"))?;
            if wanted.kind != value.value_type.to_u32() || wanted.data != value.data {
                return Err(invalid("REGF value type/data differs"));
            }
        }
        let subkeys = hive.subkeys(&key).map_err(|e| invalid(e.to_string()))?;
        if subkeys.len() != children {
            return Err(invalid("REGF subkey count differs"));
        }
        for subkey in subkeys {
            let child = if path.is_empty() {
                subkey.name.clone()
            } else {
                format!("{path}\\{}", subkey.name)
            };
            pending.push((child, subkey, depth + 1));
        }
        if pending.len() > 16_384 || visited.len() > expected.len() {
            return Err(invalid("excessive REGF tree"));
        }
    }
    if visited.len() != expected.len() {
        return Err(invalid("missing REGF keys"));
    }
    Ok(())
}

/// Publish a new artifact without replacing an existing file (including a
/// symlink). Write and fsync a same-directory temporary, verify disk bytes,
/// then use tempfile's atomic no-clobber publication. On Unix, fsync the parent
/// directory as well. A directory-sync error after publication explicitly
/// reports that the file exists with uncertain crash durability.
pub fn write_registry_hive(image: &RegistryImage, destination: &Path) -> io::Result<usize> {
    let bytes = encode_registry_hive(image)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    let persisted = std::fs::read(temporary.path())?;
    verify_registry_hive(image, &persisted)?;
    temporary
        .persist_noclobber(destination)
        .map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "hive published at {}; parent sync failed: {e}",
                destination.display()
            ),
        )
    })?;
    Ok(bytes.len())
}

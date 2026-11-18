//! Reject whole-file original SP1 executable/catalog payloads before packaging.
//! Existing desktop media are explicitly supported resource inputs; executable
//! system components must continue to be implemented in Rust.

use crate::error::{BuildError, Result};
use std::path::Path;

#[allow(dead_code)]
#[path = "../../../crypto/src/crypto/hash.rs"]
mod hash;

const FINGERPRINTS: &str = include_str!(
    "../../../../docs/desktop/baselines/win7-sp1-window-placement/original-fingerprints.json"
);

/// This check is read-only, including on failure. Call before creating images
/// or cleaning staging so a rejected input remains available for inspection.
pub fn validate_trees(esp: &Path, system: &Path) -> Result<()> {
    let fingerprints: Vec<(u64, String)> = serde_json::from_str(FINGERPRINTS)
        .map_err(|e| BuildError::InvalidFormat(format!("SP1 fingerprints: {e}")))?;
    visit(esp, &fingerprints)?;
    visit(system, &fingerprints)
}

fn visit(path: &Path, fingerprints: &[(u64, String)]) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    // Do not follow directory aliases outside the inspected staging tree.
    if meta.file_type().is_symlink() {
        return Err(BuildError::InvalidParam(format!(
            "staging symlink: {}",
            path.display()
        )));
    }
    if meta.is_dir() {
        for entry in std::fs::read_dir(path)? {
            visit(&entry?.path(), fingerprints)?;
        }
    } else if meta.is_file() {
        if fingerprints.iter().any(|(size, _)| *size == meta.len()) {
            let bytes = std::fs::read(path)?;
            let media = bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || bytes.starts_with(b"\xff\xd8\xff")
                || bytes.starts_with(b"BM")
                || bytes.starts_with(&[0, 0, 1, 0])
                || bytes.starts_with(&[0, 0, 2, 0])
                || (bytes.starts_with(b"RIFF") && matches!(bytes.get(8..12), Some(b"WAVE" | b"ACON")));
            if media { return Ok(()); }
            let digest = hash::sha256(&bytes);
            let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
            if fingerprints
                .iter()
                .any(|(size, hash)| *size == meta.len() && *hash == hex)
            {
                return Err(BuildError::InvalidParam(format!(
                    "original SP1 payload in staging; implement in Rust: {}",
                    path.display()
                )));
            }
        }
    } else {
        return Err(BuildError::InvalidParam(format!(
            "non-regular staging entry: {}",
            path.display()
        )));
    }
    Ok(())
}

//! Image integrity verification for winload.efi.
//!
//! Three independent checks run before a PE is accepted:
//!
//!   1. **SHA-256 digest** over the whole file, computed with the
//!      in-tree `crypto::hash` subsystem. When a sidecar file
//!      `<image>.sha256` exists on the system partition its hex value
//!      is compared against the computed digest; a mismatch refuses
//!      the image.
//!   2. **PE CheckSum** field (optional-header CheckSum, the same
//!      algorithm as `CheckSumMappedFile`). Images that carry a
//!      non-zero stored checksum must match; images without one skip
//!      this check, matching the Windows kernel loader behavior.
//!   3. **Optional RSA signature** (PKCS#1 v1.5 over the SHA-256
//!      digest) via the `crypto::rsa` subsystem. Verification only
//!      happens when BOTH a `<image>.sig` sidecar and a trust anchor
//!      at `\EFI\nt61\trust.pub` exist. The trust anchor file holds
//!      the RSA modulus in lowercase hex on the first line and the
//!      public exponent in hex on the second. Big integers are
//!      little-endian, matching `crypto::rsa::BigUint::from_bytes`.

use crate::pe::PE32PLUS_CHECKSUM_OFFSET;
use nt61_crypto::crypto::hash::sha256;
use nt61_crypto::crypto::rsa::{BigUint, RsaPublicKey};

/// Location of the RSA trust anchor on the system partition.
const TRUST_ANCHOR_PATH: &str = "\\EFI\\nt61\\trust.pub";

use crate::image_integrity::{hex_decode, pe_file_checksum};

fn optional_header_offset(image: &[u8]) -> Option<usize> {
    crate::boot_image::optional_header_offset(image)
}

fn stored_pe_checksum(image: &[u8]) -> u32 {
    let Some(opt) = optional_header_offset(image) else {
        return 0;
    };
    let off = opt + PE32PLUS_CHECKSUM_OFFSET;
    if off + 4 > image.len() {
        return 0;
    }
    u32::from_le_bytes([image[off], image[off + 1], image[off + 2], image[off + 3]])
}

/// Compute the SHA-256 digest of the whole image file.
pub fn image_digest(image: &[u8]) -> [u8; 32] {
    sha256(image)
}

fn digest_matches(digest: &[u8; 32], hex_line: &str) -> bool {
    let Some(expected) = hex_decode(hex_line) else {
        return false;
    };
    expected.as_slice() == digest
}

fn read_sidecar(path: &str, suffix: &str) -> Option<alloc::vec::Vec<u8>> {
    let mut sidecar = alloc::string::String::with_capacity(path.len() + suffix.len());
    sidecar.push_str(path);
    sidecar.push_str(suffix);
    crate::open_system_file_data(&sidecar)
}

#[derive(Clone)]
struct TrustAnchor {
    key: RsaPublicKey,
}

static mut TRUST_ANCHOR_CACHE: Option<TrustAnchor> = None;

fn load_trust_anchor() -> Option<TrustAnchor> {
    // SAFETY: the trust anchor is immutable while the loader runs, and
    // winload is single-threaded before ExitBootServices.
    unsafe {
        if let Some(anchor) = (*core::ptr::addr_of!(TRUST_ANCHOR_CACHE)).clone() {
            return Some(anchor);
        }
    }

    let data = crate::open_system_file_data(TRUST_ANCHOR_PATH)?;
    let text = core::str::from_utf8(&data).ok()?;
    let mut lines = text.lines();
    let modulus = hex_decode(lines.next()?)?;
    let exponent = hex_decode(lines.next()?)?;
    if modulus.is_empty() || exponent.is_empty() {
        return None;
    }
    let anchor = TrustAnchor {
        key: RsaPublicKey {
            n: BigUint::from_bytes(&modulus),
            e: BigUint::from_bytes(&exponent),
        },
    };
    unsafe {
        *core::ptr::addr_of_mut!(TRUST_ANCHOR_CACHE) = Some(anchor.clone());
    }
    Some(anchor)
}

/// Verify one PE image. `path` is the system-partition path the image
/// was read from, e.g. `\\Windows\\System32\\ntoskrnl.exe`.
///
/// Hard failures:
///   * sidecar `<path>.sha256` exists but the digest does not match,
///   * stored PE CheckSum is non-zero and does not match,
///   * a `<path>.sig` sidecar exists, a trust anchor is installed, and
///     the RSA verification fails.
///
/// Everything else (missing sidecars, missing trust anchor) passes with
/// a debug log so freshly generated images boot before code signing is
/// rolled out.
pub fn verify_image(path: &str, image: &[u8]) -> Result<(), &'static str> {
    // Verify the declared header extent before reading CheckSum. This gate runs
    // before full PE parsing and must remain safe even for truncated input.
    optional_header_offset(image).ok_or("bad PE optional header")?;
    let digest = image_digest(image);

    if let Some(sidecar) = read_sidecar(path, ".sha256") {
        let hex_line = core::str::from_utf8(&sidecar).unwrap_or("");
        if !digest_matches(&digest, hex_line) {
            return Err("SHA-256 sidecar mismatch");
        }
        debug_println!("[VERIFY] {} sha256 sidecar ok", path);
    }

    let stored = stored_pe_checksum(image);
    if stored != 0 {
        let opt = optional_header_offset(image).ok_or("bad optional header")?;
        let computed = pe_file_checksum(image, opt + PE32PLUS_CHECKSUM_OFFSET);
        if computed != stored {
            return Err("PE CheckSum mismatch");
        } else {
            debug_println!("[VERIFY] {} PE CheckSum ok ({computed:#x})", path);
        }
    }

    let driver = path.to_ascii_lowercase().ends_with(".sys");
    let skip_signature = driver && crate::active_boot_options().has(nt61_boot_types::boot_options::BootOptions::NO_SIGNATURE);
    if skip_signature {
        debug_println!("[VERIFY] {} driver RSA signature enforcement disabled for this boot", path);
    }
    if let Some(sig) = if skip_signature { None } else { read_sidecar(path, ".sig") } {
        match load_trust_anchor() {
            Some(anchor) => {
                if !anchor.key.verify_pkcs1v15(&digest, &sig) {
                    return Err("RSA signature verification failed");
                }
                debug_println!("[VERIFY] {} RSA signature ok", path);
            }
            None => {
                debug_println!(
                    "[VERIFY] {} .sig present but no trust anchor; signature ignored",
                    path
                );
            }
        }
    }

    use core::fmt::Write as _;
    let mut hex = alloc::string::String::with_capacity(64);
    for b in digest {
        let _ = core::write!(hex, "{:02x}", b);
    }
    debug_println!("[VERIFY] {} sha256={}", path, hex);
    Ok(())
}

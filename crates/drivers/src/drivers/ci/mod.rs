//! Code Integrity (ci.dll)
//
//! Implements checked PE content hashing and explicit digest authorization.
//! Certificate/catalog trust and Windows CI ABI are not implemented here.
//
//! Code Integrity uses the WDK driver naming convention
//! (CiInitialize, IMAGE_POLICY, ...).
//
//! Key responsibilities:
//!   * policy initialization and query
//!   * explicit trusted-digest validation before image execution
//!   * standard SHA-256 file and PE content digests
//!   * checked Authenticode content exclusions (no signature verification)
//
//!     When code integrity detects a violation, it returns
//!     `STATUS_INVALID_IMAGE_HASH` for content without an explicitly trusted
//!     digest, or `STATUS_INVALID_IMAGE_FORMAT` for malformed PE content.
//
//!     Clean-room implementation. Spec source: Windows Internals 6th ed.
//!     ch.10 (Code Integrity), WDK documentation.

/// CI policy flags.
pub const CI_POLICY_ENABLED: u32 = 0x00000001;
pub const CI_POLICY_DRIVER_ENFORCEMENT: u32 = 0x00000002;
pub const CI_POLICY_NOVULN: u32 = 0x00000004;

/// CI system status flags.
pub const CI_NOLMS: u32 = 0x00000001; // No LMS is available

use core::sync::atomic::{AtomicU32, Ordering};

pub use crate::crypto::hash::Sha256;
mod image_hash;
pub use image_hash::{compute_pe_hash, ImageHashError};

static CI_POLICY: AtomicU32 = AtomicU32::new(0);

/// Standard SHA-256, shared with the kernel cryptography provider.
pub fn compute_sha256(data: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(data);
    hash.finalize()
}

const STATUS_INVALID_IMAGE_FORMAT: u32 = 0xc000007b;
const STATUS_INVALID_IMAGE_HASH: u32 = 0xc0000428;

/// Verify PE content against an explicitly supplied trusted digest set.
/// The caller owns the trust decision for that set. This does not validate
/// PKCS#7, certificate chains, timestamps, revocation or catalog signatures.
pub fn validate_pe_image_with_hashes(image: &[u8], trusted: &[[u8; 32]]) -> u32 {
    let Ok(digest) = compute_pe_hash(image) else {
        return STATUS_INVALID_IMAGE_FORMAT;
    };
    if trusted.iter().any(|expected| expected == &digest) {
        0
    } else {
        STATUS_INVALID_IMAGE_HASH
    }
}

/// No images are implicitly trusted by the kernel CI provider.
pub fn validate_pe_image(image: &[u8]) -> u32 {
    validate_pe_image_with_hashes(image, &[])
}

/// Initialise the Code Integrity module.
pub fn init() {
    CI_POLICY.store(CI_POLICY_ENABLED | CI_POLICY_DRIVER_ENFORCEMENT, Ordering::Release);
}

/// Query policy separately from validation; disabled enforcement is not
/// represented as a successful signature verification.
pub fn policy() -> u32 {
    CI_POLICY.load(Ordering::Acquire)
}

/// SeCiCheckImage rejects unknown or malformed images even before policy
/// initialization. Callers needing a trusted allowlist must supply one to
/// `validate_pe_image_with_hashes`; no uninitialized allow-all gate exists.
pub fn se_ci_check_image(image: &[u8]) -> u32 {
    validate_pe_image(image)
}

pub fn smoke_test() -> bool {
    compute_sha256(b"Hello, World!") == [
        0xdf, 0xfd, 0x60, 0x21, 0xbb, 0x2b, 0xd5, 0xb0,
        0xaf, 0x67, 0x62, 0x90, 0x80, 0x9e, 0xc3, 0xa5,
        0x31, 0x91, 0xdd, 0x81, 0xc7, 0xf7, 0x0a, 0x4b,
        0x28, 0x68, 0x8a, 0x36, 0x21, 0x82, 0x98, 0x6f,
    ]
}

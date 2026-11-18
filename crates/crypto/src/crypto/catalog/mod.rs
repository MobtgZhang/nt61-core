//! Offline verification of the DER/PKCS#7 catalog profile on the Windows 7
//! SP1 ISO. This is a cryptographic building block, not the Windows CI ABI.
//!
//! Sources: RFC 2315 sections 9/11/12 (including legacy countersignatures),
//! RFC 5280, RFC 8017 and Microsoft's CTL_INFO / CRYPTCATMEMBER / SPC_INDIRECT_DATA
//! definitions. SHA-1 is supported explicitly for historical SP1 media.
//!
//! Trust anchors, evaluation time and verified revocation evidence belong to
//! the caller. Embedded certificates are never roots by default. This module
//! does not fetch CRLs/OCSP, implement Windows root/kernel signing policy,
//! RFC 3161 timestamps, certificate policy mapping or driver installation.
extern crate alloc;
use crate::crypto::hash::{Md5, Sha1, Sha256};
use alloc::vec::Vec;
mod crl;
mod der;
mod x509;
pub use crl::{Crl, CrlSet};
use der::{Reader, Value};
use x509::Certificate;

type Result<T> = core::result::Result<T, Error>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Der,
    Limit,
    UnsupportedAlgorithm,
    UnsupportedPolicy,
    Certificate,
    Signature,
    ContentDigest,
    Attributes,
    Untrusted,
    Purpose,
    Expired,
    Revoked,
    RevocationUnknown,
    Timestamp,
    Member,
    MemberNotFound,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DigestAlgorithm {
    Sha1,
    Sha256,
    LegacyMd5,
}
impl DigestAlgorithm {
    fn from_oid(oid: &[u8]) -> Result<Self> {
        match oid {
            MD5 => Ok(Self::LegacyMd5),
            SHA1 => Ok(Self::Sha1),
            SHA256 => Ok(Self::Sha256),
            _ => Err(Error::UnsupportedAlgorithm),
        }
    }
    pub fn hash(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Self::LegacyMd5 => {
                let mut h = Md5::new();
                h.update(bytes);
                h.finalize().to_vec()
            }
            Self::Sha1 => {
                let mut h = Sha1::new();
                h.update(bytes);
                h.finalize().to_vec()
            }
            Self::Sha256 => {
                let mut h = Sha256::new();
                h.update(bytes);
                h.finalize().to_vec()
            }
        }
    }
    fn len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::LegacyMd5 => 16,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevocationStatus {
    Good,
    Revoked,
    Unknown,
}
/// The callback must consult independently authenticated, fresh revocation
/// evidence. Returning Good unconditionally explicitly bypasses that check;
/// the verifier cannot establish provenance of caller-provided evidence.
/// Arguments: certificate DER, issuer DER, evaluation time (YYYYMMDDhhmmss UTC).
pub struct TrustPolicy<'a> {
    pub anchors: &'a [&'a [u8]],
    pub evaluation_time: EvaluationTime,
    /// Historical media only: permit MD5 certificates/countersignatures. Off by default
    /// in caller policy; this does not relax CAT content/primary-signature algorithms.
    pub allow_legacy_md5: bool,
    pub revocation: &'a dyn Fn(&[u8], &[u8], u64) -> RevocationStatus,
}
/// A validated UTC calendar time; no reliance on an uninitialized RTC.
#[derive(Clone, Copy, Debug)]
pub struct EvaluationTime(u64);
impl EvaluationTime {
    pub fn from_utc(value: &[u8; 15]) -> Result<Self> {
        Ok(Self(der::time(Value {
            tag: 0x18,
            body: value,
            full: &[],
        })?))
    }
    pub fn utc(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy)]
enum Purpose {
    CodeSigning,
    Timestamp,
}
const SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 7, 2];
const DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 7, 1];
const CTL: &[u8] = &[0x2b, 6, 1, 4, 1, 0x82, 0x37, 10, 1];
const CATALOG_LIST: &[u8] = &[0x2b, 6, 1, 4, 1, 0x82, 0x37, 12, 1, 1];
const CATALOG_VERSION: &[u8] = &[0x2b, 6, 1, 4, 1, 0x82, 0x37, 12, 1, 2];
const INDIRECT: &[u8] = &[0x2b, 6, 1, 4, 1, 0x82, 0x37, 2, 1, 4];
const PE_IMAGE: &[u8] = &[0x2b, 6, 1, 4, 1, 0x82, 0x37, 2, 1, 15];
const CABINET: &[u8] = &[0x2b, 6, 1, 4, 1, 0x82, 0x37, 2, 1, 25];
const CONTENT_TYPE: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 9, 3];
const MESSAGE_DIGEST: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 9, 4];
const SIGNING_TIME: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 9, 5];
const COUNTERSIGN: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 9, 6];
const RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 1];
const SHA1_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 5];
const SHA256_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 11];
const MD5: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 2, 5];
const SHA1: &[u8] = &[0x2b, 0x0e, 3, 2, 26];
const SHA256: &[u8] = &[0x60, 0x86, 0x48, 1, 0x65, 3, 4, 2, 1];

/// Parsed metadata is untrusted until authenticate succeeds.
pub struct Catalog<'a> {
    content: Value<'a>,
    entries: &'a [u8],
    certificates: Vec<Certificate<'a>>,
    signer: Signer<'a>,
}
/// Token can only be constructed after signature, path and time checks pass.
/// It borrows immutable catalog bytes, so checks cannot be invalidated by
/// mutating the catalog after authentication.
pub struct AuthenticatedCatalog<'a> {
    entries: &'a [u8],
    pub signer_sha256: [u8; 32],
    pub timestamp: Option<u64>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemberKind {
    PeImage,
    Cabinet,
    Other,
}
#[derive(Clone, Copy, Debug)]
pub struct Member<'a> {
    pub reference: &'a [u8],
    pub kind: MemberKind,
    pub algorithm: DigestAlgorithm,
    pub digest: &'a [u8],
}
/// Integrity-only evidence, deliberately separate from a trusted catalog.
#[derive(Debug)]
pub struct SignatureEvidence {
    pub signer_sha256: [u8; 32],
    pub timestamp: Option<u64>,
    pub certificate_count: usize,
    pub member_count: usize,
}
impl<'a> Catalog<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(Error::Limit);
        }
        let mut outer = Reader::new(der::one(bytes, 0x30)?.body);
        if outer.expect(6)?.body != SIGNED_DATA {
            return Err(Error::Der);
        }
        let mut sd = Reader::new(der::one(outer.expect(0xa0)?.body, 0x30)?.body);
        outer.finish()?;
        if der::integer(sd.expect(2)?)? != 1 {
            return Err(Error::UnsupportedPolicy);
        }
        let mut algorithms = Reader::new(sd.expect(0x31)?.body);
        let algorithm = DigestAlgorithm::from_oid(der::algorithm(algorithms.expect(0x30)?)?)?;
        algorithms.finish()?;
        if algorithm == DigestAlgorithm::LegacyMd5 {
            return Err(Error::UnsupportedAlgorithm);
        }
        let mut ci = Reader::new(sd.expect(0x30)?.body);
        if ci.expect(6)?.body != CTL {
            return Err(Error::UnsupportedPolicy);
        }
        let content = der::one(ci.expect(0xa0)?.body, 0x30)?;
        ci.finish()?;
        let mut certs = Reader::new(sd.expect(0xa0)?.body);
        let mut certificates = Vec::new();
        while !certs.empty() {
            if certificates.len() == 32 {
                return Err(Error::Limit);
            }
            let v = certs.expect(0x30)?;
            certificates.push(Certificate::parse(v.full)?);
        }
        // Embedded CRLs are not evidence until separately authenticated.
        if sd.peek() == Some(0xa1) {
            return Err(Error::UnsupportedPolicy);
        }
        let mut signers = Reader::new(sd.expect(0x31)?.body);
        let signer = Signer::parse(signers.expect(0x30)?)?;
        signers.finish()?; // Multiple primary signers require another policy.
        sd.finish()?;
        if signer.algorithm != algorithm {
            return Err(Error::Attributes);
        }
        let mut ctl = Reader::new(content.body);
        let mut usage = Reader::new(ctl.expect(0x30)?.body);
        if usage.expect(6)?.body != CATALOG_LIST {
            return Err(Error::UnsupportedPolicy);
        }
        usage.finish()?;
        if ctl.expect(4)?.body.len() != 16 {
            return Err(Error::Der);
        }
        der::time(ctl.read()?)?;
        if der::algorithm(ctl.expect(0x30)?)? != CATALOG_VERSION {
            return Err(Error::UnsupportedPolicy);
        }
        let entries = ctl.expect(0x30)?.body;
        // Optional catalog attributes; their enclosing signed bytes remain
        // covered. Validate their TLVs but do not use them as trust inputs.
        if !ctl.empty() {
            let mut attrs = Reader::new(der::one(ctl.expect(0xa0)?.body, 0x30)?.body);
            let mut count = 0;
            while !attrs.empty() {
                count += 1;
                if count > 32 {
                    return Err(Error::Limit);
                }
                let mut extension = Reader::new(attrs.expect(0x30)?.body);
                extension.expect(6)?;
                if extension.peek() == Some(1) {
                    return Err(Error::UnsupportedPolicy);
                }
                extension.expect(4)?;
                extension.finish()?;
            }
        }
        ctl.finish()?;
        let result = Self {
            content,
            entries,
            certificates,
            signer,
        };
        result.member_count()?; // Validate every member before any lookup.
        Ok(result)
    }
    pub fn members(&self) -> Members<'a> {
        Members {
            reader: Reader::new(self.entries),
            count: 0,
        }
    }
    pub fn member_count(&self) -> Result<usize> {
        self.members().try_fold(0, |n, m| m.map(|_| n + 1))
    }
    fn signing_certificate(&self, signer: &Signer<'a>) -> Result<&Certificate<'a>> {
        let mut matching = self
            .certificates
            .iter()
            .filter(|c| c.serial == signer.serial && c.issuer == signer.issuer);
        let cert = matching.next().ok_or(Error::Certificate)?;
        if matching.next().is_some() {
            return Err(Error::Certificate);
        }
        Ok(cert)
    }
    fn signature(&self) -> Result<(&Certificate<'a>, Option<(&Certificate<'a>, u64)>)> {
        // RFC 2315's ANY content hashes value octets, excluding the CTL
        // SEQUENCE tag/length. This differs from hashing the entire TLV.
        let cert = self.signing_certificate(&self.signer)?;
        self.signer.verify(cert, self.content.body, CTL)?;
        let timestamp = if let Some(counter) = self.signer.counter {
            let signer = Signer::parse(counter)?;
            if signer.counter.is_some() {
                return Err(Error::Timestamp);
            }
            let tsa = self.signing_certificate(&signer)?;
            signer.verify(tsa, self.signer.signature, DATA)?;
            Some((tsa, signer.time.ok_or(Error::Timestamp)?))
        } else {
            None
        };
        Ok((cert, timestamp))
    }
    /// Check cryptographic integrity without granting trust or checking expiry.
    /// Intended for diagnostics; it cannot produce AuthenticatedCatalog.
    pub fn verify_signatures(&self) -> Result<SignatureEvidence> {
        let (cert, stamp) = self.signature()?;
        Ok(SignatureEvidence {
            signer_sha256: crate::crypto::hash::sha256(cert.der),
            timestamp: stamp.map(|(_, t)| t),
            certificate_count: self.certificates.len(),
            member_count: self.member_count()?,
        })
    }
    /// Validate explicit trust, both signing purposes, authenticated timestamp,
    /// certificate validity and caller-provided revocation status. The signed
    /// signingTime of the primary signer alone never extends its validity.
    pub fn authenticate(&self, policy: &TrustPolicy<'_>) -> Result<AuthenticatedCatalog<'a>> {
        if policy.anchors.is_empty() {
            return Err(Error::Untrusted);
        }
        if policy.anchors.len() > 16 {
            return Err(Error::Limit);
        }
        let (cert, stamp) = self.signature()?;
        if let Some(counter) = self.signer.counter {
            if Signer::parse(counter)?.algorithm == DigestAlgorithm::LegacyMd5
                && !policy.allow_legacy_md5
            {
                return Err(Error::UnsupportedAlgorithm);
            }
        }
        let time = if let Some((tsa, time)) = stamp {
            if time > policy.evaluation_time.0 {
                return Err(Error::Timestamp);
            }
            x509::chain(tsa, &self.certificates, policy, time, Purpose::Timestamp)?;
            time
        } else {
            policy.evaluation_time.0
        };
        x509::chain(cert, &self.certificates, policy, time, Purpose::CodeSigning)?;
        Ok(AuthenticatedCatalog {
            entries: self.entries,
            signer_sha256: crate::crypto::hash::sha256(cert.der),
            timestamp: stamp.map(|(_, t)| t),
        })
    }
}
impl<'a> AuthenticatedCatalog<'a> {
    pub fn members(&self) -> Members<'a> {
        Members {
            reader: Reader::new(self.entries),
            count: 0,
        }
    }
    /// Only compare an already-computed digest of the indicated SIP kind.
    /// A name or a matching raw file hash cannot authorize a PE image.
    pub fn require_member(
        &self,
        kind: MemberKind,
        algorithm: DigestAlgorithm,
        digest: &[u8],
    ) -> Result<()> {
        if digest.len() != algorithm.len() {
            return Err(Error::Member);
        }
        for member in self.members() {
            let member = member?;
            if member.kind == kind && member.algorithm == algorithm && member.digest == digest {
                return Ok(());
            }
        }
        Err(Error::MemberNotFound)
    }
}

pub struct Members<'a> {
    reader: Reader<'a>,
    count: usize,
}
impl<'a> Iterator for Members<'a> {
    type Item = Result<Member<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.reader.empty() {
            return None;
        }
        self.count += 1;
        if self.count > 100_000 {
            self.reader = Reader::new(&[]);
            return Some(Err(Error::Limit));
        }
        let result = self.reader.expect(0x30).and_then(parse_member);
        if result.is_err() {
            self.reader = Reader::new(&[]);
        }
        Some(result)
    }
}
fn attribute(value: Value<'_>) -> Result<(&[u8], Value<'_>)> {
    let mut r = Reader::new(value.body);
    let oid = r.expect(6)?.body;
    let values = r.expect(0x31)?;
    r.finish()?;
    Ok((oid, values))
}
fn parse_member(value: Value<'_>) -> Result<Member<'_>> {
    let mut r = Reader::new(value.body);
    let reference = r.expect(4)?.body;
    let mut attrs = Reader::new(r.expect(0x31)?.body);
    r.finish()?;
    let mut member = None;
    let mut count = 0;
    while !attrs.empty() {
        count += 1;
        if count > 32 {
            return Err(Error::Limit);
        }
        let (oid, values) = attribute(attrs.expect(0x30)?)?;
        if oid != INDIRECT {
            continue;
        }
        if member.is_some() {
            return Err(Error::Member);
        }
        let mut indirect = Reader::new(der::one(values.body, 0x30)?.body);
        let mut data = Reader::new(indirect.expect(0x30)?.body);
        let kind = match data.expect(6)?.body {
            PE_IMAGE => MemberKind::PeImage,
            CABINET => MemberKind::Cabinet,
            _ => MemberKind::Other,
        };
        if !data.empty() {
            data.read()?;
        }
        data.finish()?;
        let mut info = Reader::new(indirect.expect(0x30)?.body);
        let algorithm = DigestAlgorithm::from_oid(der::algorithm(info.expect(0x30)?)?)?;
        let digest = info.expect(4)?.body;
        info.finish()?;
        indirect.finish()?;
        if digest.len() != algorithm.len() {
            return Err(Error::Member);
        }
        // SP1 CTL tags are uppercase ASCII hex in UTF-16LE with a NUL.
        if reference.len() != digest.len() * 4 + 2 || !reference.ends_with(&[0, 0]) {
            return Err(Error::Member);
        }
        for (pair, &byte) in reference[..reference.len() - 2].chunks_exact(4).zip(digest) {
            let hex = |n: u8| if n < 10 { b'0' + n } else { b'A' + n - 10 };
            if pair != [hex(byte >> 4), 0, hex(byte & 15), 0] {
                return Err(Error::Member);
            }
        }
        member = Some(Member {
            reference,
            kind,
            algorithm,
            digest,
        });
    }
    member.ok_or(Error::Member)
}

struct Signer<'a> {
    issuer: &'a [u8],
    serial: &'a [u8],
    algorithm: DigestAlgorithm,
    attrs: Value<'a>,
    signature: &'a [u8],
    counter: Option<Value<'a>>,
    content_type: Option<&'a [u8]>,
    digest: &'a [u8],
    time: Option<u64>,
}
impl<'a> Signer<'a> {
    fn parse(value: Value<'a>) -> Result<Self> {
        let mut r = Reader::new(value.body);
        if der::integer(r.expect(2)?)? != 1 {
            return Err(Error::UnsupportedPolicy);
        }
        let mut id = Reader::new(r.expect(0x30)?.body);
        let issuer = id.expect(0x30)?.full;
        let serial = der::positive(id.expect(2)?)?;
        id.finish()?;
        let algorithm = DigestAlgorithm::from_oid(der::algorithm(r.expect(0x30)?)?)?;
        let attrs = r.expect(0xa0)?;
        let signature_algorithm = der::algorithm(r.expect(0x30)?)?;
        if signature_algorithm != RSA
            && !matches!(
                (algorithm, signature_algorithm),
                (DigestAlgorithm::Sha1, SHA1_RSA) | (DigestAlgorithm::Sha256, SHA256_RSA)
            )
        {
            return Err(Error::UnsupportedAlgorithm);
        }
        let signature = r.expect(4)?.body;
        let mut counter = None;
        if !r.empty() {
            let mut unsigned = Reader::new(r.expect(0xa1)?.body);
            while !unsigned.empty() {
                let (oid, values) = attribute(unsigned.expect(0x30)?)?;
                if oid != COUNTERSIGN || counter.is_some() {
                    return Err(Error::UnsupportedPolicy);
                }
                counter = Some(der::one(values.body, 0x30)?);
            }
        }
        r.finish()?;
        let mut signed = Reader::new(attrs.body);
        let mut content_type = None;
        let mut digest = None;
        let mut time = None;
        let mut seen = Vec::new();
        let mut previous = &[][..];
        while !signed.empty() {
            if seen.len() == 32 {
                return Err(Error::Limit);
            }
            let attr = signed.expect(0x30)?;
            if attr.full < previous {
                return Err(Error::Attributes);
            }
            previous = attr.full;
            let (oid, values) = attribute(attr)?;
            if seen.contains(&oid) {
                return Err(Error::Attributes);
            }
            seen.push(oid);
            match oid {
                CONTENT_TYPE => content_type = Some(der::one(values.body, 6)?.body),
                MESSAGE_DIGEST => digest = Some(der::one(values.body, 4)?.body),
                SIGNING_TIME => {
                    let mut t = Reader::new(values.body);
                    time = Some(der::time(t.read()?)?);
                    t.finish()?;
                }
                _ => (), // Authenticode publisher attributes remain signed.
            }
        }
        let digest = digest.ok_or(Error::Attributes)?;
        if digest.len() != algorithm.len() {
            return Err(Error::Attributes);
        }
        Ok(Self {
            issuer,
            serial,
            algorithm,
            attrs,
            signature,
            counter,
            content_type,
            digest,
            time,
        })
    }
    fn verify(&self, cert: &Certificate<'_>, content: &[u8], expected_type: &[u8]) -> Result<()> {
        if self.content_type != Some(expected_type)
            && !(expected_type == DATA && self.content_type.is_none())
        {
            return Err(Error::Attributes);
        }
        if self.algorithm.hash(content) != self.digest {
            return Err(Error::ContentDigest);
        }
        // SignedAttributes are hashed with the SET tag, not the IMPLICIT [0].
        let mut signed = self.attrs.full.to_vec();
        signed[0] = 0x31;
        if expected_type == DATA && self.algorithm == DigestAlgorithm::LegacyMd5 {
            cert.verify(&signed, self.algorithm, self.signature)
                .or_else(|_| cert.verify_legacy_counter(&signed, self.signature))
        } else {
            cert.verify(&signed, self.algorithm, self.signature)
        }
    }
}

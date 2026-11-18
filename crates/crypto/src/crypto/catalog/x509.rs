//! Narrow RSA X.509 path validation for catalog and countersignature chains.
//! RFC 5280: validity, signatures, CA/pathLen, KU, EKU, critical extensions.
use super::der::{self, Reader, Value};
use super::{DigestAlgorithm, Error, Purpose, Result, RevocationStatus, TrustPolicy};
use crate::crypto::rsa::{BigUint, RsaPublicKey};

const RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 1];
const MD5_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 4];
const SHA1_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 5];
const SHA256_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 11];
const CODE: &[u8] = &[0x2b, 6, 1, 5, 5, 7, 3, 3];
const STAMP: &[u8] = &[0x2b, 6, 1, 5, 5, 7, 3, 8];
const ANY: &[u8] = &[0x55, 0x1d, 0x25, 0];

#[derive(Clone)]
pub(super) struct Certificate<'a> {
    pub der: &'a [u8],
    pub issuer: &'a [u8],
    pub serial: &'a [u8],
    pub(super) subject: &'a [u8],
    tbs: &'a [u8],
    signature: &'a [u8],
    algorithm: DigestAlgorithm,
    key: &'a [u8],
    before: u64,
    after: u64,
    ca: bool,
    path_len: Option<u32>,
    ku: Option<u8>,
    eku: Option<&'a [u8]>,
}
impl<'a> Certificate<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > 16 * 1024 {
            return Err(Error::Limit);
        }
        let mut r = Reader::new(der::one(bytes, 0x30)?.body);
        let tbs = r.expect(0x30)?;
        let algorithm_value = r.expect(0x30)?;
        let algorithm = match der::algorithm(algorithm_value)? {
            x if x == MD5_RSA => DigestAlgorithm::LegacyMd5,
            x if x == SHA1_RSA => DigestAlgorithm::Sha1,
            x if x == SHA256_RSA => DigestAlgorithm::Sha256,
            _ => return Err(Error::UnsupportedAlgorithm),
        };
        let signature = der::bit_string(r.expect(3)?)?;
        r.finish()?;
        let mut t = Reader::new(tbs.body);
        let version = if t.peek() == Some(0xa0) {
            der::integer(der::one(t.read()?.body, 2)?)?
        } else {
            0
        };
        if version > 2 {
            return Err(Error::Certificate);
        }
        let serial = der::positive(t.expect(2)?)?;
        if serial.len() > 20 || serial.iter().all(|&b| b == 0) {
            return Err(Error::Certificate);
        }
        if t.expect(0x30)?.full != algorithm_value.full {
            return Err(Error::Certificate);
        }
        let issuer = t.expect(0x30)?.full;
        let mut dates = Reader::new(t.expect(0x30)?.body);
        let before = der::time(dates.read()?)?;
        let after = der::time(dates.read()?)?;
        dates.finish()?;
        if after < before {
            return Err(Error::Certificate);
        }
        let subject = t.expect(0x30)?.full;
        let mut spki = Reader::new(t.expect(0x30)?.body);
        if der::algorithm(spki.expect(0x30)?)? != RSA {
            return Err(Error::UnsupportedAlgorithm);
        }
        let key = der::bit_string(spki.expect(3)?)?;
        spki.finish()?;
        parse_key(key)?;
        let mut result = Self {
            der: bytes,
            issuer,
            serial,
            subject,
            tbs: tbs.full,
            signature,
            algorithm,
            key,
            before,
            after,
            ca: false,
            path_len: None,
            ku: None,
            eku: None,
        };
        // Unique IDs and other versions are outside this supported profile.
        if !t.empty() {
            if version != 2 {
                return Err(Error::Certificate);
            }
            let extensions = der::one(t.expect(0xa3)?.body, 0x30)?;
            result.extensions(extensions)?;
        }
        t.finish()?;
        Ok(result)
    }
    fn extensions(&mut self, value: Value<'a>) -> Result<()> {
        let mut r = Reader::new(value.body);
        let mut seen: super::alloc::vec::Vec<&[u8]> = super::alloc::vec::Vec::new();
        while !r.empty() {
            if seen.len() == 32 {
                return Err(Error::Limit);
            }
            let mut e = Reader::new(r.expect(0x30)?.body);
            let oid = e.expect(6)?.body;
            if seen.contains(&oid) {
                return Err(Error::Certificate);
            }
            seen.push(oid);
            let critical = if e.peek() == Some(1) {
                if e.expect(1)?.body != [0xff] {
                    return Err(Error::Der);
                }
                true
            } else {
                false
            };
            let data = e.expect(4)?.body;
            e.finish()?;
            match oid {
                [0x55, 0x1d, 0x13] => {
                    let mut bc = Reader::new(der::one(data, 0x30)?.body);
                    if bc.peek() == Some(1) {
                        if bc.expect(1)?.body != [0xff] {
                            return Err(Error::Der);
                        }
                        self.ca = true;
                    }
                    if !bc.empty() {
                        self.path_len = Some(der::integer(bc.expect(2)?)?);
                    }
                    bc.finish()?;
                    if self.path_len.is_some() && !self.ca {
                        return Err(Error::Certificate);
                    }
                }
                [0x55, 0x1d, 0x0f] => {
                    let bits = der::one(data, 3)?.body;
                    if bits.len() < 2
                        || bits.len() > 3
                        || bits[0] > 7
                        || bits[bits.len() - 1] & ((1u16 << bits[0]) - 1) as u8 != 0
                    {
                        return Err(Error::Der);
                    }
                    self.ku = Some(bits[1]);
                }
                [0x55, 0x1d, 0x25] => {
                    let list = der::one(data, 0x30)?.body;
                    let mut purposes = Reader::new(list);
                    if purposes.empty() {
                        return Err(Error::Certificate);
                    }
                    while !purposes.empty() {
                        purposes.expect(6)?;
                    }
                    self.eku = Some(list);
                }
                // Unsupported path constraints cannot be silently ignored.
                [0x55, 0x1d, 0x1e | 0x24 | 0x36] => return Err(Error::UnsupportedPolicy),
                _ if critical => return Err(Error::UnsupportedPolicy),
                _ => (),
            }
        }
        Ok(())
    }
    fn permits(&self, purpose: Purpose, leaf: bool) -> Result<()> {
        let wanted = match purpose {
            Purpose::CodeSigning => CODE,
            Purpose::Timestamp => STAMP,
        };
        if let Some(list) = self.eku {
            let mut r = Reader::new(list);
            while !r.empty() {
                let oid = r.expect(6)?.body;
                if oid == wanted || (!leaf && oid == ANY) {
                    return Ok(());
                }
            }
            return Err(Error::Purpose);
        }
        if leaf {
            Err(Error::Purpose)
        } else {
            Ok(())
        }
    }
    fn valid(&self, time: u64) -> Result<()> {
        if time < self.before || time > self.after {
            Err(Error::Expired)
        } else {
            Ok(())
        }
    }
    /// Direct CRLs require a CA with an explicit cRLSign key usage in this
    /// conservative profile. Trust in this issuer is established by chain().
    pub(super) fn crl_issuer(&self, time: u64) -> Result<()> {
        self.valid(time)?;
        if !self.ca || self.ku.is_none_or(|ku| ku & 2 == 0) {
            return Err(Error::Purpose);
        }
        Ok(())
    }
    pub fn verify(&self, data: &[u8], digest: DigestAlgorithm, sig: &[u8]) -> Result<()> {
        let digest = digest.hash(data);
        if parse_key(self.key)?.verify_pkcs1v15(digest.as_slice(), sig) {
            Ok(())
        } else {
            Err(Error::Signature)
        }
    }
    pub fn verify_legacy_counter(&self, data: &[u8], sig: &[u8]) -> Result<()> {
        let digest = DigestAlgorithm::LegacyMd5.hash(data);
        if parse_key(self.key)?.verify_legacy_raw_md5(&digest, sig) {
            Ok(())
        } else {
            Err(Error::Signature)
        }
    }
}
fn parse_key(bytes: &[u8]) -> Result<RsaPublicKey> {
    let mut r = Reader::new(der::one(bytes, 0x30)?.body);
    let n = der::positive(r.expect(2)?)?;
    let e = der::integer(r.expect(2)?)?;
    r.finish()?;
    if !(128..=512).contains(&n.len())
        || n[0] & 128 == 0
        || n[n.len() - 1] & 1 == 0
        || e < 3
        || e & 1 == 0
    {
        return Err(Error::UnsupportedAlgorithm);
    }
    Ok(RsaPublicKey {
        n: BigUint::from_bytes(n),
        e: BigUint::from(e),
    })
}

pub(super) fn chain<'a>(
    leaf: &Certificate<'a>,
    certificates: &[Certificate<'a>],
    policy: &TrustPolicy<'a>,
    time: u64,
    purpose: Purpose,
) -> Result<()> {
    leaf.permits(purpose, true)?;
    if leaf.ca || leaf.ku.is_some_and(|v| v & 128 == 0) {
        return Err(Error::Purpose);
    }
    let mut path = super::alloc::vec::Vec::new();
    let mut budget = 128usize;
    walk(
        leaf,
        certificates,
        policy,
        time,
        purpose,
        0,
        &mut path,
        &mut budget,
    )
}
fn walk<'a>(
    cert: &Certificate<'a>,
    certificates: &[Certificate<'a>],
    policy: &TrustPolicy<'a>,
    time: u64,
    purpose: Purpose,
    ca_below: u32,
    path: &mut super::alloc::vec::Vec<&'a [u8]>,
    budget: &mut usize,
) -> Result<()> {
    if *budget == 0 || path.len() >= 8 {
        return Err(Error::Limit);
    }
    *budget -= 1;
    cert.valid(time)?;
    if cert.algorithm == DigestAlgorithm::LegacyMd5 && !policy.allow_legacy_md5 {
        return Err(Error::UnsupportedAlgorithm);
    }
    cert.permits(purpose, false)?;
    if policy.anchors.contains(&cert.der) {
        return Ok(());
    }
    if path.contains(&cert.der) {
        return Err(Error::Untrusted);
    }
    path.push(cert.der);
    let mut error = Error::Untrusted;
    // External anchors are parsed only from caller-selected, provisioned DER.
    // No certificate carried in SignedData becomes trusted by being present.
    for parent in certificates
        .iter()
        .cloned()
        .map(Ok)
        .chain(policy.anchors.iter().map(|d| Certificate::parse(d)))
    {
        let parent = parent?;
        if parent.subject != cert.issuer || parent.der == cert.der {
            continue;
        }
        let anchor = policy.anchors.contains(&parent.der);
        if !anchor && (!parent.ca || parent.ku.is_some_and(|v| v & 4 == 0)) {
            continue;
        }
        if parent.path_len.is_some_and(|limit| ca_below > limit) {
            continue;
        }
        if parent
            .verify(cert.tbs, cert.algorithm, cert.signature)
            .is_err()
        {
            continue;
        }
        match (policy.revocation)(cert.der, parent.der, time) {
            RevocationStatus::Revoked => {
                error = Error::Revoked;
                continue;
            }
            RevocationStatus::Unknown => {
                error = Error::RevocationUnknown;
                continue;
            }
            RevocationStatus::Good => (),
        }
        let next_below = ca_below + u32::from(parent.ca && parent.subject != parent.issuer);
        match walk(
            &parent,
            certificates,
            policy,
            time,
            purpose,
            next_below,
            path,
            budget,
        ) {
            Ok(()) => {
                path.pop();
                return Ok(());
            }
            Err(e) => error = e,
        }
    }
    path.pop();
    Err(error)
}

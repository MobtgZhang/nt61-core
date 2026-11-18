//! Authenticated offline, direct, full CRLs (RFC 5280 section 5).
//!
//! No network fetching, OCSP, delta/indirect/partitioned CRLs, or distribution
//! point matching. Unsupported scopes are rejected, never treated as a full
//! issuer list. Callers provision CRLs; absence, expiry or invalid signatures
//! return Unknown. A Good status is meaningful only within the explicit
//! catalog trust/time policy that supplied the issuer certificate.
use super::alloc::{collections::BTreeSet, vec::Vec};
use super::der::{self, Reader, Value};
use super::x509::Certificate;
use super::{DigestAlgorithm, Error, Result, RevocationStatus};

pub struct Crl<'a> {
    tbs: &'a [u8],
    signature: &'a [u8],
    issuer: &'a [u8],
    algorithm: DigestAlgorithm,
    this_update: u64,
    next_update: u64,
    revoked: Vec<(&'a [u8], u64)>,
}

impl<'a> Crl<'a> {
    /// Parse bounded DER; parsing alone does not authenticate the list.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(Error::Limit);
        }
        let mut r = Reader::new(der::one(bytes, 0x30)?.body);
        let tbs = r.expect(0x30)?;
        let algorithm_value = r.expect(0x30)?;
        let algorithm = match der::algorithm(algorithm_value)? {
            [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 5] => DigestAlgorithm::Sha1,
            [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 1, 11] => DigestAlgorithm::Sha256,
            _ => return Err(Error::UnsupportedAlgorithm),
        };
        let signature = der::bit_string(r.expect(3)?)?;
        r.finish()?;
        let mut t = Reader::new(tbs.body);
        let v2 = if t.peek() == Some(2) {
            if der::integer(t.expect(2)?)? != 1 {
                return Err(Error::UnsupportedPolicy);
            }
            true
        } else {
            false
        };
        if t.expect(0x30)?.full != algorithm_value.full {
            return Err(Error::Certificate);
        }
        let issuer = t.expect(0x30)?.full;
        let this_update = der::time(t.read()?)?;
        // Require nextUpdate: an unbounded freshness interval cannot give Good.
        let next_update = der::time(t.read()?)?;
        if this_update >= next_update {
            return Err(Error::Certificate);
        }
        let mut revoked = Vec::new();
        let mut seen = BTreeSet::new();
        if t.peek() == Some(0x30) {
            let mut entries = Reader::new(t.expect(0x30)?.body);
            while !entries.empty() {
                if revoked.len() == 100_000 {
                    return Err(Error::Limit);
                }
                let mut entry = Reader::new(entries.expect(0x30)?.body);
                let serial = der::positive(entry.expect(2)?)?;
                if serial.len() > 20 || serial.iter().all(|&b| b == 0) || !seen.insert(serial) {
                    return Err(Error::Certificate);
                }
                let time = der::time(entry.read()?)?;
                if time > this_update {
                    return Err(Error::Certificate);
                }
                if !entry.empty() {
                    if !v2 {
                        return Err(Error::Certificate);
                    }
                    extensions(entry.expect(0x30)?, true)?;
                }
                entry.finish()?;
                revoked.push((serial, time));
            }
        }
        if !t.empty() {
            if !v2 {
                return Err(Error::Certificate);
            }
            extensions(der::one(t.expect(0xa0)?.body, 0x30)?, false)?;
        }
        t.finish()?;
        Ok(Self {
            tbs: tbs.full,
            signature,
            issuer,
            algorithm,
            this_update,
            next_update,
            revoked,
        })
    }

    fn status(
        &self,
        cert: &Certificate<'_>,
        issuer: &Certificate<'_>,
        time: u64,
    ) -> Result<RevocationStatus> {
        if self.issuer != issuer.subject || cert.issuer != issuer.subject {
            return Err(Error::Certificate);
        }
        if time < self.this_update || time >= self.next_update {
            return Err(Error::Expired);
        }
        issuer.crl_issuer(time)?;
        issuer.verify(self.tbs, self.algorithm, self.signature)?;
        Ok(
            if self
                .revoked
                .iter()
                .any(|&(serial, date)| serial == cert.serial && date <= time)
            {
                RevocationStatus::Revoked
            } else {
                RevocationStatus::Good
            },
        )
    }
}

/// Validated syntax for at most 32 provisioned full CRLs. Certificates and
/// signatures are checked against the *actual chain issuer* on each lookup.
pub struct CrlSet<'a> {
    crls: Vec<Crl<'a>>,
}
impl<'a> CrlSet<'a> {
    pub fn parse(bytes: &[&'a [u8]]) -> Result<Self> {
        if bytes.len() > 32 {
            return Err(Error::Limit);
        }
        Ok(Self {
            crls: bytes
                .iter()
                .map(|bytes| Crl::parse(bytes))
                .collect::<Result<_>>()?,
        })
    }

    /// Adapter for TrustPolicy::revocation. Unsupported/missing/expired or
    /// incorrectly signed evidence fails closed. A revoked result takes
    /// precedence over other valid full CRLs in the provisioned set.
    pub fn status(&self, cert: &[u8], issuer: &[u8], time: u64) -> RevocationStatus {
        let (Ok(cert), Ok(issuer)) = (Certificate::parse(cert), Certificate::parse(issuer)) else {
            return RevocationStatus::Unknown;
        };
        let mut status = RevocationStatus::Unknown;
        for crl in &self.crls {
            match crl.status(&cert, &issuer, time) {
                Ok(RevocationStatus::Revoked) => return RevocationStatus::Revoked,
                Ok(RevocationStatus::Good) => status = RevocationStatus::Good,
                _ => (),
            }
        }
        status
    }
}

fn extensions(value: Value<'_>, entry: bool) -> Result<()> {
    let mut r = Reader::new(value.body);
    let mut seen = BTreeSet::new();
    while !r.empty() {
        if seen.len() == 32 {
            return Err(Error::Limit);
        }
        let mut e = Reader::new(r.expect(0x30)?.body);
        let oid = e.expect(6)?.body;
        if !seen.insert(oid) {
            return Err(Error::Certificate);
        }
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
            [0x55, 0x1d, 0x15] if entry => {
                // removeFromCRL applies to delta lists, unsupported here.
                let reason = der::one(data, 10)?.body;
                if reason.len() != 1 || reason[0] > 10 || reason[0] == 7 || reason[0] == 8 {
                    return Err(Error::UnsupportedPolicy);
                }
            }
            [0x55, 0x1d, 0x18] if entry => {
                der::time(der::one(data, 0x18)?)?;
            }
            [0x55, 0x1d, 0x14] if !entry => {
                der::positive(der::one(data, 2)?)?;
            }
            [0x55, 0x1d, 0x23] if !entry => {
                // Only keyIdentifier; the CRL signature binds the actual key.
                let mut aki = Reader::new(der::one(data, 0x30)?.body);
                if aki.expect(0x80)?.body.is_empty() {
                    return Err(Error::Certificate);
                }
                aki.finish()?;
            }
            // Delta, issuingDistributionPoint, certificateIssuer, freshestCRL:
            // even noncritical variants must not authorize a full issuer list.
            [0x55, 0x1d, 0x1b | 0x1c | 0x1d | 0x2e] => return Err(Error::UnsupportedPolicy),
            _ if critical => return Err(Error::UnsupportedPolicy),
            _ => (),
        }
    }
    Ok(())
}

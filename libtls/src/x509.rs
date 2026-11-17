//! X.509 certificate parsing and chain validation (RFC 5280 subset).
//!
//! Supported: RSA (PKCS#1 v1.5 and PSS with SHA-256) and ECDSA P-256
//! signatures, `basicConstraints`, `subjectAltName` (dNSName / iPAddress),
//! validity windows, and a bounded chain walk. Key usage and name constraints
//! are not enforced; revocation is out of scope (documented gap).

use alloc::vec::Vec;

use crate::der::*;
use libcrypto::{verify_p256_sha256, verify_p384_sha384, RsaError, RsaPublicKey};

/// X.509 failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X509Error {
    Der(DerError),
    UnsupportedAlgorithm,
    BadCertificate,
    BadSignature,
    NotTrusted,
    HostnameMismatch,
    Expired,
    NotYetValid,
    ChainTooLong,
    NotACa,
    PathLenExceeded,
}

impl X509Error {
    /// Short description for diagnostics (CLI messages, logs).
    pub fn as_str(self) -> &'static str {
        match self {
            X509Error::Der(_) => "certificate chain: malformed DER",
            X509Error::UnsupportedAlgorithm => "certificate chain: unsupported algorithm",
            X509Error::BadCertificate => "certificate chain: invalid certificate",
            X509Error::BadSignature => "certificate chain: bad signature",
            X509Error::NotTrusted => "certificate chain: not anchored in the trust store",
            X509Error::HostnameMismatch => "hostname mismatch",
            X509Error::Expired => "certificate expired",
            X509Error::NotYetValid => "certificate not yet valid",
            X509Error::ChainTooLong => "certificate chain: too long",
            X509Error::NotACa => "certificate chain: issuer is not a CA",
            X509Error::PathLenExceeded => "certificate chain: path length exceeded",
        }
    }
}

impl From<DerError> for X509Error {
    fn from(e: DerError) -> Self {
        X509Error::Der(e)
    }
}

/// Signature algorithm of a certificate (SHA-256 variants only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigAlg {
    RsaPkcs1Sha256,
    RsaPssSha256,
    EcdsaP256Sha256,
    EcdsaP384Sha384,
}

const OID_SHA256_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
const OID_RSA_PSS: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a];
const OID_ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const OID_ECDSA_SHA384: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x03];
const OID_RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_PRIME256V1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const OID_SECP384R1: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];
const OID_BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];
const OID_SAN: &[u8] = &[0x55, 0x1d, 0x11];
const OID_KEY_USAGE: &[u8] = &[0x55, 0x1d, 0x0f];
const OID_EXT_KEY_USAGE: &[u8] = &[0x55, 0x1d, 0x25];
const OID_SERVER_AUTH: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];
// Common critical extensions we understand well enough to accept without
// acting on them (policies, AIA/CRL pointers, SKI/AKI). Anything else that is
// critical is rejected per RFC 5280.
const KNOWN_CRITICAL: &[&[u8]] = &[
    &[0x55, 0x1d, 0x0e], // subjectKeyIdentifier
    &[0x55, 0x1d, 0x23], // authorityKeyIdentifier
    &[0x55, 0x1d, 0x20], // certificatePolicies
    &[0x55, 0x1d, 0x1f], // cRLDistributionPoints
    &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x01], // authorityInfoAccess
    &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x18], // TLS Feature
    &[0x2b, 0x06, 0x01, 0x04, 0x01, 0xd6, 0x79, 0x02, 0x04, 0x02], // SCT list
];

/// A parsed certificate. All slices point into the input DER.
#[derive(Debug, Clone)]
pub struct Certificate<'a> {
    /// TBSCertificate encoding (signed bytes).
    pub tbs: &'a [u8],
    pub issuer: &'a [u8],
    pub subject: &'a [u8],
    pub not_before: i64,
    pub not_after: i64,
    /// SubjectPublicKeyInfo encoding.
    pub spki: &'a [u8],
    pub dns_names: Vec<&'a [u8]>,
    pub ip_addresses: Vec<[u8; 4]>,
    pub is_ca: bool,
    pub path_len: Option<u8>,
    /// `keyUsage.digitalSignature` (true when the extension is absent).
    pub digital_signature: bool,
    /// `extKeyUsage` contains `serverAuth` (true when the extension is absent).
    pub server_auth: bool,
    /// Signature algorithm; `None` when unsupported (usable as a trust anchor,
    /// not verifiable as an issuer).
    pub sig_alg: Option<SigAlg>,
    pub signature: &'a [u8],
}

/// Host identity to match against the leaf SAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host<'a> {
    Dns(&'a str),
    Ip([u8; 4]),
}

pub fn parse_certificate(der: &[u8]) -> core::result::Result<Certificate<'_>, X509Error> {
    let mut outer = Der::new(der);
    let mut cert = outer.expect_sequence()?;
    let tbs_full = cert.expect(TAG_SEQUENCE)?;
    let mut sig_alg_seq = cert.expect_sequence()?;
    let sig_oid = oid_bytes(&sig_alg_seq.next_tlv()?)?;
    let sig_alg = match sig_oid {
        OID_SHA256_RSA => Some(SigAlg::RsaPkcs1Sha256),
        OID_RSA_PSS => Some(SigAlg::RsaPssSha256),
        OID_ECDSA_SHA256 => Some(SigAlg::EcdsaP256Sha256),
        OID_ECDSA_SHA384 => Some(SigAlg::EcdsaP384Sha384),
        _ => None,
    };
    let signature = bit_string_bytes(&cert.expect(TAG_BIT_STRING)?)?;

    let mut t = Der::new(tbs_full.value);
    // version [0] EXPLICIT INTEGER DEFAULT v1
    if t.peek_tag() == Some(0xa0) {
        let mut v = Der::new(t.next_tlv()?.value);
        let _version = integer_bytes(&v.expect(TAG_INTEGER)?)?;
    }
    let _serial = integer_bytes(&t.expect(TAG_INTEGER)?)?;
    // signature AlgorithmIdentifier inside TBS
    let _inner_alg = t.expect_sequence()?;
    let issuer = t.expect(TAG_SEQUENCE)?.full;
    let mut validity = t.expect_sequence()?;
    let nb = validity.next_tlv()?;
    let na = validity.next_tlv()?;
    let not_before = time_to_unix(nb.tag, nb.value)?;
    let not_after = time_to_unix(na.tag, na.value)?;
    let subject = t.expect(TAG_SEQUENCE)?.full;
    let spki = t.expect(TAG_SEQUENCE)?.full;

    let mut dns_names = Vec::new();
    let mut ip_addresses = Vec::new();
    let mut is_ca = false;
    let mut path_len = None;
    let mut digital_signature = true;
    let mut server_auth = true;

    // Optional issuerUniqueID [1], subjectUniqueID [2], extensions [3].
    while !t.is_empty() {
        let tlv = t.next_tlv()?;
        if tlv.tag == 0xa3 {
            let mut exts = Der::new(tlv.value);
            let mut list = exts.expect_sequence()?;
            while !list.is_empty() {
                let mut ext = list.expect_sequence()?;
                let oid = oid_bytes(&ext.next_tlv()?)?;
                let mut critical = false;
                let mut value = ext.next_tlv()?;
                if value.tag == TAG_BOOLEAN {
                    critical = boolean(&value)?;
                    value = ext.next_tlv()?;
                }
                if value.tag != TAG_OCTET_STRING {
                    return Err(X509Error::BadCertificate);
                }
                match oid {
                    OID_BASIC_CONSTRAINTS => {
                        let mut bc = Der::new(value.value);
                        let mut seq = bc.expect_sequence()?;
                        if !seq.is_empty() && seq.peek_tag() == Some(TAG_BOOLEAN) {
                            is_ca = boolean(&seq.next_tlv()?)?;
                        }
                        if !seq.is_empty() && seq.peek_tag() == Some(TAG_INTEGER) {
                            let p = integer_bytes(&seq.next_tlv()?)?;
                            path_len = Some(p.iter().fold(0u8, |acc, b| acc.saturating_mul(10).saturating_add(*b)));
                        }
                    }
                    OID_KEY_USAGE => {
                        // BIT STRING: bit 0 is digitalSignature.
                        let mut ku = Der::new(value.value);
                        let bits = bit_string_bytes(&ku.next_tlv()?)?;
                        digital_signature =
                            bits.first().map(|b| b & 0x80 != 0).unwrap_or(true);
                    }
                    OID_EXT_KEY_USAGE => {
                        let mut eku = Der::new(value.value);
                        let mut list = eku.expect_sequence()?;
                        let mut found = false;
                        while !list.is_empty() {
                            if oid_bytes(&list.next_tlv()?)? == OID_SERVER_AUTH {
                                found = true;
                            }
                        }
                        server_auth = found;
                    }
                    OID_SAN => {
                        let mut san = Der::new(value.value);
                        let mut names = san.expect_sequence()?;
                        while !names.is_empty() {
                            let name = names.next_tlv()?;
                            match name.tag {
                                0x82 => dns_names.push(name.value),
                                0x87 if name.value.len() == 4 => {
                                    ip_addresses.push([
                                        name.value[0],
                                        name.value[1],
                                        name.value[2],
                                        name.value[3],
                                    ]);
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {
                        // RFC 5280: reject unknown *critical* extensions.
                        if critical && !KNOWN_CRITICAL.contains(&oid) {
                            return Err(X509Error::UnsupportedAlgorithm);
                        }
                    }
                }
            }
        }
    }

    Ok(Certificate {
        tbs: tbs_full.full,
        issuer,
        subject,
        not_before,
        not_after,
        spki,
        dns_names,
        ip_addresses,
        is_ca,
        path_len,
        digital_signature,
        server_auth,
        sig_alg,
        signature,
    })
}

/// SubjectPublicKeyInfo → RSA public key.
pub fn rsa_public_key(spki: &[u8]) -> core::result::Result<RsaPublicKey, X509Error> {
    let mut s = Der::new(spki);
    let mut seq = s.expect_sequence()?;
    let mut alg = seq.expect_sequence()?;
    let oid = oid_bytes(&alg.next_tlv()?)?;
    if oid != OID_RSA_ENCRYPTION {
        return Err(X509Error::UnsupportedAlgorithm);
    }
    let key = bit_string_bytes(&seq.expect(TAG_BIT_STRING)?)?;
    let mut k = Der::new(key);
    let mut rs = k.expect_sequence()?;
    let n = integer_bytes(&rs.next_tlv()?)?.to_vec();
    let e = integer_bytes(&rs.next_tlv()?)?.to_vec();
    Ok(RsaPublicKey::new(n, e))
}

/// SubjectPublicKeyInfo → NIST curve affine coordinates.
///
/// `curve_oid` selects the expected curve (`OID_PRIME256V1` or
/// `OID_SECP384R1`); the key encoding is `0x04 || X || Y` with 32- or 48-byte
/// coordinates.
fn ec_public_key(
    spki: &[u8],
    curve_oid: &[u8],
) -> core::result::Result<(Vec<u8>, Vec<u8>), X509Error> {
    let mut s = Der::new(spki);
    let mut seq = s.expect_sequence()?;
    let mut alg = seq.expect_sequence()?;
    let oid = oid_bytes(&alg.next_tlv()?)?;
    if oid != OID_EC_PUBLIC_KEY {
        return Err(X509Error::UnsupportedAlgorithm);
    }
    let curve = oid_bytes(&alg.next_tlv()?)?;
    if curve != curve_oid {
        return Err(X509Error::UnsupportedAlgorithm);
    }
    let key = bit_string_bytes(&seq.expect(TAG_BIT_STRING)?)?;
    if key.is_empty() {
        return Err(X509Error::BadCertificate);
    }
    let coord = (key.len() - 1) / 2;
    if key[0] != 0x04 || !matches!(coord, 32 | 48) || key.len() != 1 + 2 * coord {
        return Err(X509Error::BadCertificate);
    }
    Ok((key[1..1 + coord].to_vec(), key[1 + coord..].to_vec()))
}

/// SubjectPublicKeyInfo → P-256 affine coordinates.
pub fn p256_public_key(spki: &[u8]) -> core::result::Result<(Vec<u8>, Vec<u8>), X509Error> {
    ec_public_key(spki, OID_PRIME256V1)
}

/// SubjectPublicKeyInfo → P-384 affine coordinates.
pub fn p384_public_key(spki: &[u8]) -> core::result::Result<(Vec<u8>, Vec<u8>), X509Error> {
    ec_public_key(spki, OID_SECP384R1)
}

/// Verify that `child` was signed by `issuer`.
pub fn verify_issuer(child: &Certificate<'_>, issuer: &Certificate<'_>) -> core::result::Result<(), X509Error> {
    if child.issuer != issuer.subject {
        return Err(X509Error::NotTrusted);
    }
    let alg = child.sig_alg.ok_or(X509Error::UnsupportedAlgorithm)?;
    match alg {
        SigAlg::RsaPkcs1Sha256 | SigAlg::RsaPssSha256 => {
            let key = rsa_public_key(issuer.spki)?;
            let result = match alg {
                SigAlg::RsaPkcs1Sha256 => key.verify_pkcs1_sha256(child.tbs, child.signature),
                _ => key.verify_pss_sha256(child.tbs, child.signature, 32),
            };
            match result {
                Ok(()) => Ok(()),
                Err(RsaError::VerificationFailed) | Err(RsaError::BadSignature) => {
                    Err(X509Error::BadSignature)
                }
                Err(_) => Err(X509Error::BadCertificate),
            }
        }
        SigAlg::EcdsaP256Sha256 | SigAlg::EcdsaP384Sha384 => {
            let (x, y) = match alg {
                SigAlg::EcdsaP384Sha384 => p384_public_key(issuer.spki)?,
                _ => p256_public_key(issuer.spki)?,
            };
            let mut der = Der::new(child.signature);
            let mut sig = der.expect_sequence()?;
            let r = integer_bytes(&sig.next_tlv()?)?;
            let s = integer_bytes(&sig.next_tlv()?)?;
            let result = match alg {
                SigAlg::EcdsaP384Sha384 => verify_p384_sha384(&x, &y, child.tbs, r, s),
                _ => verify_p256_sha256(&x, &y, child.tbs, r, s),
            };
            result.map_err(|_| X509Error::BadSignature)
        }
    }
}

fn hostname_matches(leaf: &Certificate<'_>, host: Host<'_>) -> bool {
    match host {
        Host::Ip(addr) => leaf.ip_addresses.contains(&addr),
        Host::Dns(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            for san in &leaf.dns_names {
                let Ok(san) = core::str::from_utf8(san) else {
                    continue;
                };
                let san = san.trim_end_matches('.').to_ascii_lowercase();
                if san == name {
                    return true;
                }
                // Single leftmost-label wildcard only.
                if let Some(suffix) = san.strip_prefix("*.") {
                    if let Some(rest) = name.strip_suffix(suffix) {
                        if rest.ends_with('.') && !rest[..rest.len() - 1].contains('.') {
                            return true;
                        }
                    }
                }
            }
            false
        }
    }
}

/// Validate a certificate chain and the leaf identity.
///
/// `chain` is the server-provided order (leaf first). `roots` are DER-encoded
/// trust anchors. `now` must come from the trusted clock.
pub fn verify_chain(
    chain: &[Certificate<'_>],
    roots: &[Vec<u8>],
    host: Host<'_>,
    now: i64,
) -> core::result::Result<(), X509Error> {
    if chain.is_empty() {
        return Err(X509Error::BadCertificate);
    }
    let leaf = &chain[0];
    if !leaf.digital_signature || !leaf.server_auth {
        return Err(X509Error::BadCertificate);
    }
    if !hostname_matches(leaf, host) {
        return Err(X509Error::HostnameMismatch);
    }
    // Bound the depth before walking.
    if chain.len() > 8 {
        return Err(X509Error::ChainTooLong);
    }

    let check_time = |c: &Certificate<'_>| -> core::result::Result<(), X509Error> {
        if now < c.not_before {
            return Err(X509Error::NotYetValid);
        }
        if now > c.not_after {
            return Err(X509Error::Expired);
        }
        Ok(())
    };

    // A trust anchor that does not parse is a broken trust store, not an
    // untrusted server: report it instead of silently ignoring the anchor and
    // failing the connection with "not anchored".
    let mut anchors: Vec<Certificate<'_>> = Vec::with_capacity(roots.len());
    for der in roots {
        anchors.push(parse_certificate(der)?);
    }

    // Walk from the leaf towards a trust anchor. The server may send the
    // anchor itself and extra cross-signing certs, so stop as soon as the
    // current certificate is anchored and never assume a fixed chain order.
    let mut current = leaf;
    for depth in 0..8u8 {
        check_time(current)?;
        // A signature failure from the certificate that *claims* to be the
        // issuer is a real error (bad signature, unsupported algorithm), not a
        // missing trust anchor: report it as such so the failure is actionable.
        let mut anchor_err: Option<X509Error> = None;
        let mut anchored = false;
        for root in &anchors {
            if root.subject == current.subject && root.spki == current.spki {
                anchored = true;
                break;
            }
            if root.subject == current.issuer {
                match verify_issuer(current, root) {
                    Ok(()) => {
                        anchored = true;
                        break;
                    }
                    Err(e) => anchor_err = Some(e),
                }
            }
        }
        if anchored {
            return Ok(());
        }

        let mut next: Option<&Certificate<'_>> = None;
        for candidate in chain.iter().skip(1) {
            if candidate.subject == current.issuer {
                match verify_issuer(current, candidate) {
                    Ok(()) => {
                        next = Some(candidate);
                        break;
                    }
                    Err(e) => anchor_err = Some(e),
                }
            }
        }
        let Some(candidate) = next else {
            return Err(anchor_err.unwrap_or(X509Error::NotTrusted));
        };
        if !candidate.is_ca {
            return Err(X509Error::NotACa);
        }
        if let Some(limit) = candidate.path_len {
            if depth > limit {
                return Err(X509Error::PathLenExceeded);
            }
        }
        current = candidate;
    }
    Err(X509Error::ChainTooLong)
}
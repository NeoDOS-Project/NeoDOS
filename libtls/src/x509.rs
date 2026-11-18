//! X.509 certificate parsing and chain validation (RFC 5280 subset).
//!
//! Supported: RSA (PKCS#1 v1.5 and PSS with SHA-256) and ECDSA P-256/384
//! signatures, `basicConstraints`, `keyUsage` for anchors and intermediates,
//! `subjectAltName` (dNSName / iPAddress), validity windows, `pathLenConstraint`
//! and a bounded chain walk. Name constraints and revocation are not
//! implemented (documented gap); `must-staple`/SCT are not enforced.

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
    KeyUsageNotCertSign,
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
            X509Error::KeyUsageNotCertSign => {
                "certificate chain: issuer keyUsage does not allow keyCertSign"
            }
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
    /// Complete DER encoding, as handed to the parser.
    pub der: &'a [u8],
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
    pub path_len: Option<u32>,
    /// `keyUsage.digitalSignature` (true when the extension is absent).
    pub digital_signature: bool,
    /// `keyUsage.keyCertSign`: the certificate may sign other certificates.
    /// True when the extension is absent (a CA without `keyUsage` is common).
    pub key_cert_sign: bool,
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

/// Interpret a DER INTEGER payload as a non-negative `u32`.
///
/// A negative value (high bit set without a sign byte) is invalid for
/// `pathLenConstraint`. Leading sign zeroes are stripped; values wider than 32
/// bits saturate, which is enough because the parser never needs more than
/// "very large".
fn non_negative_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || bytes[0] & 0x80 != 0 {
        return None;
    }
    let mut value = bytes;
    while value.len() > 1 && value[0] == 0 {
        value = &value[1..];
    }
    if value.len() > 4 {
        return Some(u32::MAX);
    }
    Some(value.iter().fold(0u32, |acc, b| (acc << 8) | *b as u32))
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
    let mut key_cert_sign = true;
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
                            let tlv = seq.next_tlv()?;
                            let _ = integer_bytes(&tlv)?;
                            path_len = Some(
                                non_negative_u32(tlv.value).ok_or(X509Error::BadCertificate)?,
                            );
                        }
                    }
                    OID_KEY_USAGE => {
                        // BIT STRING, bit 0 = digitalSignature, bit 5 =
                        // keyCertSign (both in the first content byte).
                        let mut ku = Der::new(value.value);
                        let bits = bit_string_bytes(&ku.next_tlv()?)?;
                        digital_signature = bits.first().map(|b| b & 0x80 != 0).unwrap_or(true);
                        key_cert_sign = bits.first().map(|b| b & 0x04 != 0).unwrap_or(true);
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

    // DER is not self-delimiting at the caller's boundary: the certificate
    // must consume the whole input. Trailing bytes would let two different
    // blobs share a fingerprint space and are rejected (RFC 5280 DER).
    if !cert.is_empty() || !outer.is_empty() {
        return Err(X509Error::BadCertificate);
    }

    Ok(Certificate {
        der,
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
        key_cert_sign,
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

/// Human-readable form of a DER-encoded `Name` (RDNSequence).
///
/// Produces the OpenSSL order (`C=…, O=…, CN=…`), one `ATTR=value` per RDN,
/// with the attribute names a certificate viewer shows. Unknown OIDs and
/// unsupported string types fall back to the dotted OID and are skipped.
pub fn name_to_string(name_der: &[u8]) -> alloc::string::String {
    use alloc::string::String;

    fn label(oid: &[u8]) -> &'static str {
        match oid {
            [0x55, 0x04, 0x03] => "CN",
            [0x55, 0x04, 0x06] => "C",
            [0x55, 0x04, 0x07] => "L",
            [0x55, 0x04, 0x08] => "ST",
            [0x55, 0x04, 0x0a] => "O",
            [0x55, 0x04, 0x0b] => "OU",
            [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x01] => "emailAddress",
            _ => "OID",
        }
    }

    let mut parts: alloc::vec::Vec<String> = alloc::vec::Vec::new();
    let Ok(mut seq) = Der::new(name_der).expect_sequence() else {
        return String::new();
    };
    while !seq.is_empty() {
        // Each RDN is a SET OF AttributeTypeAndValue.
        let Ok(rdn_tlv) = seq.expect(TAG_SET) else { break };
        let mut rdn = Der::new(rdn_tlv.value);
        while !rdn.is_empty() {
            let Ok(mut attr) = rdn.expect_sequence() else { break };
            let Ok(first) = attr.next_tlv() else { continue };
            let Ok(oid) = oid_bytes(&first) else { continue };
            let Ok(value) = attr.next_tlv() else { continue };
            // UTF8String, PrintableString, IA5String and T61String share the
            // byte representation we care about.
            let text = core::str::from_utf8(value.value).unwrap_or("");
            let tag = label(oid);
            if text.is_empty() || tag == "OID" {
                continue;
            }
            let mut part = String::new();
            part.push_str(tag);
            part.push('=');
            part.push_str(text);
            parts.push(part);
        }
    }
    // DER holds RDNs root-first; viewers show them leaf-first.
    let mut out = String::new();
    for part in parts.iter().rev() {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(part);
    }
    out
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
    // A certificate used as a trust anchor must itself be usable as a CA and
    // be inside its validity window. This also covers `--ca` and any anchor
    // that bypassed the store's own revalidation.
    let anchor_checks = |root: &Certificate<'_>| -> core::result::Result<(), X509Error> {
        if !root.is_ca {
            return Err(X509Error::NotACa);
        }
        if !root.key_cert_sign {
            return Err(X509Error::KeyUsageNotCertSign);
        }
        check_time(root)
    };

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
                if let Err(e) = anchor_checks(root) {
                    anchor_err = Some(e);
                    continue;
                }
                anchored = true;
                break;
            }
            if root.subject == current.issuer {
                if let Err(e) = anchor_checks(root) {
                    anchor_err = Some(e);
                    continue;
                }
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
        if !candidate.key_cert_sign {
            return Err(X509Error::KeyUsageNotCertSign);
        }
        if let Some(limit) = candidate.path_len {
            if depth as u32 > limit {
                return Err(X509Error::PathLenExceeded);
            }
        }
        current = candidate;
    }
    Err(X509Error::ChainTooLong)
}
#[cfg(test)]
mod fixture_tests {
    use super::*;

    use x509_fixtures::{fixtures, NOW};

    const HOST: Host<'static> = Host::Dns("test.homelab.arpa");

    macro_rules! root_rsa { () => { fixtures().ca_rsa.as_slice() } }
    macro_rules! root_pss { () => { fixtures().ca_pss.as_slice() } }
    macro_rules! root_p256 { () => { fixtures().ca_p256.as_slice() } }
    macro_rules! root_p384 { () => { fixtures().ca_p384.as_slice() } }
    macro_rules! root_other { () => { fixtures().ca_other_rsa.as_slice() } }
    macro_rules! root_expired { () => { fixtures().ca_expired.as_slice() } }
    macro_rules! root_bad_ku { () => { fixtures().ca_bad_key_usage.as_slice() } }
    macro_rules! leaf_rsa { () => { fixtures().leaf_rsa.as_slice() } }
    macro_rules! leaf_pss { () => { fixtures().leaf_pss.as_slice() } }
    macro_rules! leaf_p256 { () => { fixtures().leaf_p256.as_slice() } }
    macro_rules! leaf_p384 { () => { fixtures().leaf_p384.as_slice() } }
    macro_rules! leaf_wrong_host { () => { fixtures().leaf_wrong_host.as_slice() } }
    macro_rules! leaf_expired { () => { fixtures().leaf_expired.as_slice() } }
    macro_rules! leaf_not_yet { () => { fixtures().leaf_not_yet.as_slice() } }
    macro_rules! leaf_valid_expired_root { () => { fixtures().leaf_valid_expired_root.as_slice() } }
    macro_rules! leaf_other_ca { () => { fixtures().leaf_other_ca.as_slice() } }
    macro_rules! leaf_bad_ku { () => { fixtures().leaf_bad_ku.as_slice() } }
    macro_rules! leaf_via_intermediate { () => { fixtures().leaf_via_intermediate.as_slice() } }
    macro_rules! leaf_via_bad_ku { () => { fixtures().leaf_via_bad_ku.as_slice() } }
    macro_rules! leaf_pathlen0 { () => { fixtures().leaf_pathlen0.as_slice() } }
    macro_rules! leaf_pathlen300 { () => { fixtures().leaf_pathlen300.as_slice() } }
    macro_rules! leaf_pathlen_violation { () => { fixtures().leaf_pathlen_violation.as_slice() } }
    macro_rules! int_pathlen0 { () => { fixtures().intermediate_pathlen0.as_slice() } }
    macro_rules! int_pathlen300 { () => { fixtures().intermediate_pathlen300.as_slice() } }
    macro_rules! int_bad_ku { () => { fixtures().intermediate_bad_ku.as_slice() } }
    macro_rules! int_layer1 { () => { fixtures().intermediate_pathlen1_layer.as_slice() } }
    macro_rules! int_layer0 { () => { fixtures().intermediate_pathlen0_layer.as_slice() } }

    fn roots(ders: &[&[u8]]) -> Vec<Vec<u8>> {
        ders.iter().map(|d| d.to_vec()).collect()
    }

    fn parse(der: &[u8]) -> Certificate<'_> {
        parse_certificate(der).expect("fixture parses")
    }

    fn check(
        chain: &[&[u8]],
        anchors: &[&[u8]],
        host: Host<'_>,
    ) -> core::result::Result<(), X509Error> {
        let certs: Vec<Certificate<'_>> = chain.iter().map(|d| parse(d)).collect();
        verify_chain(&certs, &roots(anchors), host, NOW)
    }

    #[test]
    fn accepts_supported_signature_algorithms() {
        assert_eq!(check(&[leaf_rsa!()], &[root_rsa!()], HOST), Ok(()));
        assert_eq!(check(&[leaf_pss!()], &[root_pss!()], HOST), Ok(()));
        assert_eq!(check(&[leaf_p256!()], &[root_p256!()], HOST), Ok(()));
        assert_eq!(check(&[leaf_p384!()], &[root_p384!()], HOST), Ok(()));
    }

    #[test]
    fn accepts_a_chain_through_an_intermediate() {
        // leaf -> intermediate(pathlen 1) -> root
        let leaf = parse(leaf_via_intermediate!());
        let intermediate = parse(fixtures().intermediate_pathlen1.as_slice());
        let chain = [leaf, intermediate];
        assert_eq!(
            verify_chain(&chain, &roots(&[root_rsa!()]), HOST, NOW),
            Ok(())
        );
    }

    #[test]
    fn rejects_a_wrong_chain() {
        assert_eq!(
            check(&[leaf_other_ca!()], &[root_rsa!()], HOST),
            Err(X509Error::NotTrusted)
        );
        // The same leaf is valid under its real root, so the failure above is
        // about the anchor, not the leaf.
        assert_eq!(check(&[leaf_other_ca!()], &[root_other!()], HOST), Ok(()));
    }

    #[test]
    fn rejects_a_hostname_mismatch() {
        assert_eq!(
            check(&[leaf_wrong_host!()], &[root_rsa!()], HOST),
            Err(X509Error::HostnameMismatch)
        );
    }

    #[test]
    fn rejects_expired_and_not_yet_valid_leaves() {
        assert_eq!(
            check(&[leaf_expired!()], &[root_rsa!()], HOST),
            Err(X509Error::Expired)
        );
        assert_eq!(
            check(&[leaf_not_yet!()], &[root_rsa!()], HOST),
            Err(X509Error::NotYetValid)
        );
    }

    #[test]
    fn rejects_an_expired_anchor_even_if_the_leaf_is_valid() {
        assert_eq!(
            check(&[leaf_valid_expired_root!()], &[root_expired!()], HOST),
            Err(X509Error::Expired)
        );
    }

    #[test]
    fn rejects_a_non_ca_anchor() {
        assert_eq!(
            check(&[leaf_rsa!()], &[leaf_rsa!()], HOST),
            Err(X509Error::NotACa)
        );
    }

    #[test]
    fn rejects_an_anchor_without_key_cert_sign() {
        assert_eq!(
            check(&[leaf_bad_ku!()], &[root_bad_ku!()], HOST),
            Err(X509Error::KeyUsageNotCertSign)
        );
    }

    #[test]
    fn rejects_an_intermediate_without_key_cert_sign() {
        let leaf = parse(leaf_via_bad_ku!());
        let intermediate = parse(int_bad_ku!());
        let chain = [leaf, intermediate];
        assert_eq!(
            verify_chain(&chain, &roots(&[root_rsa!()]), HOST, NOW),
            Err(X509Error::KeyUsageNotCertSign)
        );
    }

    #[test]
    fn rejects_a_pathlen_violation() {
        // leaf -> int2(pathlen 1) -> int1(pathlen 0) -> root.
        let leaf = parse(leaf_pathlen_violation!());
        let int2 = parse(int_layer1!());
        let int1 = parse(int_layer0!());
        let chain = [leaf, int2, int1];
        assert_eq!(
            verify_chain(&chain, &roots(&[root_rsa!()]), HOST, NOW),
            Err(X509Error::PathLenExceeded)
        );
    }

    #[test]
    fn accepts_pathlen_zero_and_large_values() {
        // pathLen 0 is valid when the intermediate signs an end entity.
        let leaf0 = parse(leaf_pathlen0!());
        let int0 = parse(int_pathlen0!());
        assert_eq!(
            verify_chain(&[leaf0, int0], &roots(&[root_rsa!()]), HOST, NOW),
            Ok(())
        );

        // pathLen 300 must be parsed as a real integer, not decimal bytes.
        let leaf300 = parse(leaf_pathlen300!());
        let int300 = parse(int_pathlen300!());
        assert_eq!(int300.path_len, Some(300));
        assert_eq!(
            verify_chain(&[leaf300, int300], &roots(&[root_rsa!()]), HOST, NOW),
            Ok(())
        );
    }

    #[test]
    fn rejects_trailing_bytes_and_truncated_der() {
        let mut padded = root_rsa!().to_vec();
        padded.push(0);
        assert_eq!(
            parse_certificate(&padded).unwrap_err(),
            X509Error::BadCertificate
        );

        let truncated = &root_rsa!()[..root_rsa!().len() - 1];
        assert!(parse_certificate(truncated).is_err());
    }
}

//! Validation of an imported certificate as a trust anchor.
//!
//! Pure: no registry, no syscalls, so it is host-tested against a PKI generated
//! at test time with fixed dates (`tools/x509-fixtures`). The rules mirror what makes
//! a certificate usable as a CA:
//!
//! * its DER is at most [`crate::MAX_CERTIFICATE_DER`] bytes;
//! * it parses as DER (and consumes the whole input);
//! * `basicConstraints.cA` is true;
//! * `keyUsage` (when present) allows `keyCertSign` — a CA without a `keyUsage`
//!   extension is commonly accepted by every OS and is accepted here too;
//! * it is inside its validity window (judged with the trusted clock).
//!
//! A server certificate is *not* an authority and is rejected with a message
//! that says so.

use alloc::string::String;
use alloc::vec::Vec;

use libtls::x509::{parse_certificate, X509Error, Certificate};

use crate::MAX_CERTIFICATE_DER;

/// Why a certificate cannot be used as a trust anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustError {
    /// Nothing usable in the input (no PEM block, or not DER at all).
    Empty,
    /// DER/ASN.1 parse failure.
    Malformed,
    /// Longer than the trust store can persist and read back.
    TooLarge,
    /// `basicConstraints.cA` is false or missing: a server certificate.
    NotACa,
    /// `keyUsage` is present but does not allow `keyCertSign`.
    KeyUsageNotCertSign,
    /// Outside its validity window.
    Expired,
    NotYetValid,
    /// A certificate that does not parse far enough to judge (unsupported
    /// signature algorithm, unknown critical extension, …).
    Unsupported,
}

impl TrustError {
    /// Message for the CLI (Spanish, user-facing).
    pub fn as_str(self) -> &'static str {
        match self {
            TrustError::Empty => "no contiene ningún certificado",
            TrustError::Malformed => "el certificado está mal formado",
            TrustError::TooLarge => "el certificado supera el límite de 8 KiB del almacén",
            TrustError::NotACa => "no es una CA: no tiene basicConstraints CA:TRUE \
                                   (confiar en un certificado de servidor es otra función)",
            TrustError::KeyUsageNotCertSign => "su keyUsage no permite firmar certificados",
            TrustError::Expired => "el certificado está caducado",
            TrustError::NotYetValid => "el certificado aún no es válido",
            TrustError::Unsupported => "no se pudo interpretar (algoritmo o extensión no soportada)",
        }
    }
}

/// Input split into candidate DER certificates.
pub struct Candidate {
    pub der: Vec<u8>,
}

/// Split `bytes` into DER candidates: PEM blocks when the input looks like PEM,
/// otherwise the bytes are taken as a single DER certificate.
pub fn candidates(bytes: &[u8]) -> Vec<Candidate> {
    if libtls::pem::is_pem(bytes) {
        libtls::pem::decode(bytes)
            .into_iter()
            .map(|der| Candidate { der })
            .collect()
    } else if !bytes.is_empty() {
        alloc::vec![Candidate {
            der: bytes.to_vec()
        }]
    } else {
        Vec::new()
    }
}

/// A certificate accepted as a trust anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    pub der: Vec<u8>,
    /// SHA-256 of `der`.
    pub fingerprint: [u8; 32],
    pub subject: String,
    pub issuer: String,
    pub not_before: i64,
    pub not_after: i64,
}

impl Validated {
    pub fn fingerprint_hex(&self) -> String {
        crate::hex(&self.fingerprint)
    }
}

fn judge(cert: &Certificate<'_>, der: &[u8], now: i64) -> Result<Validated, TrustError> {
    if !cert.is_ca {
        return Err(TrustError::NotACa);
    }
    if !cert.key_cert_sign {
        return Err(TrustError::KeyUsageNotCertSign);
    }
    if now < cert.not_before {
        return Err(TrustError::NotYetValid);
    }
    if now > cert.not_after {
        return Err(TrustError::Expired);
    }
    Ok(Validated {
        der: der.to_vec(),
        fingerprint: libcrypto::Sha256::digest(der),
        subject: libtls::x509::name_to_string(cert.subject),
        issuer: libtls::x509::name_to_string(cert.issuer),
        not_before: cert.not_before,
        not_after: cert.not_after,
    })
}

/// Validate `der` as a trust anchor. `now` comes from the trusted clock.
pub fn validate(der: &[u8], now: i64) -> Result<Validated, TrustError> {
    if der.is_empty() {
        return Err(TrustError::Empty);
    }
    // The store keeps one DER blob per key; a larger one could not be read
    // back, so reject it before touching the Registry.
    if der.len() > MAX_CERTIFICATE_DER {
        return Err(TrustError::TooLarge);
    }
    let cert = match parse_certificate(der) {
        Ok(cert) => cert,
        Err(X509Error::UnsupportedAlgorithm) => return Err(TrustError::Unsupported),
        Err(_) => return Err(TrustError::Malformed),
    };
    judge(&cert, der, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    use x509_fixtures::{fixtures, NOW};

    macro_rules! ca_der { () => { fixtures().ca_rsa.as_slice() } }
    macro_rules! ca_pem { () => { fixtures().ca_rsa_pem.as_slice() } }
    macro_rules! ca_no_key_usage { () => { fixtures().ca_no_key_usage.as_slice() } }
    macro_rules! ca_bad_key_usage { () => { fixtures().ca_bad_key_usage.as_slice() } }
    macro_rules! ca_expired { () => { fixtures().ca_expired.as_slice() } }
    macro_rules! ca_not_yet_valid { () => { fixtures().ca_not_yet_valid.as_slice() } }
    macro_rules! ca_at_limit { () => { fixtures().ca_at_limit.as_slice() } }
    macro_rules! ca_over_limit { () => { fixtures().ca_over_limit.as_slice() } }
    macro_rules! leaf { () => { fixtures().leaf_rsa.as_slice() } }

    #[test]
    fn accepts_a_ca_from_der() {
        let v = validate(ca_der!(), NOW).expect("CA accepted");
        assert_eq!(v.subject, "C=ES, O=NeoDOS Tests, CN=NeoDOS Test RSA Root");
        assert_eq!(v.issuer, v.subject, "self-signed");
        assert_eq!(v.der, ca_der!());
        assert_eq!(v.fingerprint_hex().len(), 64);
    }

    #[test]
    fn pem_with_a_private_key_imports_only_the_certificate() {
        let mut pem = ca_pem!().to_vec();
        pem.extend_from_slice(
            b"\n-----BEGIN PRIVATE KEY-----\nMAIFAA==\n-----END PRIVATE KEY-----\n",
        );
        let cands = candidates(&pem);
        assert_eq!(cands.len(), 1, "the PRIVATE KEY block must be ignored");
        assert!(validate(&cands[0].der, NOW).is_ok());
    }

    #[test]
    fn rejects_a_server_certificate() {
        assert_eq!(validate(leaf!(), NOW), Err(TrustError::NotACa));
    }

    #[test]
    fn accepts_a_ca_without_key_usage() {
        assert!(validate(ca_no_key_usage!(), NOW).is_ok());
    }

    #[test]
    fn rejects_key_usage_without_key_cert_sign() {
        assert_eq!(
            validate(ca_bad_key_usage!(), NOW),
            Err(TrustError::KeyUsageNotCertSign)
        );
    }

    #[test]
    fn rejects_expired_and_not_yet_valid() {
        assert_eq!(validate(ca_expired!(), NOW), Err(TrustError::Expired));
        assert_eq!(
            validate(ca_not_yet_valid!(), NOW),
            Err(TrustError::NotYetValid)
        );
        assert_eq!(validate(ca_der!(), 0), Err(TrustError::NotYetValid));
        assert_eq!(validate(ca_der!(), 4_000_000_000), Err(TrustError::Expired));
    }

    #[test]
    fn size_limit_is_enforced_before_storage() {
        assert_eq!(ca_at_limit!().len(), MAX_CERTIFICATE_DER);
        assert!(validate(ca_at_limit!(), NOW).is_ok());
        assert!(ca_over_limit!().len() > MAX_CERTIFICATE_DER);
        assert_eq!(validate(ca_over_limit!(), NOW), Err(TrustError::TooLarge));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(validate(&[], NOW), Err(TrustError::Empty));
        assert_eq!(validate(&[0xff, 0x00, 0x13], NOW), Err(TrustError::Malformed));
        assert!(candidates(b"not a certificate").len() == 1);
        assert!(libtls::pem::decode(b"junk").is_empty());
    }
}

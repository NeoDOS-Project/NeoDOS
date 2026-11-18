//! Validation of an imported certificate as a trust anchor.
//!
//! Pure: no registry, no syscalls, so it is host-tested against real
//! certificates. The rules mirror what makes a certificate usable as a CA:
//!
//! * it parses as DER;
//! * `basicConstraints.cA` is true;
//! * `keyUsage` (when present) allows `keyCertSign` — a CA without a `keyUsage`
//!   extension is commonly accepted by every OS and is accepted here too;
//! * it is inside its validity window (judged with the trusted clock).
//!
//! A server certificate is *not* an authority and is rejected with a message
//! that says so.

use alloc::string::String;
use alloc::vec::Vec;

use libtls::x509::{parse_certificate, Certificate};

/// Why a certificate cannot be used as a trust anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustError {
    /// Nothing usable in the input (no PEM block, or not DER at all).
    Empty,
    /// DER/ASN.1 parse failure.
    Malformed,
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
    // A parse error before the extensions are read means "not a certificate";
    // an unsupported algorithm still lets us read the CA fields.
    let cert = parse_certificate(der).map_err(|_| TrustError::Malformed)?;
    judge(&cert, der, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real test material (see the repository's E2E); skipped when absent.
    fn read(path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    const CA_DER: &str = "/home/amartinper/cert_generator/myCA/ca.der";
    const CA_PEM: &str = "/home/amartinper/cert_generator/myCA/ca.pem";
    const LEAF_PEM: &str =
        "/home/amartinper/cert_generator/certs/inventory.home.arpa/inventory.home.arpa.crt";
    /// 2026-11-17: inside the fixture CA's window.
    const NOW: i64 = 1_763_400_000;

    #[test]
    fn accepts_a_real_ca_from_der() {
        let Some(der) = read(CA_DER) else { return };
        let v = validate(&der, NOW).expect("CA accepted");
        assert_eq!(v.subject, "CN=MyHomeCA, O=MyHomeCA, L=Home, ST=Local, C=ES");
        assert_eq!(v.issuer, v.subject, "self-signed");
        assert_eq!(v.der, der);
        assert_eq!(v.fingerprint_hex().len(), 64);
    }

    #[test]
    fn pem_with_a_private_key_imports_only_the_certificate() {
        let Some(pem) = read(CA_PEM) else { return };
        let cands = candidates(&pem);
        assert_eq!(cands.len(), 1, "the PRIVATE KEY block must be ignored");
        assert!(validate(&cands[0].der, NOW).is_ok());
    }

    #[test]
    fn rejects_a_server_certificate() {
        let Some(pem) = read(LEAF_PEM) else { return };
        let cands = candidates(&pem);
        assert_eq!(cands.len(), 1);
        assert_eq!(validate(&cands[0].der, NOW), Err(TrustError::NotACa));
    }

    #[test]
    fn rejects_expired_and_not_yet_valid() {
        let Some(der) = read(CA_DER) else { return };
        assert_eq!(validate(&der, 0), Err(TrustError::NotYetValid));
        assert_eq!(validate(&der, 4_000_000_000), Err(TrustError::Expired));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(validate(&[], NOW), Err(TrustError::Empty));
        assert_eq!(validate(&[0xff, 0x00, 0x13], NOW), Err(TrustError::Malformed));
        assert!(candidates(b"not a certificate").len() == 1);
        assert!(libtls::pem::decode(b"junk").is_empty());
    }
}

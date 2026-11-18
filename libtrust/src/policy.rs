//! Pure trust-store policy: stored-value decoding and anchor revalidation.
//!
//! This module deliberately has no Registry or syscall dependency. The store
//! feeds it the bytes it read (and the metadata the kernel returned), and the
//! same rules run in host tests. That keeps the security decisions testable
//! with deterministic fixtures instead of the developer's machine.

use alloc::string::String;
use alloc::vec::Vec;

use crate::validate::{validate, TrustError};
use crate::MAX_CERTIFICATE_DER;

/// `REG_BINARY` from the Cm ABI.
pub const REG_BINARY: u32 = 3;

/// Why a persisted value cannot be used as an anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreValueError {
    /// The key has no `Der` value (or it cannot be opened).
    Missing,
    /// The value exists but the kernel could not deliver it this time.
    Unreadable,
    /// The value exists but is not `REG_BINARY`.
    WrongType,
    /// The length reported by the kernel does not match the bytes delivered.
    Truncated,
    /// The value is larger than [`MAX_CERTIFICATE_DER`].
    TooLarge,
    /// SHA-256(DER) does not match the key name.
    FingerprintMismatch,
}

impl StoreValueError {
    pub fn as_str(self) -> &'static str {
        match self {
            StoreValueError::Missing => "falta el valor Der",
            StoreValueError::Unreadable => "no se pudo leer el valor Der",
            StoreValueError::WrongType => "el valor Der no es REG_BINARY",
            StoreValueError::Truncated => "el valor Der está truncado",
            StoreValueError::TooLarge => "el valor Der supera el límite de 8 KiB",
            StoreValueError::FingerprintMismatch => "la huella SHA-256 no coincide",
        }
    }

    /// True when `repair` may delete the key without touching a possibly valid
    /// certificate. A transient read failure (`Unreadable`) is not structural.
    pub fn is_structural_corruption(self) -> bool {
        matches!(
            self,
            StoreValueError::Missing
                | StoreValueError::WrongType
                | StoreValueError::Truncated
                | StoreValueError::TooLarge
                | StoreValueError::FingerprintMismatch
        )
    }
}

/// Why an anchor was rejected while loading the stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorIssue {
    /// The registry value itself is unusable.
    Store(StoreValueError),
    /// The DER is intact but is not a valid CA anchor.
    Invalid(TrustError),
}

impl AnchorIssue {
    pub fn as_str(self) -> &'static str {
        match self {
            AnchorIssue::Store(e) => e.as_str(),
            AnchorIssue::Invalid(e) => e.as_str(),
        }
    }
}

/// One rejected anchor, kept for diagnostics (`certctl status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedAnchor {
    /// Key name / SHA-256 fingerprint (lowercase hex), or computed for a
    /// system-bundle entry.
    pub fingerprint: String,
    /// Registry key path or bundle path.
    pub location: String,
    pub issue: AnchorIssue,
}

/// Result of loading the trust stores at a given time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnchorReport {
    /// DER of every usable anchor.
    pub roots: Vec<Vec<u8>>,
    /// Entries skipped, with the reason.
    pub rejected: Vec<RejectedAnchor>,
}

/// Decode one `Der` registry value read through the kernel's Cm ABI.
///
/// `value_type` and `declared_len` are the header fields returned by
/// `cm_query_value`; `data` is the payload actually delivered. The checks are
/// deliberately strict: a value whose *reported* length is larger than the
/// bytes we received is truncated, and a key name that does not match
/// SHA-256(data) was tampered with.
pub fn decode_stored_der(
    value_type: u32,
    declared_len: usize,
    data: &[u8],
    expected_fingerprint: &str,
) -> Result<Vec<u8>, StoreValueError> {
    if value_type != REG_BINARY {
        return Err(StoreValueError::WrongType);
    }
    if declared_len > MAX_CERTIFICATE_DER {
        return Err(StoreValueError::TooLarge);
    }
    if data.len() != declared_len {
        return Err(StoreValueError::Truncated);
    }
    if !expected_fingerprint.eq_ignore_ascii_case(&crate::hex(&libcrypto::Sha256::digest(data))) {
        return Err(StoreValueError::FingerprintMismatch);
    }
    Ok(data.to_vec())
}

/// Revalidate every stored candidate and produce the set TLS must trust.
///
/// Each item is `(fingerprint, location, stored DER)`. A bad entry is recorded
/// and skipped; it never aborts the load or hides the remaining valid roots.
pub fn collect_anchors<I>(candidates: I, now: i64) -> AnchorReport
where
    I: IntoIterator<Item = (String, String, Result<Vec<u8>, StoreValueError>)>,
{
    let mut report = AnchorReport::default();
    for (fingerprint, location, candidate) in candidates {
        let der = match candidate {
            Ok(der) => der,
            Err(issue) => {
                report.rejected.push(RejectedAnchor {
                    fingerprint,
                    location,
                    issue: AnchorIssue::Store(issue),
                });
                continue;
            }
        };
        match validate(&der, now) {
            Ok(_) => report.roots.push(der),
            Err(e) => report.rejected.push(RejectedAnchor {
                fingerprint,
                location,
                issue: AnchorIssue::Invalid(e),
            }),
        }
    }
    report
}

/// SHA-256 fingerprint of DER, lowercase hex.
pub fn fingerprint_hex(der: &[u8]) -> String {
    crate::hex(&libcrypto::Sha256::digest(der))
}

#[cfg(test)]
mod tests {
    use super::*;

    use x509_fixtures::{fixtures, NOW};

    macro_rules! ca_rsa { () => { fixtures().ca_rsa.as_slice() } }
    macro_rules! ca_p256 { () => { fixtures().ca_p256.as_slice() } }
    macro_rules! ca_expired { () => { fixtures().ca_expired.as_slice() } }
    macro_rules! ca_at_limit { () => { fixtures().ca_at_limit.as_slice() } }
    macro_rules! ca_over_limit { () => { fixtures().ca_over_limit.as_slice() } }

    fn stored(der: &[u8]) -> (String, String, Result<Vec<u8>, StoreValueError>) {
        let fp = fingerprint_hex(der);
        (
            fp.clone(),
            alloc::format!("reg\\{}", fp),
            Ok(der.to_vec()),
        )
    }

    #[test]
    fn decodes_a_matching_binary_value() {
        let fp = fingerprint_hex(ca_rsa!());
        let der = decode_stored_der(REG_BINARY, ca_rsa!().len(), ca_rsa!(), &fp).unwrap();
        assert_eq!(der, ca_rsa!());
    }

    #[test]
    fn rejects_the_wrong_value_type() {
        let fp = fingerprint_hex(ca_rsa!());
        assert_eq!(
            decode_stored_der(1, ca_rsa!().len(), ca_rsa!(), &fp),
            Err(StoreValueError::WrongType)
        );
    }

    #[test]
    fn rejects_a_truncated_value() {
        let fp = fingerprint_hex(ca_rsa!());
        assert_eq!(
            decode_stored_der(REG_BINARY, ca_rsa!().len() + 1, ca_rsa!(), &fp),
            Err(StoreValueError::Truncated)
        );
    }

    #[test]
    fn rejects_a_value_over_the_limit() {
        let fp = fingerprint_hex(ca_over_limit!());
        assert_eq!(
            decode_stored_der(REG_BINARY, ca_over_limit!().len(), ca_over_limit!(), &fp),
            Err(StoreValueError::TooLarge)
        );
    }

    #[test]
    fn accepts_a_value_exactly_at_the_limit() {
        assert_eq!(ca_at_limit!().len(), crate::MAX_CERTIFICATE_DER);
        let fp = fingerprint_hex(ca_at_limit!());
        assert!(decode_stored_der(REG_BINARY, ca_at_limit!().len(), ca_at_limit!(), &fp).is_ok());
    }

    #[test]
    fn rejects_a_fingerprint_mismatch() {
        let wrong = "0".repeat(64);
        assert_eq!(
            decode_stored_der(REG_BINARY, ca_rsa!().len(), ca_rsa!(), &wrong),
            Err(StoreValueError::FingerprintMismatch)
        );
    }

    #[test]
    fn repair_only_treats_structural_problems_as_deletable() {
        assert!(StoreValueError::Missing.is_structural_corruption());
        assert!(StoreValueError::WrongType.is_structural_corruption());
        assert!(StoreValueError::Truncated.is_structural_corruption());
        assert!(StoreValueError::TooLarge.is_structural_corruption());
        assert!(StoreValueError::FingerprintMismatch.is_structural_corruption());
        assert!(!StoreValueError::Unreadable.is_structural_corruption());
    }

    #[test]
    fn keeps_valid_roots_and_reports_the_rest() {
        let mut candidates = alloc::vec![
            stored(ca_rsa!()),
            stored(ca_p256!()),
            stored(ca_expired!()),
        ];
        candidates.push((
            "f".repeat(64),
            "reg\\broken".to_string(),
            Err(StoreValueError::WrongType),
        ));
        candidates.push((
            "e".repeat(64),
            "reg\\garbage".to_string(),
            Ok(alloc::vec![0xff, 0x00, 0x13]),
        ));

        let report = collect_anchors(candidates, NOW);
        assert_eq!(report.roots.len(), 2);
        assert_eq!(report.roots[0], ca_rsa!());
        assert_eq!(report.roots[1], ca_p256!());
        assert_eq!(report.rejected.len(), 3);
        assert_eq!(
            report.rejected[0].issue,
            AnchorIssue::Invalid(TrustError::Expired)
        );
        assert_eq!(
            report.rejected[1].issue,
            AnchorIssue::Store(StoreValueError::WrongType)
        );
        assert_eq!(
            report.rejected[2].issue,
            AnchorIssue::Invalid(TrustError::Malformed)
        );
    }
}

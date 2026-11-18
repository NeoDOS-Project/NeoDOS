//! Registry-backed trust store (system bundle + user CAs).
//!
//! The system bundle is a read-only file shipped by the image; user CAs live in
//! one registry key per certificate. Keeping them apart is deliberate: deleting
//! a user CA is a `delete_key` on its own key and can never touch the image's
//! roots, and an interrupted import leaves at most one incomplete key, which is
//! ignored (and can be cleaned by `certctl repair`).
//!
//! Every read is validated again: the Registry is storage, not a trust
//! decision. A value that is not `REG_BINARY`, is truncated, exceeds
//! [`crate::MAX_CERTIFICATE_DER`], does not hash to its key name, or no longer
//! parses as a CA is skipped and reported. A broken entry never panics and
//! never hides the remaining roots.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use libneodos::registry::RegistryKey;
use libneodos::syscall;

use crate::policy::{
    collect_anchors, decode_stored_der, fingerprint_hex, AnchorReport, StoreValueError,
    REG_BINARY,
};
use crate::validate::{validate, TrustError, Validated};
use crate::{hex, Entry, Source, MAX_CERTIFICATE_DER, REGISTRY_ROOT};

/// Same path as `libneodos::paths::TRUST_BUNDLE`, kept local so the store has
/// no other dependency.
pub const SYSTEM_BUNDLE: &str = libneodos::paths::TRUST_BUNDLE;

const VALUE_DER: &str = "Der";
const VALUE_SUBJECT: &str = "Subject";
const VALUE_ISSUER: &str = "Issuer";
const VALUE_NOT_BEFORE: &str = "NotBefore";
const VALUE_NOT_AFTER: &str = "NotAfter";

/// Metadata of the system bundle (for `certctl status`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BundleInfo {
    /// Format version (`0` for an unversioned raw DER bundle).
    pub version: u16,
    /// Build time in Unix seconds (`0` when unknown).
    pub built_unix: i64,
    /// Number of anchors.
    pub count: usize,
}

/// Result of importing one certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Imported {
    /// Stored now, with this fingerprint.
    Installed(String),
    /// Already present as a user CA.
    AlreadyUser(String),
    /// Already shipped by the image (cannot be removed).
    AlreadySystem(String),
}

impl Imported {
    pub fn fingerprint(&self) -> &str {
        match self {
            Imported::Installed(f) | Imported::AlreadyUser(f) | Imported::AlreadySystem(f) => f,
        }
    }
}

// ── System bundle ──

fn read_file(path: &str) -> Option<Vec<u8>> {
    // One extra byte lets us distinguish "exactly MAX_BUNDLE" from "larger
    // than MAX_BUNDLE": ReadContent returns the buffer size when the file does
    // not fit, so silently truncating here would trust a subset of the bundle.
    let mut buf = alloc::vec![0u8; crate::store::MAX_BUNDLE + 1];
    let ob = alloc::format!("\\Global\\FileSystem\\{}", path);
    let fd = syscall::sys_ob_open(&ob, syscall::ob_access::READ).ok()?;
    let n = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::ReadContent, &mut buf);
    let _ = syscall::sys_close(fd);
    let n = n.ok()? as usize;
    if n == 0 || n > crate::store::MAX_BUNDLE {
        return None;
    }
    buf.truncate(n);
    Some(buf)
}

/// Largest system bundle we will read (a full public root store is ~150 KiB).
pub const MAX_BUNDLE: usize = 256 * 1024;

/// The system bundle's anchors and metadata.
pub fn system_bundle() -> (BundleInfo, Vec<Vec<u8>>) {
    let Some(bytes) = read_file(SYSTEM_BUNDLE) else {
        return (BundleInfo::default(), Vec::new());
    };
    // A versioned bundle that fails to parse is corrupt, not "unversioned":
    // do not fall back to splitting it into a partial trust set.
    if bytes.starts_with(&libtls::der::BUNDLE_MAGIC) {
        let Some(bundle) = libtls::der::parse_bundle(&bytes) else {
            return (BundleInfo::default(), Vec::new());
        };
        return (
            BundleInfo {
                version: bundle.version,
                built_unix: bundle.built_unix,
                count: bundle.certs.len(),
            },
            bundle.certs.into_iter().map(|c| c.to_vec()).collect(),
        );
    }
    let certs: Vec<Vec<u8>> = libtls::der::split_certificates(&bytes)
        .into_iter()
        .map(|c| c.to_vec())
        .collect();
    (
        BundleInfo {
            version: 0,
            built_unix: 0,
            count: certs.len(),
        },
        certs,
    )
}

// ── User CAs ──

fn open_root() -> Option<RegistryKey> {
    RegistryKey::open(REGISTRY_ROOT).ok()
}

/// Fingerprints of every user CA key, in registry order.
///
/// Read paths never create the tree: an unprivileged caller must still be able
/// to enumerate the public anchors without needing write access.
pub fn user_fingerprints() -> Vec<String> {
    let mut out = Vec::new();
    let Some(root) = open_root() else { return out };
    let mut name = [0u8; 256];
    let mut index = 0u32;
    while let Some(len) = root.enum_key(index, &mut name) {
        if let Ok(fp) = core::str::from_utf8(&name[..len]) {
            out.push(fp.to_string());
        }
        index += 1;
    }
    out
}

fn has_value(key: &RegistryKey, name: &str) -> bool {
    let mut buf = [0u8; 256];
    let mut index = 0u32;
    while let Some(len) = key.enum_value(index, &mut buf) {
        if len == name.len() && buf[..len].eq_ignore_ascii_case(name.as_bytes()) {
            return true;
        }
        index += 1;
    }
    false
}

/// Read and structurally validate the DER of one user CA.
///
/// This is stricter than [`user_der`]: it distinguishes a missing value from a
/// transient read failure and from corruption, so `repair` never deletes a
/// certificate merely because one read failed.
pub fn user_der_strict(fingerprint: &str) -> Result<Vec<u8>, StoreValueError> {
    let key = RegistryKey::open(&alloc::format!("{}\\{}", REGISTRY_ROOT, fingerprint))
        .map_err(|_| StoreValueError::Unreadable)?;
    let mut buf = alloc::vec![0u8; MAX_CERTIFICATE_DER + 8];
    let Some(info) = key.query_value_info(VALUE_DER, &mut buf) else {
        return Err(if has_value(&key, VALUE_DER) {
            StoreValueError::Unreadable
        } else {
            StoreValueError::Missing
        });
    };
    if info.value_type != REG_BINARY {
        return Err(StoreValueError::WrongType);
    }
    if info.data_len > MAX_CERTIFICATE_DER {
        return Err(StoreValueError::TooLarge);
    }
    if !info.complete {
        return Err(StoreValueError::Truncated);
    }
    decode_stored_der(
        info.value_type,
        info.data_len,
        &buf[8..8 + info.data_len],
        fingerprint,
    )
}

/// Read the DER of one user CA (by fingerprint), if it is structurally valid.
pub fn user_der(fingerprint: &str) -> Option<Vec<u8>> {
    user_der_strict(fingerprint).ok()
}

fn user_entry(fingerprint: &str, der: &[u8]) -> Entry {
    let mut subject = String::new();
    let mut issuer = String::new();
    let mut not_before = 0i64;
    let mut not_after = 0i64;
    let mut is_ca = false;
    if let Ok(cert) = libtls::x509::parse_certificate(der) {
        subject = libtls::x509::name_to_string(cert.subject);
        issuer = libtls::x509::name_to_string(cert.issuer);
        not_before = cert.not_before;
        not_after = cert.not_after;
        is_ca = cert.is_ca;
    }
    Entry {
        fingerprint: fingerprint.to_string(),
        subject,
        issuer,
        not_before,
        not_after,
        is_ca,
        source: Source::User,
        location: alloc::format!("{}\\{}", REGISTRY_ROOT, fingerprint),
    }
}

/// Every structurally readable user CA (incomplete keys skipped).
pub fn user_entries() -> Vec<Entry> {
    let mut out = Vec::new();
    for fp in user_fingerprints() {
        if let Some(der) = user_der(&fp) {
            out.push(user_entry(&fp, &der));
        }
    }
    out
}

/// Every system anchor as an entry (parsed for display).
pub fn system_entries() -> Vec<Entry> {
    let (_, certs) = system_bundle();
    let mut out = Vec::new();
    for der in &certs {
        let Ok(cert) = libtls::x509::parse_certificate(der) else {
            continue;
        };
        out.push(Entry {
            fingerprint: hex(&libcrypto::Sha256::digest(der)),
            subject: libtls::x509::name_to_string(cert.subject),
            issuer: libtls::x509::name_to_string(cert.issuer),
            not_before: cert.not_before,
            not_after: cert.not_after,
            is_ca: cert.is_ca,
            source: Source::System,
            location: SYSTEM_BUNDLE.to_string(),
        });
    }
    out
}

/// Revalidate both stores and return the anchors TLS must trust, plus every
/// entry that was skipped. `now` comes from the trusted clock.
pub fn anchor_report(now: i64) -> AnchorReport {
    let mut candidates: Vec<(String, String, Result<Vec<u8>, StoreValueError>)> = Vec::new();

    let (_, system) = system_bundle();
    for der in system {
        let fp = fingerprint_hex(&der);
        candidates.push((fp, SYSTEM_BUNDLE.to_string(), Ok(der)));
    }
    for fp in user_fingerprints() {
        let location = alloc::format!("{}\\{}", REGISTRY_ROOT, fp);
        candidates.push((fp.clone(), location, user_der_strict(&fp)));
    }

    collect_anchors(candidates, now)
}

/// System anchors followed by the user's CAs — what TLS must trust.
pub fn anchors(now: i64) -> Vec<Vec<u8>> {
    anchor_report(now).roots
}

/// Import one DER certificate. `now` comes from the trusted clock.
///
/// De-duplicates against the system bundle and the user's keys by SHA-256.
pub fn import_der(der: &[u8], now: i64) -> Result<Imported, TrustError> {
    let v = validate(der, now)?;
    let fp = v.fingerprint_hex();
    let (_, system) = system_bundle();
    if system
        .iter()
        .any(|c| libcrypto::Sha256::digest(c) == v.fingerprint)
    {
        return Ok(Imported::AlreadySystem(fp));
    }
    if user_der(&fp).is_some() {
        return Ok(Imported::AlreadyUser(fp));
    }
    store(&v).map_err(|_| TrustError::Unsupported)?;
    Ok(Imported::Installed(fp))
}

/// Import every certificate found in `bytes` (PEM with one or several, or a
/// single DER). Returns one result per candidate, in order.
pub fn import_all(bytes: &[u8], now: i64) -> Vec<Result<Imported, TrustError>> {
    let cands = crate::validate::candidates(bytes);
    if cands.is_empty() {
        return alloc::vec![Err(TrustError::Empty)];
    }
    cands
        .into_iter()
        .map(|c| import_der(&c.der, now))
        .collect()
}

/// Write one validated CA, `Der` last so an interrupted import is detectable.
fn store(v: &Validated) -> Result<(), TrustError> {
    let path = alloc::format!("{}\\{}", REGISTRY_ROOT, v.fingerprint_hex());
    let key = RegistryKey::create_tree(&path).map_err(|_| TrustError::Unsupported)?;
    key.set_string(VALUE_SUBJECT, v.subject.as_bytes())
        .map_err(|_| TrustError::Unsupported)?;
    key.set_string(VALUE_ISSUER, v.issuer.as_bytes())
        .map_err(|_| TrustError::Unsupported)?;
    key.set_dword(VALUE_NOT_BEFORE, v.not_before as u32)
        .map_err(|_| TrustError::Unsupported)?;
    key.set_dword(VALUE_NOT_AFTER, v.not_after as u32)
        .map_err(|_| TrustError::Unsupported)?;
    key.set_binary(VALUE_DER, &v.der)
        .map_err(|_| TrustError::Unsupported)?;
    key.flush().map_err(|_| TrustError::Unsupported)?;
    Ok(())
}

/// Remove a user CA. System anchors are never touched (and cannot be named
/// here).
pub fn remove(fingerprint: &str) -> Result<(), TrustError> {
    let fp = fingerprint.to_ascii_lowercase();
    if !user_fingerprints().iter().any(|f| *f == fp) {
        return Err(TrustError::Empty);
    }
    let root = open_root().ok_or(TrustError::Unsupported)?;
    root.delete_key(&fp).map_err(|_| TrustError::Unsupported)?;
    root.flush().map_err(|_| TrustError::Unsupported)?;
    Ok(())
}

/// Delete user keys that have no usable `Der` value (interrupted imports).
///
/// Only structural problems are cleaned up. A CA that parses but is expired,
/// not yet valid or not a CA is left in place and reported by
/// [`anchor_report`]; deleting it is the administrator's decision.
pub fn repair() -> usize {
    let mut removed = 0;
    for fp in user_fingerprints() {
        let structural = match user_der_strict(&fp) {
            Ok(_) => false,
            Err(e) => e.is_structural_corruption(),
        };
        if !structural {
            continue;
        }
        if let Some(root) = open_root() {
            if root.delete_key(&fp).is_ok() {
                let _ = root.flush();
                removed += 1;
            }
        }
    }
    removed
}

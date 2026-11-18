//! Registry-backed trust store (system bundle + user CAs).
//!
//! The system bundle is a read-only file shipped by the image; user CAs live in
//! one registry key per certificate. Keeping them apart is deliberate: deleting
//! a user CA is a `delete_key` on its own key and can never touch the image's
//! roots, and an interrupted import leaves at most one incomplete key, which is
//! ignored (and can be cleaned by `certctl repair`).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use libneodos::registry::RegistryKey;
use libneodos::syscall;

use crate::validate::{validate, TrustError, Validated};
use crate::{hex, Entry, Source, REGISTRY_ROOT};

/// Same path as `libneodos::paths::TRUST_BUNDLE`, kept local so the store has
/// no other dependency.
pub const SYSTEM_BUNDLE: &str = libneodos::paths::TRUST_BUNDLE;

const VALUE_DER: &str = "Der";
const VALUE_SUBJECT: &str = "Subject";
const VALUE_ISSUER: &str = "Issuer";
const VALUE_NOT_BEFORE: &str = "NotBefore";
const VALUE_NOT_AFTER: &str = "NotAfter";

/// Largest value we accept from the registry (a DER certificate is ~1-2 KiB).
const MAX_VALUE: usize = 8 * 1024;

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
    let mut buf = alloc::vec![0u8; crate::store::MAX_BUNDLE];
    let ob = alloc::format!("\\Global\\FileSystem\\{}", path);
    let fd = syscall::sys_ob_open(&ob, syscall::ob_access::READ).ok()?;
    let n = syscall::sys_ob_query_info(fd, syscall::ObInfoClass::ReadContent, &mut buf);
    let _ = syscall::sys_close(fd);
    let n = n.ok()? as usize;
    if n == 0 {
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
    match libtls::der::parse_bundle(&bytes) {
        Some(bundle) => (
            BundleInfo {
                version: bundle.version,
                built_unix: bundle.built_unix,
                count: bundle.certs.len(),
            },
            bundle.certs.into_iter().map(|c| c.to_vec()).collect(),
        ),
        // Unversioned bundle: still usable, no metadata.
        None => {
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
    }
}

// ── User CAs ──

fn root_key() -> Result<RegistryKey, TrustError> {
    RegistryKey::create_tree(REGISTRY_ROOT).map_err(|_| TrustError::Unsupported)
}

/// Fingerprints of every user CA key, in registry order.
pub fn user_fingerprints() -> Vec<String> {
    let mut out = Vec::new();
    let Ok(root) = root_key() else { return out };
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

/// Read the DER of one user CA (by fingerprint).
pub fn user_der(fingerprint: &str) -> Option<Vec<u8>> {
    let key = RegistryKey::open(&alloc::format!("{}\\{}", REGISTRY_ROOT, fingerprint)).ok()?;
    // `query_binary` deja `[value_type u32][len u32][data]` en el buffer y
    // devuelve la longitud de `data`: hay que saltar la cabecera de 8 bytes.
    let mut buf = alloc::vec![0u8; MAX_VALUE + 8];
    let n = key.query_binary(VALUE_DER, &mut buf);
    if n == 0 || n > MAX_VALUE {
        return None;
    }
    Some(buf[8..8 + n].to_vec())
}

fn user_entry(fingerprint: &str, der: &[u8]) -> Entry {
    let mut subject = String::new();
    let mut issuer = String::new();
    let mut not_before = 0i64;
    let mut not_after = 0i64;
    if let Ok(cert) = libtls::x509::parse_certificate(der) {
        subject = libtls::x509::name_to_string(cert.subject);
        issuer = libtls::x509::name_to_string(cert.issuer);
        not_before = cert.not_before;
        not_after = cert.not_after;
        let _ = cert.is_ca;
    }
    Entry {
        fingerprint: fingerprint.to_string(),
        subject,
        issuer,
        not_before,
        not_after,
        is_ca: true,
        source: Source::User,
        location: alloc::format!("{}\\{}", REGISTRY_ROOT, fingerprint),
    }
}

/// Every user CA, with metadata read from its key (incomplete keys skipped).
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

/// System anchors followed by the user's CAs — what TLS must trust.
pub fn anchors() -> Vec<Vec<u8>> {
    let (_, mut certs) = system_bundle();
    for fp in user_fingerprints() {
        if let Some(der) = user_der(&fp) {
            certs.push(der);
        }
    }
    certs
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
    let root = root_key()?;
    root.delete_key(&fp).map_err(|_| TrustError::Unsupported)?;
    root.flush().map_err(|_| TrustError::Unsupported)?;
    Ok(())
}

/// Delete user keys that have no `Der` value (interrupted imports).
pub fn repair() -> usize {
    let mut removed = 0;
    for fp in user_fingerprints() {
        if user_der(&fp).is_none() {
            if let Ok(root) = root_key() {
                if root.delete_key(&fp).is_ok() {
                    let _ = root.flush();
                    removed += 1;
                }
            }
        }
    }
    removed
}

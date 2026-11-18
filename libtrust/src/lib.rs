//! Trust anchors for NeoDOS: the image's system bundle plus the CAs the user
//! imported.
//!
//! Two stores, on purpose:
//!
//! * **System** — `\NeoDOS\System\config\roots.der`, a versioned DER bundle
//!   shipped by the image (read-only). Deleting a user CA can never touch it.
//! * **User** — one registry key per imported CA under
//!   [`REGISTRY_ROOT`], so listing, deleting and de-duplicating are per-entry
//!   operations and an interrupted import cannot leave a half-written bundle.
//!
//! Only **certificate authorities** are accepted here: a server certificate
//! (a leaf) is rejected. Trusting one specific server certificate is a
//! different feature (pinning) and is deliberately not covered.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod validate;

#[cfg(feature = "store")]
pub mod store;

pub use validate::{validate, Candidate, TrustError, Validated};

/// Registry key holding one subkey per user-imported CA, named after its
/// SHA-256 fingerprint in lowercase hex.
pub const REGISTRY_ROOT: &str = "\\Registry\\Machine\\Software\\NeoDOS\\Trust\\UserCAs";

/// Where a trust anchor comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Shipped by the image (read-only).
    System,
    /// Imported by the user (deletable).
    User,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::System => "system",
            Source::User => "user",
        }
    }
}

/// One anchor, as shown by `certctl list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// SHA-256 of the DER, lowercase hex.
    pub fingerprint: alloc::string::String,
    pub subject: alloc::string::String,
    pub issuer: alloc::string::String,
    pub not_before: i64,
    pub not_after: i64,
    pub is_ca: bool,
    pub source: Source,
    /// Where the DER lives (for `show`): bundle path or registry key.
    pub location: alloc::string::String,
}

/// Lowercase hex rendering of a byte slice.
pub fn hex(bytes: &[u8]) -> alloc::string::String {
    use core::fmt::Write;
    let mut out = alloc::string::String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

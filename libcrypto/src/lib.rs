//! Cryptographic primitives for NeoDOS userland (#712 / M9).
//!
//! Pure, `no_std` and host-testable: every primitive is checked against its
//! RFC/NIST vectors in `cargo test`. The crate deliberately exposes only the
//! algorithms TLS and the update tooling need:
//!
//! * SHA-256, HMAC-SHA256 and HKDF (RFC 5869),
//! * ChaCha20-Poly1305 AEAD (RFC 8439),
//! * X25519 key agreement (RFC 7748),
//! * RSA PKCS#1 v1.5 / PSS verification (RFC 8017) and ECDSA P-256
//!   verification (FIPS 186-4).
//!
//! No external crates: the Ring 3 toolchain has no C compiler and the project
//! keeps its dependency surface at zero (see the TLS design, D1).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod aead;
pub mod ecdsa;
pub mod hmac;
pub mod rsa;
pub mod sha256;
pub mod x25519;

pub use aead::{AeadError, ChaCha20Poly1305, TAG_LEN};
pub use ecdsa::{verify_p256_sha256, EcdsaError};
pub use rsa::{RsaError, RsaPublicKey};
pub use hmac::{hkdf, hkdf_expand, hkdf_extract, hmac_sha256, HkdfError, HMAC_LEN};
pub use sha256::{Sha256, SHA256_LEN};
pub use x25519::{x25519, X25519_LEN};

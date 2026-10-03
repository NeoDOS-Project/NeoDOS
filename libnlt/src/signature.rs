//! Ed25519 signing and verification for NLT files (`I18N-P12`).
//!
//! Enabled with the `signatures` feature. Signing uses the [`ed25519-compact`]
//! crate, a self-contained `no_std` implementation with no transitive
//! dependencies, so the runtime/size cost is only paid by builds that opt in.
//!
//! The signature covers the whole file **except** the signature bytes
//! themselves (i.e. header + payload + region block). The default development
//! key pair is derived deterministically; production images are expected to
//! embed their own public key.
//!
//! [`ed25519-compact`]: https://crates.io/crates/ed25519-compact

use ed25519_compact::{KeyPair, PublicKey, Seed, Signature};

/// Size of an Ed25519 signature in bytes.
pub const SIGNATURE_SIZE: usize = 64;
/// Size of an Ed25519 public key in bytes.
pub const PUBLIC_KEY_SIZE: usize = 32;

/// Deterministic development seed. Tooling uses this when no key is supplied.
pub const DEV_SEED: [u8; 32] = *b"NeoDOS-NLT-dev-seed-000000000001";

/// Derive the public key for a 32-byte secret seed.
pub fn public_from_seed(seed: &[u8; 32]) -> Option<[u8; 32]> {
    let s = Seed::from_slice(seed).ok()?;
    let kp = KeyPair::try_from_seed(s).ok()?;
    let mut pk = [0u8; PUBLIC_KEY_SIZE];
    pk.copy_from_slice(&*kp.pk);
    Some(pk)
}

/// Sign `message` with a 32-byte secret seed, writing 64 bytes to `out`.
///
/// Returns `false` for an invalid (e.g. all-zero) seed.
pub fn sign(seed: &[u8; 32], message: &[u8], out: &mut [u8; SIGNATURE_SIZE]) -> bool {
    let s = match Seed::from_slice(seed) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let kp = match KeyPair::try_from_seed(s) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig = kp.sk.sign(message, None);
    out.copy_from_slice(sig.as_ref());
    true
}

/// Verify an Ed25519 signature over `message`.
pub fn verify(public_key: &[u8; PUBLIC_KEY_SIZE], message: &[u8], signature: &[u8; SIGNATURE_SIZE]) -> bool {
    let pk = match PublicKey::from_slice(public_key) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let sig = match Signature::from_slice(signature) {
        Ok(s) => s,
        Err(_) => return false,
    };
    pk.verify(message, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let msg = b"the quick brown fox";
        let mut sig = [0u8; SIGNATURE_SIZE];
        assert!(sign(&DEV_SEED, msg, &mut sig));
        let pk = public_from_seed(&DEV_SEED).unwrap();
        assert!(verify(&pk, msg, &sig));
    }

    #[test]
    fn tampered_message_fails() {
        let mut sig = [0u8; SIGNATURE_SIZE];
        assert!(sign(&DEV_SEED, b"original", &mut sig));
        let pk = public_from_seed(&DEV_SEED).unwrap();
        assert!(!verify(&pk, b"modified", &sig));
    }

    #[test]
    fn rfc8032_test_vector_1() {
        // RFC 8032 §7.1 TEST 1
        let seed: [u8; 32] = hex32("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
        let pk = public_from_seed(&seed).unwrap();
        assert_eq!(
            hex32("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"),
            pk
        );
        let mut sig = [0u8; SIGNATURE_SIZE];
        assert!(sign(&seed, b"", &mut sig));
        assert_eq!(
            hex_bytes("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"),
            sig
        );
        assert!(verify(&pk, b"", &sig));
    }

    fn hex32(s: &str) -> [u8; 32] {
        let v = hex_vec(s);
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        out
    }

    fn hex_bytes(s: &str) -> [u8; 64] {
        let v = hex_vec(s);
        let mut out = [0u8; 64];
        out.copy_from_slice(&v);
        out
    }

    fn hex_vec(s: &str) -> std::vec::Vec<u8> {
        let b = s.as_bytes();
        let mut v = std::vec::Vec::new();
        let mut i = 0;
        while i + 1 < b.len() {
            let hi = hexval(b[i]);
            let lo = hexval(b[i + 1]);
            v.push((hi << 4) | lo);
            i += 2;
        }
        v
    }

    fn hexval(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => 0,
        }
    }
}

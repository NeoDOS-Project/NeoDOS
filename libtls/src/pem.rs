//! PEM decoding for certificate bundles.
//!
//! Importing a CA is a userland operation, so the target needs its own PEM
//! reader: `-----BEGIN CERTIFICATE-----` blocks with base64 bodies. Every other
//! block (`PRIVATE KEY`, `PARAMETERS`, …) is ignored — importing a key file must
//! never make the tool keep or log the key.

use alloc::vec::Vec;

const BEGIN: &[u8] = b"-----BEGIN CERTIFICATE-----";
const END: &[u8] = b"-----END CERTIFICATE-----";

/// Decode every `CERTIFICATE` block in `input` as DER.
///
/// Blocks that are not certificates are skipped, so a combined
/// certificate+key file imports only the certificate. Truncated or undecodable
/// blocks are skipped as well; `decode` returns an empty list when nothing
/// usable was found, and the caller reports that.
pub fn decode(input: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = input;
    while let Some(start) = find(rest, BEGIN) {
        let after = &rest[start + BEGIN.len()..];
        let Some(end) = find(after, END) else {
            break;
        };
        if let Some(der) = base64_decode(&after[..end]) {
            if !der.is_empty() {
                out.push(der);
            }
        }
        rest = &after[end + END.len()..];
    }
    out
}

/// True when `input` looks like PEM at all (any `-----BEGIN` marker).
pub fn is_pem(input: &[u8]) -> bool {
    find(input, b"-----BEGIN ").is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Standard base64 (padding optional). `None` on an invalid character.
fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &c in input {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        let v = value(c)?;
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x30 0x02 0x05 0x00` in base64.
    const BODY: &str = "MAIFAA==";

    fn pem(count: usize, extra: &str) -> Vec<u8> {
        let mut s = alloc::string::String::new();
        for _ in 0..count {
            s.push_str("-----BEGIN CERTIFICATE-----\n");
            s.push_str(BODY);
            s.push_str("\n-----END CERTIFICATE-----\n");
        }
        s.push_str(extra);
        s.into_bytes()
    }

    #[test]
    fn decodes_one_certificate() {
        let certs = decode(&pem(1, ""));
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0], &[0x30, 0x02, 0x05, 0x00]);
    }

    #[test]
    fn decodes_several_certificates() {
        assert_eq!(decode(&pem(3, "")).len(), 3);
    }

    #[test]
    fn ignores_private_key_blocks() {
        // A combined certificate+key file must import only the certificate.
        let extra = "-----BEGIN PRIVATE KEY-----\nMAIFAA==\n-----END PRIVATE KEY-----\n";
        let certs = decode(&pem(1, extra));
        assert_eq!(certs.len(), 1);
    }

    #[test]
    fn ignores_garbage_and_truncated_blocks() {
        assert!(decode(b"no pem here").is_empty());
        assert!(decode(b"-----BEGIN CERTIFICATE-----\nMAIFAA==\n").is_empty());
        assert!(decode(b"-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n").is_empty());
    }

    #[test]
    fn detects_pem() {
        assert!(is_pem(&pem(1, "")));
        assert!(!is_pem(&[0x30, 0x02, 0x05, 0x00]));
    }
}

//! HMAC-SHA256 (RFC 2104) and HKDF (RFC 5869).

use crate::sha256::{Sha256, SHA256_LEN};

/// HMAC-SHA256 output length.
pub const HMAC_LEN: usize = SHA256_LEN;
/// SHA-256 block size.
const BLOCK_LEN: usize = 64;

/// HKDF failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HkdfError {
    /// The output buffer is longer than 255 * HashLen.
    TooLong,
}

/// HMAC-SHA256.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; HMAC_LEN] {
    let mut block = [0u8; BLOCK_LEN];
    if key.len() > BLOCK_LEN {
        block[..SHA256_LEN].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK_LEN];
    let mut opad = [0x5cu8; BLOCK_LEN];
    for i in 0..BLOCK_LEN {
        ipad[i] ^= block[i];
        opad[i] ^= block[i];
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(data);
    let inner = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&inner);
    outer.finalize()
}

/// HKDF-Extract (RFC 5869 §2.2). An empty salt means a zero-filled one.
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; HMAC_LEN] {
    if salt.is_empty() {
        hmac_sha256(&[0u8; HMAC_LEN], ikm)
    } else {
        hmac_sha256(salt, ikm)
    }
}

/// HKDF-Expand (RFC 5869 §2.3).
pub fn hkdf_expand(prk: &[u8], info: &[u8], out: &mut [u8]) -> Result<(), HkdfError> {
    if out.len() > 255 * HMAC_LEN {
        return Err(HkdfError::TooLong);
    }
    let mut previous: [u8; HMAC_LEN] = [0u8; HMAC_LEN];
    let mut have_previous = false;
    let mut written = 0usize;
    let mut counter: u8 = 1;
    while written < out.len() {
        let mut message = alloc::vec::Vec::with_capacity(previous.len() + info.len() + 1);
        if have_previous {
            message.extend_from_slice(&previous);
        }
        message.extend_from_slice(info);
        message.push(counter);
        previous = hmac_sha256(prk, &message);
        have_previous = true;
        let take = (out.len() - written).min(HMAC_LEN);
        out[written..written + take].copy_from_slice(&previous[..take]);
        written += take;
        counter = counter.wrapping_add(1);
    }
    Ok(())
}

/// One-shot HKDF (extract + expand).
pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], out: &mut [u8]) -> Result<(), HkdfError> {
    let prk = hkdf_extract(salt, ikm);
    hkdf_expand(&prk, info, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn hex(bytes: &[u8]) -> alloc::string::String {
        use core::fmt::Write;
        let mut out = alloc::string::String::new();
        for b in bytes {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    fn unhex(s: &str) -> alloc::vec::Vec<u8> {
        let mut out = vec![0u8; s.len() / 2];
        let bytes = s.as_bytes();
        for i in 0..out.len() {
            let hi = (bytes[i * 2] as char).to_digit(16).unwrap() as u8;
            let lo = (bytes[i * 2 + 1] as char).to_digit(16).unwrap() as u8;
            out[i] = (hi << 4) | lo;
        }
        out
    }

    #[test]
    fn hmac_rfc4231_vectors() {
        // Case 1
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        // Case 2
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Case 3
        assert_eq!(
            hex(&hmac_sha256(&[0xaa; 20], &[0xdd; 50])),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"
        );
        // Case 6 (key longer than the block size)
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn hkdf_rfc5869_case1() {
        let ikm = unhex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let salt = unhex("000102030405060708090a0b0c");
        let info = unhex("f0f1f2f3f4f5f6f7f8f9");
        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(
            hex(&prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
        );
        let mut okm = [0u8; 42];
        hkdf_expand(&prk, &info, &mut okm).unwrap();
        assert_eq!(
            hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn hkdf_rfc5869_case3_empty_salt_info() {
        let ikm = unhex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let prk = hkdf_extract(&[], &ikm);
        assert_eq!(
            hex(&prk),
            "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04"
        );
        let mut okm = [0u8; 42];
        hkdf_expand(&prk, &[], &mut okm).unwrap();
        assert_eq!(
            hex(&okm),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
        );
    }

    #[test]
    fn hkdf_expand_is_bounded() {
        let prk = [0u8; 32];
        let mut too_long = vec![0u8; 255 * 32 + 1];
        assert_eq!(hkdf_expand(&prk, &[], &mut too_long), Err(HkdfError::TooLong));
        let mut max = vec![0u8; 255 * 32];
        assert!(hkdf_expand(&prk, &[], &mut max).is_ok());
    }
}

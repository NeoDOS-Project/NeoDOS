//! RSA signature verification (RFC 8017): PKCS#1 v1.5 and PSS with SHA-256.
//!
//! Only *verification* is implemented (public exponent), which is all a TLS
//! client needs. Big integers are little-endian `u32` limbs; modular
//! exponentiation uses Montgomery multiplication (the RSA modulus is odd), so
//! no general division is required.

use crate::sha256::Sha256;

/// RSA verification failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsaError {
    /// The modulus or exponent is unusable (too small, even modulus, ...).
    BadKey,
    /// The signature integer is not in `[0, n)` or its length is wrong.
    BadSignature,
    /// The padding or digest did not match.
    VerificationFailed,
    /// The signature scheme is not supported.
    Unsupported,
}

/// DER prefix of `DigestInfo` for SHA-256 (RFC 8017 §9.2 note 1).
const SHA256_DIGEST_INFO_PREFIX: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

/// RSA public key.
#[derive(Debug, Clone)]
pub struct RsaPublicKey {
    /// Modulus, big-endian without leading zero bytes.
    pub n: alloc::vec::Vec<u8>,
    /// Public exponent, big-endian without leading zero bytes.
    pub e: alloc::vec::Vec<u8>,
}

impl RsaPublicKey {
    pub fn new(n: alloc::vec::Vec<u8>, e: alloc::vec::Vec<u8>) -> Self {
        RsaPublicKey { n, e }
    }

    /// PKCS#1 v1.5 signature with SHA-256 (`rsa_pkcs1_sha256`).
    pub fn verify_pkcs1_sha256(&self, message: &[u8], signature: &[u8]) -> Result<(), RsaError> {
        let key_len = self.n.len();
        let m = self.modexp(signature, key_len)?;
        let mut expected = alloc::vec::Vec::with_capacity(key_len);
        expected.push(0x00);
        expected.push(0x01);
        let hashed = Sha256::digest(message);
        let t_len = SHA256_DIGEST_INFO_PREFIX.len() + hashed.len();
        if key_len < t_len + 11 {
            return Err(RsaError::BadKey);
        }
        expected.resize(expected.len() + (key_len - t_len - 3), 0xff);
        expected.push(0x00);
        expected.extend_from_slice(&SHA256_DIGEST_INFO_PREFIX);
        expected.extend_from_slice(&hashed);
        if m == expected {
            Ok(())
        } else {
            Err(RsaError::VerificationFailed)
        }
    }

    /// RSASSA-PSS with SHA-256, MGF1-SHA256 and an explicit salt length.
    pub fn verify_pss_sha256(
        &self,
        message: &[u8],
        signature: &[u8],
        salt_len: usize,
    ) -> Result<(), RsaError> {
        let key_len = self.n.len();
        // emBits = modBits - 1; emLen = ceil(emBits / 8).
        let em_bits = key_len * 8 - 1;
        let em_len = (em_bits + 7) / 8;
        let m = self.modexp(signature, key_len)?;
        // The integer must fit emLen bytes.
        if m.len() > em_len {
            return Err(RsaError::VerificationFailed);
        }
        let mut em = alloc::vec![0u8; em_len];
        em[em_len - m.len()..].copy_from_slice(&m);

        let h_len = 32;
        if em_len < h_len + salt_len + 2 {
            return Err(RsaError::BadKey);
        }
        let masked_db_len = em_len - h_len - 1;
        if em[em_len - 1] != 0xbc {
            return Err(RsaError::VerificationFailed);
        }
        let masked_db = &em[..masked_db_len];
        let h = &em[masked_db_len..masked_db_len + h_len];

        let unused_bits = em_len * 8 - em_bits;
        if unused_bits > 0 && (masked_db[0] >> (8 - unused_bits)) != 0 {
            return Err(RsaError::VerificationFailed);
        }
        let db_mask = mgf1_sha256(h, masked_db_len);
        let mut db = alloc::vec![0u8; masked_db_len];
        for i in 0..masked_db_len {
            db[i] = masked_db[i] ^ db_mask[i];
        }
        if unused_bits > 0 {
            db[0] &= 0xff >> unused_bits;
        }
        let ps_len = masked_db_len - salt_len - 1;
        if db[..ps_len].iter().any(|b| *b != 0) || db[ps_len] != 0x01 {
            return Err(RsaError::VerificationFailed);
        }
        let salt = &db[ps_len + 1..];

        let mut m_prime = alloc::vec::Vec::with_capacity(8 + h_len + salt.len());
        m_prime.extend_from_slice(&[0u8; 8]);
        m_prime.extend_from_slice(&Sha256::digest(message));
        m_prime.extend_from_slice(salt);
        let h_prime = Sha256::digest(&m_prime);
        if h_prime[..] == h[..] {
            Ok(())
        } else {
            Err(RsaError::VerificationFailed)
        }
    }

    /// `signature^e mod n`, returned as exactly `key_len` big-endian bytes.
    fn modexp(&self, signature: &[u8], key_len: usize) -> Result<alloc::vec::Vec<u8>, RsaError> {
        let n = Big::from_be(&self.n);
        if n.limbs.is_empty() || n.limbs.len() < 8 || n.limbs[0] & 1 == 0 {
            return Err(RsaError::BadKey);
        }
        let sig = Big::from_be(signature);
        if sig.cmp(&n) != core::cmp::Ordering::Less {
            return Err(RsaError::BadSignature);
        }
        let e = Big::from_be(&self.e);
        if e.limbs.is_empty() {
            return Err(RsaError::BadKey);
        }
        let mont = Mont::new(&n);
        let base = mont.to_mont(&sig);
        let result = mont.pow(&base, &e);
        let out = mont.from_mont(&result);
        let mut bytes = out.to_be();
        if bytes.len() > key_len {
            return Err(RsaError::VerificationFailed);
        }
        let mut padded = alloc::vec![0u8; key_len - bytes.len()];
        padded.append(&mut bytes);
        Ok(padded)
    }
}

fn mgf1_sha256(seed: &[u8], len: usize) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(len);
    let mut counter: u32 = 0;
    while out.len() < len {
        let mut input = alloc::vec::Vec::with_capacity(seed.len() + 4);
        input.extend_from_slice(seed);
        input.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&Sha256::digest(&input));
        counter = counter.wrapping_add(1);
    }
    out.truncate(len);
    out
}

// ── Big integers (little-endian u32 limbs) ──

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Big {
    pub(crate) limbs: alloc::vec::Vec<u32>,
}

impl Big {
    pub(crate) fn from_be(bytes: &[u8]) -> Self {
        let mut limbs = alloc::vec::Vec::with_capacity((bytes.len() + 3) / 4);
        let mut i = bytes.len();
        while i > 0 {
            let start = i.saturating_sub(4);
            let mut v = 0u32;
            for b in &bytes[start..i] {
                v = (v << 8) | *b as u32;
            }
            limbs.push(v);
            i = start;
        }
        while limbs.len() > 1 && *limbs.last().unwrap() == 0 {
            limbs.pop();
        }
        Big { limbs }
    }

    pub(crate) fn to_be(&self) -> alloc::vec::Vec<u8> {
        let mut bytes = alloc::vec::Vec::with_capacity(self.limbs.len() * 4);
        for limb in self.limbs.iter().rev() {
            bytes.extend_from_slice(&limb.to_be_bytes());
        }
        while bytes.len() > 1 && bytes[0] == 0 {
            bytes.remove(0);
        }
        bytes
    }

    pub(crate) fn from_u32(v: u32) -> Self {
        Big { limbs: alloc::vec![v] }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.limbs.iter().all(|l| *l == 0)
    }

    pub(crate) fn from_hex(hex: &str) -> Self {
        let mut bytes = alloc::vec::Vec::with_capacity(hex.len() / 2);
        let b = hex.as_bytes();
        let mut i = 0;
        if b.len() % 2 == 1 {
            let hi = (b[0] as char).to_digit(16).unwrap() as u8;
            bytes.push(hi);
            i = 1;
        }
        while i + 1 < b.len() {
            let hi = (b[i] as char).to_digit(16).unwrap() as u8;
            let lo = (b[i + 1] as char).to_digit(16).unwrap() as u8;
            bytes.push((hi << 4) | lo);
            i += 2;
        }
        Big::from_be(&bytes)
    }

    pub(crate) fn len(&self) -> usize {
        let mut n = self.limbs.len();
        while n > 1 && self.limbs[n - 1] == 0 {
            n -= 1;
        }
        n
    }

    pub(crate) fn bit(&self, i: usize) -> bool {
        (self.limbs[i / 32] >> (i % 32)) & 1 == 1
    }

    pub(crate) fn bits(&self) -> usize {
        let n = self.len();
        if n == 0 {
            return 0;
        }
        let top = self.limbs[n - 1];
        n * 32 - top.leading_zeros() as usize
    }

    pub(crate) fn cmp(&self, other: &Big) -> core::cmp::Ordering {
        let a = self.len();
        let b = other.len();
        if a != b {
            return a.cmp(&b);
        }
        for i in (0..a).rev() {
            if self.limbs[i] != other.limbs[i] {
                return self.limbs[i].cmp(&other.limbs[i]);
            }
        }
        core::cmp::Ordering::Equal
    }

    pub(crate) fn sub(&self, other: &Big) -> Big {
        let mut out = alloc::vec![0u32; self.len().max(other.len())];
        let mut borrow = 0i64;
        for i in 0..out.len() {
            let a = *self.limbs.get(i).unwrap_or(&0) as i64;
            let b = *other.limbs.get(i).unwrap_or(&0) as i64;
            let v = a - b - borrow;
            if v < 0 {
                out[i] = (v + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                out[i] = v as u32;
                borrow = 0;
            }
        }
        let mut big = Big { limbs: out };
        while big.limbs.len() > 1 && *big.limbs.last().unwrap() == 0 {
            big.limbs.pop();
        }
        big
    }

    pub(crate) fn add(&self, other: &Big) -> Big {
        let mut out = alloc::vec![0u32; self.len().max(other.len())];
        let mut carry = 0u64;
        for i in 0..out.len() {
            let v = *self.limbs.get(i).unwrap_or(&0) as u64
                + *other.limbs.get(i).unwrap_or(&0) as u64
                + carry;
            out[i] = v as u32;
            carry = v >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        Big { limbs: out }
    }
}

/// Montgomery context for an odd modulus.
pub(crate) struct Mont {
    pub(crate) n: Big,
    pub(crate) n0inv: u32,
    pub(crate) rr: Big, // R^2 mod n
    pub(crate) len: usize,
}

impl Mont {
    pub(crate) fn new(n: &Big) -> Self {
        let len = n.limbs.len();
        let n0inv = {
            let mut x = n.limbs[0];
            for _ in 0..5 {
                x = x.wrapping_mul(2u32.wrapping_sub(n.limbs[0].wrapping_mul(x)));
            }
            x.wrapping_neg()
        };
        // R = 2^(32*len); compute R^2 mod n by doubling 1 (64*len times).
        let mut rr = Big { limbs: alloc::vec![1] };
        for _ in 0..(64 * len) {
            rr = add_mod(&rr.add(&rr), n);
        }
        Mont {
            n: n.clone(),
            n0inv,
            rr,
            len,
        }
    }

    pub(crate) fn mont_mul(&self, a: &Big, b: &Big) -> Big {
        let len = self.len;
        let mut t = alloc::vec![0u32; 2 * len + 4];
        for i in 0..len {
            let ai = *a.limbs.get(i).unwrap_or(&0) as u64;
            let mut carry = 0u64;
            for j in 0..len {
                let v = t[i + j] as u64 + ai * *b.limbs.get(j).unwrap_or(&0) as u64 + carry;
                t[i + j] = v as u32;
                carry = v >> 32;
            }
            let mut k = i + len;
            while carry != 0 {
                let v = t[k] as u64 + carry;
                t[k] = v as u32;
                carry = v >> 32;
                k += 1;
            }
        }
        // Montgomery reduction.
        for i in 0..len {
            let m = t[i].wrapping_mul(self.n0inv);
            let mut carry = 0u64;
            for j in 0..len {
                let v = t[i + j] as u64 + m as u64 * self.n.limbs[j] as u64 + carry;
                t[i + j] = v as u32;
                carry = v >> 32;
            }
            let mut k = i + len;
            while carry != 0 {
                let v = t[k] as u64 + carry;
                t[k] = v as u32;
                carry = v >> 32;
                k += 1;
                if k >= t.len() {
                    break;
                }
            }
        }
        // The REDC result is < 2n but can carry into limb 2*len (R > n).
        let mut out = Big {
            limbs: t[len..2 * len + 2].to_vec(),
        };
        // Drop any overflow limbs the product cannot really have (defensive).
        while out.cmp(&self.n) != core::cmp::Ordering::Less {
            out = out.sub(&self.n);
        }
        while out.limbs.len() > 1 && *out.limbs.last().unwrap() == 0 {
            out.limbs.pop();
        }
        out
    }

    pub(crate) fn to_mont(&self, a: &Big) -> Big {
        self.mont_mul(a, &self.rr)
    }

    pub(crate) fn from_mont(&self, a: &Big) -> Big {
        self.mont_mul(a, &Big { limbs: alloc::vec![1] })
    }

    pub(crate) fn pow(&self, base: &Big, e: &Big) -> Big {
        let mut result = self.to_mont(&Big { limbs: alloc::vec![1] });
        let bits = e.bits();
        for i in (0..bits).rev() {
            result = self.mont_mul(&result, &result);
            if e.bit(i) {
                result = self.mont_mul(&result, base);
            }
        }
        result
    }
}

/// Reduce `a` in `[0, 2n)` modulo `n` with one conditional subtraction.
fn add_mod(a: &Big, n: &Big) -> Big {
    if a.cmp(n) != core::cmp::Ordering::Less {
        a.sub(n)
    } else {
        a.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    // Generated with OpenSSL 4.0.3 (rsa_keygen_bits:2048, SHA-256, PSS salt 32).
    const N: &str = "9a24760c419f0023d3924a230703ede1f99023f6e2d394937b273d13d13097b4\
fe5bde7626a5629bfc25849c1377f79c72cb50e956199b46538f286c6f766713568fad02f80c97f923\
b4c49411609aaf058b273278920a2b5d5e57c077c74c80c93f120fe268fac6841ef28b5411a78b6243\
b20fcbbfacbe9d75cf5b8f0d8a52e2edede26729d1b1f94745424552c4c22ad63edea209c7585fed85\
a6195083ef3c1167021fb00c31b663d197d0d109d229d41ad4257401f9e2fffcd22cc237b3f49dcc852\
3c19cefed8b5a826514560d6b684344ecccacb547d1dc9e09fde394eecbcf12d9cca512121ff953fda11\
0a4246f22e27c1138b1196fc82eb44a823b";
    const MSG: &[u8] = b"NeoDOS TLS test vector";
    const PKCS1: &str = "0a72c8cb002eff6e5d4159306ee673991ff0aafe1964609685d5508cfda1c41f0f4265ed\
bfc360da98420c77c7cea05840ccb9df4521a7c5dd61d7b507c400715b72bcb60e6e1ab82154a85248\
6c0c13e4d6192f2fa1056ebb2e43201fda371ed259ee700b75e8f22baf349a1c817817b5f7285c9d9a\
41914db20a62d7a2216c24c1ab198de402005ce30ac629c3e38631e9ca3fe7cfadeca0f998ef24ca85\
a27400a245348f83d8cd490e27d07686dff7b44b38ea0c157de6e968542433984c787f0fe762fdba60\
61cc34f93e8c2c1544810d16a018704266f4edef6833e04980c8571136ed4d8e0e327b482712df5e5e\
78f530b197db7d7155d2e354e6395e";
    const PSS: &str = "29f0de5969cefb78c7413f1c74a594d4d21ec22187f67ee4244bf192cf1c25998b19b666\
59785797965c79b55232b1798867547fc30ee25db7c0f9a5129ae0f98a0bb64ce19c7a8130cc99ade7\
8954a32b0d1a90f017081e0eec099d99a9ee53a403ee38ff1effa9438379710398919f5e2a3e7efdc0\
a30cc21c49454742c4cf828dff9b7791ffb990c0523ef517d1a7a25434950981bf7facff97f39a2d27\
5c872cfe0f145c14baa15405465690cc1e6f1c40652384bc972515b5b852de2f5718f42c250d57505c\
bea55afe0a8ebb664a9f7d09720351421984e58beac0bb043440788333bab4815ce590a9faf1fb1b8b\
75c9ef3fd40a12bc3ed5ae9bf70089";

    fn unhex(s: &str) -> alloc::vec::Vec<u8> {
        let mut out = vec![0u8; s.len() / 2];
        let b = s.as_bytes();
        for i in 0..out.len() {
            let hi = (b[i * 2] as char).to_digit(16).unwrap() as u8;
            let lo = (b[i * 2 + 1] as char).to_digit(16).unwrap() as u8;
            out[i] = (hi << 4) | lo;
        }
        out
    }

    fn key() -> RsaPublicKey {
        RsaPublicKey::new(unhex(N), vec![0x01, 0x00, 0x01])
    }

    #[test]
    fn rsa_pkcs1_v15_sha256_verifies() {
        let sig = unhex(PKCS1);
        assert_eq!(key().verify_pkcs1_sha256(MSG, &sig), Ok(()));
        // A different message must fail.
        assert_eq!(
            key().verify_pkcs1_sha256(b"other", &sig),
            Err(RsaError::VerificationFailed)
        );
        // A tampered signature must fail.
        let mut bad = sig.clone();
        bad[10] ^= 0x01;
        assert_eq!(
            key().verify_pkcs1_sha256(MSG, &bad),
            Err(RsaError::VerificationFailed)
        );
    }

    #[test]
    fn rsa_pss_sha256_verifies() {
        let sig = unhex(PSS);
        assert_eq!(key().verify_pss_sha256(MSG, &sig, 32), Ok(()));
        assert_eq!(
            key().verify_pss_sha256(b"other", &sig, 32),
            Err(RsaError::VerificationFailed)
        );
        let mut bad = sig.clone();
        bad[200] ^= 0x08;
        assert_eq!(
            key().verify_pss_sha256(MSG, &bad, 32),
            Err(RsaError::VerificationFailed)
        );
    }

    #[test]
    fn rsa_rejects_bad_keys_and_signatures() {
        let sig = unhex(PKCS1);
        // Even modulus.
        let mut even = unhex(N);
        even[255] &= 0xfe;
        let k = RsaPublicKey::new(even, vec![0x01, 0x00, 0x01]);
        assert_eq!(k.verify_pkcs1_sha256(MSG, &sig), Err(RsaError::BadKey));
        // Signature >= modulus (all 0xff is larger than a 2048-bit modulus
        // unless the modulus starts with 0xff).
        let too_big = vec![0xffu8; 256];
        assert_eq!(
            key().verify_pkcs1_sha256(MSG, &too_big),
            Err(RsaError::BadSignature)
        );
    }
}

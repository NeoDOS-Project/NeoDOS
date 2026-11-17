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
        let em_len = em_bits.div_ceil(8);
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
        let mut limbs = alloc::vec::Vec::with_capacity(bytes.len().div_ceil(4));
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

    // The three slices are indexed in lockstep and have different lengths.
    #[allow(clippy::needless_range_loop)]
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

    // The three slices are indexed in lockstep and have different lengths.
    #[allow(clippy::needless_range_loop)]
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
        // In place: a 2048-bit key needs 4096 doublings and the userland heap
        // never reuses freed memory, so per-step allocations exhausted it.
        let mut rr = Big::from_u32(1);
        for _ in 0..(64 * len) {
            rr.double_mod_in_place(n);
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

    // `from_mont` reads as a conversion, not a method on `a`.
    #[allow(clippy::wrong_self_convention)]
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

impl Big {
    /// `self = 2 * self mod n` without allocating a new value per step.
    fn double_mod_in_place(&mut self, n: &Big) {
        let mut carry = 0u32;
        for limb in self.limbs.iter_mut() {
            let v = ((*limb as u64) << 1) | carry as u64;
            *limb = v as u32;
            carry = (v >> 32) as u32;
        }
        if carry != 0 {
            self.limbs.push(carry);
        }
        if self.cmp(n) != core::cmp::Ordering::Less {
            *self = self.sub(n);
        }
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
        // Test vectors are written as indented string continuations, so the
        // first segment keeps its leading spaces.
        let digits: alloc::vec::Vec<u8> =
            s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
        let mut out = vec![0u8; digits.len() / 2];
        let b = &digits[..];
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

    const N4096: &str = "    ad5468aa730ea1c87de75324252834a8bb88800da21417741ee95589d530e7ca1834ba3e929c\
    bb6c3ab4700441d667f53c32f2995e897431905b78b02bc22fc1b3f4b8ed4cc533237ab70cca\
    80f0d0bbf352a862a1d6542598a46c70b1426e26237d746f6ca257dd20808ef624f5501b5bb8\
    410739800cc53d0ed9071a7c32595f7b04f4624d6dfe4fc742704b9724d7cfaedab1af86a9fc\
    77b2e55013078f9eaa1a705ae71577e08a33c510ba332aa384463a500dabcff88e4ed4e58f6a\
    ef82c1f3e20d72ab63cfedf93e6251f4785c35fb53fb2a8e710963812051c5afc179eebffd28\
    3da65de472b2a5bb82c0010bb9c2fc51c10bdf0814e642ce9bb19d01f350851335a085175a76\
    f59d43c7fd404c8529e1332c5ea2b87d6a369b6097baea0579c79919ebc1fafe067bc37573c6\
    8117a64e961d3b526f54f88c06ba1fd074c41c65a3a0798baa7ff7df68d09c10f134f016ff60\
    06cd1dfcd71a77fd02abf3917f3061d41d1245706f8f341d84c9c12538171324a47dc125a929\
    88f1158d9b273a7210ecbf794d967fa71346c6add7784ca9ae7405dff61ff4b8942f9d049b77\
    4990637d76c7caa5d46aff2ef56f1beedeb26a6a5f29d0c089c7e7ca0203a5927b514128c5a5\
    a2732a14b27ff20cab5f4b2caaab8eb983b6aab0755a9c0057bba519f189f464c0e31625b83f\
    0fd352738f3e6305178fbb26ebb23300ce95";
    const SIG4096: &str = "    8c8f64243fdbc4f8fe8a22071171b78db90a724e4b3fc3ad7b01f0bf77dd640f743c9420f892\
    3816106a9c122243c89246480fee588bd3e4a23a64db71033f98577a2b11a986561c96d6a79c\
    fe8ebc025785aafe548e969d64e8e32b706a38a11d27483fd2800722ea942e247df1e18f3e62\
    712acacb3b0510c4fc71d0a2f3d8367a768f46737695898c827bc4f9064d36cfd87abafe5aeb\
    4af0c03c14ec030ca14efae173e75718f9e5c28f3eb1653e71de248e7b58564c46d928bd0cdb\
    26cd38bbd32f12ff903d076756ee245afbd9186723cb5f6342d88a4b99c994c21a8ce0179841\
    63b6f05ffdd31fe5cb713138c811815d39bf29301f733efd6b0cfc8902457598a6131fa6a572\
    f19b68dab0ff69d2dcbf97a2391a247dfc7429d3b54f0a665ea4f4d6a62d99de40d8b82d8e1c\
    acfbec72611b97e6f7fca09f1f579645ca303851bc9bed598c77ab910f997d2ec2cb36fede85\
    9dc5e41deb458e675d2ea9a9bcb9331ae7955c4edeadfc1ba63bd954109f8eb69727b652496c\
    e6e0e58a757c1c2e7228973724b514f3ae8bfd8347109f5042970de152bb99e5c09b7c1fb502\
    81241a38c289aa7fa026ee5cfb4e05a483d3b0aaec787a48606acaa3ae8c4ab9bcc1f18ee033\
    07cb64887d66fd56376b67867fb5bb33f194e6d70adea777c43f41564b1b9100661bbdebdb94\
    b04c2bc51d03e5794c69369725e59528ab6a";

    #[test]
    fn rsa_4096_pkcs1_verifies() {
        // Real chains use 4096-bit roots (ISRG Root X1, GTS Root R4), so the
        // Montgomery context has to work with 128 limbs.
        let key = RsaPublicKey::new(unhex(N4096), vec![0x01, 0x00, 0x01]);
        assert_eq!(key.verify_pkcs1_sha256(MSG, &unhex(SIG4096)), Ok(()));
        assert_eq!(
            key.verify_pkcs1_sha256(b"other", &unhex(SIG4096)),
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

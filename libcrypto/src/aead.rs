//! ChaCha20-Poly1305 AEAD (RFC 8439).
//!
//! The two primitives are validated independently (ChaCha20 against the RFC
//! block vector, Poly1305 against the RFC §2.5.2 tag, also cross-checked with
//! OpenSSL) and then composed exactly as §2.8 prescribes.

/// Poly1305 tag length.
pub const TAG_LEN: usize = 16;

/// AEAD failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AeadError {
    /// The authentication tag did not match.
    AuthFailed,
    /// The output buffer was too small.
    BufferTooSmall,
}

const CONSTANTS: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

#[inline]
fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// One ChaCha20 keystream block.
pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[..4].copy_from_slice(&CONSTANTS);
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[i * 4],
            key[i * 4 + 1],
            key[i * 4 + 2],
            key[i * 4 + 3],
        ]);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4],
            nonce[i * 4 + 1],
            nonce[i * 4 + 2],
            nonce[i * 4 + 3],
        ]);
    }
    let mut work = state;
    for _ in 0..10 {
        quarter_round(&mut work, 0, 4, 8, 12);
        quarter_round(&mut work, 1, 5, 9, 13);
        quarter_round(&mut work, 2, 6, 10, 14);
        quarter_round(&mut work, 3, 7, 11, 15);
        quarter_round(&mut work, 0, 5, 10, 15);
        quarter_round(&mut work, 1, 6, 11, 12);
        quarter_round(&mut work, 2, 7, 8, 13);
        quarter_round(&mut work, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[i * 4..i * 4 + 4].copy_from_slice(&work[i].wrapping_add(state[i]).to_le_bytes());
    }
    out
}

/// XOR `data` with the ChaCha20 keystream starting at `counter`.
fn chacha20_xor(key: &[u8; 32], nonce: &[u8; 12], mut counter: u32, data: &mut [u8]) {
    let mut offset = 0usize;
    while offset < data.len() {
        let block = chacha20_block(key, counter, nonce);
        counter = counter.wrapping_add(1);
        let take = (data.len() - offset).min(64);
        for i in 0..take {
            data[offset + i] ^= block[i];
        }
        offset += take;
    }
}

// ── Poly1305 (32-bit Donna limbs) ──

struct Poly1305 {
    r: [u32; 5],
    h: [u32; 5],
    pad: [u32; 4],
    buf: [u8; 16],
    buf_len: usize,
}

impl Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        // 26-bit limbs of r, read as the overlapping words at 0/3/6/9/12 and
        // clamped by the masks (RFC 8439 §2.5).
        let w = |off: usize| -> u32 {
            u32::from_le_bytes([key[off], key[off + 1], key[off + 2], key[off + 3]])
        };
        Poly1305 {
            r: [
                w(0) & 0x03ff_ffff,
                (w(3) >> 2) & 0x03ff_ff03,
                (w(6) >> 4) & 0x03ff_c0ff,
                (w(9) >> 6) & 0x03f0_3fff,
                (w(12) >> 8) & 0x000f_ffff,
            ],
            h: [0u32; 5],
            pad: [
                w(16),
                w(20),
                w(24),
                w(28),
            ],
            buf: [0u8; 16],
            buf_len: 0,
        }
    }

    fn blocks(&mut self, data: &[u8], final_block: bool) {
        let hibit: u32 = if final_block { 0 } else { 1 << 24 };
        let r = self.r;
        // 2^130 ≡ 5 (mod p): terms that wrap from limb 5+ pick up a 5.
        let s1 = r[1].wrapping_mul(5);
        let s2 = r[2].wrapping_mul(5);
        let s3 = r[3].wrapping_mul(5);
        let s4 = r[4].wrapping_mul(5);
        let mut h0 = self.h[0];
        let mut h1 = self.h[1];
        let mut h2 = self.h[2];
        let mut h3 = self.h[3];
        let mut h4 = self.h[4];
        for chunk in data.chunks(16) {
            // Overlapping 32-bit little-endian words at offsets 0, 3, 6, 9, 12
            // (the 26-bit limb boundaries), zero-padded for a partial block.
            let word = |off: usize| -> u32 {
                let mut b = [0u8; 4];
                for i in 0..4 {
                    if off + i < chunk.len() {
                        b[i] = chunk[off + i];
                    }
                }
                u32::from_le_bytes(b)
            };
            h0 = h0.wrapping_add(word(0) & 0x3ff_ffff);
            h1 = h1.wrapping_add((word(3) >> 2) & 0x3ff_ffff);
            h2 = h2.wrapping_add((word(6) >> 4) & 0x3ff_ffff);
            h3 = h3.wrapping_add((word(9) >> 6) & 0x3ff_ffff);
            h4 = h4.wrapping_add((word(12) >> 8) | hibit);

            let d0 = (h0 as u64 * r[0] as u64)
                + (h1 as u64 * s4 as u64)
                + (h2 as u64 * s3 as u64)
                + (h3 as u64 * s2 as u64)
                + (h4 as u64 * s1 as u64);
            let mut d1 = (h0 as u64 * r[1] as u64)
                + (h1 as u64 * r[0] as u64)
                + (h2 as u64 * s4 as u64)
                + (h3 as u64 * s3 as u64)
                + (h4 as u64 * s2 as u64);
            let mut d2 = (h0 as u64 * r[2] as u64)
                + (h1 as u64 * r[1] as u64)
                + (h2 as u64 * r[0] as u64)
                + (h3 as u64 * s4 as u64)
                + (h4 as u64 * s3 as u64);
            let mut d3 = (h0 as u64 * r[3] as u64)
                + (h1 as u64 * r[2] as u64)
                + (h2 as u64 * r[1] as u64)
                + (h3 as u64 * r[0] as u64)
                + (h4 as u64 * s4 as u64);
            let mut d4 = (h0 as u64 * r[4] as u64)
                + (h1 as u64 * r[3] as u64)
                + (h2 as u64 * r[2] as u64)
                + (h3 as u64 * r[1] as u64)
                + (h4 as u64 * r[0] as u64);

            let mut c = (d0 >> 26) as u32;
            h0 = (d0 as u32) & 0x3ff_ffff;
            d1 += c as u64;
            c = (d1 >> 26) as u32;
            h1 = (d1 as u32) & 0x3ff_ffff;
            d2 += c as u64;
            c = (d2 >> 26) as u32;
            h2 = (d2 as u32) & 0x3ff_ffff;
            d3 += c as u64;
            c = (d3 >> 26) as u32;
            h3 = (d3 as u32) & 0x3ff_ffff;
            d4 += c as u64;
            c = (d4 >> 26) as u32;
            h4 = (d4 as u32) & 0x3ff_ffff;
            h0 += c.wrapping_mul(5);
            c = h0 >> 26;
            h0 &= 0x3ff_ffff;
            h1 += c;
        }
        self.h = [h0, h1, h2, h3, h4];
    }

    fn update(&mut self, mut data: &[u8]) {
        if self.buf_len > 0 {
            let take = (16 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 16 {
                let block = self.buf;
                self.blocks(&block, false);
                self.buf_len = 0;
            }
        }
        let full = data.len() / 16 * 16;
        if full > 0 {
            self.blocks(&data[..full], false);
            data = &data[full..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finalize(mut self) -> [u8; TAG_LEN] {
        if self.buf_len > 0 {
            // The trailing partial block carries an explicit 0x01 terminator
            // (the 2^128 bit of a full block) and is processed with hibit = 0.
            let mut last = [0u8; 16];
            last[..self.buf_len].copy_from_slice(&self.buf[..self.buf_len]);
            last[self.buf_len] = 1;
            self.blocks(&last, true);
        }
        let mut h0 = self.h[0];
        let mut h1 = self.h[1];
        let mut h2 = self.h[2];
        let mut h3 = self.h[3];
        let mut h4 = self.h[4];

        let mut c = h1 >> 26;
        h1 &= 0x3ff_ffff;
        h2 += c;
        c = h2 >> 26;
        h2 &= 0x3ff_ffff;
        h3 += c;
        c = h3 >> 26;
        h3 &= 0x3ff_ffff;
        h4 += c;
        c = h4 >> 26;
        h4 &= 0x3ff_ffff;
        h0 += c.wrapping_mul(5);
        c = h0 >> 26;
        h0 &= 0x3ff_ffff;
        h1 += c;

        let mut g0 = h0.wrapping_add(5);
        c = g0 >> 26;
        g0 &= 0x3ff_ffff;
        let mut g1 = h1.wrapping_add(c);
        c = g1 >> 26;
        g1 &= 0x3ff_ffff;
        let mut g2 = h2.wrapping_add(c);
        c = g2 >> 26;
        g2 &= 0x3ff_ffff;
        let mut g3 = h3.wrapping_add(c);
        c = g3 >> 26;
        g3 &= 0x3ff_ffff;
        let g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);

        let mask = (g4 >> 31).wrapping_sub(1);
        g0 &= mask;
        g1 &= mask;
        g2 &= mask;
        g3 &= mask;
        let g4m = g4 & mask;
        let not_mask = !mask;
        h0 = (h0 & not_mask) | g0;
        h1 = (h1 & not_mask) | g1;
        h2 = (h2 & not_mask) | g2;
        h3 = (h3 & not_mask) | g3;
        h4 = (h4 & not_mask) | g4m;

        let h0w = (h0 | (h1 << 26)) as u64;
        let h1w = ((h1 >> 6) | (h2 << 20)) as u64;
        let h2w = ((h2 >> 12) | (h3 << 14)) as u64;
        let h3w = ((h3 >> 18) | (h4 << 8)) as u64;

        let f = h0w + self.pad[0] as u64;
        let h0 = f as u32;
        let f = h1w + self.pad[1] as u64 + (f >> 32);
        let h1 = f as u32;
        let f = h2w + self.pad[2] as u64 + (f >> 32);
        let h2 = f as u32;
        let f = h3w + self.pad[3] as u64 + (f >> 32);
        let h3 = f as u32;

        let mut tag = [0u8; TAG_LEN];
        tag[0..4].copy_from_slice(&h0.to_le_bytes());
        tag[4..8].copy_from_slice(&h1.to_le_bytes());
        tag[8..12].copy_from_slice(&h2.to_le_bytes());
        tag[12..16].copy_from_slice(&h3.to_le_bytes());
        tag
    }
}

fn poly1305(key: &[u8; 32], data: &[u8]) -> [u8; TAG_LEN] {
    let mut p = Poly1305::new(key);
    p.update(data);
    p.finalize()
}

fn pad16(out: &mut alloc::vec::Vec<u8>, len: usize) {
    let rem = len % 16;
    if rem != 0 {
        out.resize(out.len() + (16 - rem), 0);
    }
}

fn tag_input(aad: &[u8], ciphertext: &[u8]) -> alloc::vec::Vec<u8> {
    let mut mac_data = alloc::vec::Vec::with_capacity(aad.len() + ciphertext.len() + 32);
    mac_data.extend_from_slice(aad);
    pad16(&mut mac_data, aad.len());
    mac_data.extend_from_slice(ciphertext);
    pad16(&mut mac_data, ciphertext.len());
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    mac_data
}

/// ChaCha20-Poly1305 AEAD (RFC 8439 §2.8).
pub struct ChaCha20Poly1305;

impl ChaCha20Poly1305 {
    /// Encrypt `plaintext` into `out` and return the tag.
    pub fn seal(
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        plaintext: &[u8],
        out: &mut [u8],
    ) -> Result<[u8; TAG_LEN], AeadError> {
        if out.len() < plaintext.len() {
            return Err(AeadError::BufferTooSmall);
        }
        out[..plaintext.len()].copy_from_slice(plaintext);
        chacha20_xor(key, nonce, 1, &mut out[..plaintext.len()]);
        let poly_key = chacha20_block(key, 0, nonce);
        let mut poly_key32 = [0u8; 32];
        poly_key32.copy_from_slice(&poly_key[..32]);
        Ok(poly1305(
            &poly_key32,
            &tag_input(aad, &out[..plaintext.len()]),
        ))
    }

    /// Verify the tag and decrypt into `out`.
    pub fn open(
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
        tag: &[u8; TAG_LEN],
        out: &mut [u8],
    ) -> Result<(), AeadError> {
        if out.len() < ciphertext.len() {
            return Err(AeadError::BufferTooSmall);
        }
        let poly_key = chacha20_block(key, 0, nonce);
        let mut poly_key32 = [0u8; 32];
        poly_key32.copy_from_slice(&poly_key[..32]);
        let expected = poly1305(&poly_key32, &tag_input(aad, ciphertext));
        // Constant-time comparison.
        let mut diff = 0u8;
        for i in 0..TAG_LEN {
            diff |= expected[i] ^ tag[i];
        }
        if diff != 0 {
            return Err(AeadError::AuthFailed);
        }
        out[..ciphertext.len()].copy_from_slice(ciphertext);
        chacha20_xor(key, nonce, 1, &mut out[..ciphertext.len()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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

    fn hex(bytes: &[u8]) -> alloc::string::String {
        use core::fmt::Write;
        let mut out = alloc::string::String::new();
        for b in bytes {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    fn key32(s: &str) -> [u8; 32] {
        let mut k = [0u8; 32];
        k.copy_from_slice(&unhex(s));
        k
    }

    #[test]
    fn chacha20_rfc8439_block() {
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        let nonce = [0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0];
        assert_eq!(
            hex(&chacha20_block(&key, 1, &nonce)),
            "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e\
             d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e"
        );
    }

    #[test]
    fn poly1305_rfc8439_2_5_2() {
        // Cross-checked with `openssl mac -macopt hexkey:... POLY1305`.
        let key = key32("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
        let tag = poly1305(&key, b"Cryptographic Forum Research Group");
        assert_eq!(hex(&tag), "a8061dc1305136c6c22b8baf0c0127a9");
    }

    #[test]
    fn aead_rfc8439_2_8_2() {
        let key = key32("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let nonce = [0x07, 0, 0, 0, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
        let aad = unhex("50515253c0c1c2c3c4c5c6c7");
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you \
                          only one tip for the future, sunscreen would be it.";
        let mut ciphertext = vec![0u8; plaintext.len()];
        let tag = ChaCha20Poly1305::seal(&key, &nonce, &aad, plaintext, &mut ciphertext).unwrap();
        assert_eq!(
            hex(&ciphertext),
            "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6\
             3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36\
             92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc\
             3ff4def08e4b7a9de576d26586cec64b6116"
        );
        assert_eq!(hex(&tag), "1ae10b594f09e26a7e902ecbd0600691");

        let mut out = vec![0u8; ciphertext.len()];
        ChaCha20Poly1305::open(&key, &nonce, &aad, &ciphertext, &tag, &mut out).unwrap();
        assert_eq!(&out[..], plaintext);
    }

    #[test]
    fn aead_rejects_tampering() {
        let key = [7u8; 32];
        let nonce = [9u8; 12];
        let plaintext = b"hello tls record";
        let mut ciphertext = vec![0u8; plaintext.len()];
        let tag = ChaCha20Poly1305::seal(&key, &nonce, b"aad", plaintext, &mut ciphertext).unwrap();

        let mut out = vec![0u8; ciphertext.len()];
        // Flip one ciphertext bit.
        let mut bad = ciphertext.clone();
        bad[0] ^= 1;
        assert_eq!(
            ChaCha20Poly1305::open(&key, &nonce, b"aad", &bad, &tag, &mut out),
            Err(AeadError::AuthFailed)
        );
        // Flip one tag bit.
        let mut bad_tag = tag;
        bad_tag[15] ^= 0x80;
        assert_eq!(
            ChaCha20Poly1305::open(&key, &nonce, b"aad", &ciphertext, &bad_tag, &mut out),
            Err(AeadError::AuthFailed)
        );
        // Wrong AAD.
        assert_eq!(
            ChaCha20Poly1305::open(&key, &nonce, b"bad", &ciphertext, &tag, &mut out),
            Err(AeadError::AuthFailed)
        );
        // Untampered round-trip.
        assert!(ChaCha20Poly1305::open(&key, &nonce, b"aad", &ciphertext, &tag, &mut out).is_ok());
        assert_eq!(&out[..], plaintext);
    }
}

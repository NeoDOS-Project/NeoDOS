//! X25519 key agreement (RFC 7748).
//!
//! Field elements are five 51-bit limbs in a `u64`, reduced after every
//! multiplication (the curve25519-donna-c64 representation). The Montgomery
//! ladder follows RFC 7748 §5 with a constant-time conditional swap.

/// X25519 key/point length.
pub const X25519_LEN: usize = 32;

const MASK51: u64 = (1 << 51) - 1;

type Fe = [u64; 5];

const ZERO: Fe = [0, 0, 0, 0, 0];
const ONE: Fe = [1, 0, 0, 0, 0];

fn fe_add(a: &Fe, b: &Fe) -> Fe {
    // Inputs are < 2^51; keep headroom for a few additions before reduction.
    let mut out = [0u64; 5];
    for i in 0..5 {
        out[i] = a[i] + b[i];
    }
    out
}

fn fe_sub(a: &Fe, b: &Fe) -> Fe {
    // a - b + 2p, limbwise (2p = [2*(2^51-19), 2*(2^51-1), ...]).
    let two_p = [
        0xF_FFFF_FFFF_FFDA,
        0xF_FFFF_FFFF_FFFE,
        0xF_FFFF_FFFF_FFFE,
        0xF_FFFF_FFFF_FFFE,
        0xF_FFFF_FFFF_FFFE,
    ];
    let mut out = [0u64; 5];
    for i in 0..5 {
        out[i] = a[i] + two_p[i] - b[i];
    }
    out
}

fn fe_reduce(mut h: Fe) -> Fe {
    // Carry chain; 2^255 ≡ 19 (mod p).
    let mut c;
    c = h[0] >> 51;
    h[0] &= MASK51;
    h[1] += c;
    c = h[1] >> 51;
    h[1] &= MASK51;
    h[2] += c;
    c = h[2] >> 51;
    h[2] &= MASK51;
    h[3] += c;
    c = h[3] >> 51;
    h[3] &= MASK51;
    h[4] += c;
    c = h[4] >> 51;
    h[4] &= MASK51;
    h[0] += c * 19;
    // One more pass for the carry out of h0.
    c = h[0] >> 51;
    h[0] &= MASK51;
    h[1] += c;
    h
}

fn fe_mul(a: &Fe, b: &Fe) -> Fe {
    let a = fe_reduce(*a);
    let b = fe_reduce(*b);
    let a0 = a[0] as u128;
    let a1 = a[1] as u128;
    let a2 = a[2] as u128;
    let a3 = a[3] as u128;
    let a4 = a[4] as u128;
    let b0 = b[0] as u128;
    let b1 = b[1] as u128;
    let b2 = b[2] as u128;
    let b3 = b[3] as u128;
    let b4 = b[4] as u128;

    let t0 = a0 * b0 + 19 * (a1 * b4 + a2 * b3 + a3 * b2 + a4 * b1);
    let t1 = a0 * b1 + a1 * b0 + 19 * (a2 * b4 + a3 * b3 + a4 * b2);
    let t2 = a0 * b2 + a1 * b1 + a2 * b0 + 19 * (a3 * b4 + a4 * b3);
    let t3 = a0 * b3 + a1 * b2 + a2 * b1 + a3 * b0 + 19 * (a4 * b4);
    let t4 = a0 * b4 + a1 * b3 + a2 * b2 + a3 * b1 + a4 * b0;

    // Split the 128-bit accumulators into 51-bit limbs.
    let mut h = [0u64; 5];
    let mut carry: u128;
    carry = t0 >> 51;
    h[0] = (t0 & MASK51 as u128) as u64;
    let t1 = t1 + carry;
    carry = t1 >> 51;
    h[1] = (t1 & MASK51 as u128) as u64;
    let t2 = t2 + carry;
    carry = t2 >> 51;
    h[2] = (t2 & MASK51 as u128) as u64;
    let t3 = t3 + carry;
    carry = t3 >> 51;
    h[3] = (t3 & MASK51 as u128) as u64;
    let t4 = t4 + carry;
    let top = (t4 >> 51) as u64;
    h[4] = (t4 & MASK51 as u128) as u64;
    h[0] += top * 19;
    fe_reduce(h)
}

fn fe_sq(a: &Fe) -> Fe {
    fe_mul(a, a)
}

fn fe_mul_small(a: &Fe, small: u64) -> Fe {
    let mut h = [0u128; 5];
    for i in 0..5 {
        h[i] = a[i] as u128 * small as u128;
    }
    // Normalize.
    let mut c = 0u128;
    let mut out = [0u64; 5];
    for i in 0..5 {
        let v = h[i] + c;
        out[i] = (v & MASK51 as u128) as u64;
        c = v >> 51;
    }
    out[0] += (c as u64) * 19;
    fe_reduce(out)
}

fn fe_invert(z: &Fe) -> Fe {
    // z^(p-2) = z^(2^255-21) by square-and-multiply over the fixed exponent.
    let exponent: [u8; 32] = {
        let mut e = [0xffu8; 32];
        e[0] = 0x7f;
        e[31] = 0xeb;
        e
    };
    let mut result = ONE;
    for byte in exponent.iter() {
        for bit in (0..8).rev() {
            result = fe_sq(&result);
            if (byte >> bit) & 1 == 1 {
                result = fe_mul(&result, z);
            }
        }
    }
    result
}

fn fe_from_bytes(bytes: &[u8; 32]) -> Fe {
    let load = |off: usize| -> u64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&bytes[off..off + 8]);
        u64::from_le_bytes(buf)
    };
    [
        load(0) & MASK51,
        (load(6) >> 3) & MASK51,
        (load(12) >> 6) & MASK51,
        (load(19) >> 1) & MASK51,
        (load(24) >> 12) & MASK51,
    ]
}

fn fe_to_bytes(fe: &Fe) -> [u8; 32] {
    let mut h = fe_reduce(*fe);
    // Final subtraction of p if needed (constant time enough for our use).
    let mut q = (h[4] + 19) >> 51;
    q = (h[0] + q) >> 51;
    q = (h[1] + q) >> 51;
    q = (h[2] + q) >> 51;
    q = (h[3] + q) >> 51;
    let q = (h[4] + q) >> 51;
    h[0] += 19 * q;
    let mut c = h[0] >> 51;
    h[0] &= MASK51;
    h[1] += c;
    c = h[1] >> 51;
    h[1] &= MASK51;
    h[2] += c;
    c = h[2] >> 51;
    h[2] &= MASK51;
    h[3] += c;
    c = h[3] >> 51;
    h[3] &= MASK51;
    h[4] += c;
    h[4] &= MASK51;

    let mut out = [0u8; 32];
    let words = [
        h[0] | (h[1] << 51),
        (h[1] >> 13) | (h[2] << 38),
        (h[2] >> 26) | (h[3] << 25),
        (h[3] >> 39) | (h[4] << 12),
    ];
    for i in 0..4 {
        out[i * 8..i * 8 + 8].copy_from_slice(&words[i].to_le_bytes());
    }
    out
}

fn cswap(swap: u64, a: &mut Fe, b: &mut Fe) {
    let mask = 0u64.wrapping_sub(swap);
    for i in 0..5 {
        let t = mask & (a[i] ^ b[i]);
        a[i] ^= t;
        b[i] ^= t;
    }
}

/// X25519 scalar multiplication (`scalar` may be any 32 bytes; it is clamped
/// per RFC 7748).
pub fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; X25519_LEN] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;

    let x1 = fe_from_bytes(point);
    let mut x2 = ONE;
    let mut z2 = ZERO;
    let mut x3 = x1;
    let mut z3 = ONE;
    let mut swap: u64 = 0;

    for t in (0..255).rev() {
        let k_t = ((k[t / 8] >> (t % 8)) & 1) as u64;
        swap ^= k_t;
        cswap(swap, &mut x2, &mut x3);
        cswap(swap, &mut z2, &mut z3);
        swap = k_t;

        let a = fe_add(&x2, &z2);
        let aa = fe_sq(&a);
        let b = fe_sub(&x2, &z2);
        let bb = fe_sq(&b);
        let e = fe_sub(&aa, &bb);
        let c = fe_add(&x3, &z3);
        let d = fe_sub(&x3, &z3);
        let da = fe_mul(&d, &a);
        let cb = fe_mul(&c, &b);
        x3 = fe_sq(&fe_add(&da, &cb));
        z3 = fe_mul(&x1, &fe_sq(&fe_sub(&da, &cb)));
        x2 = fe_mul(&aa, &bb);
        z2 = fe_mul(&e, &fe_add(&aa, &fe_mul_small(&e, 121_665)));
    }
    cswap(swap, &mut x2, &mut x3);
    cswap(swap, &mut z2, &mut z3);

    let result = fe_mul(&x2, &fe_invert(&z2));
    fe_to_bytes(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex32(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        let b = s.as_bytes();
        for i in 0..32 {
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

    #[test]
    fn x25519_rfc7748_vectors() {
        // RFC 7748 §5.2
        let a = unhex32("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let b = unhex32("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
        let base = unhex32("0900000000000000000000000000000000000000000000000000000000000000");

        let a_pub = x25519(&a, &base);
        let b_pub = x25519(&b, &base);
        assert_eq!(
            hex(&a_pub),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
        assert_eq!(
            hex(&b_pub),
            "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
        );
        let shared_a = x25519(&a, &b_pub);
        let shared_b = x25519(&b, &a_pub);
        assert_eq!(hex(&shared_a), "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
        assert_eq!(shared_a, shared_b);
    }

    #[test]
    fn x25519_iterated_vector() {
        // RFC 7748 §5.2, 1 iteration of the 1000-iteration test.
        let k = unhex32("0900000000000000000000000000000000000000000000000000000000000000");
        let u = unhex32("0900000000000000000000000000000000000000000000000000000000000000");
        let out = x25519(&k, &u);
        assert_eq!(
            hex(&out),
            "422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079"
        );
    }
}

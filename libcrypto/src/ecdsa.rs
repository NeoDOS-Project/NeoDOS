//! ECDSA P-256 (prime256v1) signature verification with SHA-256.
//!
//! Verification only, as a TLS client needs. The curve arithmetic is Jacobian
//! coordinates over a Montgomery field (`crate::rsa::{Big, Mont}`), with the
//! scalar operations done modulo the group order `n`.

use crate::rsa::{Big, Mont};
use crate::sha256::Sha256;

/// ECDSA verification failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcdsaError {
    /// The public key is not a valid P-256 point.
    BadPublicKey,
    /// `r` or `s` is zero or not reduced modulo the group order.
    BadSignature,
    /// The signature does not match.
    VerificationFailed,
    /// The point at infinity appeared during verification.
    InvalidPoint,
}

const P_HEX: &str = "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff";
const N_HEX: &str = "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551";
const GX_HEX: &str = "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296";
const GY_HEX: &str = "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
const B_HEX: &str = "5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b";

struct Curve {
    p: Mont,
    n: Mont,
    p_big: Big,
    n_big: Big,
    n_minus_2: Big,
    b: Big, // Montgomery form
    g: Point,
}

#[derive(Clone)]
struct Point {
    x: Big,
    y: Big,
    z: Big, // Montgomery form; z == 0 means infinity
}

impl Curve {
    fn new() -> Self {
        let p_big = Big::from_hex(P_HEX);
        let n_big = Big::from_hex(N_HEX);
        let p = Mont::new(&p_big);
        let n = Mont::new(&n_big);
        let one = Big::from_u32(1);
        let b = p.to_mont(&Big::from_hex(B_HEX));
        let g = Point {
            x: p.to_mont(&Big::from_hex(GX_HEX)),
            y: p.to_mont(&Big::from_hex(GY_HEX)),
            z: p.to_mont(&one),
        };
        Curve {
            p,
            n,
            n_minus_2: n_big.sub(&Big::from_u32(2)),
            p_big,
            n_big,
            b,
            g,
        }
    }

    fn fadd(&self, a: &Big, b: &Big) -> Big {
        let s = a.add(b);
        if s.cmp(&self.p_big) != core::cmp::Ordering::Less {
            s.sub(&self.p_big)
        } else {
            s
        }
    }

    fn fsub(&self, a: &Big, b: &Big) -> Big {
        if a.cmp(b) != core::cmp::Ordering::Less {
            a.sub(b)
        } else {
            a.add(&self.p_big).sub(b)
        }
    }

    fn fdouble(&self, a: &Big) -> Big {
        self.fadd(a, a)
    }

    fn fmul(&self, a: &Big, b: &Big) -> Big {
        self.p.mont_mul(a, b)
    }

    fn fsqr(&self, a: &Big) -> Big {
        self.p.mont_mul(a, a)
    }

    fn fsmall(&self, a: &Big, k: u32) -> Big {
        let mut out = a.clone();
        for _ in 1..k {
            out = self.fadd(&out, a);
        }
        out
    }

    fn is_inf(&self, p: &Point) -> bool {
        p.z.is_zero()
    }

    fn dbl(&self, p: &Point) -> Point {
        if self.is_inf(p) || p.y.is_zero() {
            return Point {
                x: Big::from_u32(1),
                y: Big::from_u32(1),
                z: Big::from_u32(0),
            };
        }
        // dbl-2001-b (a = -3)
        let delta = self.fsqr(&p.z);
        let gamma = self.fsqr(&p.y);
        let beta = self.fmul(&p.x, &gamma);
        let alpha = self.fsmall(&self.fmul(&self.fsub(&p.x, &delta), &self.fadd(&p.x, &delta)), 3);
        let x3 = self.fsub(&self.fsqr(&alpha), &self.fsmall(&beta, 8));
        let z3 = self.fsub(
            &self.fsub(&self.fsqr(&self.fadd(&p.y, &p.z)), &gamma),
            &delta,
        );
        let y3 = self.fsub(
            &self.fmul(&alpha, &self.fsub(&self.fsmall(&beta, 4), &x3)),
            &self.fsmall(&self.fsqr(&gamma), 8),
        );
        Point { x: x3, y: y3, z: z3 }
    }

    fn add(&self, p: &Point, q: &Point) -> Point {
        if self.is_inf(p) {
            return q.clone();
        }
        if self.is_inf(q) {
            return p.clone();
        }
        let z1z1 = self.fsqr(&p.z);
        let z2z2 = self.fsqr(&q.z);
        let u1 = self.fmul(&p.x, &z2z2);
        let u2 = self.fmul(&q.x, &z1z1);
        let s1 = self.fmul(&p.y, &self.fmul(&q.z, &z2z2));
        let s2 = self.fmul(&q.y, &self.fmul(&p.z, &z1z1));
        if u1.cmp(&u2) == core::cmp::Ordering::Equal {
            if s1.cmp(&s2) == core::cmp::Ordering::Equal {
                return self.dbl(p);
            }
            return Point {
                x: Big::from_u32(1),
                y: Big::from_u32(1),
                z: Big::from_u32(0),
            };
        }
        let h = self.fsub(&u2, &u1);
        let i = self.fsqr(&self.fdouble(&h));
        let j = self.fmul(&h, &i);
        let r = self.fdouble(&self.fsub(&s2, &s1));
        let v = self.fmul(&u1, &i);
        let x3 = self.fsub(&self.fsub(&self.fsqr(&r), &j), &self.fdouble(&v));
        let y3 = self.fsub(
            &self.fmul(&r, &self.fsub(&v, &x3)),
            &self.fdouble(&self.fmul(&s1, &j)),
        );
        let z3 = self.fmul(
            &self.fsub(&self.fsub(&self.fsqr(&self.fadd(&p.z, &q.z)), &z1z1), &z2z2),
            &h,
        );
        Point { x: x3, y: y3, z: z3 }
    }

    fn mul(&self, p: &Point, k: &Big) -> Point {
        let mut result = Point {
            x: Big::from_u32(1),
            y: Big::from_u32(1),
            z: Big::from_u32(0),
        };
        for i in (0..k.bits()).rev() {
            result = self.dbl(&result);
            if k.bit(i) {
                result = self.add(&result, p);
            }
        }
        result
    }

    fn on_curve(&self, x: &Big, y: &Big) -> bool {
        // y^2 == x^3 - 3x + b in Montgomery form.
        let xm = self.p.to_mont(x);
        let ym = self.p.to_mont(y);
        let lhs = self.fsqr(&ym);
        let x3 = self.fmul(&self.fsqr(&xm), &xm);
        let three_x = self.fsmall(&xm, 3);
        let rhs = self.fadd(&self.fsub(&x3, &three_x), &self.b);
        lhs.cmp(&rhs) == core::cmp::Ordering::Equal
    }
}

fn is_zero_or_ge(a: &Big, n: &Big) -> bool {
    a.is_zero() || a.cmp(n) != core::cmp::Ordering::Less
}

/// Verify an ECDSA P-256 / SHA-256 signature.
///
/// `qx`/`qy` are the affine public-key coordinates (big-endian, up to 32
/// bytes). `r`/`s` are the signature scalars (big-endian).
pub fn verify_p256_sha256(
    qx: &[u8],
    qy: &[u8],
    message: &[u8],
    r: &[u8],
    s: &[u8],
) -> Result<(), EcdsaError> {
    let c = Curve::new();
    let x = Big::from_be(qx);
    let y = Big::from_be(qy);
    if x.is_zero() || y.is_zero() || x.cmp(&c.p_big) != core::cmp::Ordering::Less
        || y.cmp(&c.p_big) != core::cmp::Ordering::Less
        || !c.on_curve(&x, &y)
    {
        return Err(EcdsaError::BadPublicKey);
    }
    let q = Point {
        x: c.p.to_mont(&x),
        y: c.p.to_mont(&y),
        z: c.p.to_mont(&Big::from_u32(1)),
    };

    let r_big = Big::from_be(r);
    let s_big = Big::from_be(s);
    if is_zero_or_ge(&r_big, &c.n_big) || is_zero_or_ge(&s_big, &c.n_big) {
        return Err(EcdsaError::BadSignature);
    }

    // e = SHA-256(message) as an integer mod n (the digest is 256 bits).
    let digest = Big::from_be(&Sha256::digest(message));
    let e = if digest.cmp(&c.n_big) == core::cmp::Ordering::Less {
        digest
    } else {
        digest.sub(&c.n_big)
    };

    // w = s^-1 mod n; u1 = e*w; u2 = r*w.
    let s_m = c.n.to_mont(&s_big);
    let w_m = c.n.pow(&s_m, &c.n_minus_2);
    let u1 = c.n.from_mont(&c.n.mont_mul(&c.n.to_mont(&e), &w_m));
    let u2 = c.n.from_mont(&c.n.mont_mul(&c.n.to_mont(&r_big), &w_m));

    let point = c.add(&c.mul(&c.g, &u1), &c.mul(&q, &u2));
    if c.is_inf(&point) {
        return Err(EcdsaError::InvalidPoint);
    }
    // Affine X = Jacobian X / Z^2; `pow` keeps the result in Montgomery form.
    let z_inv = c.p.pow(&point.z, &c.p_big.sub(&Big::from_u32(2)));
    let z_inv2 = c.fsqr(&z_inv);
    let x_affine = c.p.from_mont(&c.fmul(&point.x, &z_inv2));
    let v = if x_affine.cmp(&c.n_big) == core::cmp::Ordering::Less {
        x_affine
    } else {
        x_affine.sub(&c.n_big)
    };
    if v.cmp(&r_big) == core::cmp::Ordering::Equal {
        Ok(())
    } else {
        Err(EcdsaError::VerificationFailed)
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

    // Generated with OpenSSL 4.0.3 (prime256v1, SHA-256).
    const QX: &str = "d90c95d7a1259395668b8ced8fcddab41f0650bbc8a57a4a97871fb3e267d51c";
    const QY: &str = "5ea3981cb52a8ccfa1af6f63165e473dd0004335a4c8171c6bbac8fe2bed0759";
    const MSG: &[u8] = b"NeoDOS TLS test vector";
    const R: &str = "dd4787a7ac1dbedc22580b77a513c1c4bc9be9b6918463b816998c9a4833185c";
    const S: &str = "15765f0e54d8d7d8dc464ae4a4e3a9d416d5b90eed5ed930d2f475bfdf031960";

    #[test]
    fn field_arithmetic_smoke() {
        let c = Curve::new();
        let two = c.p.to_mont(&Big::from_u32(2));
        let three = c.p.to_mont(&Big::from_u32(3));
        let six = c.fmul(&two, &three);
        assert_eq!(c.p.from_mont(&six), Big::from_u32(6));
        // pow with a small exponent: 3^5 = 243.
        let five = Big::from_u32(5);
        let pow = c.p.pow(&three, &five);
        assert_eq!(c.p.from_mont(&pow), Big::from_u32(243));
        // Fermat inverse on a small modulus: 3 * 3^(101-2) == 1 (mod 101).
        let m = Mont::new(&Big::from_u32(101));
        let t3 = m.to_mont(&Big::from_u32(3));
        let inv3 = m.pow(&t3, &Big::from_u32(99));
        assert_eq!(m.from_mont(&inv3), Big::from_u32(34));
        // pow with a multi-byte exponent on a small modulus: 3^256 mod 101.
        let p256_small = m.pow(&t3, &Big::from_u32(256));
        assert_eq!(m.from_mont(&p256_small), Big::from_u32(79));
        // Cross-checked with Python: a*b, a^256 and a^-1 (mod p).
        let a = Big::from_hex("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef");
        let b = Big::from_hex("fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321");
        let am = c.p.to_mont(&a);
        let bm = c.p.to_mont(&b);
        assert_eq!(
            am,
            Big::from_hex("8188888945c048cfb5147ae0338bf256a2e02467edcba9875d1fdb985d1fdb98")
        );
        assert_eq!(
            c.p.from_mont(&c.fmul(&am, &bm)),
            Big::from_hex("30f7a9d8a7a6051f213ee09c7b80ef9c6c5177f341f956733e54855f4154e4c4")
        );
        assert_eq!(
            c.fmul(&am, &am),
            Big::from_hex("27ea2fa9eadf4f19261cb91b7344935f6976198d14edc4faa3f848526e14b685")
        );
        assert_eq!(
            c.p.from_mont(&c.p.pow(&am, &Big::from_u32(256))),
            Big::from_hex("1685f13e5a2ed0c99a38ebf48b8138c2ae80570096ad1df9d9c32251389c4580")
        );
        assert_eq!(
            c.p.from_mont(&c.p.pow(&am, &c.p_big.sub(&Big::from_u32(2)))),
            Big::from_hex("8056a93bbc74c29585a874f6b858572477c5f7aa71a2d0b0416f09ed94008b74")
        );
        // -1 * -1 = 1, (-1)*2 = p-2.
        let minus1 = c.p_big.sub(&Big::from_u32(1));
        let m1 = c.p.to_mont(&minus1);
        let one = c.p.from_mont(&c.fmul(&m1, &m1));
        assert_eq!(one, Big::from_u32(1));
        let m2 = c.p.from_mont(&c.fmul(&m1, &two));
        assert_eq!(m2, c.p_big.sub(&Big::from_u32(2)));
        // G must satisfy the curve equation.
        let gx = c.p.from_mont(&c.g.x);
        let gy = c.p.from_mont(&c.g.y);
        assert!(c.on_curve(&gx, &gy), "G not on curve");
        let div = c.p.mont_mul(&six, &c.p.pow(&three, &c.p_big.sub(&Big::from_u32(2))));
        assert_eq!(c.p.from_mont(&div), Big::from_u32(2));
    }

    #[test]
    fn ecdsa_p256_sha256_verifies() {
        assert_eq!(
            verify_p256_sha256(&unhex(QX), &unhex(QY), MSG, &unhex(R), &unhex(S)),
            Ok(())
        );
        // Different message.
        assert_eq!(
            verify_p256_sha256(&unhex(QX), &unhex(QY), b"other", &unhex(R), &unhex(S)),
            Err(EcdsaError::VerificationFailed)
        );
        // Tampered r.
        let mut r = unhex(R);
        r[0] ^= 1;
        assert_eq!(
            verify_p256_sha256(&unhex(QX), &unhex(QY), MSG, &r, &unhex(S)),
            Err(EcdsaError::VerificationFailed)
        );
    }

    #[test]
    fn ecdsa_rejects_bad_inputs() {
        // Public key not on the curve.
        let mut y = unhex(QY);
        y[31] ^= 1;
        assert_eq!(
            verify_p256_sha256(&unhex(QX), &y, MSG, &unhex(R), &unhex(S)),
            Err(EcdsaError::BadPublicKey)
        );
        // Zero scalars.
        assert_eq!(
            verify_p256_sha256(&unhex(QX), &unhex(QY), MSG, &[0], &[0]),
            Err(EcdsaError::BadSignature)
        );
        // r >= n.
        let n = unhex(N_HEX);
        assert_eq!(
            verify_p256_sha256(&unhex(QX), &unhex(QY), MSG, &n, &unhex(S)),
            Err(EcdsaError::BadSignature)
        );
    }
}

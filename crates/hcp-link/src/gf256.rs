//! GF(2^8) arithmetic for Reed-Solomon erasure coding.
//!
//! Note the polynomial: RS conventionally uses 0x11d (x^8+x^4+x^3+x^2+1), the
//! CCSDS/storage field — *different* from AES's 0x11b. Kept separate on purpose;
//! this module is only for the link-layer erasure code, not the cipher.
//!
//! Log/exp tables make multiplication a couple of table lookups, which matters
//! when encoding kilobytes of a bitstream into frames.

/// Primitive polynomial for the RS field.
const POLY: u16 = 0x11d;
/// A generator of the multiplicative group.
const GENERATOR: u8 = 0x02;

/// Precomputed log/antilog tables for GF(256).
pub struct Gf256 {
    exp: [u8; 512],
    log: [u8; 256],
}

impl Gf256 {
    pub fn new() -> Self {
        let mut exp = [0u8; 512];
        let mut log = [0u8; 256];
        let mut x: u16 = 1;
        for i in 0..255 {
            exp[i] = x as u8;
            log[x as usize] = i as u8;
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= POLY;
            }
        }
        // duplicate for index wraparound so mul needs no modulo
        for i in 255..512 {
            exp[i] = exp[i - 255];
        }
        let _ = GENERATOR; // documents intent; table built from 0x02 implicitly
        Gf256 { exp, log }
    }

    #[inline]
    pub fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            0
        } else {
            self.exp[self.log[a as usize] as usize + self.log[b as usize] as usize]
        }
    }

    #[inline]
    pub fn div(&self, a: u8, b: u8) -> u8 {
        debug_assert!(b != 0, "division by zero in GF(256)");
        if a == 0 {
            0
        } else {
            // log[a] - log[b] mod 255, kept non-negative via +255
            self.exp[self.log[a as usize] as usize + 255 - self.log[b as usize] as usize]
        }
    }

    #[inline]
    pub fn inv(&self, a: u8) -> u8 {
        debug_assert!(a != 0, "inverse of zero in GF(256)");
        // a^-1 = exp[255 - log[a]]
        self.exp[255 - self.log[a as usize] as usize]
    }
}

impl Default for Gf256 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_identity_and_zero() {
        let gf = Gf256::new();
        for a in 0u16..256 {
            let a = a as u8;
            assert_eq!(gf.mul(a, 1), a);
            assert_eq!(gf.mul(a, 0), 0);
            assert_eq!(gf.mul(0, a), 0);
        }
    }

    #[test]
    fn every_nonzero_has_inverse() {
        let gf = Gf256::new();
        for a in 1u16..256 {
            let a = a as u8;
            assert_eq!(gf.mul(a, gf.inv(a)), 1, "inverse failed for {a:#04x}");
        }
    }

    #[test]
    fn div_is_inverse_of_mul() {
        let gf = Gf256::new();
        for a in 0u16..256 {
            for b in 1u16..256 {
                let (a, b) = (a as u8, b as u8);
                let p = gf.mul(a, b);
                assert_eq!(gf.div(p, b), a);
            }
        }
    }

    #[test]
    fn mul_is_commutative_and_associative() {
        let gf = Gf256::new();
        for &a in &[3u8, 7, 0x53, 0xff, 0x1d] {
            for &b in &[2u8, 9, 0xa1, 0x80] {
                assert_eq!(gf.mul(a, b), gf.mul(b, a));
                for &c in &[5u8, 0x40] {
                    assert_eq!(gf.mul(gf.mul(a, b), c), gf.mul(a, gf.mul(b, c)));
                }
            }
        }
    }
}

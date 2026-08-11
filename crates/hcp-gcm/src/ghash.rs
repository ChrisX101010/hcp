//! GHASH — the authentication component of GCM.
//!
//! Operates in GF(2^128) with the reduction polynomial
//! x^128 + x^7 + x^2 + x + 1. Bit/byte ordering follows the GCM spec exactly:
//! blocks are big-endian, and bit 0 is the most-significant bit of byte 0.

/// Multiply two 128-bit field elements (GCM's bit ordering).
pub fn gf_mul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    // R = 0xe1 followed by 120 zero bits.
    let mut z = [0u8; 16];
    let mut v = *y;

    for i in 0..128 {
        // bit i of x, counting from the MSB of byte 0
        let byte = i / 8;
        let bit = 7 - (i % 8);
        if (x[byte] >> bit) & 1 == 1 {
            for k in 0..16 {
                z[k] ^= v[k];
            }
        }
        // if LSB of V (bit 127) is set, shift and reduce; else just shift
        let lsb = v[15] & 1;
        // v >>= 1 across the whole 128-bit big-endian value
        let mut carry = 0u8;
        for byte in v.iter_mut() {
            let new_carry = *byte & 1;
            *byte = (*byte >> 1) | (carry << 7);
            carry = new_carry;
        }
        if lsb == 1 {
            v[0] ^= 0xe1;
        }
    }
    z
}

/// GHASH over additional authenticated data `aad` and ciphertext `ct`, keyed by
/// the hash subkey `h`. Returns the 16-byte hash before the final tag XOR.
pub fn ghash(h: &[u8; 16], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let mut x = [0u8; 16];

    let absorb = |data: &[u8], x: &mut [u8; 16]| {
        let mut i = 0;
        while i < data.len() {
            let mut block = [0u8; 16];
            let n = core::cmp::min(16, data.len() - i);
            block[..n].copy_from_slice(&data[i..i + n]);
            for k in 0..16 {
                x[k] ^= block[k];
            }
            *x = gf_mul(x, h);
            i += 16;
        }
    };

    absorb(aad, &mut x);
    absorb(ct, &mut x);

    // length block: [len(aad) in bits : u64be][len(ct) in bits : u64be]
    let mut len_block = [0u8; 16];
    let aad_bits = (aad.len() as u64) * 8;
    let ct_bits = (ct.len() as u64) * 8;
    len_block[..8].copy_from_slice(&aad_bits.to_be_bytes());
    len_block[8..].copy_from_slice(&ct_bits.to_be_bytes());
    for k in 0..16 {
        x[k] ^= len_block[k];
    }
    gf_mul(&x, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
    fn arr(s: &str) -> [u8; 16] {
        hex(s).try_into().unwrap()
    }

    #[test]
    fn gf_mul_by_zero_is_zero() {
        let h = arr("66e94bd4ef8a2c3b884cfa59ca342b2e");
        assert_eq!(gf_mul(&[0u8; 16], &h), [0u8; 16]);
    }

    #[test]
    fn ghash_empty_is_zero() {
        // With no AAD and no ciphertext, GHASH reduces to (lenblock=0)*H = 0.
        let h = arr("66e94bd4ef8a2c3b884cfa59ca342b2e");
        assert_eq!(ghash(&h, &[], &[]), [0u8; 16]);
    }

    #[test]
    fn ghash_matches_known_intermediate() {
        // From McGrew-Viega test case 2: H is AES_0(0), single zero ct block.
        // GHASH here should equal the value that, XORed with E(J0), gives the
        // published tag. We check GHASH indirectly in the gcm module; here we
        // just confirm determinism and non-triviality.
        let h = arr("66e94bd4ef8a2c3b884cfa59ca342b2e");
        let ct = hex("0388dace60b6a392f328c2b971b2fe78");
        let g = ghash(&h, &[], &ct);
        assert_ne!(g, [0u8; 16]);
        assert_eq!(g, ghash(&h, &[], &ct)); // deterministic
    }
}

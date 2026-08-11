//! Software AES-128 block encryption, used by GCM to encrypt counter blocks.
//!
//! This is the software companion to the hardware core in `hcp-crypto`: same
//! algorithm (FIPS-197), same S-box derivation from GF(2^8). GCM needs to
//! encrypt counter blocks on whatever CPU is running the protocol, so a compact
//! software path lives here. The hardware core handles bulk data on an FPGA;
//! this handles the control-plane crypto on the host. Both agree bit-for-bit.

/// Multiply in GF(2^8) with the AES polynomial 0x11B.
fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0u8;
    for _ in 0..8 {
        if b & 1 != 0 {
            p ^= a;
        }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    p
}

fn gf_inv(a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    let mut r = 1u8;
    let mut base = a;
    let mut e = 254u32;
    while e > 0 {
        if e & 1 == 1 {
            r = gf_mul(r, base);
        }
        base = gf_mul(base, base);
        e >>= 1;
    }
    r
}

fn sbox(a: u8) -> u8 {
    let inv = gf_inv(a);
    inv ^ inv.rotate_left(1) ^ inv.rotate_left(2) ^ inv.rotate_left(3) ^ inv.rotate_left(4) ^ 0x63
}

/// Expanded AES-128 key: 11 round keys of 16 bytes each.
pub struct Aes128 {
    round_keys: [[u8; 16]; 11],
}

impl Aes128 {
    pub fn new(key: &[u8; 16]) -> Self {
        let rcon = [0x01u8, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];
        let mut w = [[0u8; 4]; 44];
        for i in 0..4 {
            w[i] = [key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]];
        }
        for i in 4..44 {
            let mut t = w[i - 1];
            if i % 4 == 0 {
                // RotWord
                t = [t[1], t[2], t[3], t[0]];
                // SubWord
                for b in t.iter_mut() {
                    *b = sbox(*b);
                }
                t[0] ^= rcon[i / 4 - 1];
            }
            for j in 0..4 {
                w[i][j] = w[i - 4][j] ^ t[j];
            }
        }
        let mut round_keys = [[0u8; 16]; 11];
        for r in 0..11 {
            for c in 0..4 {
                for b in 0..4 {
                    round_keys[r][4 * c + b] = w[4 * r + c][b];
                }
            }
        }
        Aes128 { round_keys }
    }

    fn add_round_key(state: &mut [u8; 16], rk: &[u8; 16]) {
        for i in 0..16 {
            state[i] ^= rk[i];
        }
    }

    fn sub_bytes(state: &mut [u8; 16]) {
        for b in state.iter_mut() {
            *b = sbox(*b);
        }
    }

    fn shift_rows(state: &mut [u8; 16]) {
        // state is column-major: index = col*4 + row
        let s = *state;
        // row r shifted left by r
        for r in 1..4 {
            for c in 0..4 {
                state[c * 4 + r] = s[((c + r) % 4) * 4 + r];
            }
        }
    }

    fn mix_columns(state: &mut [u8; 16]) {
        for c in 0..4 {
            let i = c * 4;
            let a0 = state[i];
            let a1 = state[i + 1];
            let a2 = state[i + 2];
            let a3 = state[i + 3];
            state[i] = gf_mul(a0, 2) ^ gf_mul(a1, 3) ^ a2 ^ a3;
            state[i + 1] = a0 ^ gf_mul(a1, 2) ^ gf_mul(a2, 3) ^ a3;
            state[i + 2] = a0 ^ a1 ^ gf_mul(a2, 2) ^ gf_mul(a3, 3);
            state[i + 3] = gf_mul(a0, 3) ^ a1 ^ a2 ^ gf_mul(a3, 2);
        }
    }

    /// Encrypt a single 16-byte block in place-free form.
    pub fn encrypt_block(&self, input: &[u8; 16]) -> [u8; 16] {
        let mut state = *input;
        Self::add_round_key(&mut state, &self.round_keys[0]);
        for r in 1..10 {
            Self::sub_bytes(&mut state);
            Self::shift_rows(&mut state);
            Self::mix_columns(&mut state);
            Self::add_round_key(&mut state, &self.round_keys[r]);
        }
        Self::sub_bytes(&mut state);
        Self::shift_rows(&mut state);
        Self::add_round_key(&mut state, &self.round_keys[10]);
        state
    }
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

    #[test]
    fn fips197_c1_block() {
        let key: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let pt: [u8; 16] = hex("00112233445566778899aabbccddeeff").try_into().unwrap();
        let ct = Aes128::new(&key).encrypt_block(&pt);
        assert_eq!(
            ct.to_vec(),
            hex("69c4e0d86a7b0430d8cdb78070b4c55a"),
            "software AES must match the hardware core and FIPS-197"
        );
    }

    #[test]
    fn all_zero_block() {
        let ct = Aes128::new(&[0u8; 16]).encrypt_block(&[0u8; 16]);
        assert_eq!(ct.to_vec(), hex("66e94bd4ef8a2c3b884cfa59ca342b2e"));
    }
}

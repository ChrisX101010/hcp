//! AES-128-GCM authenticated encryption (AEAD).
//!
//! GCM gives you two things at once over the raw AES block cipher:
//!   - **confidentiality** via counter (CTR) mode, so identical plaintext
//!     blocks don't produce identical ciphertext (the fatal flaw of ECB), and
//!   - **integrity/authenticity** via a 16-byte tag, so any tampering — with
//!     the ciphertext *or* the associated data — is detected on decrypt.
//!
//! This is what you actually run over the transport to protect an image or a
//! message. The raw core in `hcp-crypto` is the primitive; this is the usable
//! construction built on it.
//!
//! ## Critical usage rule
//!
//! **Never reuse a (key, nonce) pair.** GCM's security collapses catastrophically
//! if you do — an attacker can forge tags. Use a fresh random 96-bit nonce per
//! message, or a counter that never repeats for a given key. [`SequenceNonce`]
//! helps enforce the counter discipline.

use crate::aes::Aes128;
use crate::ghash::ghash;

/// Errors from GCM operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GcmError {
    /// The authentication tag did not match — the message was tampered with,
    /// corrupted, or encrypted under a different key/nonce. The plaintext is
    /// NOT returned, by design.
    AuthenticationFailed,
}

impl core::fmt::Display for GcmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            GcmError::AuthenticationFailed => write!(f, "GCM authentication failed"),
        }
    }
}

impl std::error::Error for GcmError {}

/// The 16-byte authentication tag GCM appends to a message.
pub const TAG_LEN: usize = 16;
/// Standard GCM nonce length in bytes (96 bits) — the recommended size.
pub const NONCE_LEN: usize = 12;

fn inc32(block: &mut [u8; 16]) {
    // increment the rightmost 32 bits, modulo 2^32
    let mut c = u32::from_be_bytes([block[12], block[13], block[14], block[15]]);
    c = c.wrapping_add(1);
    block[12..].copy_from_slice(&c.to_be_bytes());
}

/// Compute J0 (the pre-counter block) from the nonce.
fn compute_j0(aes: &Aes128, h: &[u8; 16], nonce: &[u8]) -> [u8; 16] {
    if nonce.len() == 12 {
        // Fast path for the standard 96-bit nonce: J0 = IV || 0^31 || 1
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(nonce);
        j0[15] = 1;
        j0
    } else {
        // General case: J0 = GHASH(H, {}, IV padded with its length)
        let _ = aes; // aes not needed here, kept for signature symmetry
        ghash(h, &[], nonce_with_len(nonce).as_slice())
    }
}

fn nonce_with_len(nonce: &[u8]) -> Vec<u8> {
    // GHASH input for a non-96-bit IV: IV || 0^s || 0^64 || len(IV)_64
    let mut v = nonce.to_vec();
    while v.len() % 16 != 0 {
        v.push(0);
    }
    v.extend_from_slice(&[0u8; 8]);
    v.extend_from_slice(&((nonce.len() as u64) * 8).to_be_bytes());
    v
}

/// The AES-128-GCM cipher, built from an expanded key and its hash subkey.
pub struct Aes128Gcm {
    aes: Aes128,
    h: [u8; 16],
}

impl Aes128Gcm {
    /// Create a cipher from a 16-byte key.
    pub fn new(key: &[u8; 16]) -> Self {
        let aes = Aes128::new(key);
        let h = aes.encrypt_block(&[0u8; 16]); // hash subkey H = E_K(0)
        Aes128Gcm { aes, h }
    }

    fn gctr(&self, icb: &[u8; 16], input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        let mut counter = *icb;
        let mut i = 0;
        while i < input.len() {
            let ks = self.aes.encrypt_block(&counter);
            let n = core::cmp::min(16, input.len() - i);
            for j in 0..n {
                out.push(input[i + j] ^ ks[j]);
            }
            inc32(&mut counter);
            i += 16;
        }
        out
    }

    /// Encrypt `plaintext` with `nonce`, authenticating `aad` (which is sent in
    /// the clear but tamper-protected). Returns `(ciphertext, tag)`.
    pub fn encrypt(&self, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; TAG_LEN]) {
        let j0 = compute_j0(&self.aes, &self.h, nonce);
        let mut icb = j0;
        inc32(&mut icb);
        let ct = self.gctr(&icb, plaintext);

        let s = ghash(&self.h, aad, &ct);
        let ej0 = self.aes.encrypt_block(&j0);
        let mut tag = [0u8; TAG_LEN];
        for i in 0..TAG_LEN {
            tag[i] = s[i] ^ ej0[i];
        }
        (ct, tag)
    }

    /// Decrypt and verify. Returns the plaintext only if the tag is valid;
    /// otherwise `AuthenticationFailed` and no plaintext (never release
    /// unverified plaintext).
    pub fn decrypt(
        &self,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
        tag: &[u8; TAG_LEN],
    ) -> Result<Vec<u8>, GcmError> {
        let j0 = compute_j0(&self.aes, &self.h, nonce);

        // Recompute the expected tag over the received ciphertext.
        let s = ghash(&self.h, aad, ciphertext);
        let ej0 = self.aes.encrypt_block(&j0);
        let mut expected = [0u8; TAG_LEN];
        for i in 0..TAG_LEN {
            expected[i] = s[i] ^ ej0[i];
        }

        // Constant-time comparison to avoid a timing oracle on the tag.
        if !ct_eq(&expected, tag) {
            return Err(GcmError::AuthenticationFailed);
        }

        let mut icb = j0;
        inc32(&mut icb);
        Ok(self.gctr(&icb, ciphertext))
    }
}

/// Constant-time 16-byte equality.
fn ct_eq(a: &[u8; TAG_LEN], b: &[u8; TAG_LEN]) -> bool {
    let mut diff = 0u8;
    for i in 0..TAG_LEN {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// A monotonic 96-bit nonce generator that helps enforce "never reuse a nonce
/// under one key." Combine a per-node random 32-bit prefix with a 64-bit
/// counter; the counter is checked for exhaustion. This is not the only valid
/// strategy (random 96-bit nonces are fine below ~2^32 messages), but it makes
/// the counter approach hard to misuse.
#[derive(Debug, Clone)]
pub struct SequenceNonce {
    prefix: [u8; 4],
    counter: u64,
}

impl SequenceNonce {
    pub fn new(prefix: [u8; 4]) -> Self {
        Self { prefix, counter: 0 }
    }

    /// Next nonce, or `None` if the 64-bit counter has been exhausted (which in
    /// practice never happens, but we refuse to wrap rather than reuse).
    pub fn next(&mut self) -> Option<[u8; NONCE_LEN]> {
        let c = self.counter;
        self.counter = self.counter.checked_add(1)?;
        let mut n = [0u8; NONCE_LEN];
        n[..4].copy_from_slice(&self.prefix);
        n[4..].copy_from_slice(&c.to_be_bytes());
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        if s.is_empty() {
            return Vec::new();
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
    fn key16(s: &str) -> [u8; 16] {
        hex(s).try_into().unwrap()
    }

    // McGrew & Viega, "The Galois/Counter Mode of Operation (GCM)", Appendix B.

    #[test]
    fn test_case_1_empty() {
        let gcm = Aes128Gcm::new(&key16("00000000000000000000000000000000"));
        let (ct, tag) = gcm.encrypt(&hex("000000000000000000000000"), &[], &[]);
        assert!(ct.is_empty());
        assert_eq!(tag.to_vec(), hex("58e2fccefa7e3061367f1d57a4e7455a"));
    }

    #[test]
    fn test_case_2_one_block() {
        let gcm = Aes128Gcm::new(&key16("00000000000000000000000000000000"));
        let pt = hex("00000000000000000000000000000000");
        let (ct, tag) = gcm.encrypt(&hex("000000000000000000000000"), &[], &pt);
        assert_eq!(ct, hex("0388dace60b6a392f328c2b971b2fe78"));
        assert_eq!(tag.to_vec(), hex("ab6e47d42cec13bdf53a67b21257bddf"));
    }

    #[test]
    fn test_case_3_full_message() {
        let gcm = Aes128Gcm::new(&key16("feffe9928665731c6d6a8f9467308308"));
        let pt = hex("d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255");
        let iv = hex("cafebabefacedbaddecaf888");
        let (ct, tag) = gcm.encrypt(&iv, &[], &pt);
        assert_eq!(ct, hex("42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091473f5985"));
        assert_eq!(tag.to_vec(), hex("4d5c2af327cd64a62cf35abd2ba6fab4"));
    }

    #[test]
    fn test_case_4_with_aad() {
        let gcm = Aes128Gcm::new(&key16("feffe9928665731c6d6a8f9467308308"));
        let pt = hex("d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39");
        let aad = hex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let iv = hex("cafebabefacedbaddecaf888");
        let (ct, tag) = gcm.encrypt(&iv, &aad, &pt);
        assert_eq!(ct, hex("42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091"));
        assert_eq!(tag.to_vec(), hex("5bc94fbc3221a5db94fae95ae7121a47"));
    }

    #[test]
    fn decrypt_roundtrip_recovers_plaintext() {
        let gcm = Aes128Gcm::new(&key16("feffe9928665731c6d6a8f9467308308"));
        let pt = b"ship this hardware image over LoRa, please";
        let iv = hex("cafebabefacedbaddecaf888");
        let aad = b"image-digest:sha256:1122...";
        let (ct, tag) = gcm.encrypt(&iv, aad, pt);
        let out = gcm.decrypt(&iv, aad, &ct, &tag).unwrap();
        assert_eq!(out, pt);
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let gcm = Aes128Gcm::new(&key16("feffe9928665731c6d6a8f9467308308"));
        let iv = hex("cafebabefacedbaddecaf888");
        let (mut ct, tag) = gcm.encrypt(&iv, &[], b"secret payload");
        ct[0] ^= 0x01; // flip one bit
        assert_eq!(
            gcm.decrypt(&iv, &[], &ct, &tag),
            Err(GcmError::AuthenticationFailed)
        );
    }

    #[test]
    fn tampered_aad_is_rejected() {
        let gcm = Aes128Gcm::new(&key16("feffe9928665731c6d6a8f9467308308"));
        let iv = hex("cafebabefacedbaddecaf888");
        let (ct, tag) = gcm.encrypt(&iv, b"digest:AAAA", b"payload");
        // Attacker swaps the associated data.
        assert_eq!(
            gcm.decrypt(&iv, b"digest:BBBB", &ct, &tag),
            Err(GcmError::AuthenticationFailed)
        );
    }

    #[test]
    fn wrong_nonce_is_rejected() {
        let gcm = Aes128Gcm::new(&key16("feffe9928665731c6d6a8f9467308308"));
        let (ct, tag) = gcm.encrypt(&hex("cafebabefacedbaddecaf888"), &[], b"payload");
        assert_eq!(
            gcm.decrypt(&hex("000000000000000000000000"), &[], &ct, &tag),
            Err(GcmError::AuthenticationFailed)
        );
    }

    #[test]
    fn sequence_nonce_never_repeats() {
        let mut seq = SequenceNonce::new([0xDE, 0xAD, 0xBE, 0xEF]);
        let a = seq.next().unwrap();
        let b = seq.next().unwrap();
        assert_ne!(a, b);
        assert_eq!(&a[..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }
}

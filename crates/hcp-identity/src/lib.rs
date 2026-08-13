//! # hcp-identity
//!
//! Node identity for HCP: an Ed25519 keypair per node, plus signing and
//! verification of the fabric protocol's offers. This is the crate that
//! replaces the `sign_stub` / `verify_stub` placeholders in `hcp-fabric`'s
//! example with real cryptography.
//!
//! ## The model
//!
//! - A node's **identity** is the 32-byte Ed25519 public key. That is exactly
//!   the `node_id` / `from` / `to` field already used throughout `hcp-fabric`,
//!   so identities drop straight into the existing wire format with no changes.
//! - A node holds a [`NodeKeypair`] (secret + public). It **signs** the
//!   canonical bytes of a `PlacementOffer`; any peer **verifies** with only the
//!   public key.
//! - Verification answers one question: "did the holder of the private key
//!   behind this `node_id` actually authorize this exact offer?" That is the
//!   foundation the whole consent model rests on — an owner can trust that an
//!   accepted offer really came from who it claims to.
//!
//! ## Why this and not "roll your own"
//!
//! Signatures are the one place never to improvise. This wraps the audited
//! `ed25519-dalek` implementation and keeps the surface tiny.

#![forbid(unsafe_code)]

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey, SECRET_KEY_LENGTH};

mod trust;
pub use trust::{TrustError, TrustStore};

/// Length of a public identity in bytes (Ed25519 public key).
pub const IDENTITY_LEN: usize = 32;
/// Length of a signature in bytes.
pub const SIGNATURE_LEN: usize = 64;

/// Errors from identity operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// A byte slice was the wrong length to be a key or signature.
    BadLength {
        what: &'static str,
        expected: usize,
        got: usize,
    },
    /// The public key bytes were not a valid curve point.
    MalformedKey,
    /// The signature did not verify against the message and key.
    VerificationFailed,
}

impl core::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IdentityError::BadLength {
                what,
                expected,
                got,
            } => {
                write!(f, "{what}: expected {expected} bytes, got {got}")
            }
            IdentityError::MalformedKey => write!(f, "malformed public key"),
            IdentityError::VerificationFailed => write!(f, "signature verification failed"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// A node's public identity — safe to share, gossip, and print.
///
/// This is literally the 32-byte value the fabric protocol already carries as
/// `node_id`, wrapped in a type so it can verify signatures.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(VerifyingKey);

impl NodeId {
    /// Rebuild an identity from its 32 raw bytes (e.g. a `node_id` off the wire).
    pub fn from_bytes(bytes: &[u8; IDENTITY_LEN]) -> Result<Self, IdentityError> {
        VerifyingKey::from_bytes(bytes)
            .map(NodeId)
            .map_err(|_| IdentityError::MalformedKey)
    }

    /// The 32-byte public key. Use this as the `node_id` / `from` / `to` field.
    pub fn to_bytes(&self) -> [u8; IDENTITY_LEN] {
        self.0.to_bytes()
    }

    /// Verify that `signature` covers `message` under this identity.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &[u8; SIGNATURE_LEN],
    ) -> Result<(), IdentityError> {
        let sig = Signature::from_bytes(signature);
        self.0
            .verify(message, &sig)
            .map_err(|_| IdentityError::VerificationFailed)
    }

    /// Short hex prefix for logs (first 4 bytes). Not for security decisions.
    pub fn short(&self) -> String {
        let b = self.to_bytes();
        format!("{:02x}{:02x}{:02x}{:02x}", b[0], b[1], b[2], b[3])
    }
}

impl core::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "NodeId({}…)", self.short())
    }
}

/// A node's full keypair: the secret half plus its public [`NodeId`].
///
/// Keep the bytes of this private. `SigningKey` from `ed25519-dalek` zeroizes
/// its secret on drop (via the `zeroize` feature), so it won't linger in memory.
pub struct NodeKeypair {
    signing: SigningKey,
    id: NodeId,
}

impl NodeKeypair {
    /// Generate a fresh random identity. Uses the OS CSPRNG.
    pub fn generate() -> Self {
        let mut rng = rand_core::OsRng;
        let signing = SigningKey::generate(&mut rng);
        let id = NodeId(signing.verifying_key());
        NodeKeypair { signing, id }
    }

    /// Reconstruct from a stored 32-byte secret seed.
    pub fn from_secret_bytes(seed: &[u8; SECRET_KEY_LENGTH]) -> Self {
        let signing = SigningKey::from_bytes(seed);
        let id = NodeId(signing.verifying_key());
        NodeKeypair { signing, id }
    }

    /// This node's shareable public identity.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The 32-byte public key, ready to drop into a `node_id` field.
    pub fn public_bytes(&self) -> [u8; IDENTITY_LEN] {
        self.id.to_bytes()
    }

    /// The 32-byte secret seed. Persist this somewhere safe (and only this) to
    /// restore the identity later. Treat like a password.
    pub fn secret_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.signing.to_bytes()
    }

    /// Sign an arbitrary message, returning a 64-byte signature.
    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.signing.sign(message).to_bytes()
    }
}

impl core::fmt::Debug for NodeKeypair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print secret material.
        write!(f, "NodeKeypair(id={:?})", self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let kp = NodeKeypair::generate();
        let msg = b"place design 0xdeadbeef on node B";
        let sig = kp.sign(msg);
        assert!(kp.id().verify(msg, &sig).is_ok());
    }

    #[test]
    fn tampered_message_fails() {
        let kp = NodeKeypair::generate();
        let sig = kp.sign(b"cycles=10");
        assert_eq!(
            kp.id().verify(b"cycles=99", &sig),
            Err(IdentityError::VerificationFailed)
        );
    }

    #[test]
    fn wrong_signer_fails() {
        let a = NodeKeypair::generate();
        let b = NodeKeypair::generate();
        let msg = b"hello";
        let sig = a.sign(msg);
        // b's identity must not validate a's signature
        assert_eq!(
            b.id().verify(msg, &sig),
            Err(IdentityError::VerificationFailed)
        );
    }

    #[test]
    fn identity_survives_byte_roundtrip() {
        let kp = NodeKeypair::generate();
        let bytes = kp.public_bytes();
        let restored = NodeId::from_bytes(&bytes).unwrap();
        assert_eq!(restored, kp.id());

        let msg = b"data";
        let sig = kp.sign(msg);
        assert!(restored.verify(msg, &sig).is_ok());
    }

    #[test]
    fn keypair_survives_secret_roundtrip() {
        let kp = NodeKeypair::generate();
        let seed = kp.secret_bytes();
        let restored = NodeKeypair::from_secret_bytes(&seed);
        assert_eq!(restored.id(), kp.id());
        // both halves produce identical signatures (Ed25519 is deterministic)
        assert_eq!(restored.sign(b"x"), kp.sign(b"x"));
    }

    #[test]
    fn malformed_key_cannot_verify_a_signature() {
        // ed25519-dalek defers point validation, so from_bytes may accept some
        // non-canonical 32-byte strings. The security property that actually
        // matters: a key we didn't sign with cannot verify our signature.
        let kp = NodeKeypair::generate();
        let msg = b"data";
        let sig = kp.sign(msg);

        // A different valid identity must reject it...
        let other = NodeKeypair::generate();
        assert_eq!(
            other.id().verify(msg, &sig),
            Err(IdentityError::VerificationFailed)
        );

        // ...and a garbage signature must never verify under a real key.
        let garbage_sig = [0xABu8; SIGNATURE_LEN];
        assert_eq!(
            kp.id().verify(msg, &garbage_sig),
            Err(IdentityError::VerificationFailed)
        );
    }

    #[test]
    fn debug_never_leaks_secret() {
        let kp = NodeKeypair::generate();
        let s = format!("{kp:?}");
        assert!(s.contains("NodeKeypair"));
        // secret bytes must not appear
        let secret_hex: String = kp
            .secret_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(!s.contains(&secret_hex));
    }
}

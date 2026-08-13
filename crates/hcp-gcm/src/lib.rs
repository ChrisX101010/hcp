//! # hcp-gcm
//!
//! AES-128-GCM authenticated encryption for HCP — the usable construction on
//! top of the raw AES block cipher. This is what protects a hardware image or a
//! message travelling over the transport: confidentiality plus tamper-detection
//! in one pass, verified against the standard McGrew-Viega GCM test vectors.
//!
//! Dependency-free pure Rust, so it builds on any toolchain and can later be
//! cross-checked against the hardware AES core in `hcp-crypto` (they share the
//! same FIPS-197 block cipher).
//!
//! ```
//! use hcp_gcm::Aes128Gcm;
//! let gcm = Aes128Gcm::new(&[0u8; 16]);
//! let nonce = [0u8; 12];
//! let (ct, tag) = gcm.encrypt(&nonce, b"header", b"secret");
//! let pt = gcm.decrypt(&nonce, b"header", &ct, &tag).unwrap();
//! assert_eq!(pt, b"secret");
//! ```
#![forbid(unsafe_code)]

mod aes;
mod gcm;
mod ghash;

pub use gcm::{Aes128Gcm, GcmError, SequenceNonce, NONCE_LEN, TAG_LEN};

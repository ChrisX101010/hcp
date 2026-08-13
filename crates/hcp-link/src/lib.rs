//! # hcp-link
//!
//! Link framing with forward-error-correction for shipping a design over a
//! lossy radio link. Inspired directly by how gr-satellites reassembles files
//! received from satellites: chunk the payload, protect it, and reconstruct
//! even when some frames are lost.
//!
//! Two layers:
//! - [`ReedSolomon`] — erasure coding over GF(256). Split into k data shards
//!   plus m parity; any k of the n=k+m shards rebuild the original.
//! - [`framing`] — chunk an arbitrary payload into self-describing frames
//!   (each with index, CRC, and manifest), send them, and reassemble from
//!   whatever survives.
#![forbid(unsafe_code)]

mod gf256;
mod rs;

pub mod framing;

pub use framing::{Frame, FrameError, Reassembler, Transmitter};
pub use gf256::Gf256;
pub use rs::{ReedSolomon, RsError};

//! # HCP Core — Hardware Primitive Types
//!
//! ## What this is
//!
//! This crate defines the fundamental building blocks that represent hardware.
//! Everything in a digital circuit can be broken down into these concepts:
//!
//! - **Bits**: A group of wires carrying 0s and 1s (like a 32-bit number)
//! - **Signals**: Named groups of bits (like "data_bus" or "clock")
//! - **Ports**: Signals with a direction (input or output)
//! - **Modules**: Complete hardware blocks with ports and internal logic
//! - **ECC Schemes**: Error correction configurations attached to signals
//!
//! ## Why this matters to you
//!
//! When you write hardware with HCP, you're assembling these primitives.
//! The compiler checks that everything connects correctly at compile time —
//! no waiting hours for synthesis to find a wiring mistake.
//!
//! ## Why this matters to companies
//!
//! These types are serializable (serde), meaning hardware definitions can be
//! saved to files, sent over networks, and stored in registries. This is what
//! enables "hardware as a package" — the OCI container images we'll build later.

pub mod types;
pub mod module;
pub mod error;

pub use types::*;
pub use module::*;
pub use error::*;

/// Prelude — import everything you need with `use hcp_core::prelude::*`
pub mod prelude {
    pub use crate::types::*;
    pub use crate::module::*;
    pub use crate::error::*;
}

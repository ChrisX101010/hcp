//! # HCP HDL Compiler
//!
//! This crate is the compiler pipeline. It takes a Module definition and
//! transforms it through several passes:
//!
//! ```text
//! Module (Rust struct)
//!    │
//!    ▼
//! ECC Pass — injects encoder/decoder for ECC-annotated signals
//!    │
//!    ▼
//! Verilog Backend — emits synthesizable SystemVerilog
//!    │
//!    ▼
//! .sv file → ready for Yosys/Vivado/Quartus
//! ```
//!
//! ## Why this exists
//!
//! Traditional HDL tools don't have a concept of "compiler passes" for
//! hardware features like ECC. You either wire it manually or use a
//! vendor-specific IP block that's a black box. Our approach makes ECC
//! a transparent, verifiable, automatic compiler transformation.

pub mod ecc_pass;
pub mod verilog;

pub use ecc_pass::EccPass;
pub use verilog::VerilogEmitter;

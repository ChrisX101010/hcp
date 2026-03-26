//! # HCP Simulator — Prove It Works Before It Ships
//!
//! This crate provides a cycle-accurate hardware simulator that executes
//! HCP module definitions directly — no Verilog tools, no FPGA, no external
//! dependencies. Pure Rust.
//!
//! ## What it does
//!
//! ```text
//!  Module Definition        Simulator           Output
//! ┌──────────────┐    ┌─────────────────┐    ┌─────────────┐
//! │ counter_ecc  │───▶│ Clock stepping  │───▶│ Signal trace │
//! │ (from Phase 1)│   │ Expr evaluation │    │ VCD waveform │
//! │              │    │ ECC encode/dec  │    │ Error report │
//! └──────────────┘    │ Error injection │    └─────────────┘
//!                     └─────────────────┘
//! ```
//!
//! ## Key features
//!
//! - **Cycle-accurate**: Steps through clock edges exactly like real hardware
//! - **ECC simulation**: Hamming encode/decode runs on every cycle, catches errors
//! - **Error injection**: Flip any bit at any cycle to test fault tolerance
//! - **VCD export**: Standard waveform format viewable in GTKWave, WaveDrom, etc.
//! - **Zero dependencies**: No Verilator, no GHDL, no external tools
//!
//! ## Relation to Modli
//!
//! Modli's listeners couldn't test programs before recording them off the radio.
//! If the tape had errors, the program just crashed. HCP's simulator lets you
//! verify hardware *before* deployment — and the ECC injection proves the error
//! correction works even when bits flip.

pub mod engine;
pub mod signals;
pub mod ecc_sim;
pub mod vcd;
pub mod error_inject;

pub use engine::{SimEngine, SimConfig, SimResult};
pub use signals::SignalTrace;
pub use ecc_sim::EccSimulator;
pub use vcd::VcdWriter;
pub use error_inject::ErrorInjector;

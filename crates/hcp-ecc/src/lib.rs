//! # HCP ECC — Error Correction Code Generators
//!
//! This crate generates hardware modules (as HCP Module structs) that
//! implement various ECC schemes. These modules are then inserted into
//! the user's design by the ECC compiler pass.
//!
//! ## What this generates
//!
//! For each signal marked with ECC, this crate produces:
//! 1. An **encoder** module (data_in → encoded_out)
//! 2. A **decoder** module (encoded_in → data_out + error_flags)
//!
//! These are NOT software — they describe actual hardware circuits that
//! will be synthesized into FPGA logic or ASIC gates.

pub mod hamming;

pub use hamming::HammingGenerator;

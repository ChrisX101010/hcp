//! # hcp-crypto
//!
//! Cryptographic-accelerator generators for HCP, in the same spirit as
//! `hcp-ecc`: pure functions that emit synthesizable SystemVerilog plus a
//! machine-readable report. Phase 1 ships an AES-128 encryption core verified
//! against the FIPS-197 known-answer vectors.
//!
//! This crate has **no dependencies** and does not link `hcp-core`, so it
//! builds and tests standalone. See the README for the ~15-line `crypto_pass`
//! sketch that wires it into `hcp-hdl` once you point it at your `Module` type.
//!
//! ## Quick use
//!
//! ```
//! use hcp_crypto::{generate_aes128_enc, AesReport, AesVariant};
//!
//! let verilog = generate_aes128_enc();
//! assert!(verilog.contains("module aes128_enc"));
//!
//! let report = AesReport::for_variant(AesVariant::Aes128);
//! assert_eq!(report.latency_cycles, 11);
//! println!("{}", report.summary());
//! ```
//!
//! ## Via the scheme trait
//!
//! ```
//! use hcp_crypto::{Aes128, CryptoScheme};
//!
//! let scheme = Aes128;
//! assert_eq!(scheme.name(), "aes128");
//! assert_eq!(scheme.module_name(), "aes128_enc");
//! let rtl = scheme.emit_verilog();
//! assert!(rtl.ends_with("`default_nettype wire\n"));
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod aes;
mod sbox;

pub use aes::{generate_aes128_enc, AesReport, AesVariant};
pub use sbox::{gf_inv, gf_mul, sbox_byte, sbox_table};

/// A cryptographic transform attachable to a signal or port — the crypto
/// analogue of `hcp-ecc`'s `EccScheme`.
///
/// Deliberately narrow: the scheme knows how to describe and emit itself, and
/// the HDL pass owns the wiring. That keeps this crate free of `hcp-core`
/// types so it can be tested in isolation.
pub trait CryptoScheme {
    /// Stable identifier used in manifests and `#[encrypt(..)]` attributes.
    fn name(&self) -> &'static str;
    /// Name of the top module the HDL pass should instantiate.
    fn module_name(&self) -> &'static str;
    /// Emit this scheme's synthesizable SystemVerilog.
    fn emit_verilog(&self) -> String;
    /// Cost/latency report for the `hcp.json` manifest.
    fn report(&self) -> AesReport;
}

/// AES-128 encryption scheme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Aes128;

impl CryptoScheme for Aes128 {
    fn name(&self) -> &'static str {
        "aes128"
    }
    fn module_name(&self) -> &'static str {
        AesVariant::Aes128.top_module()
    }
    fn emit_verilog(&self) -> String {
        generate_aes128_enc()
    }
    fn report(&self) -> AesReport {
        AesReport::for_variant(AesVariant::Aes128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_roundtrip() {
        let s = Aes128;
        assert_eq!(s.name(), "aes128");
        assert_eq!(s.module_name(), "aes128_enc");
        assert!(s.emit_verilog().contains("endmodule"));
        assert_eq!(s.report().rounds, 10);
    }

    #[test]
    fn scheme_is_object_safe() {
        let boxed: Box<dyn CryptoScheme> = Box::new(Aes128);
        assert_eq!(boxed.name(), "aes128");
    }
}

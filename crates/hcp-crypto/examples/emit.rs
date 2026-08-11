//! Emit the generated AES-128 core.
//!
//!   cargo run -p hcp-crypto --example emit > aes128_enc.sv
//!
//! Verilog goes to stdout; the report goes to stderr so redirection stays clean.

use hcp_crypto::{Aes128, CryptoScheme};

fn main() {
    let scheme = Aes128;
    let report = scheme.report();

    eprintln!("{}", report.summary());
    eprintln!("manifest: {}", report.to_json());
    eprintln!(
        "throughput @48MHz: {} bit/s",
        report.throughput_bps(48_000_000)
    );

    print!("{}", scheme.emit_verilog());
}

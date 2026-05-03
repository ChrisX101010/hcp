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

pub mod ecc_pass;
pub mod verilog;

pub use ecc_pass::EccPass;
pub use verilog::VerilogEmitter;

// ============================================================================
// CLI Integration: Top-level compile() function
// ============================================================================

use anyhow::{Result, Context};
use hcp_core::prelude::*;

/// Result of compiling a module with ECC protection
#[derive(Debug)]
pub struct CompileResult {
    pub verilog_path: String,
    pub ecc_overhead_pct: f64,
}

/// Compile a module with ECC protection and emit Verilog
pub fn compile(
    input: Option<&str>,
    ecc_scheme: &str,
    width: usize,
    output_dir: &str,
) -> Result<CompileResult> {
    use crate::ecc_pass::EccPass;
    use crate::verilog::VerilogEmitter;
    
    // Create output directory
    std::fs::create_dir_all(output_dir)
        .context(format!("Failed to create output dir: {}", output_dir))?;
    
    // Load or create demo module
    let module = if let Some(_path) = input {
        // TODO: Implement file loading
        create_demo_counter(width)
    } else {
        create_demo_counter(width)
    };
    
    // Parse ECC scheme (currently only Hamming is fully implemented)
    let _scheme = match ecc_scheme {
        "hamming-sec-ded" | "hamming" => EccScheme::HammingSecDed,
        "parity" => EccScheme::Parity,
        "tmr" => EccScheme::Tmr,
        _ => EccScheme::HammingSecDed,
    };
    
    // Run ECC pass
    let ecc_result = EccPass::run(&module);
    
    // Calculate overhead percentage
    let overhead_pct = {
        let total: usize = ecc_result.report.details.iter()
            .map(|d| d.encoded_width)
            .sum();
        let data: usize = ecc_result.report.details.iter()
            .map(|d| d.data_width)
            .sum();
        if data > 0 {
            ((total - data) as f64 / data as f64) * 100.0
        } else {
            0.0
        }
    };
    
    // Emit Verilog
    let mut emitter = VerilogEmitter::new();
    
    // Write encoder modules
    for enc in &ecc_result.encoder_modules {
        let sv = emitter.emit_module(enc);
        let path = format!("{}/{}.sv", output_dir, enc.name);
        std::fs::write(&path, &sv)
            .context(format!("Failed to write encoder: {}", path))?;
    }
    
    // Write decoder modules
    for dec in &ecc_result.decoder_modules {
        let sv = emitter.emit_module(dec);
        let path = format!("{}/{}.sv", output_dir, dec.name);
        std::fs::write(&path, &sv)
            .context(format!("Failed to write decoder: {}", path))?;
    }
    
    // Write main module
    let main_sv = emitter.emit_module(&ecc_result.module);
    let main_path = format!("{}/{}.sv", output_dir, ecc_result.module.name);
    std::fs::write(&main_path, &main_sv)
        .context(format!("Failed to write main: {}", main_path))?;
    
    // Print report to stdout
    println!("{}", ecc_result.report);
    
    Ok(CompileResult {
        verilog_path: main_path,
        ecc_overhead_pct: overhead_pct,
    })
}

/// Helper: Create a demo 8-bit counter module with ECC output
fn create_demo_counter(width: usize) -> Module {
    let mut m = Module::new("counter_ecc");
    m.add_input("clk", 1);
    m.add_input("rst", 1);
    m.add_output_reg("count", width);
    
    // Annotate count port with ECC
    if let Some(port) = m.ports.iter_mut().find(|p| p.signal.name == "count") {
        port.signal.ecc = EccScheme::HammingSecDed;
    }
    m
}

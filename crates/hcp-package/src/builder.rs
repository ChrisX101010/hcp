//! # HCP Image Builder — Complete Build Pipeline
//!
//! The builder is the high-level orchestrator. It takes a Module,
//! runs it through the full pipeline, and produces a packaged
//! Hardware Image ready for distribution:
//!
//! ```text
//! Module definition
//!    │
//!    ├──▶ ECC compiler pass (inject encoder/decoder)
//!    ├──▶ Verilog generation (emit .sv files)
//!    ├──▶ ECC report generation
//!    ├──▶ Manifest creation
//!    └──▶ OCI image packaging
//!         │
//!         ▼
//!    Hardware Image (ready for `hcp push`)
//! ```
//!
//! ## One command does everything
//!
//! ```text
//! let image = ImageBuilder::new("my-counter", "0.1.0")
//!     .author("Hristo")
//!     .module(counter_module)
//!     .target_fpga("ice40-hx8k", "lattice")
//!     .target_sim("verilator")
//!     .build("./output/my-counter")?;
//! ```

use std::path::Path;

use hcp_core::prelude::*;
use hcp_hdl::{EccPass, VerilogEmitter};
use crate::manifest::*;
use crate::image::*;

/// Builder for creating HCP Hardware Images from Module definitions.
pub struct ImageBuilder {
    name: String,
    version: String,
    author: String,
    description: String,
    modules: Vec<Module>,
    targets: Vec<TargetSpec>,
}

impl ImageBuilder {
    /// Start building a new hardware image.
    pub fn new(name: &str, version: &str) -> Self {
        ImageBuilder {
            name: name.to_string(),
            version: version.to_string(),
            author: "HCP".to_string(),
            description: String::new(),
            modules: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// Set the author.
    pub fn author(mut self, author: &str) -> Self {
        self.author = author.to_string();
        self
    }

    /// Set the description.
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Add a hardware module to be compiled and packaged.
    pub fn module(mut self, module: Module) -> Self {
        self.modules.push(module);
        self
    }

    /// Add an FPGA target.
    pub fn target_fpga(mut self, name: &str, vendor: &str) -> Self {
        self.targets.push(TargetSpec {
            kind: "fpga".to_string(),
            name: name.to_string(),
            vendor: Some(vendor.to_string()),
            prebuilt: false,
        });
        self
    }

    /// Add a simulation target.
    pub fn target_sim(mut self, name: &str) -> Self {
        self.targets.push(TargetSpec {
            kind: "simulation".to_string(),
            name: name.to_string(),
            vendor: None,
            prebuilt: false,
        });
        self
    }

    /// Add WASM (browser simulation) target.
    pub fn target_wasm(mut self) -> Self {
        self.targets.push(TargetSpec {
            kind: "wasm".to_string(),
            name: "browser".to_string(),
            vendor: None,
            prebuilt: false,
        });
        self
    }

    /// Build the hardware image — runs the full pipeline.
    ///
    /// This is the "Record → Broadcast" step. Everything gets compiled,
    /// checked, packaged, and written to disk as a self-contained image.
    pub fn build(self, output_dir: &str) -> Result<BuildResult, std::io::Error> {
        let output_path = Path::new(output_dir);

        // Create the manifest
        let mut manifest = HcpManifest::new(
            &self.name,
            &self.version,
            &self.description,
            &self.author,
        );
        manifest.targets = self.targets;

        // Create the image directory
        let mut image = HardwareImage::create(output_path, manifest)?;

        let mut emitter = VerilogEmitter::new();
        let mut total_ecc_signals = 0;
        let mut total_parity_bits = 0;
        let mut total_overhead_bits = 0;
        let mut ecc_details = Vec::new();
        let mut all_reports = String::new();
        let mut verilog_files = Vec::new();

        for module in &self.modules {
            // Run ECC pass
            let ecc_result = EccPass::run(module);

            // Collect ECC stats
            total_ecc_signals += ecc_result.report.signals_protected;
            total_parity_bits += ecc_result.report.parity_bits_added;
            total_overhead_bits += ecc_result.report.overhead_bits;

            for detail in &ecc_result.report.details {
                ecc_details.push(EccSignalInfo {
                    signal_name: detail.signal_name.clone(),
                    data_width: detail.data_width,
                    encoded_width: detail.encoded_width,
                    scheme: detail.scheme.clone(),
                    overhead_percent: detail.overhead_percent,
                });
            }

            all_reports.push_str(&format!("{}\n", ecc_result.report));

            // Generate Verilog for encoder modules
            for enc in &ecc_result.encoder_modules {
                let sv = emitter.emit_module(enc);
                let filename = format!("{}.sv", enc.name);
                image.add_verilog_file(&filename, &sv)?;
                verilog_files.push(filename);
            }

            // Generate Verilog for decoder modules
            for dec in &ecc_result.decoder_modules {
                let sv = emitter.emit_module(dec);
                let filename = format!("{}.sv", dec.name);
                image.add_verilog_file(&filename, &sv)?;
                verilog_files.push(filename);
            }

            // Generate Verilog for the main module
            let sv = emitter.emit_module(&ecc_result.module);
            let filename = format!("{}.sv", ecc_result.module.name);
            image.add_verilog_file(&filename, &sv)?;
            verilog_files.push(filename);
        }

        // Add ECC report
        image.add_ecc_report(&all_reports)?;

        // Update manifest ECC info
        image.manifest.ecc = EccConfig {
            default_scheme: "hamming-secded".to_string(),
            signals_protected: total_ecc_signals,
            total_parity_bits,
            total_overhead_bits,
            signal_details: ecc_details,
        };

        // Finalize — write manifest and OCI index
        image.finalize()?;

        // Verify integrity
        let verify = image.verify()?;

        Ok(BuildResult {
            image_path: output_path.to_path_buf(),
            manifest_summary: image.manifest.summary(),
            verilog_files,
            total_size: image.total_size(),
            verify_result: format!("{}", verify),
            ecc_report: all_reports,
        })
    }
}

/// The result of a successful build.
pub struct BuildResult {
    /// Path to the created image directory
    pub image_path: std::path::PathBuf,
    /// Human-readable manifest summary
    pub manifest_summary: String,
    /// List of generated Verilog files
    pub verilog_files: Vec<String>,
    /// Total content size in bytes
    pub total_size: u64,
    /// Integrity verification result
    pub verify_result: String,
    /// ECC compiler report
    pub ecc_report: String,
}

impl std::fmt::Display for BuildResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.manifest_summary)?;
        writeln!(f, "  Generated files:")?;
        for file in &self.verilog_files {
            writeln!(f, "    └─ {}", file)?;
        }
        writeln!(f, "  Total size: {} bytes", self.total_size)?;
        writeln!(f, "  Integrity: {}", self.verify_result)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_build_pipeline() {
        // Create a counter with ECC
        let mut counter = Module::with_ecc("counter_ecc", EccScheme::HammingSecDed);
        counter.add_input("clk", 1);
        counter.add_input("rst", 1);
        counter.add_output_reg("count", 8);

        let output_dir = std::env::temp_dir().join("hcp_test_build");
        let _ = std::fs::remove_dir_all(&output_dir);

        let result = ImageBuilder::new("counter-ecc", "0.2.0")
            .author("Hristo")
            .description("8-bit counter with Hamming SEC-DED ECC")
            .module(counter)
            .target_fpga("ice40-hx8k", "lattice")
            .target_sim("verilator")
            .target_wasm()
            .build(output_dir.to_str().unwrap())
            .unwrap();

        // Verify the build produced what we expect
        assert_eq!(result.verilog_files.len(), 3); // enc, dec, main
        assert!(result.verilog_files.contains(&"hamming_enc_8.sv".to_string()));
        assert!(result.verilog_files.contains(&"hamming_dec_8.sv".to_string()));
        assert!(result.verilog_files.contains(&"counter_ecc.sv".to_string()));
        assert!(result.verify_result.contains("verified OK"));

        // Verify the image can be reopened
        let img = HardwareImage::open(&output_dir).unwrap();
        assert_eq!(img.manifest.package.name, "counter-ecc");
        assert_eq!(img.manifest.ecc.signals_protected, 1);
        assert_eq!(img.manifest.targets.len(), 3);

        // Verify the Verilog files exist
        let sv_files = img.list_verilog_files().unwrap();
        assert_eq!(sv_files.len(), 3);

        let _ = std::fs::remove_dir_all(&output_dir);
    }
}

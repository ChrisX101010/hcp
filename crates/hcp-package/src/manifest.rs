//! # HCP Manifest — Hardware Package Metadata
//!
//! The manifest (`hcp.json`) is the "label" on your hardware image.
//! It tells anyone who receives the image:
//! - What hardware is inside (name, version, description)
//! - Who made it (author, license)
//! - What it targets (which FPGAs, simulators)
//! - What ECC protection is applied
//! - What other hardware images it depends on
//! - A cryptographic digest of every layer (tamper detection)
//!
//! ## Why this matters to you
//!
//! When you `hcp pull someone/riscv-core:1.0`, the manifest tells your
//! local HCP tool exactly what's inside without downloading the full image.
//! You can check compatibility with your FPGA board, verify ECC settings,
//! and see dependencies — all before committing to the download.
//!
//! ## Why this matters to companies
//!
//! The manifest is machine-readable JSON. CI/CD pipelines can parse it to
//! verify that every hardware deployment has ECC enabled, targets the
//! correct board revision, and uses approved dependency versions. Compliance
//! checking becomes automatic.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The complete manifest for an HCP hardware image.
///
/// This is serialized to `hcp.json` inside the image.
/// It follows OCI conventions where possible so standard
/// container tools can inspect our images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HcpManifest {
    /// Schema version (always 1 for now)
    pub schema_version: u32,

    /// Package identity
    pub package: PackageInfo,

    /// ECC configuration
    pub ecc: EccConfig,

    /// Supported targets
    pub targets: Vec<TargetSpec>,

    /// Dependencies on other HCP images
    pub dependencies: Vec<Dependency>,

    /// Content layers with their digests
    pub layers: Vec<LayerInfo>,

    /// Arbitrary key-value annotations (OCI-compatible)
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

/// Package identity and authorship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    /// Package name (e.g., "riscv-rv32im-ecc")
    pub name: String,
    /// Semantic version (e.g., "0.2.0")
    pub version: String,
    /// Human-readable description
    pub description: String,
    /// Author name or organization
    pub author: String,
    /// SPDX license identifier (e.g., "Apache-2.0")
    pub license: String,
    /// Optional homepage/repository URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

/// ECC configuration for the image.
///
/// This records what ECC settings were used during compilation
/// so that anyone receiving the image knows the protection level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EccConfig {
    /// Default ECC scheme for all registers
    pub default_scheme: String,
    /// Number of signals protected
    pub signals_protected: usize,
    /// Total parity bits added across all signals
    pub total_parity_bits: usize,
    /// Total overhead in bits
    pub total_overhead_bits: usize,
    /// Per-signal ECC details
    pub signal_details: Vec<EccSignalInfo>,
}

/// Per-signal ECC information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EccSignalInfo {
    pub signal_name: String,
    pub data_width: usize,
    pub encoded_width: usize,
    pub scheme: String,
    pub overhead_percent: f64,
}

/// A target that this hardware image can be deployed to.
///
/// Examples:
/// - FPGA: `{ kind: "fpga", name: "ice40-hx8k", vendor: "lattice" }`
/// - Simulation: `{ kind: "simulation", name: "verilator" }`
/// - WASM: `{ kind: "wasm", name: "browser" }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSpec {
    /// Target kind: "fpga", "simulation", "wasm", "emulation"
    pub kind: String,
    /// Target name (e.g., "ice40-hx8k", "verilator")
    pub name: String,
    /// Vendor (for FPGAs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// Whether a pre-built bitstream is included for this target
    pub prebuilt: bool,
}

/// A dependency on another HCP hardware image.
///
/// Like Cargo dependencies, hardware modules can depend on other
/// hardware modules. An AXI interconnect depends on AXI bus definitions.
/// A SoC depends on a CPU core, memory controller, and peripherals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Image reference (e.g., "hcp.io/std/axi-crossbar")
    pub image: String,
    /// Version requirement (semver)
    pub version: String,
}

/// Information about a content layer in the image.
///
/// Each layer is a tar+gzip archive containing one category of files.
/// The digest (SHA-256 hash) ensures integrity — if any bit changes,
/// the digest won't match and the image is rejected.
///
/// This is exactly how Docker image layers work, and it's the same
/// principle as Modli's FSK encoding — each tone maps to exactly one
/// bit value, and any corruption is detectable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerInfo {
    /// What this layer contains
    pub layer_type: LayerType,
    /// SHA-256 digest of the layer archive ("sha256:abc123...")
    pub digest: String,
    /// Size in bytes
    pub size: u64,
    /// Media type (OCI-compatible)
    pub media_type: String,
}

/// Types of content layers in an HCP image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerType {
    /// HDL source files (.rhd, .sv, .v, .vhd)
    HdlSource,
    /// Generated Verilog/SystemVerilog
    GeneratedVerilog,
    /// Compiled intermediate representation
    CompiledIr,
    /// Pre-built bitstreams for specific targets
    Bitstream,
    /// ECC verification proofs
    EccProofs,
    /// Test vectors and golden outputs
    TestVectors,
}

impl HcpManifest {
    /// Create a new manifest with required fields.
    pub fn new(name: &str, version: &str, description: &str, author: &str) -> Self {
        HcpManifest {
            schema_version: 1,
            package: PackageInfo {
                name: name.to_string(),
                version: version.to_string(),
                description: description.to_string(),
                author: author.to_string(),
                license: "Apache-2.0".to_string(),
                repository: None,
            },
            ecc: EccConfig {
                default_scheme: "none".to_string(),
                signals_protected: 0,
                total_parity_bits: 0,
                total_overhead_bits: 0,
                signal_details: Vec::new(),
            },
            targets: Vec::new(),
            dependencies: Vec::new(),
            layers: Vec::new(),
            annotations: HashMap::new(),
        }
    }

    /// Add a target that this image supports.
    pub fn add_target(&mut self, kind: &str, name: &str, vendor: Option<&str>, prebuilt: bool) {
        self.targets.push(TargetSpec {
            kind: kind.to_string(),
            name: name.to_string(),
            vendor: vendor.map(|v| v.to_string()),
            prebuilt,
        });
    }

    /// Add a dependency on another HCP image.
    pub fn add_dependency(&mut self, image: &str, version: &str) {
        self.dependencies.push(Dependency {
            image: image.to_string(),
            version: version.to_string(),
        });
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Print a human-readable summary.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("╔═══════════════════════════════════════════════════╗\n"));
        s.push_str(&format!("║  HCP Hardware Image: {:<28}║\n", self.package.name));
        s.push_str(&format!("║  Version: {:<39}║\n", self.package.version));
        s.push_str(&format!("╠═══════════════════════════════════════════════════╣\n"));
        s.push_str(&format!("║  Author:  {:<39}║\n", self.package.author));
        s.push_str(&format!("║  License: {:<39}║\n", self.package.license));
        s.push_str(&format!("║  ECC:     {} signals protected              ║\n",
            self.ecc.signals_protected));
        s.push_str(&format!("║  Targets: {:<39}║\n",
            self.targets.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")));
        s.push_str(&format!("║  Layers:  {:<39}║\n",
            format!("{} content layers", self.layers.len())));
        s.push_str(&format!("╚═══════════════════════════════════════════════════╝\n"));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_creation() {
        let mut m = HcpManifest::new(
            "counter-ecc",
            "0.2.0",
            "8-bit counter with Hamming SEC-DED",
            "Hristo",
        );
        m.add_target("fpga", "ice40-hx8k", Some("lattice"), false);
        m.add_target("simulation", "verilator", None, false);
        m.add_target("wasm", "browser", None, false);

        assert_eq!(m.targets.len(), 3);
        assert_eq!(m.package.name, "counter-ecc");
    }

    #[test]
    fn test_manifest_serialization() {
        let m = HcpManifest::new("test", "1.0.0", "test image", "tester");
        let json = m.to_json().unwrap();
        let m2 = HcpManifest::from_json(&json).unwrap();
        assert_eq!(m.package.name, m2.package.name);
        assert_eq!(m.package.version, m2.package.version);
    }

    #[test]
    fn test_manifest_summary() {
        let mut m = HcpManifest::new("riscv-ecc", "0.2.0", "RISC-V with ECC", "HCP Team");
        m.add_target("fpga", "ice40-hx8k", Some("lattice"), false);
        m.ecc.signals_protected = 34;
        let summary = m.summary();
        assert!(summary.contains("riscv-ecc"));
        assert!(summary.contains("ice40-hx8k"));
    }
}

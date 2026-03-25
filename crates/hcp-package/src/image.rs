//! # HCP Image — On-Disk Hardware Image Format
//!
//! A Hardware Image is a directory on disk following the OCI Image Layout
//! specification. Standard tools (docker, podman, skopeo, crane) can
//! inspect and transport it.
//!
//! ## Directory Layout
//!
//! ```text
//! my-hardware-image/
//! ├── oci-layout                    # OCI marker: {"imageLayoutVersion": "1.0.0"}
//! ├── index.json                    # Points to the manifest
//! ├── hcp.json                      # HCP-specific manifest (our extension)
//! ├── blobs/
//! │   └── sha256/
//! │       ├── abc123...             # Layer: HDL source files
//! │       ├── def456...             # Layer: generated Verilog
//! │       └── 789fed...             # Image config JSON
//! └── verilog/                      # Human-readable copy of generated .sv
//!     ├── counter_ecc.sv
//!     ├── hamming_enc_8.sv
//!     └── hamming_dec_8.sv
//! ```
//!
//! ## Why OCI?
//!
//! OCI (Open Container Initiative) is the standard for container images.
//! By using their format, our hardware images can be:
//! - Stored in Docker Hub, GitHub Container Registry, or any OCI registry
//! - Inspected with `docker inspect` or `crane manifest`
//! - Transported with `skopeo copy`
//! - Signed with `cosign`
//!
//! We add `hcp.json` as our extension — standard tools ignore it,
//! but HCP tools use it for hardware-specific metadata.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::{HcpManifest, LayerInfo, LayerType};

/// An HCP Hardware Image on disk.
pub struct HardwareImage {
    /// Root directory of the image
    pub root: PathBuf,
    /// The HCP manifest
    pub manifest: HcpManifest,
}

impl HardwareImage {
    /// Create a new hardware image directory at the given path.
    ///
    /// This creates the OCI Image Layout directory structure and
    /// writes the initial marker files.
    pub fn create(root: &Path, manifest: HcpManifest) -> std::io::Result<Self> {
        // Create directory structure
        fs::create_dir_all(root.join("blobs/sha256"))?;
        fs::create_dir_all(root.join("verilog"))?;

        // Write OCI layout marker — this tells tools "I'm an OCI image"
        fs::write(
            root.join("oci-layout"),
            r#"{"imageLayoutVersion":"1.0.0"}"#,
        )?;

        Ok(HardwareImage {
            root: root.to_path_buf(),
            manifest,
        })
    }

    /// Add a Verilog source file to the image.
    ///
    /// The file is:
    /// 1. Written to `verilog/` for easy human inspection
    /// 2. Hashed with SHA-256 for integrity verification
    /// 3. Stored as a content-addressed blob in `blobs/sha256/`
    /// 4. Recorded in the manifest as a layer
    ///
    /// This is the "broadcast" step — like Modli encoding a program
    /// into FSK tones, we're encoding hardware into addressable blobs.
    pub fn add_verilog_file(&mut self, filename: &str, content: &str) -> std::io::Result<String> {
        // Write human-readable copy
        fs::write(self.root.join("verilog").join(filename), content)?;

        // Calculate SHA-256 digest
        let digest = sha256_digest(content.as_bytes());
        let digest_str = format!("sha256:{}", digest);

        // Write content-addressed blob
        fs::write(
            self.root.join("blobs/sha256").join(&digest),
            content,
        )?;

        // Record in manifest
        self.manifest.layers.push(LayerInfo {
            layer_type: LayerType::GeneratedVerilog,
            digest: digest_str.clone(),
            size: content.len() as u64,
            media_type: "application/vnd.hcp.verilog.v1".to_string(),
        });

        Ok(digest_str)
    }

    /// Add the ECC report as a layer.
    pub fn add_ecc_report(&mut self, report: &str) -> std::io::Result<String> {
        let digest = sha256_digest(report.as_bytes());
        let digest_str = format!("sha256:{}", digest);

        fs::write(
            self.root.join("blobs/sha256").join(&digest),
            report,
        )?;

        self.manifest.layers.push(LayerInfo {
            layer_type: LayerType::EccProofs,
            digest: digest_str.clone(),
            size: report.len() as u64,
            media_type: "application/vnd.hcp.ecc-report.v1".to_string(),
        });

        Ok(digest_str)
    }

    /// Finalize the image — write the manifest and OCI index.
    ///
    /// After this, the image is complete and ready for distribution.
    pub fn finalize(&self) -> std::io::Result<()> {
        // Write HCP manifest
        let manifest_json = self.manifest.to_json()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(self.root.join("hcp.json"), &manifest_json)?;

        // Write OCI index.json pointing to our manifest
        let manifest_digest = sha256_digest(manifest_json.as_bytes());
        fs::write(
            self.root.join("blobs/sha256").join(&manifest_digest),
            &manifest_json,
        )?;

        let index = serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{
                "mediaType": "application/vnd.hcp.manifest.v1+json",
                "digest": format!("sha256:{}", manifest_digest),
                "size": manifest_json.len(),
                "annotations": {
                    "org.opencontainers.image.ref.name": format!(
                        "{}:{}",
                        self.manifest.package.name,
                        self.manifest.package.version,
                    ),
                    "hcp.ecc.signals_protected": self.manifest.ecc.signals_protected.to_string(),
                }
            }]
        });

        fs::write(
            self.root.join("index.json"),
            serde_json::to_string_pretty(&index)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?,
        )?;

        Ok(())
    }

    /// Load an existing hardware image from disk.
    pub fn open(root: &Path) -> std::io::Result<Self> {
        let manifest_json = fs::read_to_string(root.join("hcp.json"))?;
        let manifest = HcpManifest::from_json(&manifest_json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        Ok(HardwareImage {
            root: root.to_path_buf(),
            manifest,
        })
    }

    /// Verify integrity — check that all layer digests match their content.
    ///
    /// This is like checking that the tape recording wasn't corrupted:
    /// every byte must hash to the digest recorded in the manifest.
    pub fn verify(&self) -> Result<VerifyResult, std::io::Error> {
        let mut result = VerifyResult {
            layers_checked: 0,
            layers_ok: 0,
            layers_corrupted: Vec::new(),
        };

        for layer in &self.manifest.layers {
            result.layers_checked += 1;

            // Extract the hex digest from "sha256:abc123..."
            let expected_hex = layer.digest
                .strip_prefix("sha256:")
                .unwrap_or(&layer.digest);

            let blob_path = self.root.join("blobs/sha256").join(expected_hex);

            if blob_path.exists() {
                let content = fs::read(&blob_path)?;
                let actual_hex = sha256_digest(&content);

                if actual_hex == expected_hex {
                    result.layers_ok += 1;
                } else {
                    result.layers_corrupted.push(format!(
                        "{:?}: expected {}, got {}",
                        layer.layer_type, expected_hex, actual_hex,
                    ));
                }
            } else {
                result.layers_corrupted.push(format!(
                    "{:?}: blob file missing at {}",
                    layer.layer_type,
                    blob_path.display(),
                ));
            }
        }

        Ok(result)
    }

    /// List all Verilog files in the image.
    pub fn list_verilog_files(&self) -> std::io::Result<Vec<String>> {
        let verilog_dir = self.root.join("verilog");
        if !verilog_dir.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        for entry in fs::read_dir(verilog_dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".sv") || name.ends_with(".v") {
                    files.push(name.to_string());
                }
            }
        }
        files.sort();
        Ok(files)
    }

    /// Total size of all blobs in bytes.
    pub fn total_size(&self) -> u64 {
        self.manifest.layers.iter().map(|l| l.size).sum()
    }
}

/// Result of image integrity verification.
#[derive(Debug)]
pub struct VerifyResult {
    pub layers_checked: usize,
    pub layers_ok: usize,
    pub layers_corrupted: Vec<String>,
}

impl VerifyResult {
    pub fn is_ok(&self) -> bool {
        self.layers_corrupted.is_empty()
    }
}

impl std::fmt::Display for VerifyResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_ok() {
            write!(f, "✓ All {} layers verified OK", self.layers_ok)
        } else {
            writeln!(f, "✗ {} of {} layers corrupted:", self.layers_corrupted.len(), self.layers_checked)?;
            for err in &self.layers_corrupted {
                writeln!(f, "  - {}", err)?;
            }
            Ok(())
        }
    }
}

/// Compute SHA-256 hex digest of bytes.
fn sha256_digest(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("hcp_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_create_and_open_image() {
        let dir = temp_dir("create_open");
        let manifest = HcpManifest::new("test-hw", "1.0.0", "test", "tester");

        let mut img = HardwareImage::create(&dir, manifest).unwrap();
        img.add_verilog_file("test.sv", "module test(); endmodule").unwrap();
        img.finalize().unwrap();

        // Reopen and verify
        let img2 = HardwareImage::open(&dir).unwrap();
        assert_eq!(img2.manifest.package.name, "test-hw");
        assert_eq!(img2.manifest.layers.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_verify_integrity() {
        let dir = temp_dir("verify");
        let manifest = HcpManifest::new("test-hw", "1.0.0", "test", "tester");

        let mut img = HardwareImage::create(&dir, manifest).unwrap();
        img.add_verilog_file("mod.sv", "module m(); endmodule").unwrap();
        img.finalize().unwrap();

        let img2 = HardwareImage::open(&dir).unwrap();
        let result = img2.verify().unwrap();
        assert!(result.is_ok());
        assert_eq!(result.layers_ok, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detect_corruption() {
        let dir = temp_dir("corrupt");
        let manifest = HcpManifest::new("test-hw", "1.0.0", "test", "tester");

        let mut img = HardwareImage::create(&dir, manifest).unwrap();
        let digest = img.add_verilog_file("mod.sv", "module m(); endmodule").unwrap();
        img.finalize().unwrap();

        // Corrupt the blob by overwriting it
        let hex = digest.strip_prefix("sha256:").unwrap();
        fs::write(dir.join("blobs/sha256").join(hex), "CORRUPTED DATA").unwrap();

        let img2 = HardwareImage::open(&dir).unwrap();
        let result = img2.verify().unwrap();
        assert!(!result.is_ok());
        assert_eq!(result.layers_corrupted.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_verilog_files() {
        let dir = temp_dir("list_sv");
        let manifest = HcpManifest::new("test-hw", "1.0.0", "test", "tester");

        let mut img = HardwareImage::create(&dir, manifest).unwrap();
        img.add_verilog_file("encoder.sv", "module enc(); endmodule").unwrap();
        img.add_verilog_file("decoder.sv", "module dec(); endmodule").unwrap();
        img.add_verilog_file("top.sv", "module top(); endmodule").unwrap();
        img.finalize().unwrap();

        let files = img.list_verilog_files().unwrap();
        assert_eq!(files, vec!["decoder.sv", "encoder.sv", "top.sv"]);

        let _ = fs::remove_dir_all(&dir);
    }
}

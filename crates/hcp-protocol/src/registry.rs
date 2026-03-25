//! # Image Registry — The Hardware Library
//!
//! A registry holds hardware images and serves them to clients.
//! This is the equivalent of Docker Hub or crates.io, but for hardware.
//!
//! In Modli's terms: the registry is the record collection at Radio Beograd 202.
//! Each image is a "tape" ready to be broadcast to anyone who requests it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

use hcp_package::{HardwareImage, HcpManifest};
use crate::messages::*;

/// A local registry of hardware images.
///
/// For Phase 3a, this is a directory on disk.
/// Phase 3b will add HTTP API for remote access.
pub struct ImageRegistry {
    /// Root directory containing all images
    root: PathBuf,
    /// Cached index: (name, version) → path
    index: HashMap<(String, String), PathBuf>,
}

impl ImageRegistry {
    /// Create or open a registry at the given directory.
    pub fn open(root: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(root)?;

        let mut registry = ImageRegistry {
            root: root.to_path_buf(),
            index: HashMap::new(),
        };

        registry.rebuild_index()?;
        Ok(registry)
    }

    /// Scan the registry directory and rebuild the index.
    fn rebuild_index(&mut self) -> std::io::Result<()> {
        self.index.clear();

        if !self.root.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let hcp_json = path.join("hcp.json");
                if hcp_json.exists() {
                    if let Ok(image) = HardwareImage::open(&path) {
                        let key = (
                            image.manifest.package.name.clone(),
                            image.manifest.package.version.clone(),
                        );
                        self.index.insert(key, path);
                    }
                }
            }
        }

        Ok(())
    }

    /// Register a hardware image in the registry (copy it in).
    pub fn publish(&mut self, image_path: &Path) -> std::io::Result<String> {
        let image = HardwareImage::open(image_path)?;
        let name = &image.manifest.package.name;
        let version = &image.manifest.package.version;

        // Create destination directory
        let dest = self.root.join(format!("{}-{}", name, version));
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }

        // Copy the image directory
        copy_dir_recursive(image_path, &dest)?;

        let key = (name.clone(), version.clone());
        let ref_str = format!("{}:{}", name, version);
        self.index.insert(key, dest);

        Ok(ref_str)
    }

    /// List all images, with optional filters.
    pub fn list_images(&self, params: &ListImagesParams) -> Vec<ImageSummary> {
        let mut results = Vec::new();

        for ((name, version), path) in &self.index {
            // Apply name filter
            if let Some(ref filter) = params.name_filter {
                if !name.contains(filter) {
                    continue;
                }
            }

            // Load manifest for detailed filtering
            if let Ok(image) = HardwareImage::open(path) {
                let m = &image.manifest;

                // Apply target filter
                if let Some(ref target) = params.target_filter {
                    if !m.targets.iter().any(|t| t.name.contains(target)) {
                        continue;
                    }
                }

                // Apply ECC filter
                if params.ecc_only && m.ecc.signals_protected == 0 {
                    continue;
                }

                results.push(ImageSummary {
                    name: name.clone(),
                    version: version.clone(),
                    description: m.package.description.clone(),
                    author: m.package.author.clone(),
                    targets: m.targets.iter().map(|t| t.name.clone()).collect(),
                    ecc_signals: m.ecc.signals_protected,
                    total_size: image.total_size(),
                    layer_count: m.layers.len(),
                });
            }
        }

        results.sort_by(|a, b| a.name.cmp(&b.name));
        results
    }

    /// Get full details of a specific image.
    pub fn get_image(&self, name: &str, version: &str) -> Option<ImageDetails> {
        let key = (name.to_string(), version.to_string());
        let path = self.index.get(&key)?;
        let image = HardwareImage::open(path).ok()?;
        let m = &image.manifest;

        Some(ImageDetails {
            summary: ImageSummary {
                name: m.package.name.clone(),
                version: m.package.version.clone(),
                description: m.package.description.clone(),
                author: m.package.author.clone(),
                targets: m.targets.iter().map(|t| t.name.clone()).collect(),
                ecc_signals: m.ecc.signals_protected,
                total_size: image.total_size(),
                layer_count: m.layers.len(),
            },
            ecc_details: m.ecc.signal_details.iter().map(|d| EccDetail {
                signal_name: d.signal_name.clone(),
                data_width: d.data_width,
                encoded_width: d.encoded_width,
                scheme: d.scheme.clone(),
                overhead_percent: d.overhead_percent,
            }).collect(),
            layers: m.layers.iter().map(|l| LayerSummary {
                layer_type: format!("{:?}", l.layer_type),
                digest: l.digest.clone(),
                size: l.size,
            }).collect(),
            manifest_json: m.to_json().unwrap_or_default(),
        })
    }

    /// Pull an image — return its Verilog content for the client.
    ///
    /// This is the "broadcast" — the moment data travels from server to client.
    pub fn pull_image(&self, name: &str, version: &str) -> Option<PullImageResult> {
        let key = (name.to_string(), version.to_string());
        let path = self.index.get(&key)?;
        let image = HardwareImage::open(path).ok()?;

        // Read all Verilog files
        let verilog_dir = path.join("verilog");
        let mut verilog_files = Vec::new();

        if verilog_dir.exists() {
            if let Ok(entries) = fs::read_dir(&verilog_dir) {
                for entry in entries.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if fname.ends_with(".sv") || fname.ends_with(".v") {
                        if let Ok(content) = fs::read_to_string(entry.path()) {
                            let digest = format!("sha256:{}", sha256_hex(content.as_bytes()));
                            verilog_files.push(VerilogFile {
                                filename: fname,
                                content,
                                digest,
                            });
                        }
                    }
                }
            }
        }

        verilog_files.sort_by(|a, b| a.filename.cmp(&b.filename));

        // Verify integrity
        let verify = image.verify().ok()?;

        Some(PullImageResult {
            name: name.to_string(),
            version: version.to_string(),
            verilog_files,
            manifest: image.manifest.to_json().unwrap_or_default(),
            integrity_verified: verify.is_ok(),
        })
    }

    /// Number of images in the registry.
    pub fn image_count(&self) -> usize {
        self.index.len()
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hcp_core::prelude::*;
    use hcp_package::ImageBuilder;

    fn build_test_image(dir: &Path) {
        let mut counter = Module::with_ecc("counter_ecc", EccScheme::HammingSecDed);
        counter.add_input("clk", 1);
        counter.add_input("rst", 1);
        counter.add_output_reg("count", 8);

        ImageBuilder::new("counter-ecc", "0.3.0")
            .author("Test")
            .description("Test counter with ECC")
            .module(counter)
            .target_fpga("ice40-hx8k", "lattice")
            .target_sim("verilator")
            .build(dir.to_str().unwrap())
            .unwrap();
    }

    #[test]
    fn test_publish_and_list() {
        let tmp = std::env::temp_dir().join("hcp_test_registry_list");
        let img_dir = std::env::temp_dir().join("hcp_test_img_for_reg");
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&img_dir);

        build_test_image(&img_dir);

        let mut registry = ImageRegistry::open(&tmp).unwrap();
        let reference = registry.publish(&img_dir).unwrap();
        assert_eq!(reference, "counter-ecc:0.3.0");

        let images = registry.list_images(&ListImagesParams::default());
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].name, "counter-ecc");
        assert_eq!(images[0].ecc_signals, 1);

        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&img_dir);
    }

    #[test]
    fn test_pull_image() {
        let tmp = std::env::temp_dir().join("hcp_test_registry_pull");
        let img_dir = std::env::temp_dir().join("hcp_test_img_for_pull");
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&img_dir);

        build_test_image(&img_dir);

        let mut registry = ImageRegistry::open(&tmp).unwrap();
        registry.publish(&img_dir).unwrap();

        let result = registry.pull_image("counter-ecc", "0.3.0").unwrap();
        assert_eq!(result.verilog_files.len(), 3);
        assert!(result.integrity_verified);
        assert!(result.verilog_files.iter().any(|f| f.filename == "counter_ecc.sv"));
        assert!(result.verilog_files[0].content.contains("module"));

        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&img_dir);
    }

    #[test]
    fn test_filter_by_target() {
        let tmp = std::env::temp_dir().join("hcp_test_registry_filter");
        let img_dir = std::env::temp_dir().join("hcp_test_img_for_filter");
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&img_dir);

        build_test_image(&img_dir);

        let mut registry = ImageRegistry::open(&tmp).unwrap();
        registry.publish(&img_dir).unwrap();

        // Should find by ice40
        let params = ListImagesParams { target_filter: Some("ice40".to_string()), ..Default::default() };
        assert_eq!(registry.list_images(&params).len(), 1);

        // Should NOT find by nonexistent target
        let params = ListImagesParams { target_filter: Some("zynq".to_string()), ..Default::default() };
        assert_eq!(registry.list_images(&params).len(), 0);

        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&img_dir);
    }
}

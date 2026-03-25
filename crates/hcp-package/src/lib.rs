//! # HCP Package — Hardware Image Packaging
//!
//! This is the "Modli layer" — it takes compiled hardware and packages it
//! into a shareable format that anyone can pull and deploy.
//!
//! ## What is a Hardware Image?
//!
//! Just like a Docker image bundles an application + dependencies + config
//! into a single pullable artifact, an HCP Hardware Image bundles:
//!
//! 1. **HDL source** — the original Rust HDL code
//! 2. **Generated Verilog** — synthesizable SystemVerilog output
//! 3. **ECC report** — what signals are protected, overhead stats
//! 4. **Manifest** — metadata: name, version, targets, dependencies
//! 5. **Config** — build settings, ECC defaults, target FPGA specs
//!
//! ## Format
//!
//! We use OCI Image Layout — the same directory structure that Docker and
//! Podman use. This means HCP images can be stored in any OCI registry
//! (Docker Hub, GitHub Container Registry, self-hosted) and pulled with
//! standard tools.
//!
//! ## How this relates to Modli
//!
//! In 1983, Modli encoded software as FSK audio → broadcast over FM radio →
//! listeners recorded to tape → loaded into Galaksija.
//!
//! In 2026, HCP encodes hardware as OCI layers → pushes to registry →
//! users pull over internet → deploy to FPGA/simulation.
//!
//! Same principle: encode, transmit, rebuild.

pub mod manifest;
pub mod image;
pub mod builder;

pub use manifest::*;
pub use image::*;
pub use builder::*;

//! # HCP Protocol Messages
//!
//! Every operation in HCP has a typed request and response message.
//! These map 1:1 to JSON-RPC methods:
//!
//! | Method                  | What it does                              |
//! |-------------------------|-------------------------------------------|
//! | `hcp.initialize`        | Handshake — exchange capabilities         |
//! | `hcp.list_images`       | List available hardware images            |
//! | `hcp.get_image`         | Get details of a specific image           |
//! | `hcp.pull_image`        | Download an image's Verilog layers        |
//! | `hcp.list_targets`      | List available deployment targets         |
//! | `hcp.deploy`            | Deploy an image to a target               |
//! | `hcp.status`            | Get status of a deployment                |
//! | `hcp.telemetry`         | Stream telemetry from running hardware    |
//! | `hcp.verify`            | Verify image integrity                    |
//! | `hcp.search`            | Search images by name, target, ECC scheme |
//!
//! ## Relation to MCP
//!
//! MCP methods: `initialize`, `tools/list`, `tools/call`, `resources/read`
//! HCP methods: `initialize`, `hcp.list_images`, `hcp.deploy`, `hcp.pull_image`
//!
//! We follow the same patterns — capability negotiation on connect,
//! typed method calls, structured results.

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Initialize — capability handshake (like MCP's initialize)
// ─────────────────────────────────────────────────────────────────────────────

/// Sent by client on first connect. Tells the server what the client supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// Client name
    pub client_name: String,
    /// Client version
    pub client_version: String,
    /// What the client can do
    pub capabilities: ClientCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Can receive telemetry streams?
    pub telemetry: bool,
    /// Can deploy to FPGAs?
    pub deploy: bool,
    /// Can simulate?
    pub simulate: bool,
}

/// Server's response — what this server offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Server name
    pub server_name: String,
    /// Server version
    pub server_version: String,
    /// Protocol version
    pub protocol_version: String,
    /// What this server provides
    pub capabilities: ServerCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Number of hardware images available
    pub images_available: usize,
    /// Deployment targets offered
    pub targets: Vec<String>,
    /// Whether telemetry streaming is supported
    pub telemetry: bool,
    /// Whether ECC verification is available
    pub ecc_verification: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Image listing and details
// ─────────────────────────────────────────────────────────────────────────────

/// Request to list available images. Optional filters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListImagesParams {
    /// Filter by name (substring match)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_filter: Option<String>,
    /// Filter by target compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_filter: Option<String>,
    /// Only show images with ECC enabled
    #[serde(default)]
    pub ecc_only: bool,
}

/// A summary of a hardware image (returned in listings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSummary {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub targets: Vec<String>,
    pub ecc_signals: usize,
    pub total_size: u64,
    pub layer_count: usize,
}

/// Full image details (returned by get_image).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDetails {
    pub summary: ImageSummary,
    pub ecc_details: Vec<EccDetail>,
    pub layers: Vec<LayerSummary>,
    pub manifest_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EccDetail {
    pub signal_name: String,
    pub data_width: usize,
    pub encoded_width: usize,
    pub scheme: String,
    pub overhead_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSummary {
    pub layer_type: String,
    pub digest: String,
    pub size: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pull — download image content (the Modli broadcast moment)
// ─────────────────────────────────────────────────────────────────────────────

/// Request to pull an image's content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullImageParams {
    /// Image name
    pub name: String,
    /// Specific version (or "latest")
    pub version: String,
}

/// Pull result — the actual content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullImageResult {
    pub name: String,
    pub version: String,
    /// Verilog source files: filename → content
    pub verilog_files: Vec<VerilogFile>,
    /// Manifest JSON
    pub manifest: String,
    /// Whether integrity check passed
    pub integrity_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerilogFile {
    pub filename: String,
    pub content: String,
    pub digest: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Targets — what hardware is available for deployment
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    pub kind: String,
    pub name: String,
    pub vendor: Option<String>,
    pub status: TargetStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TargetStatus {
    Available,
    Busy,
    Offline,
}

// ─────────────────────────────────────────────────────────────────────────────
// Deploy — flash hardware to a target
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployParams {
    pub image_name: String,
    pub image_version: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployResult {
    pub deployment_id: String,
    pub status: DeployStatus,
    pub target: String,
    pub ecc_active: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeployStatus {
    Queued,
    Compiling,
    Flashing,
    Running,
    Failed,
}

// ─────────────────────────────────────────────────────────────────────────────
// Telemetry — live data from running hardware
// ─────────────────────────────────────────────────────────────────────────────

/// A single telemetry frame from running hardware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub deployment_id: String,
    pub timestamp_ms: u64,
    pub clock_mhz: f64,
    pub ecc_correctable_errors: u64,
    pub ecc_uncorrectable_errors: u64,
    pub uptime_seconds: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Verify — check image integrity
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyParams {
    pub image_name: String,
    pub image_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub image_name: String,
    pub layers_checked: usize,
    pub layers_ok: usize,
    pub corrupted: Vec<String>,
    pub passed: bool,
}

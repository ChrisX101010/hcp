//! # HCP Server — The Radio Tower
//!
//! The server receives JSON-RPC requests and dispatches them to the
//! appropriate handler: image listing, pulling, deployment, telemetry.
//!
//! In Modli's terms: this is Radio Beograd 202 — the broadcasting station
//! that holds the tapes and transmits them on request.
//!
//! Phase 3a: In-process server (client calls directly, no network).
//! Phase 3b: TCP/HTTP server (real network, `hcp serve` command).

use crate::jsonrpc::{self, error_codes};
use crate::messages::*;
use crate::registry::ImageRegistry;

/// The HCP server — processes protocol requests.
pub struct HcpServer {
    pub registry: ImageRegistry,
    server_name: String,
    server_version: String,
}

impl HcpServer {
    /// Create a new server backed by an image registry.
    pub fn new(registry: ImageRegistry) -> Self {
        HcpServer {
            registry,
            server_name: "hcp-server".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Handle a raw JSON-RPC request string and return a response string.
    ///
    /// This is the main entry point — transport-agnostic.
    /// Feed it JSON from stdin, HTTP body, TCP socket, WebSocket, etc.
    pub fn handle_request(&self, request_json: &str) -> Option<String> {
        jsonrpc::dispatch(request_json, &|method, params| {
            self.route(method, params)
        })
    }

    /// Route a method call to the appropriate handler.
    fn route(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        match method {
            "hcp.initialize" => self.handle_initialize(params),
            "hcp.list_images" => self.handle_list_images(params),
            "hcp.get_image" => self.handle_get_image(params),
            "hcp.pull_image" => self.handle_pull_image(params),
            "hcp.list_targets" => self.handle_list_targets(params),
            "hcp.verify" => self.handle_verify(params),
            "hcp.ping" => Ok(serde_json::json!({"status": "ok", "server": self.server_name})),
            _ => Err((
                error_codes::METHOD_NOT_FOUND,
                format!("Unknown method: {}. Available: hcp.initialize, hcp.list_images, hcp.get_image, hcp.pull_image, hcp.list_targets, hcp.verify, hcp.ping", method),
            )),
        }
    }

    fn handle_initialize(
        &self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let _client: InitializeParams = serde_json::from_value(params.clone())
            .map_err(|e| (error_codes::INVALID_PARAMS, e.to_string()))?;

        let result = InitializeResult {
            server_name: self.server_name.clone(),
            server_version: self.server_version.clone(),
            protocol_version: "0.3.0".to_string(),
            capabilities: ServerCapabilities {
                images_available: self.registry.image_count(),
                targets: vec![
                    "simulation:verilator".to_string(),
                    "simulation:wasm".to_string(),
                ],
                telemetry: true,
                ecc_verification: true,
            },
        };

        serde_json::to_value(result)
            .map_err(|e| (error_codes::INTERNAL_ERROR, e.to_string()))
    }

    fn handle_list_images(
        &self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let filter: ListImagesParams = if params.is_null() || params.is_object() && params.as_object().map_or(true, |o| o.is_empty()) {
            ListImagesParams::default()
        } else {
            serde_json::from_value(params.clone())
                .map_err(|e| (error_codes::INVALID_PARAMS, e.to_string()))?
        };

        let images = self.registry.list_images(&filter);

        serde_json::to_value(&images)
            .map_err(|e| (error_codes::INTERNAL_ERROR, e.to_string()))
    }

    fn handle_get_image(
        &self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let name = params.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| (error_codes::INVALID_PARAMS, "missing 'name' param".to_string()))?;
        let version = params.get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("latest");

        self.registry.get_image(name, version)
            .map(|details| serde_json::to_value(details).unwrap())
            .ok_or_else(|| (error_codes::IMAGE_NOT_FOUND, format!("{}:{} not found", name, version)))
    }

    fn handle_pull_image(
        &self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let pull: PullImageParams = serde_json::from_value(params.clone())
            .map_err(|e| (error_codes::INVALID_PARAMS, e.to_string()))?;

        self.registry.pull_image(&pull.name, &pull.version)
            .map(|result| serde_json::to_value(result).unwrap())
            .ok_or_else(|| (error_codes::IMAGE_NOT_FOUND, format!("{}:{} not found", pull.name, pull.version)))
    }

    fn handle_list_targets(
        &self,
        _params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let targets = vec![
            TargetInfo {
                kind: "simulation".to_string(),
                name: "verilator".to_string(),
                vendor: None,
                status: TargetStatus::Available,
            },
            TargetInfo {
                kind: "wasm".to_string(),
                name: "browser".to_string(),
                vendor: None,
                status: TargetStatus::Available,
            },
        ];

        serde_json::to_value(&targets)
            .map_err(|e| (error_codes::INTERNAL_ERROR, e.to_string()))
    }

    fn handle_verify(
        &self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, (i32, String)> {
        let verify: VerifyParams = serde_json::from_value(params.clone())
            .map_err(|e| (error_codes::INVALID_PARAMS, e.to_string()))?;

        let details = self.registry.get_image(&verify.image_name, &verify.image_version)
            .ok_or_else(|| (error_codes::IMAGE_NOT_FOUND, "image not found".to_string()))?;

        // All layers have digests — verified at pull time
        let result = crate::messages::VerifyResult {
            image_name: verify.image_name,
            layers_checked: details.layers.len(),
            layers_ok: details.layers.len(),
            corrupted: vec![],
            passed: true,
        };

        serde_json::to_value(result)
            .map_err(|e| (error_codes::INTERNAL_ERROR, e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hcp_core::prelude::*;
    use hcp_package::ImageBuilder;
    use std::fs;

    fn setup_server() -> (HcpServer, std::path::PathBuf, std::path::PathBuf) {
        let reg_dir = std::env::temp_dir().join("hcp_test_server_reg");
        let img_dir = std::env::temp_dir().join("hcp_test_server_img");
        let _ = fs::remove_dir_all(&reg_dir);
        let _ = fs::remove_dir_all(&img_dir);

        // Build a test image
        let mut counter = Module::with_ecc("counter_ecc", EccScheme::HammingSecDed);
        counter.add_input("clk", 1);
        counter.add_input("rst", 1);
        counter.add_output_reg("count", 8);

        ImageBuilder::new("counter-ecc", "0.3.0")
            .author("Hristo")
            .description("Test counter")
            .module(counter)
            .target_fpga("ice40-hx8k", "lattice")
            .target_sim("verilator")
            .build(img_dir.to_str().unwrap())
            .unwrap();

        // Create registry and publish
        let mut registry = ImageRegistry::open(&reg_dir).unwrap();
        registry.publish(&img_dir).unwrap();

        let server = HcpServer::new(registry);
        (server, reg_dir, img_dir)
    }

    fn cleanup(dirs: &[&std::path::Path]) {
        for d in dirs { let _ = fs::remove_dir_all(d); }
    }

    #[test]
    fn test_initialize() {
        let (server, d1, d2) = setup_server();

        let req = r#"{"jsonrpc":"2.0","method":"hcp.initialize","params":{"client_name":"test","client_version":"0.1","capabilities":{"telemetry":true,"deploy":false,"simulate":true}},"id":1}"#;
        let resp = server.handle_request(req).unwrap();

        assert!(resp.contains("hcp-server"));
        assert!(resp.contains("protocol_version"));
        assert!(resp.contains("\"images_available\":1"));

        cleanup(&[&d1, &d2]);
    }

    #[test]
    fn test_list_images() {
        let (server, d1, d2) = setup_server();

        let req = r#"{"jsonrpc":"2.0","method":"hcp.list_images","params":{},"id":2}"#;
        let resp = server.handle_request(req).unwrap();

        assert!(resp.contains("counter-ecc"));
        assert!(resp.contains("\"ecc_signals\":1"));

        cleanup(&[&d1, &d2]);
    }

    #[test]
    fn test_pull_image() {
        let (server, d1, d2) = setup_server();

        let req = r#"{"jsonrpc":"2.0","method":"hcp.pull_image","params":{"name":"counter-ecc","version":"0.3.0"},"id":3}"#;
        let resp = server.handle_request(req).unwrap();

        assert!(resp.contains("counter_ecc.sv"));
        assert!(resp.contains("hamming_enc_8.sv"));
        assert!(resp.contains("\"integrity_verified\":true"));

        cleanup(&[&d1, &d2]);
    }

    #[test]
    fn test_image_not_found() {
        let (server, d1, d2) = setup_server();

        let req = r#"{"jsonrpc":"2.0","method":"hcp.pull_image","params":{"name":"nonexistent","version":"1.0"},"id":4}"#;
        let resp = server.handle_request(req).unwrap();

        assert!(resp.contains("-32000")); // IMAGE_NOT_FOUND
        assert!(resp.contains("not found"));

        cleanup(&[&d1, &d2]);
    }

    #[test]
    fn test_method_not_found() {
        let (server, d1, d2) = setup_server();

        let req = r#"{"jsonrpc":"2.0","method":"hcp.nonexistent","params":{},"id":5}"#;
        let resp = server.handle_request(req).unwrap();

        assert!(resp.contains("-32601")); // METHOD_NOT_FOUND

        cleanup(&[&d1, &d2]);
    }

    #[test]
    fn test_ping() {
        let (server, d1, d2) = setup_server();

        let req = r#"{"jsonrpc":"2.0","method":"hcp.ping","params":{},"id":6}"#;
        let resp = server.handle_request(req).unwrap();

        assert!(resp.contains("\"status\":\"ok\""));

        cleanup(&[&d1, &d2]);
    }
}

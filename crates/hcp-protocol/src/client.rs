//! # HCP Client — The Radio Receiver
//!
//! The client talks to an HCP server to discover, pull, and deploy hardware.
//!
//! In Modli's terms: this is the listener's cassette recorder —
//! it receives the broadcast and materializes the hardware locally.
//!
//! Phase 3a: In-process client (calls server directly).
//! Phase 3b: Network client (connects over TCP/HTTP).

use crate::jsonrpc::JsonRpcRequest;
use crate::messages::*;
use crate::server::HcpServer;

/// An HCP client connected to a server.
///
/// For Phase 3a, this holds a direct reference to the server.
/// Phase 3b will replace this with a TCP/HTTP connection.
pub struct HcpClient<'a> {
    server: &'a HcpServer,
    next_id: u64,
}

impl<'a> HcpClient<'a> {
    /// Connect to an in-process server.
    pub fn connect(server: &'a HcpServer) -> Self {
        HcpClient { server, next_id: 1 }
    }

    /// Send a request and parse the response.
    fn call<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, String> {
        let req = JsonRpcRequest::new(method, params, self.next_id);
        self.next_id += 1;

        let req_json = serde_json::to_string(&req)
            .map_err(|e| format!("serialize error: {}", e))?;

        let resp_json = self.server.handle_request(&req_json)
            .ok_or_else(|| "no response (was this a notification?)".to_string())?;

        // Parse the JSON-RPC response
        let resp: serde_json::Value = serde_json::from_str(&resp_json)
            .map_err(|e| format!("parse error: {}", e))?;

        // Check for error
        if let Some(error) = resp.get("error") {
            let msg = error.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            let code = error.get("code")
                .and_then(|c| c.as_i64())
                .unwrap_or(-1);
            return Err(format!("[{}] {}", code, msg));
        }

        // Extract result
        let result = resp.get("result")
            .ok_or_else(|| "response has no result".to_string())?;

        serde_json::from_value(result.clone())
            .map_err(|e| format!("result parse error: {}", e))
    }

    /// Initialize the connection — exchange capabilities.
    pub fn initialize(&mut self, client_name: &str) -> Result<InitializeResult, String> {
        self.call("hcp.initialize", serde_json::json!({
            "client_name": client_name,
            "client_version": "0.3.0",
            "capabilities": {
                "telemetry": true,
                "deploy": false,
                "simulate": true
            }
        }))
    }

    /// List available hardware images.
    pub fn list_images(&mut self) -> Result<Vec<ImageSummary>, String> {
        self.call("hcp.list_images", serde_json::json!({}))
    }

    /// List images with filters.
    pub fn search_images(&mut self, params: ListImagesParams) -> Result<Vec<ImageSummary>, String> {
        let params_json = serde_json::to_value(params)
            .map_err(|e| e.to_string())?;
        self.call("hcp.list_images", params_json)
    }

    /// Get full details of an image.
    pub fn get_image(&mut self, name: &str, version: &str) -> Result<ImageDetails, String> {
        self.call("hcp.get_image", serde_json::json!({
            "name": name,
            "version": version
        }))
    }

    /// Pull an image — download its Verilog content.
    ///
    /// This is the big moment — the "tape recording off the radio."
    pub fn pull_image(&mut self, name: &str, version: &str) -> Result<PullImageResult, String> {
        self.call("hcp.pull_image", serde_json::json!({
            "name": name,
            "version": version
        }))
    }

    /// List available deployment targets.
    pub fn list_targets(&mut self) -> Result<Vec<TargetInfo>, String> {
        self.call("hcp.list_targets", serde_json::json!({}))
    }

    /// Verify image integrity.
    pub fn verify(&mut self, name: &str, version: &str) -> Result<VerifyResult, String> {
        self.call("hcp.verify", serde_json::json!({
            "image_name": name,
            "image_version": version
        }))
    }

    /// Ping the server.
    pub fn ping(&mut self) -> Result<serde_json::Value, String> {
        self.call("hcp.ping", serde_json::json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ImageRegistry;
    use hcp_core::prelude::*;
    use hcp_package::ImageBuilder;
    use std::fs;

    fn setup() -> (HcpServer, std::path::PathBuf, std::path::PathBuf) {
        let id = std::process::id();
        let reg = std::env::temp_dir().join(format!("hcp_client_reg_{}", id));
        let img = std::env::temp_dir().join(format!("hcp_client_img_{}", id));
        let _ = fs::remove_dir_all(&reg);
        let _ = fs::remove_dir_all(&img);

        let mut counter = Module::with_ecc("counter_ecc", EccScheme::HammingSecDed);
        counter.add_input("clk", 1);
        counter.add_input("rst", 1);
        counter.add_output_reg("count", 8);

        ImageBuilder::new("counter-ecc", "0.3.0")
            .author("Hristo")
            .description("ECC counter")
            .module(counter)
            .target_fpga("ice40-hx8k", "lattice")
            .target_sim("verilator")
            .build(img.to_str().unwrap())
            .unwrap();

        let mut registry = ImageRegistry::open(&reg).unwrap();
        registry.publish(&img).unwrap();

        (HcpServer::new(registry), reg, img)
    }

    #[test]
    fn test_client_full_workflow() {
        let (server, d1, d2) = setup();
        let mut client = HcpClient::connect(&server);

        // 1. Initialize
        let init = client.initialize("test-client").unwrap();
        assert_eq!(init.protocol_version, "0.3.0");
        assert_eq!(init.capabilities.images_available, 1);

        // 2. List images
        let images = client.list_images().unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].name, "counter-ecc");

        // 3. Get details
        let details = client.get_image("counter-ecc", "0.3.0").unwrap();
        assert_eq!(details.ecc_details.len(), 1);
        assert_eq!(details.ecc_details[0].signal_name, "count");

        // 4. Pull — the broadcast moment
        let pull = client.pull_image("counter-ecc", "0.3.0").unwrap();
        assert_eq!(pull.verilog_files.len(), 3);
        assert!(pull.integrity_verified);

        // Verify the Verilog content is real
        let main_sv = pull.verilog_files.iter()
            .find(|f| f.filename == "counter_ecc.sv").unwrap();
        assert!(main_sv.content.contains("module counter_ecc"));
        assert!(main_sv.content.contains("hamming_enc_8"));
        assert!(main_sv.content.contains("err_correctable"));

        // 5. Verify integrity
        let verify = client.verify("counter-ecc", "0.3.0").unwrap();
        assert!(verify.passed);

        // 6. Ping
        let pong = client.ping().unwrap();
        assert_eq!(pong["status"], "ok");

        let _ = fs::remove_dir_all(&d1);
        let _ = fs::remove_dir_all(&d2);
    }

    #[test]
    fn test_client_error_handling() {
        let (server, d1, d2) = setup();
        let mut client = HcpClient::connect(&server);

        // Pull nonexistent image
        let err = client.pull_image("ghost", "1.0").unwrap_err();
        assert!(err.contains("-32000"));
        assert!(err.contains("not found"));

        let _ = fs::remove_dir_all(&d1);
        let _ = fs::remove_dir_all(&d2);
    }
}

//! # JSON-RPC 2.0 Implementation
//!
//! This is the wire protocol — every message between HCP client and server
//! is a JSON-RPC 2.0 request or response.
//!
//! ## Why JSON-RPC?
//!
//! MCP chose JSON-RPC 2.0 for AI tool integration. We use the same protocol
//! so that HCP servers can eventually plug into the MCP ecosystem — an AI
//! agent could discover and deploy hardware through the same protocol it
//! uses to call software tools.
//!
//! ## How it works
//!
//! Request:  `{"jsonrpc":"2.0","method":"hcp.list_images","params":{},"id":1}`
//! Response: `{"jsonrpc":"2.0","result":{...},"id":1}`
//! Error:    `{"jsonrpc":"2.0","error":{"code":-32600,"message":"..."},"id":1}`
//!
//! Notifications (no id) are used for telemetry streams:
//! `{"jsonrpc":"2.0","method":"hcp.telemetry","params":{"ecc_errors":0}}`

use serde::{Deserialize, Serialize};

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    /// None = notification (no response expected)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
}

impl JsonRpcRequest {
    /// Create a new request.
    pub fn new(method: &str, params: serde_json::Value, id: u64) -> Self {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: Some(id),
        }
    }

    /// Create a notification (no response expected).
    pub fn notification(method: &str, params: serde_json::Value) -> Self {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: None,
        }
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<u64>,
}

impl JsonRpcResponse {
    /// Create a success response.
    pub fn success(id: u64, result: serde_json::Value) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id: Some(id),
        }
    }

    /// Create an error response.
    pub fn error(id: u64, code: i32, message: &str) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
            id: Some(id),
        }
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Standard JSON-RPC error codes.
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    /// HCP-specific error codes (application-defined, -32000 to -32099)
    pub const IMAGE_NOT_FOUND: i32 = -32000;
    pub const TARGET_UNAVAILABLE: i32 = -32001;
    pub const ECC_VERIFICATION_FAILED: i32 = -32002;
    pub const DEPLOYMENT_FAILED: i32 = -32003;
    pub const INTEGRITY_CHECK_FAILED: i32 = -32004;
}

/// Route a JSON-RPC request string to a handler and return a response string.
///
/// This is the core dispatch function. It parses the JSON, finds the method,
/// calls the handler, and serializes the response. Transport-agnostic —
/// works over stdin/stdout, HTTP, TCP, WebSocket, or anything else.
pub fn dispatch(
    request_json: &str,
    handler: &dyn Fn(&str, &serde_json::Value) -> Result<serde_json::Value, (i32, String)>,
) -> Option<String> {
    // Parse the request
    let request: JsonRpcRequest = match serde_json::from_str(request_json) {
        Ok(r) => r,
        Err(e) => {
            let resp = JsonRpcResponse::error(0, error_codes::PARSE_ERROR, &e.to_string());
            return Some(serde_json::to_string(&resp).unwrap());
        }
    };

    // Notifications don't get responses
    let id = match request.id {
        Some(id) => id,
        None => return None,
    };

    // Call the handler
    let response = match handler(&request.method, &request.params) {
        Ok(result) => JsonRpcResponse::success(id, result),
        Err((code, msg)) => JsonRpcResponse::error(id, code, &msg),
    };

    Some(serde_json::to_string(&response).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new("hcp.list_images", serde_json::json!({}), 1);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"hcp.list_images\""));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn test_response_serialization() {
        let resp = JsonRpcResponse::success(1, serde_json::json!({"count": 5}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_error_response() {
        let resp = JsonRpcResponse::error(1, error_codes::IMAGE_NOT_FOUND, "no such image");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32000"));
        assert!(json.contains("no such image"));
    }

    #[test]
    fn test_dispatch_success() {
        let handler = |method: &str, _params: &serde_json::Value| -> Result<serde_json::Value, (i32, String)> {
            if method == "hcp.ping" {
                Ok(serde_json::json!({"status": "ok"}))
            } else {
                Err((error_codes::METHOD_NOT_FOUND, format!("unknown: {}", method)))
            }
        };

        let req = r#"{"jsonrpc":"2.0","method":"hcp.ping","params":{},"id":1}"#;
        let resp = dispatch(req, &handler).unwrap();
        assert!(resp.contains("\"status\":\"ok\""));
    }

    #[test]
    fn test_dispatch_method_not_found() {
        let handler = |_method: &str, _params: &serde_json::Value| -> Result<serde_json::Value, (i32, String)> {
            Err((error_codes::METHOD_NOT_FOUND, "unknown".to_string()))
        };

        let req = r#"{"jsonrpc":"2.0","method":"hcp.nonexistent","params":{},"id":2}"#;
        let resp = dispatch(req, &handler).unwrap();
        assert!(resp.contains("-32601"));
    }

    #[test]
    fn test_dispatch_notification_no_response() {
        let handler = |_method: &str, _params: &serde_json::Value| -> Result<serde_json::Value, (i32, String)> {
            Ok(serde_json::json!({}))
        };

        // No "id" field = notification = no response
        let req = r#"{"jsonrpc":"2.0","method":"hcp.telemetry","params":{}}"#;
        let resp = dispatch(req, &handler);
        assert!(resp.is_none());
    }
}

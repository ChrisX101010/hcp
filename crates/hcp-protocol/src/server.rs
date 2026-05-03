use crate::registry::ImageRegistry;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub async fn run_server(registry: Arc<ImageRegistry>, bind: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    tracing::info!("HCP server listening on {}", bind);
    loop {
        let (mut socket, _) = listener.accept().await?;
        let reg = registry.clone();
        tokio::spawn(async move {
            let mut buf = [0; 1024];
            if let Ok(n) = socket.read(&mut buf).await {
                if let Ok(req) = serde_json::from_slice::<Value>(&buf[..n]) {
                    let method = req["method"].as_str().unwrap_or("");
                    let id = &req["id"];
                    let resp = match method {
                        "hcp.ping" => json!({"jsonrpc":"2.0","result":{"server":"hcp-server","status":"ok"},"id":id}),
                        "hcp.list" => {
                            let imgs = reg.list().iter().map(|i| format!("{}:{} by Hristo — 3 target(s), {} ECC signal(s)", i.name, i.version, i.ecc_signals)).collect::<Vec<_>>();
                            json!({"jsonrpc":"2.0","result":{"images":imgs},"id":id})
                        }
                        _ => json!({"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":id})
                    };
                    let out = serde_json::to_string(&resp).unwrap();
                    let _ = socket.write_all(out.as_bytes()).await;
                }
            }
        });
    }
}

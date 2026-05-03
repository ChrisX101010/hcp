use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde_json::{json, Value};

pub async fn connect_and_ping(addr: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(addr).await?;
    let req = json!({"jsonrpc":"2.0","method":"hcp.ping","params":{},"id":1});
    stream.write_all(format!("{}\n", req).as_bytes()).await?;
    let mut buf = [0; 1024];
    let n = stream.read(&mut buf).await?;
    let resp: Value = serde_json::from_slice(&buf[..n])?;
    Ok(resp["result"]["status"].as_str().unwrap_or("unknown").to_string())
}

pub async fn list_images(addr: &str) -> std::io::Result<Vec<String>> {
    let mut stream = TcpStream::connect(addr).await?;
    let req = json!({"jsonrpc":"2.0","method":"hcp.list","params":{},"id":2});
    stream.write_all(format!("{}\n", req).as_bytes()).await?;
    let mut buf = [0; 1024];
    let n = stream.read(&mut buf).await?;
    let resp: Value = serde_json::from_slice(&buf[..n])?;
    let imgs: Vec<String> = resp["result"]["images"].as_array().unwrap_or(&vec![]).iter()
        .filter_map(|v| v.as_str().map(String::from)).collect();
    Ok(imgs)
}

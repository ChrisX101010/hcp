use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Connection closed")]
    Closed,
    #[error("Invalid message length: {0}")]
    InvalidLength(usize),
}

pub type Result<T> = std::result::Result<T, TransportError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub to: Option<String>,
    #[serde(flatten)]
    pub payload: serde_json::Value,
    pub ttl: u8,
}

impl Message {
    pub fn new(from: &str, payload: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from: from.to_string(),
            to: None,
            payload,
            ttl: 5,
        }
    }
    pub fn directed(to: &str, from: &str, payload: serde_json::Value) -> Self {
        let mut m = Self::new(from, payload);
        m.to = Some(to.to_string());
        m
    }
}

pub struct PeerConnection {
    stream: TcpStream,
    peer_id: String,
}

impl PeerConnection {
    pub async fn connect(addr: &str, peer_id: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self {
            stream,
            peer_id: peer_id.to_string(),
        })
    }

    // NEW: Constructor for wrapping an existing stream (for incoming connections)
    pub fn from_stream(stream: TcpStream, peer_id: &str) -> Self {
        Self {
            stream,
            peer_id: peer_id.to_string(),
        }
    }

    pub async fn send(&mut self, msg: &Message) -> Result<()> {
        let json = serde_json::to_vec(msg)?;
        let len = json.len() as u32;
        self.stream.write_all(&len.to_be_bytes()).await?;
        self.stream.write_all(&json).await?;
        self.stream.flush().await?;
        Ok(())
    }
    pub async fn recv(&mut self) -> Result<Message> {
        let mut len_buf = [0u8; 4];
        match self.stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(TransportError::Closed)
            }
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 || len > 10_000_000 {
            return Err(TransportError::InvalidLength(len));
        }
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).await?;
        Ok(serde_json::from_slice(&buf)?)
    }
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

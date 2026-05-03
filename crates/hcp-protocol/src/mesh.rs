use crate::registry::ImageRegistry;
use crate::transport::{Message, PeerConnection, TransportError};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};
use tracing::{info, warn, debug, error};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAdvertisement {
    pub node_id: String,
    pub hostname: String,
    pub port: u16,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    Announce { image_ref: String, digest: String },
    Request { image_ref: String },
    Respond { image_ref: String, manifest: serde_json::Value },
    Heartbeat { load: f32 },
}

pub struct MeshNode {
    pub id: String,
    #[allow(dead_code)]
    port: u16,
    #[allow(dead_code)]
    registry: Arc<ImageRegistry>,
    peers: Arc<Mutex<HashMap<String, PeerState>>>,
    tx: broadcast::Sender<Message>,
}

#[allow(dead_code)]
struct PeerState {
    addr: SocketAddr,
    last_seen: std::time::Instant,
}

impl MeshNode {
    pub fn new(port: u16, registry: Arc<ImageRegistry>) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            id: Uuid::new_v4().to_string(),
            port,
            registry,
            peers: Arc::new(Mutex::new(HashMap::new())),
            tx,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let node = Arc::new(self);
        let listener = TcpListener::bind(("0.0.0.0", node.port)).await?;
        info!("HCP mesh listening on 0.0.0.0:{}", node.port);

        let adv = node.clone();
        tokio::spawn(async move { if let Err(e) = adv.advertise().await { error!("mDNS advertise: {}", e); } });

        let disc = node.clone();
        tokio::spawn(async move { if let Err(e) = disc.discover().await { error!("mDNS discover: {}", e); } });

        let acc = node.clone();
        tokio::spawn(async move {
            loop {
                if let Ok((stream, addr)) = listener.accept().await {
                    let n = acc.clone();
                    tokio::spawn(async move { if let Err(e) = n.handle_incoming(stream, addr).await { warn!("Peer {} disconnected: {}", addr, e); } });
                }
            }
        });

        let gossip = node.clone();
        tokio::spawn(async move { gossip.run_gossip().await });

        tokio::signal::ctrl_c().await?;
        info!("Shutting down mesh node {}...", node.id);
        Ok(())
    }

    async fn advertise(&self) -> anyhow::Result<()> {
        let daemon = ServiceDaemon::new()?;
        let safe_host = "hcp-node.local.";
        let sys_hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "localhost".to_string());

        let advert = NodeAdvertisement {
            node_id: self.id.clone(),
            hostname: sys_hostname,
            port: self.port,
            capabilities: vec!["hcp-node".to_string()],
        };

        let txt: HashMap<String, String> = [("advert".to_string(), serde_json::to_string(&advert)?)].into();
        let ip: IpAddr = Ipv4Addr::LOCALHOST.into();

        let service = ServiceInfo::new(
            "_hcp._tcp.local.",
            &self.id,
            safe_host,
            ip,
            self.port,
            txt,
        )?.enable_addr_auto();

        daemon.register(service)?;
        info!("Advertised on mDNS: {} @ {}", self.id, safe_host);
        Ok(())
    }

    async fn discover(&self) -> anyhow::Result<()> {
        let daemon = ServiceDaemon::new()?;
        let browser = daemon.browse("_hcp._tcp.local.")?;
        let mut seen = HashSet::new();
        loop {
            match browser.recv_timeout(Duration::from_secs(1)) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    // FIX: Use the actual resolved network IP, not the local hostname
                    if let Some(ip) = info.get_addresses().iter().next() {
                        if let Some(prop) = info.get_properties().get("advert") {
                            if let Some(val_bytes) = prop.val() {
                                if let Ok(val_str) = std::str::from_utf8(val_bytes) {
                                    if let Ok(mut adv) = serde_json::from_str::<NodeAdvertisement>(val_str) {
                                        adv.hostname = ip.to_string(); // Override with real IP
                                        if adv.node_id != self.id && !seen.contains(&adv.node_id) {
                                            seen.insert(adv.node_id.clone());
                                            let _ = self.connect_to_peer(&adv).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(flu) => { if matches!(flu, flume::RecvTimeoutError::Disconnected) { break; } }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    async fn connect_to_peer(&self, adv: &NodeAdvertisement) -> anyhow::Result<()> {
        let addr = format!("{}:{}", adv.hostname, adv.port);
        info!("Connecting to peer {} at {}", adv.node_id, addr);
        let mut conn = match PeerConnection::connect(&addr, &adv.node_id).await {
            Ok(c) => c,
            Err(e) => { warn!("Failed to connect to {}: {}", addr, e); return Ok(()); }
        };
        let hello = Message::new(&self.id, serde_json::json!({"type":"hello","node_id":self.id}));
        if let Err(e) = conn.send(&hello).await { warn!("Failed hello to {}: {}", adv.node_id, e); return Ok(()); }
        {
            let mut p = self.peers.lock().await;
            p.insert(adv.node_id.clone(), PeerState { addr: addr.parse().unwrap_or_else(|_| "127.0.0.1:0".parse().unwrap()), last_seen: std::time::Instant::now() });
        }
        info!("Connected to peer: {}", adv.node_id);
        let tx = self.tx.clone();
        let peers = self.peers.clone();
        let pid = adv.node_id.clone();
        tokio::spawn(async move {
            loop {
                match tokio::time::timeout(Duration::from_secs(30), conn.recv()).await {
                    Ok(Ok(msg)) => {
                        if msg.payload.get("type").and_then(|v| v.as_str()) == Some("heartbeat") {
                            if let Ok(mut p) = peers.try_lock() { if let Some(peer) = p.get_mut(&pid) { peer.last_seen = std::time::Instant::now(); } }
                            continue;
                        }
                        let _ = tx.send(msg);
                    }
                    Ok(Err(TransportError::Closed)) => break,
                    Ok(Err(e)) => { warn!("Error from {}: {}", pid, e); break; }
                    Err(_) => {
                        let hb = Message::new(&pid, serde_json::json!({"type":"heartbeat"}));
                        if conn.send(&hb).await.is_err() { break; }
                    }
                }
            }
            if let Ok(mut p) = peers.try_lock() { p.remove(&pid); }
            info!("Disconnected from peer: {}", pid);
        });
        Ok(())
    }

    async fn handle_incoming(&self, stream: tokio::net::TcpStream, addr: SocketAddr) -> Result<(), TransportError> {
        let peer_id = "pending".to_string();
        let mut conn = PeerConnection::from_stream(stream, &peer_id);
        let first = conn.recv().await?;
        if first.payload.get("type").and_then(|v| v.as_str()) != Some("hello") { return Err(TransportError::Closed); }
        let peer_id = first.from.clone();
        let hello = Message::new(&self.id, serde_json::json!({"type":"hello","node_id":self.id}));
        conn.send(&hello).await?;
        info!("Peer connected: {} from {}", peer_id, addr);
        {
            let mut p = self.peers.lock().await;
            p.insert(peer_id.clone(), PeerState { addr, last_seen: std::time::Instant::now() });
        }
        let tx = self.tx.clone();
        let peers = self.peers.clone();
        let pid = peer_id.clone();
        let stream_inner = conn.into_inner();
        tokio::spawn(async move {
            let mut conn = PeerConnection::from_stream(stream_inner, &pid);
            loop {
                match tokio::time::timeout(Duration::from_secs(30), conn.recv()).await {
                    Ok(Ok(msg)) => {
                        if msg.payload.get("type").and_then(|v| v.as_str()) == Some("heartbeat") {
                            if let Ok(mut p) = peers.try_lock() { if let Some(peer) = p.get_mut(&pid) { peer.last_seen = std::time::Instant::now(); } }
                            continue;
                        }
                        let _ = tx.send(msg);
                    }
                    Ok(Err(TransportError::Closed)) => break,
                    Ok(Err(e)) => { warn!("Error from {}: {}", pid, e); break; }
                    Err(_) => {
                        let hb = Message::new(&pid, serde_json::json!({"type":"heartbeat"}));
                        if conn.send(&hb).await.is_err() { break; }
                    }
                }
            }
            if let Ok(mut p) = peers.try_lock() { p.remove(&pid); }
        });
        Ok(())
    }

    async fn run_gossip(&self) {
        let mut rx = self.tx.subscribe();
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if msg.from != self.id && msg.ttl > 0 {
                        let peers = self.peers.lock().await;
                        for (id, _) in peers.iter() {
                            if msg.to.as_ref().map(|t| t == id).unwrap_or(true) {
                                debug!("Would route {} to {}", msg.id, id);
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(_) => continue,
            }
        }
    }

    pub async fn send_message(&self, to: &str, payload: serde_json::Value) -> anyhow::Result<()> {
        let msg = Message::directed(to, &self.id, payload);
        self.tx.send(msg)?;
        Ok(())
    }

    pub async fn list_peers(&self) -> Vec<String> {
        self.peers.lock().await.keys().cloned().collect()
    }
}

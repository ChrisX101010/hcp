//! HCP Protocol — JSON-RPC, registry, transport, and P2P mesh

pub mod client;
pub mod mesh;
pub mod registry;
pub mod server;
pub mod transport;

pub use client::{connect_and_ping, list_images};
pub use mesh::{GossipMessage, MeshNode, NodeAdvertisement};
pub use registry::{HardwareImage, ImageRegistry};
pub use server::run_server;
pub use transport::{Message, PeerConnection, TransportError};

//! HCP Protocol — JSON-RPC, registry, transport, and P2P mesh

pub mod registry;
pub mod server;
pub mod client;
pub mod transport;
pub mod mesh;

pub use registry::{HardwareImage, ImageRegistry};
pub use server::run_server;
pub use client::{connect_and_ping, list_images};
pub use transport::{Message, PeerConnection, TransportError};
pub use mesh::{MeshNode, NodeAdvertisement, GossipMessage};

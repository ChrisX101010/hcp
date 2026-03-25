//! # HCP Protocol — The Radio Tower
//!
//! This crate defines the HCP protocol: how clients and servers communicate
//! to share, discover, and deploy hardware images over the network.
//!
//! ## Architecture (mirrors MCP exactly)
//!
//! MCP uses JSON-RPC 2.0 to connect AI models to tools.
//! HCP uses JSON-RPC 2.0 to connect hardware consumers to hardware providers.
//!
//! ```text
//!  HCP Client                          HCP Server
//! ┌──────────────┐                   ┌──────────────────┐
//! │ hcp pull     │  ── JSON-RPC ──▶  │ Image Registry   │
//! │ hcp deploy   │  ── JSON-RPC ──▶  │ FPGA Manager     │
//! │ hcp telemetry│  ◀── Stream ───   │ Telemetry Engine │
//! └──────────────┘                   └──────────────────┘
//! ```
//!
//! ## Protocol Primitives (like MCP)
//!
//! MCP has: Resources, Tools, Prompts, Sampling
//! HCP has: Resources, Tools, Targets, Streams
//!
//! - **Resources**: Hardware images, IP cores, pin maps, memory layouts
//! - **Tools**: Synthesize, simulate, flash, verify ECC, run tests
//! - **Targets**: Available FPGAs, simulators, emulators
//! - **Streams**: Real-time telemetry (clock, temp, ECC errors)
//!
//! ## Transport
//!
//! Phase 3a (this release): In-process — client and server in same binary.
//! Phase 3b (next): TCP + JSON-RPC over HTTP (like MCP's Streamable HTTP).
//! Phase 3c (later): mTLS for authentication, mDNS for discovery.

pub mod jsonrpc;
pub mod messages;
pub mod server;
pub mod client;
pub mod registry;

pub use jsonrpc::*;
pub use messages::*;
pub use server::HcpServer;
pub use client::HcpClient;
pub use registry::ImageRegistry;

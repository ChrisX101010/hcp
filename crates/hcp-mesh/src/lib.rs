//! # hcp-mesh
//!
//! A live view of *your* HCP mesh: which nodes are online, what fabric each is
//! offering, how they're linked, and which are trusted. This is the honest,
//! consent-based answer to "show me everything in one place."
//!
//! Crucially, this crate discovers **nothing on its own.** It aggregates only
//! what nodes have already chosen to broadcast — the `FabricCapability`
//! advertisements defined in `hcp-fabric`, which a node emits voluntarily to say
//! "I have spare fabric and I'm willing to host." A node that never advertises
//! never appears here. There is no scanning, no probing, no fingerprinting of
//! devices that didn't opt in. The map is built from consent, exactly like the
//! rest of HCP.
//!
//! Think of it as the `networkx`-style topology layer for a fleet you own,
//! not a discovery tool for networks you don't.

#![forbid(unsafe_code)]

mod graph;
mod registry;

pub use graph::{Link, LinkQuality, MeshGraph, NodeRef};
pub use registry::{MeshRegistry, NodeStatus, TrustState};

/// How long, in the registry's arbitrary tick units, before a node with no
/// fresh advertisement is considered stale (offline).
pub const DEFAULT_STALE_AFTER: u64 = 30;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert_eq!(super::DEFAULT_STALE_AFTER, 30);
    }
}

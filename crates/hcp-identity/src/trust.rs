//! A trust store: the set of node identities this node has chosen to enroll.
//!
//! Consent has two layers in HCP. The wire protocol (`hcp-fabric`) guarantees a
//! node is only a target if it advertised and accepted. This store adds the
//! second: *whose* offers a node is even willing to consider. A node placed in
//! the store is a peer you've paired with; everything else is a stranger whose
//! offers are declined before resource checks run.
//!
//! This is deliberately simple — an allowlist keyed by public identity. It is
//! the data structure a pairing/commissioning flow populates: pairing = "add
//! the other node's `NodeId` to my `TrustStore`, and mine to theirs."

use crate::NodeId;
use std::collections::HashMap;

/// Why a trust check failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustError {
    /// The identity is not enrolled in this store.
    Unknown,
    /// The identity is enrolled but has been revoked.
    Revoked,
}

impl core::fmt::Display for TrustError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TrustError::Unknown => write!(f, "identity is not enrolled"),
            TrustError::Revoked => write!(f, "identity has been revoked"),
        }
    }
}

impl std::error::Error for TrustError {}

/// A human label + status attached to an enrolled identity.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Peer {
    label: String,
    revoked: bool,
}

/// An allowlist of trusted node identities.
#[derive(Debug, Default)]
pub struct TrustStore {
    peers: HashMap<[u8; 32], Peer>,
}

impl TrustStore {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
        }
    }

    /// Enroll a peer under a human-readable label (idempotent; re-enrolling
    /// clears a prior revocation and updates the label).
    pub fn enroll(&mut self, id: NodeId, label: impl Into<String>) {
        self.peers.insert(
            id.to_bytes(),
            Peer {
                label: label.into(),
                revoked: false,
            },
        );
    }

    /// Revoke a peer without forgetting it — future checks return `Revoked`,
    /// which is more informative to an operator than silently forgetting.
    pub fn revoke(&mut self, id: &NodeId) {
        if let Some(p) = self.peers.get_mut(&id.to_bytes()) {
            p.revoked = true;
        }
    }

    /// Forget a peer entirely.
    pub fn remove(&mut self, id: &NodeId) {
        self.peers.remove(&id.to_bytes());
    }

    /// Is this identity currently trusted?
    pub fn check(&self, id: &NodeId) -> Result<(), TrustError> {
        match self.peers.get(&id.to_bytes()) {
            None => Err(TrustError::Unknown),
            Some(p) if p.revoked => Err(TrustError::Revoked),
            Some(_) => Ok(()),
        }
    }

    /// Convenience boolean.
    pub fn is_trusted(&self, id: &NodeId) -> bool {
        self.check(id).is_ok()
    }

    /// Label for an enrolled peer, if any.
    pub fn label(&self, id: &NodeId) -> Option<&str> {
        self.peers.get(&id.to_bytes()).map(|p| p.label.as_str())
    }

    /// Number of enrolled peers (including revoked).
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKeypair;

    #[test]
    fn enroll_then_trust() {
        let peer = NodeKeypair::generate().id();
        let mut store = TrustStore::new();
        assert_eq!(store.check(&peer), Err(TrustError::Unknown));
        store.enroll(peer, "workshop-fpga-1");
        assert!(store.is_trusted(&peer));
        assert_eq!(store.label(&peer), Some("workshop-fpga-1"));
    }

    #[test]
    fn revoke_is_distinct_from_unknown() {
        let peer = NodeKeypair::generate().id();
        let mut store = TrustStore::new();
        store.enroll(peer, "old-node");
        store.revoke(&peer);
        assert_eq!(store.check(&peer), Err(TrustError::Revoked));
    }

    #[test]
    fn remove_forgets_entirely() {
        let peer = NodeKeypair::generate().id();
        let mut store = TrustStore::new();
        store.enroll(peer, "x");
        store.remove(&peer);
        assert_eq!(store.check(&peer), Err(TrustError::Unknown));
    }

    #[test]
    fn re_enroll_clears_revocation() {
        let peer = NodeKeypair::generate().id();
        let mut store = TrustStore::new();
        store.enroll(peer, "x");
        store.revoke(&peer);
        store.enroll(peer, "x-again");
        assert!(store.is_trusted(&peer));
        assert_eq!(store.label(&peer), Some("x-again"));
    }

    #[test]
    fn strangers_are_not_trusted() {
        let mut store = TrustStore::new();
        store.enroll(NodeKeypair::generate().id(), "friend");
        let stranger = NodeKeypair::generate().id();
        assert!(!store.is_trusted(&stranger));
    }
}

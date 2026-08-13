//! The registry: the running picture of nodes that have advertised themselves.

use hcp_fabric::{FabricCapability, FabricClass, ResourceBudget};
use std::collections::HashMap;

/// Whether a node is currently considered reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Online,
    Stale,
}

/// Trust relationship to a node, mirrored from an identity trust store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustState {
    /// Enrolled and trusted.
    Trusted,
    /// Seen advertising but not enrolled — a stranger.
    Untrusted,
    /// Enrolled then revoked.
    Revoked,
}

/// One node's aggregated record.
#[derive(Debug, Clone)]
pub struct NodeRecord {
    pub node_id: [u8; 32],
    pub class: FabricClass,
    pub total: ResourceBudget,
    pub free: ResourceBudget,
    pub max_image_bytes: u32,
    /// Highest advertisement epoch seen from this node (newer supersedes older).
    pub epoch: u64,
    /// Registry tick when we last heard from it.
    pub last_seen: u64,
    pub trust: TrustState,
    /// Optional human label carried over from the trust store.
    pub label: Option<String>,
}

impl NodeRecord {
    /// Short hex id for display.
    pub fn short(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}",
            self.node_id[0], self.node_id[1], self.node_id[2], self.node_id[3]
        )
    }
}

/// Aggregates node advertisements into a live registry. Consumes only what
/// nodes broadcast; performs no discovery itself.
pub struct MeshRegistry {
    nodes: HashMap<[u8; 32], NodeRecord>,
    now: u64,
    stale_after: u64,
}

impl MeshRegistry {
    pub fn new(stale_after: u64) -> Self {
        MeshRegistry {
            nodes: HashMap::new(),
            now: 0,
            stale_after,
        }
    }

    /// Advance the registry clock (called on a timer by the host).
    pub fn tick(&mut self) {
        self.now += 1;
    }

    /// Current tick.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Ingest an advertisement a node voluntarily broadcast. Older epochs are
    /// ignored (anti-stale-replay); newer ones refresh the record. Trust is set
    /// to `Untrusted` by default; call [`apply_trust`](Self::apply_trust) to
    /// reflect the operator's trust store.
    pub fn observe(&mut self, cap: &FabricCapability) {
        let entry = self.nodes.entry(cap.node_id).or_insert(NodeRecord {
            node_id: cap.node_id,
            class: cap.class,
            total: cap.total,
            free: cap.free,
            max_image_bytes: cap.max_image_bytes,
            epoch: 0,
            last_seen: self.now,
            trust: TrustState::Untrusted,
            label: None,
        });

        // Only accept advertisements at least as new as what we have.
        if cap.epoch >= entry.epoch {
            entry.class = cap.class;
            entry.total = cap.total;
            entry.free = cap.free;
            entry.max_image_bytes = cap.max_image_bytes;
            entry.epoch = cap.epoch;
            entry.last_seen = self.now;
        }
    }

    /// Set a node's trust state and optional label (from the identity layer's
    /// trust store). Nodes not yet observed are created as records so trust can
    /// be pre-seeded before they advertise.
    pub fn apply_trust(&mut self, node_id: [u8; 32], trust: TrustState, label: Option<String>) {
        if let Some(rec) = self.nodes.get_mut(&node_id) {
            rec.trust = trust;
            rec.label = label;
        }
    }

    /// Status of a node given the current clock.
    pub fn status(&self, node_id: &[u8; 32]) -> Option<NodeStatus> {
        self.nodes.get(node_id).map(|r| {
            if self.now.saturating_sub(r.last_seen) > self.stale_after {
                NodeStatus::Stale
            } else {
                NodeStatus::Online
            }
        })
    }

    /// All known node records.
    pub fn nodes(&self) -> impl Iterator<Item = &NodeRecord> {
        self.nodes.values()
    }

    /// Nodes currently online.
    pub fn online(&self) -> Vec<&NodeRecord> {
        self.nodes
            .values()
            .filter(|r| self.now.saturating_sub(r.last_seen) <= self.stale_after)
            .collect()
    }

    /// Trusted nodes that can currently host a design of the given size and
    /// footprint — the "where could I place this?" query. Only online, trusted,
    /// class-matching nodes with room are returned.
    pub fn candidates_for(
        &self,
        class: FabricClass,
        image_bytes: u32,
        footprint: &ResourceBudget,
    ) -> Vec<&NodeRecord> {
        self.online()
            .into_iter()
            .filter(|r| {
                r.trust == TrustState::Trusted
                    && r.class == class
                    && image_bytes <= r.max_image_bytes
                    && r.free.fits(footprint)
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(id: u8, epoch: u64, free_bram: u32) -> FabricCapability {
        FabricCapability {
            node_id: [id; 32],
            class: FabricClass::Ice40,
            total: ResourceBudget::new(7680, 7680, 32),
            free: ResourceBudget::new(7680, 7680, free_bram),
            max_image_bytes: 65536,
            epoch,
        }
    }

    #[test]
    fn observe_then_present() {
        let mut reg = MeshRegistry::new(30);
        reg.observe(&cap(1, 1, 32));
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.status(&[1u8; 32]), Some(NodeStatus::Online));
    }

    #[test]
    fn newer_epoch_supersedes_older() {
        let mut reg = MeshRegistry::new(30);
        reg.observe(&cap(1, 5, 20));
        reg.observe(&cap(1, 3, 99)); // older epoch — ignored
        let rec = reg.nodes().next().unwrap();
        assert_eq!(rec.epoch, 5);
        assert_eq!(rec.free.brams, 20);
    }

    #[test]
    fn nodes_go_stale() {
        let mut reg = MeshRegistry::new(5);
        reg.observe(&cap(1, 1, 32));
        for _ in 0..6 {
            reg.tick();
        }
        assert_eq!(reg.status(&[1u8; 32]), Some(NodeStatus::Stale));
        assert!(reg.online().is_empty());
    }

    #[test]
    fn refreshed_node_comes_back_online() {
        let mut reg = MeshRegistry::new(5);
        reg.observe(&cap(1, 1, 32));
        for _ in 0..6 {
            reg.tick();
        }
        assert_eq!(reg.status(&[1u8; 32]), Some(NodeStatus::Stale));
        reg.observe(&cap(1, 2, 32)); // fresh advert
        assert_eq!(reg.status(&[1u8; 32]), Some(NodeStatus::Online));
    }

    #[test]
    fn candidates_respect_trust_class_and_room() {
        let mut reg = MeshRegistry::new(30);
        reg.observe(&cap(1, 1, 32)); // roomy
        reg.observe(&cap(2, 1, 4)); // too little BRAM for a 20-BRAM design
        reg.apply_trust([1u8; 32], TrustState::Trusted, Some("fpga-1".into()));
        reg.apply_trust([2u8; 32], TrustState::Trusted, None);

        let footprint = ResourceBudget::new(971, 399, 20);
        let cands = reg.candidates_for(FabricClass::Ice40, 4096, &footprint);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].node_id, [1u8; 32]);
    }

    #[test]
    fn untrusted_nodes_are_not_candidates() {
        let mut reg = MeshRegistry::new(30);
        reg.observe(&cap(1, 1, 32)); // roomy but never trusted
        let footprint = ResourceBudget::new(971, 399, 20);
        assert!(reg
            .candidates_for(FabricClass::Ice40, 4096, &footprint)
            .is_empty());
    }
}

//! Topology and routing over the mesh — the `networkx`-style layer.
//!
//! Nodes learn of links from the transport (e.g. "I can hear node X at RSSI
//! -95"). This module turns those observed links into a graph and answers the
//! two questions a fleet operator actually asks: *is the mesh connected?* and
//! *what's the best path to get a design from A to B?* — where "best" weights by
//! link quality, since a design shipped over three good hops beats one bad one.

use std::collections::{BinaryHeap, HashMap, HashSet};

/// A node key in the graph (its 32-byte identity).
pub type NodeRef = [u8; 32];

/// Link quality, from a transport-provided signal metric. Higher is better;
/// callers typically map RSSI/SNR into 0..=100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkQuality(pub u8);

impl LinkQuality {
    /// Convert quality into a routing cost (lower is better). A perfect link
    /// (100) costs 1; a poor link costs much more, so routing avoids it.
    fn cost(self) -> u32 {
        // cost = 1 + (100 - q); q=100 -> 1, q=0 -> 101
        1 + (100u32.saturating_sub(self.0 as u32))
    }
}

/// An undirected link between two nodes with a quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub a: NodeRef,
    pub b: NodeRef,
    pub quality: LinkQuality,
}

/// The mesh topology graph.
#[derive(Default)]
pub struct MeshGraph {
    // adjacency: node -> (neighbor -> quality)
    adj: HashMap<NodeRef, HashMap<NodeRef, LinkQuality>>,
}

impl MeshGraph {
    pub fn new() -> Self {
        MeshGraph {
            adj: HashMap::new(),
        }
    }

    /// Add or update an undirected link. Re-adding updates the quality.
    pub fn add_link(&mut self, a: NodeRef, b: NodeRef, quality: LinkQuality) {
        self.adj.entry(a).or_default().insert(b, quality);
        self.adj.entry(b).or_default().insert(a, quality);
    }

    /// Remove a link (e.g. the transport reports it dropped).
    pub fn remove_link(&mut self, a: &NodeRef, b: &NodeRef) {
        if let Some(n) = self.adj.get_mut(a) {
            n.remove(b);
        }
        if let Some(n) = self.adj.get_mut(b) {
            n.remove(a);
        }
    }

    pub fn node_count(&self) -> usize {
        self.adj.len()
    }

    pub fn neighbors(&self, n: &NodeRef) -> Vec<(NodeRef, LinkQuality)> {
        self.adj
            .get(n)
            .map(|m| m.iter().map(|(k, v)| (*k, *v)).collect())
            .unwrap_or_default()
    }

    /// Is every node reachable from `start`? (Connectivity check for the fleet.)
    pub fn is_connected_from(&self, start: &NodeRef) -> bool {
        if self.adj.is_empty() {
            return true;
        }
        if !self.adj.contains_key(start) {
            return false;
        }
        let mut seen = HashSet::new();
        let mut stack = vec![*start];
        while let Some(n) = stack.pop() {
            if seen.insert(n) {
                for (nb, _) in self.neighbors(&n) {
                    if !seen.contains(&nb) {
                        stack.push(nb);
                    }
                }
            }
        }
        seen.len() == self.adj.len()
    }

    /// Best-quality path from `src` to `dst`, as a list of nodes including both
    /// ends. Uses Dijkstra over link cost (poor links cost more). Returns
    /// `None` if unreachable.
    pub fn best_path(&self, src: &NodeRef, dst: &NodeRef) -> Option<Vec<NodeRef>> {
        if !self.adj.contains_key(src) || !self.adj.contains_key(dst) {
            return None;
        }
        if src == dst {
            return Some(vec![*src]);
        }

        // Dijkstra
        let mut dist: HashMap<NodeRef, u32> = HashMap::new();
        let mut prev: HashMap<NodeRef, NodeRef> = HashMap::new();
        let mut heap = BinaryHeap::new();

        dist.insert(*src, 0);
        heap.push(State {
            cost: 0,
            node: *src,
        });

        while let Some(State { cost, node }) = heap.pop() {
            if node == *dst {
                // reconstruct
                let mut path = vec![*dst];
                let mut cur = *dst;
                while let Some(p) = prev.get(&cur) {
                    path.push(*p);
                    cur = *p;
                }
                path.reverse();
                return Some(path);
            }
            if cost > *dist.get(&node).unwrap_or(&u32::MAX) {
                continue;
            }
            for (nb, q) in self.neighbors(&node) {
                let next = cost + q.cost();
                if next < *dist.get(&nb).unwrap_or(&u32::MAX) {
                    dist.insert(nb, next);
                    prev.insert(nb, node);
                    heap.push(State {
                        cost: next,
                        node: nb,
                    });
                }
            }
        }
        None
    }
}

// min-heap ordering for Dijkstra
#[derive(PartialEq, Eq)]
struct State {
    cost: u32,
    node: NodeRef,
}
impl Ord for State {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // reverse for min-heap
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.node.cmp(&other.node))
    }
}
impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> NodeRef {
        [n; 32]
    }

    #[test]
    fn single_link_connectivity() {
        let mut g = MeshGraph::new();
        g.add_link(id(1), id(2), LinkQuality(90));
        assert!(g.is_connected_from(&id(1)));
        assert_eq!(g.node_count(), 2);
    }

    #[test]
    fn disconnected_component_detected() {
        let mut g = MeshGraph::new();
        g.add_link(id(1), id(2), LinkQuality(90));
        g.add_link(id(3), id(4), LinkQuality(90)); // separate island
        assert!(!g.is_connected_from(&id(1)));
    }

    #[test]
    fn best_path_prefers_quality() {
        let mut g = MeshGraph::new();
        // direct link A-D is poor; A-B-C-D is three great hops
        g.add_link(id(1), id(4), LinkQuality(5)); // cost 96
        g.add_link(id(1), id(2), LinkQuality(100)); // cost 1
        g.add_link(id(2), id(3), LinkQuality(100)); // cost 1
        g.add_link(id(3), id(4), LinkQuality(100)); // cost 1  (total 3 < 96)
        let path = g.best_path(&id(1), &id(4)).unwrap();
        assert_eq!(path, vec![id(1), id(2), id(3), id(4)]);
    }

    #[test]
    fn best_path_takes_direct_when_good() {
        let mut g = MeshGraph::new();
        g.add_link(id(1), id(2), LinkQuality(100)); // cost 1
        g.add_link(id(1), id(3), LinkQuality(100));
        g.add_link(id(3), id(2), LinkQuality(100)); // detour cost 2
        let path = g.best_path(&id(1), &id(2)).unwrap();
        assert_eq!(path, vec![id(1), id(2)]);
    }

    #[test]
    fn unreachable_returns_none() {
        let mut g = MeshGraph::new();
        g.add_link(id(1), id(2), LinkQuality(90));
        g.add_link(id(3), id(4), LinkQuality(90));
        assert!(g.best_path(&id(1), &id(4)).is_none());
    }

    #[test]
    fn removing_a_link_updates_topology() {
        let mut g = MeshGraph::new();
        g.add_link(id(1), id(2), LinkQuality(90));
        g.add_link(id(2), id(3), LinkQuality(90));
        assert!(g.best_path(&id(1), &id(3)).is_some());
        g.remove_link(&id(2), &id(3));
        assert!(g.best_path(&id(1), &id(3)).is_none());
    }

    #[test]
    fn path_to_self_is_trivial() {
        let mut g = MeshGraph::new();
        g.add_link(id(1), id(2), LinkQuality(90));
        assert_eq!(g.best_path(&id(1), &id(1)), Some(vec![id(1)]));
    }
}

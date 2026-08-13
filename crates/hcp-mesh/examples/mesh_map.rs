//! Build a live map of your mesh from what nodes advertise, then answer the
//! operator's real questions: who's online, who can host this design, and
//! what's the best route to reach them.
//!
//! Run:  cargo run -p hcp-mesh --example mesh_map
//!
//! Note what this does NOT do: it never scans, probes, or fingerprints anything.
//! Every node shown put itself on the map by broadcasting a FabricCapability.

use hcp_fabric::{FabricCapability, FabricClass, ResourceBudget};
use hcp_mesh::{LinkQuality, MeshGraph, MeshRegistry, NodeStatus, TrustState};

fn cap(id: u8, free_bram: u32, epoch: u64) -> FabricCapability {
    FabricCapability {
        node_id: [id; 32],
        class: FabricClass::Ice40,
        total: ResourceBudget::new(7680, 7680, 32),
        free: ResourceBudget::new(7680, 7680, free_bram),
        max_image_bytes: 65536,
        epoch,
    }
}

fn short(id: &[u8; 32]) -> String {
    format!("{:02x}{:02x}", id[0], id[1])
}

fn main() {
    let mut reg = MeshRegistry::new(30);

    // Four nodes advertise themselves (voluntarily).
    reg.observe(&cap(0x11, 32, 1)); // roomy
    reg.observe(&cap(0x22, 20, 1)); // exactly fits a 20-BRAM design
    reg.observe(&cap(0x33, 4, 1)); //  too small for the design
    reg.observe(&cap(0x44, 32, 1)); // roomy, but we won't trust it

    // Operator's trust store (from hcp-identity) mirrored in.
    reg.apply_trust([0x11; 32], TrustState::Trusted, Some("bench-fpga".into()));
    reg.apply_trust([0x22; 32], TrustState::Trusted, Some("shelf-fpga".into()));
    reg.apply_trust([0x33; 32], TrustState::Trusted, Some("tiny-fpga".into()));
    reg.apply_trust([0x44; 32], TrustState::Untrusted, None); // a stranger

    println!("=== mesh nodes (from their own advertisements) ===");
    let mut nodes: Vec<_> = reg.nodes().collect();
    nodes.sort_by_key(|r| r.node_id);
    for r in nodes {
        let status = match reg.status(&r.node_id).unwrap() {
            NodeStatus::Online => "online",
            NodeStatus::Stale => "stale",
        };
        let trust = match r.trust {
            TrustState::Trusted => "trusted",
            TrustState::Untrusted => "stranger",
            TrustState::Revoked => "revoked",
        };
        println!(
            "  {}  {:<11} {:<8} {:<8}  free {} BRAM",
            r.short(),
            r.label.clone().unwrap_or_else(|| "-".into()),
            status,
            trust,
            r.free.brams
        );
    }

    // "Where could I place this design?" — the AES-128 core: 20 BRAM footprint.
    let footprint = ResourceBudget::new(971, 399, 20);
    println!("\n=== candidates to host the AES-128 core (20 BRAM) ===");
    let mut cands = reg.candidates_for(FabricClass::Ice40, 4096, &footprint);
    cands.sort_by_key(|r| r.node_id);
    if cands.is_empty() {
        println!("  (none)");
    }
    for r in &cands {
        println!(
            "  {} {}  — trusted, online, {} BRAM free",
            r.short(),
            r.label.clone().unwrap_or_default(),
            r.free.brams
        );
    }
    println!("  node 33 excluded (only 4 BRAM); node 44 excluded (not trusted).");

    // Topology: how are they linked, and what's the best route?
    println!("\n=== link topology + routing ===");
    let mut g = MeshGraph::new();
    // node 11 can't hear 22 directly; it relays through 44.
    g.add_link([0x11; 32], [0x44; 32], LinkQuality(95));
    g.add_link([0x44; 32], [0x22; 32], LinkQuality(90));
    g.add_link([0x11; 32], [0x22; 32], LinkQuality(10)); // weak direct link
    g.add_link([0x22; 32], [0x33; 32], LinkQuality(80));

    println!(
        "  mesh connected from bench-fpga: {}",
        g.is_connected_from(&[0x11; 32])
    );

    if let Some(path) = g.best_path(&[0x11; 32], &[0x22; 32]) {
        let hops: Vec<String> = path.iter().map(short).collect();
        println!(
            "  best route bench-fpga -> shelf-fpga: {}",
            hops.join(" -> ")
        );
        println!("  (skips the weak direct link in favour of two strong hops)");
    }

    println!("\nEverything above came from consent: nodes advertised, the operator");
    println!("chose whom to trust. No scanning, no device fingerprinting.");
}

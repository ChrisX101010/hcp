//! A full fabric-sharing handshake between two nodes, over a simulated
//! transport. Shows the whole consent cycle end to end:
//!
//!   Node B advertises spare fabric  ->  Node A offers a design  ->
//!   Node B decides  ->  (on accept) A ships the image, B runs it.
//!
//! Run with:  cargo run -p hcp-fabric --example handshake
//!
//! The "link" here just moves byte vectors, but everything crossing it is the
//! real wire format — so this is exactly what a BLE/LoRa/WiFi transport would
//! carry. Swap `Link` for a real radio and nothing else changes.

use hcp_fabric::{
    decide_stub as decide, FabricCapability, FabricClass, PlacementDecision, PlacementOffer,
    PlacementState, ResourceBudget, WireDecode, WireEncode,
};

/// Stand-in for a radio: carries opaque frames, optionally dropping some to
/// prove the control plane tolerates a lossy link.
struct Link {
    drop_every: usize,
    sent: usize,
}
impl Link {
    fn new(drop_every: usize) -> Self {
        Self {
            drop_every,
            sent: 0,
        }
    }
    fn send(&mut self, frame: Vec<u8>) -> Option<Vec<u8>> {
        self.sent += 1;
        if self.drop_every != 0 && self.sent % self.drop_every == 0 {
            println!(
                "   (link dropped a {}-byte frame; sender will retry)",
                frame.len()
            );
            None
        } else {
            Some(frame)
        }
    }
}

fn main() {
    let node_a = [0xAAu8; 32]; // requester
    let node_b = [0xBBu8; 32]; // fabric owner

    // 1. Node B advertises what it will lend.
    let advert = FabricCapability {
        node_id: node_b,
        class: FabricClass::Ice40,
        total: ResourceBudget::new(7680, 7680, 32),
        free: ResourceBudget::new(7680, 7680, 32),
        max_image_bytes: 65536,
        epoch: 1,
    };
    let advert_frame = advert.encode();
    println!(
        "B advertises {} fabric: {} LUT / {} FF / {} BRAM free  ({} byte frame)",
        advert.class.as_str(),
        advert.free.luts,
        advert.free.ffs,
        advert.free.brams,
        advert_frame.len()
    );

    // Node A receives and parses the advertisement.
    let seen = FabricCapability::decode(&advert_frame).expect("valid advert");

    // 2. Node A has a design (our measured AES-128 core) and wants it placed.
    let footprint = ResourceBudget::new(971, 399, 20);
    let image_bytes = 4096u32;
    if !seen.can_host(image_bytes, &footprint) {
        println!("A: B can't host this design; would look for another node.");
        return;
    }
    println!("A: B can host it. Sending a signed placement offer.");

    let mut offer = PlacementOffer {
        from: node_a,
        to: node_b,
        image_digest: [0x11u8; 32], // sha-256 of the image (computed elsewhere)
        image_bytes,
        class: FabricClass::Ice40,
        footprint,
        signature: [0u8; 64],
        nonce: 0xC0FFEE,
    };
    // In the real system the identity layer signs `offer.signing_bytes()` with
    // A's Ed25519 key. Here we just mark it "present".
    offer.signature = sign_stub(&offer.signing_bytes());

    // 3. Ship the offer across the (lossy) link, retrying on drop.
    let mut link = Link::new(2);
    let mut state = PlacementState::Offered;
    let decision = loop {
        if let Some(frame) = link.send(offer.encode()) {
            let received = PlacementOffer::decode(&frame).expect("valid offer");
            // Node B decides. B checks the sig, replay set, class, size, budget.
            let d = decide(
                &received,
                &node_b,
                FabricClass::Ice40,
                &advert.free,
                advert.max_image_bytes,
                verify_stub(&received),
                false,
                0xA5,
            );
            break d;
        }
    };

    // 4. Act on the decision through the state machine.
    match decision {
        PlacementDecision::Accepted {
            lease_id,
            remaining,
        } => {
            state = state.advance(PlacementState::Accepted).unwrap();
            println!(
                "B accepts. lease #{lease_id}; B now has {} LUT / {} FF / {} BRAM free.",
                remaining.luts, remaining.ffs, remaining.brams
            );
            // A ships the actual image bytes (not shown), then marks it running.
            state = state.advance(PlacementState::Running).unwrap();
            println!("A ships the {image_bytes}-byte image; design is now RUNNING on B.");
            println!("state = {state:?}");
            state = state.advance(PlacementState::TornDown).unwrap();
            println!("Later, A releases lease #{lease_id}. state = {state:?}");
        }
        PlacementDecision::Declined(reason) => {
            let _ = state.advance(PlacementState::Declined);
            println!("B declined: {reason:?}");
        }
    }

    println!("\nEverything crossing the link was real wire-format bytes.");
    println!("Point `Link` at a BLE characteristic or a LoRa modem and this is a product.");
}

// --- stubs that the identity/transport layer replaces with real crypto ------

fn sign_stub(_msg: &[u8]) -> [u8; 64] {
    // Real impl: Ed25519 signature over `_msg` with the node's secret key.
    [0x5Au8; 64]
}
fn verify_stub(_offer: &PlacementOffer) -> bool {
    // Real impl: verify `_offer.signature` over `_offer.signing_bytes()`
    // against `_offer.from` (the requester's advertised public key).
    true
}

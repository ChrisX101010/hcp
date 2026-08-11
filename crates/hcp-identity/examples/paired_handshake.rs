//! The fabric handshake from `hcp-fabric`, now with **real signatures**.
//!
//! Shows the honest, safe version of "connect my devices and trust them":
//!   1. Two nodes generate identities.
//!   2. They pair — each enrolls the other's public id in its TrustStore.
//!      (In the field this is the commissioning step: a button press, a QR
//!      scan, a cable — some out-of-band confirmation. Here we do it directly.)
//!   3. Node A signs a real PlacementOffer; Node B verifies it against A's
//!      enrolled identity before deciding.
//!   4. A stranger's identical offer is rejected — untrusted signer.
//!
//! Run:  cargo run -p hcp-identity --example paired_handshake

use hcp_fabric::{decide, FabricClass, PlacementDecision, PlacementOffer, ResourceBudget};
use hcp_identity::{NodeKeypair, TrustStore};

fn build_signed_offer(from: &NodeKeypair, to_id: [u8; 32]) -> PlacementOffer {
    let mut offer = PlacementOffer {
        from: from.public_bytes(),
        to: to_id,
        image_digest: [0x11u8; 32],
        image_bytes: 4096,
        class: FabricClass::Ice40,
        footprint: ResourceBudget::new(971, 399, 20), // measured AES-128 core
        signature: [0u8; 64],
        nonce: 0xC0FFEE,
    };
    // Sign the canonical bytes — exactly the field hcp-fabric defined for this.
    offer.signature = from.sign(&offer.signing_bytes());
    offer
}

fn main() {
    // 1. identities
    let node_a = NodeKeypair::generate();
    let node_b = NodeKeypair::generate();
    println!("Node A id: {}", node_a.id().short());
    println!("Node B id: {}", node_b.id().short());

    // 2. pairing: B decides to trust A (and vice versa)
    let mut b_trust = TrustStore::new();
    b_trust.enroll(node_a.id(), "my-laptop");
    println!("\nB paired with A as \"my-laptop\".");

    // 3. A makes a signed offer to B
    let offer = build_signed_offer(&node_a, node_b.public_bytes());
    println!(
        "A sends a signed placement offer (nonce {:#x}).",
        offer.nonce
    );

    // B's gate: is the signer trusted, and is the signature valid?
    let signer = hcp_identity::NodeId::from_bytes(&offer.from).expect("valid id");
    let trusted = b_trust.is_trusted(&signer);
    let sig_ok = signer
        .verify(&offer.signing_bytes(), &offer.signature)
        .is_ok();
    println!("B checks: trusted={trusted}, signature_ok={sig_ok}");

    let free = ResourceBudget::new(7680, 7680, 32);
    let decision = decide(
        &offer,
        &node_b.public_bytes(),
        FabricClass::Ice40,
        &free,
        65536,
        trusted && sig_ok, // the real gate, no stubs
        false,
        1,
    );
    match decision {
        PlacementDecision::Accepted {
            lease_id,
            remaining,
        } => {
            println!(
                "B ACCEPTS. lease #{lease_id}; free now {} LUT / {} FF / {} BRAM.",
                remaining.luts, remaining.ffs, remaining.brams
            );
        }
        PlacementDecision::Declined(r) => println!("B declined: {r:?}"),
    }

    // 4. a stranger tries the same thing
    println!("\n--- stranger attempt ---");
    let stranger = NodeKeypair::generate();
    let mut evil = build_signed_offer(&stranger, node_b.public_bytes());
    // The stranger even lies about who they are, copying A's id into `from`:
    evil.from = node_a.public_bytes();
    // ...but they can't produce A's signature, so verification fails.
    let claimed = hcp_identity::NodeId::from_bytes(&evil.from).unwrap();
    let sig_ok = claimed
        .verify(&evil.signing_bytes(), &evil.signature)
        .is_ok();
    println!("Stranger forges A's id in `from`. signature_ok={sig_ok}");
    let decision = decide(
        &evil,
        &node_b.public_bytes(),
        FabricClass::Ice40,
        &free,
        65536,
        b_trust.is_trusted(&claimed) && sig_ok,
        false,
        2,
    );
    println!("B's verdict: {decision:?}");
    println!("\nIdentity spoofing fails because the signature can't be forged.");
}

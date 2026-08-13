//! Proves hcp-identity and hcp-fabric compose: a real Ed25519 signature over a
//! real PlacementOffer verifies, and any tampering breaks it.

use hcp_fabric::{FabricClass, PlacementOffer, ResourceBudget};
use hcp_identity::{NodeId, NodeKeypair};

fn signed_offer(from: &NodeKeypair, to: [u8; 32]) -> PlacementOffer {
    let mut o = PlacementOffer {
        from: from.public_bytes(),
        to,
        image_digest: [7u8; 32],
        image_bytes: 4096,
        class: FabricClass::Ice40,
        footprint: ResourceBudget::new(971, 399, 20),
        signature: [0u8; 64],
        nonce: 1,
    };
    o.signature = from.sign(&o.signing_bytes());
    o
}

#[test]
fn genuine_offer_verifies() {
    let a = NodeKeypair::generate();
    let b = NodeKeypair::generate();
    let offer = signed_offer(&a, b.public_bytes());

    let signer = NodeId::from_bytes(&offer.from).unwrap();
    assert!(signer
        .verify(&offer.signing_bytes(), &offer.signature)
        .is_ok());
}

#[test]
fn tampering_with_footprint_breaks_signature() {
    let a = NodeKeypair::generate();
    let b = NodeKeypair::generate();
    let mut offer = signed_offer(&a, b.public_bytes());

    // Attacker inflates the resource request after signing.
    offer.footprint = ResourceBudget::new(1, 1, 1);

    let signer = NodeId::from_bytes(&offer.from).unwrap();
    assert!(
        signer
            .verify(&offer.signing_bytes(), &offer.signature)
            .is_err(),
        "modifying a signed field must invalidate the signature"
    );
}

#[test]
fn changing_target_breaks_signature() {
    let a = NodeKeypair::generate();
    let b = NodeKeypair::generate();
    let c = NodeKeypair::generate();
    let mut offer = signed_offer(&a, b.public_bytes());

    // Redirect the offer to a different node.
    offer.to = c.public_bytes();

    let signer = NodeId::from_bytes(&offer.from).unwrap();
    assert!(signer
        .verify(&offer.signing_bytes(), &offer.signature)
        .is_err());
}

#[test]
fn nonce_replay_is_detectable_via_signed_bytes() {
    // The nonce is inside signing_bytes, so a replayed offer keeps a valid
    // signature but a keyed-by-nonce seen-set catches it (that check lives in
    // hcp-fabric::decide). Here we just confirm the nonce is actually signed.
    let a = NodeKeypair::generate();
    let b = NodeKeypair::generate();
    let mut offer = signed_offer(&a, b.public_bytes());
    let signer = NodeId::from_bytes(&offer.from).unwrap();

    offer.nonce = offer.nonce.wrapping_add(1);
    assert!(
        signer
            .verify(&offer.signing_bytes(), &offer.signature)
            .is_err(),
        "nonce must be covered by the signature"
    );
}

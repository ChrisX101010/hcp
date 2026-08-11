//! The offer -> decision handshake, and the consent state machine.
//!
//! Flow:
//!   requester --- PlacementOffer ---> owner
//!   owner ------- PlacementDecision -> requester
//!
//! The owner only ever *responds* to an offer; it is never obligated to accept,
//! and an offer can only be sent to a node that advertised a `FabricCapability`.
//! Those two facts are what make placement consensual by construction.

use crate::capability::{FabricClass, ResourceBudget};

/// A request to place a design on a specific node's advertised fabric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementOffer {
    /// Who is asking (public-key fingerprint of the requester).
    pub from: [u8; 32],
    /// Which node's fabric is being requested (must match an advertisement).
    pub to: [u8; 32],
    /// Content address (SHA-256) of the hardware image being offered. The image
    /// itself travels separately; this binds the offer to exact bytes.
    pub image_digest: [u8; 32],
    pub image_bytes: u32,
    pub class: FabricClass,
    pub footprint: ResourceBudget,
    /// Requester's signature over the canonical offer bytes. Verified by the
    /// owner before any decision. (Signature scheme lives in the transport/
    /// identity layer; here it is an opaque 64-byte Ed25519-shaped field.)
    pub signature: [u8; 64],
    /// Nonce to make each offer unique and replay-detectable.
    pub nonce: u64,
}

impl PlacementOffer {
    /// Canonical bytes that a signature must cover. Excludes the signature
    /// field itself. Stable and length-prefix-free because every field is
    /// fixed-width, so there is no ambiguity to exploit.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(32 + 32 + 32 + 4 + 1 + 12 + 8);
        v.extend_from_slice(&self.from);
        v.extend_from_slice(&self.to);
        v.extend_from_slice(&self.image_digest);
        v.extend_from_slice(&self.image_bytes.to_be_bytes());
        v.push(self.class.code());
        v.extend_from_slice(&self.footprint.luts.to_be_bytes());
        v.extend_from_slice(&self.footprint.ffs.to_be_bytes());
        v.extend_from_slice(&self.footprint.brams.to_be_bytes());
        v.extend_from_slice(&self.nonce.to_be_bytes());
        v
    }
}

/// Why an owner turned down an offer. Explicit reasons help the requester retry
/// intelligently instead of hammering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclineReason {
    /// Not enough free fabric for the footprint.
    InsufficientResources,
    /// Image is larger than this node accepts.
    ImageTooLarge,
    /// Fabric class does not match this node.
    WrongClass,
    /// Signature did not verify against the requester's advertised identity.
    BadSignature,
    /// Nonce already seen — replay.
    Replay,
    /// Owner policy simply says no (rate limit, denylist, quiet hours).
    PolicyRefused,
}

impl DeclineReason {
    pub const fn code(self) -> u8 {
        match self {
            DeclineReason::InsufficientResources => 1,
            DeclineReason::ImageTooLarge => 2,
            DeclineReason::WrongClass => 3,
            DeclineReason::BadSignature => 4,
            DeclineReason::Replay => 5,
            DeclineReason::PolicyRefused => 6,
        }
    }
    pub fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            1 => DeclineReason::InsufficientResources,
            2 => DeclineReason::ImageTooLarge,
            3 => DeclineReason::WrongClass,
            4 => DeclineReason::BadSignature,
            5 => DeclineReason::Replay,
            6 => DeclineReason::PolicyRefused,
            _ => return None,
        })
    }
}

/// The owner's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementDecision {
    /// Accepted. `lease_id` identifies the running placement for later teardown;
    /// `remaining` is the node's free budget after reserving this footprint.
    Accepted {
        lease_id: u64,
        remaining: ResourceBudget,
    },
    Declined(DeclineReason),
}

/// Lifecycle of a placement from the requester's side. A small explicit FSM so
/// callers can't, e.g., mark something `Running` that was never `Accepted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementState {
    Offered,
    Accepted,
    Running,
    TornDown,
    Declined,
}

impl PlacementState {
    /// Legal transitions. Anything not listed is rejected by `advance`.
    pub fn advance(self, to: PlacementState) -> Result<PlacementState, &'static str> {
        use PlacementState::*;
        let ok = matches!(
            (self, to),
            (Offered, Accepted)
                | (Offered, Declined)
                | (Accepted, Running)
                | (Accepted, TornDown)
                | (Running, TornDown)
        );
        if ok {
            Ok(to)
        } else {
            Err("illegal placement transition")
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, PlacementState::TornDown | PlacementState::Declined)
    }
}

/// Reference decision logic an owner can use. Pure and side-effect-free so it is
/// trivially testable; a real node wraps this with signature verification and a
/// seen-nonce set (both live in the identity/transport layer).
///
/// `already_seen_nonce` and `signature_ok` are passed in so this function stays
/// pure and dependency-free.
pub fn decide(
    offer: &PlacementOffer,
    our_id: &[u8; 32],
    our_class: FabricClass,
    free: &ResourceBudget,
    max_image_bytes: u32,
    signature_ok: bool,
    already_seen_nonce: bool,
    next_lease_id: u64,
) -> PlacementDecision {
    if &offer.to != our_id {
        return PlacementDecision::Declined(DeclineReason::PolicyRefused);
    }
    if !signature_ok {
        return PlacementDecision::Declined(DeclineReason::BadSignature);
    }
    if already_seen_nonce {
        return PlacementDecision::Declined(DeclineReason::Replay);
    }
    if offer.class != our_class {
        return PlacementDecision::Declined(DeclineReason::WrongClass);
    }
    if offer.image_bytes > max_image_bytes {
        return PlacementDecision::Declined(DeclineReason::ImageTooLarge);
    }
    if !free.fits(&offer.footprint) {
        return PlacementDecision::Declined(DeclineReason::InsufficientResources);
    }
    PlacementDecision::Accepted {
        lease_id: next_lease_id,
        remaining: free.minus(&offer.footprint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_offer(to: [u8; 32]) -> PlacementOffer {
        PlacementOffer {
            from: [7u8; 32],
            to,
            image_digest: [9u8; 32],
            image_bytes: 4096,
            class: FabricClass::Ice40,
            footprint: ResourceBudget::new(971, 399, 20),
            signature: [0u8; 64],
            nonce: 42,
        }
    }

    #[test]
    fn happy_path_accepts_and_reserves() {
        let me = [1u8; 32];
        let free = ResourceBudget::new(7680, 7680, 32);
        let d = decide(
            &sample_offer(me),
            &me,
            FabricClass::Ice40,
            &free,
            65536,
            true,
            false,
            100,
        );
        match d {
            PlacementDecision::Accepted {
                lease_id,
                remaining,
            } => {
                assert_eq!(lease_id, 100);
                assert_eq!(remaining, ResourceBudget::new(6709, 7281, 12));
            }
            _ => panic!("expected acceptance"),
        }
    }

    #[test]
    fn rejects_bad_signature_before_anything_else() {
        let me = [1u8; 32];
        let free = ResourceBudget::new(7680, 7680, 32);
        let d = decide(
            &sample_offer(me),
            &me,
            FabricClass::Ice40,
            &free,
            65536,
            false, // sig bad
            false,
            1,
        );
        assert_eq!(d, PlacementDecision::Declined(DeclineReason::BadSignature));
    }

    #[test]
    fn rejects_replay() {
        let me = [1u8; 32];
        let free = ResourceBudget::new(7680, 7680, 32);
        let d = decide(
            &sample_offer(me),
            &me,
            FabricClass::Ice40,
            &free,
            65536,
            true,
            true, // seen nonce
            1,
        );
        assert_eq!(d, PlacementDecision::Declined(DeclineReason::Replay));
    }

    #[test]
    fn rejects_offer_addressed_to_someone_else() {
        let me = [1u8; 32];
        let other = [2u8; 32];
        let free = ResourceBudget::new(7680, 7680, 32);
        let d = decide(
            &sample_offer(other),
            &me,
            FabricClass::Ice40,
            &free,
            65536,
            true,
            false,
            1,
        );
        assert_eq!(d, PlacementDecision::Declined(DeclineReason::PolicyRefused));
    }

    #[test]
    fn rejects_when_too_big() {
        let me = [1u8; 32];
        let free = ResourceBudget::new(500, 500, 4); // smaller than footprint
        let d = decide(
            &sample_offer(me),
            &me,
            FabricClass::Ice40,
            &free,
            65536,
            true,
            false,
            1,
        );
        assert_eq!(
            d,
            PlacementDecision::Declined(DeclineReason::InsufficientResources)
        );
    }

    #[test]
    fn fsm_rejects_illegal_transitions() {
        assert!(PlacementState::Offered
            .advance(PlacementState::Running)
            .is_err());
        assert!(PlacementState::Declined
            .advance(PlacementState::Accepted)
            .is_err());
        assert_eq!(
            PlacementState::Offered.advance(PlacementState::Accepted),
            Ok(PlacementState::Accepted)
        );
        assert!(PlacementState::Declined.is_terminal());
    }

    #[test]
    fn signing_bytes_exclude_signature_and_are_stable() {
        let mut o = sample_offer([1u8; 32]);
        let a = o.signing_bytes();
        o.signature = [0xFFu8; 64]; // changing the sig must not change signed bytes
        let b = o.signing_bytes();
        assert_eq!(a, b);
        assert_eq!(a.len(), 32 + 32 + 32 + 4 + 1 + 12 + 8);
    }
}

//! # hcp-fabric
//!
//! The consent-based fabric-sharing layer for HCP.
//!
//! A node **advertises** the reconfigurable resources it is willing to lend
//! (`FabricCapability`). Another node **offers** a design to place there
//! (`PlacementOffer`). The owner **decides** (`PlacementDecision`). Nothing is
//! ever placed on a node that did not advertise capacity and accept the offer —
//! consent is a property of the protocol, not a policy bolted on top.
//!
//! This crate is transport-agnostic: it defines the messages and the state
//! machine, and serializes to compact bytes. Whether those bytes travel over
//! WiFi, BLE, LoRa, or a serial cable is the transport layer's concern. Because
//! the payload is a *design* (a small, integrity-checked bitstream + manifest)
//! rather than a live data bus, even a 280-bit/s link is enough — the same
//! insight that let Ventilator 202 ship software over FM in 1983.
//!
//! No external dependencies: wire encoding is hand-rolled and covered by
//! round-trip tests, so this slots into the HCP workspace like `hcp-ecc`.

#![forbid(unsafe_code)]

mod capability;
mod offer;
mod wire;

pub use capability::{FabricCapability, FabricClass, ResourceBudget};
/// Alias used by the `handshake` example for readability.
pub use offer::decide as decide_stub;
pub use offer::{decide, DeclineReason, PlacementDecision, PlacementOffer, PlacementState};
pub use wire::{WireDecode, WireEncode, WireError};

/// Protocol version. Bump on any breaking wire-format change; peers exchange
/// this in the first frame so mismatches fail loudly instead of corrupting.
pub const FABRIC_PROTOCOL_VERSION: u16 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_stable() {
        assert_eq!(FABRIC_PROTOCOL_VERSION, 1);
    }
}

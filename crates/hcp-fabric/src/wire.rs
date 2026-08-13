//! Transport-agnostic wire encoding.
//!
//! Every message serializes to a compact, fixed-layout byte string with a
//! 4-byte header: `[version:u16][msg_type:u8][reserved:u8]`. There are no
//! variable-length fields, so frames are tiny and unambiguous — a
//! `FabricCapability` is 89 bytes, a `PlacementOffer` is 181. Both fit in a
//! single BLE characteristic write or a couple of LoRa frames, which is the
//! whole point: we ship *designs*, not data streams, so the control plane is
//! frugal enough for any radio.
//!
//! Encoding is big-endian throughout. No dependency on serde — the format is
//! small enough to own outright, and owning it means no version-skew surprises
//! on a constrained node.

use crate::capability::{FabricCapability, FabricClass, ResourceBudget};
use crate::offer::{DeclineReason, PlacementDecision, PlacementOffer};
use crate::FABRIC_PROTOCOL_VERSION;

/// Errors from decoding untrusted bytes off the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    TooShort { need: usize, got: usize },
    BadVersion { got: u16 },
    UnknownMsgType(u8),
    BadEnum(&'static str),
    TrailingBytes(usize),
}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WireError::TooShort { need, got } => {
                write!(f, "frame too short: need {need}, got {got}")
            }
            WireError::BadVersion { got } => write!(f, "unsupported protocol version {got}"),
            WireError::UnknownMsgType(t) => write!(f, "unknown message type {t}"),
            WireError::BadEnum(name) => write!(f, "invalid {name} value"),
            WireError::TrailingBytes(n) => write!(f, "{n} unexpected trailing bytes"),
        }
    }
}

impl std::error::Error for WireError {}

const MSG_CAPABILITY: u8 = 1;
const MSG_OFFER: u8 = 2;
const MSG_DECISION: u8 = 3;

/// Types that can be written to the wire.
pub trait WireEncode {
    fn encode(&self) -> Vec<u8>;
}

/// Types that can be parsed from the wire.
pub trait WireDecode: Sized {
    fn decode(bytes: &[u8]) -> Result<Self, WireError>;
}

fn header(msg_type: u8) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&FABRIC_PROTOCOL_VERSION.to_be_bytes());
    v.push(msg_type);
    v.push(0); // reserved
    v
}

fn check_header(bytes: &[u8], expect: u8) -> Result<usize, WireError> {
    if bytes.len() < 4 {
        return Err(WireError::TooShort {
            need: 4,
            got: bytes.len(),
        });
    }
    let ver = u16::from_be_bytes([bytes[0], bytes[1]]);
    if ver != FABRIC_PROTOCOL_VERSION {
        return Err(WireError::BadVersion { got: ver });
    }
    if bytes[2] != expect {
        return Err(WireError::UnknownMsgType(bytes[2]));
    }
    Ok(4)
}

// ---- small helpers ---------------------------------------------------------

fn put_budget(v: &mut Vec<u8>, b: &ResourceBudget) {
    v.extend_from_slice(&b.luts.to_be_bytes());
    v.extend_from_slice(&b.ffs.to_be_bytes());
    v.extend_from_slice(&b.brams.to_be_bytes());
}

fn get_u32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn get_u64(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_be_bytes(a)
}
fn get_budget(b: &[u8], o: usize) -> ResourceBudget {
    ResourceBudget::new(get_u32(b, o), get_u32(b, o + 4), get_u32(b, o + 8))
}

// ---- FabricCapability: 4 hdr + 32 id + 1 class + 12 total + 12 free + 4 max + 8 epoch = 73
impl WireEncode for FabricCapability {
    fn encode(&self) -> Vec<u8> {
        let mut v = header(MSG_CAPABILITY);
        v.extend_from_slice(&self.node_id);
        v.push(self.class.code());
        put_budget(&mut v, &self.total);
        put_budget(&mut v, &self.free);
        v.extend_from_slice(&self.max_image_bytes.to_be_bytes());
        v.extend_from_slice(&self.epoch.to_be_bytes());
        v
    }
}

impl WireDecode for FabricCapability {
    fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let mut o = check_header(bytes, MSG_CAPABILITY)?;
        const NEED: usize = 4 + 32 + 1 + 12 + 12 + 4 + 8;
        if bytes.len() < NEED {
            return Err(WireError::TooShort {
                need: NEED,
                got: bytes.len(),
            });
        }
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&bytes[o..o + 32]);
        o += 32;
        let class = FabricClass::from_code(bytes[o]).ok_or(WireError::BadEnum("FabricClass"))?;
        o += 1;
        let total = get_budget(bytes, o);
        o += 12;
        let free = get_budget(bytes, o);
        o += 12;
        let max_image_bytes = get_u32(bytes, o);
        o += 4;
        let epoch = get_u64(bytes, o);
        o += 8;
        if o != bytes.len() {
            return Err(WireError::TrailingBytes(bytes.len() - o));
        }
        Ok(FabricCapability {
            node_id,
            class,
            total,
            free,
            max_image_bytes,
            epoch,
        })
    }
}

// ---- PlacementOffer
impl WireEncode for PlacementOffer {
    fn encode(&self) -> Vec<u8> {
        let mut v = header(MSG_OFFER);
        v.extend_from_slice(&self.from);
        v.extend_from_slice(&self.to);
        v.extend_from_slice(&self.image_digest);
        v.extend_from_slice(&self.image_bytes.to_be_bytes());
        v.push(self.class.code());
        put_budget(&mut v, &self.footprint);
        v.extend_from_slice(&self.signature);
        v.extend_from_slice(&self.nonce.to_be_bytes());
        v
    }
}

impl WireDecode for PlacementOffer {
    fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let mut o = check_header(bytes, MSG_OFFER)?;
        const NEED: usize = 4 + 32 + 32 + 32 + 4 + 1 + 12 + 64 + 8;
        if bytes.len() < NEED {
            return Err(WireError::TooShort {
                need: NEED,
                got: bytes.len(),
            });
        }
        let mut from = [0u8; 32];
        from.copy_from_slice(&bytes[o..o + 32]);
        o += 32;
        let mut to = [0u8; 32];
        to.copy_from_slice(&bytes[o..o + 32]);
        o += 32;
        let mut image_digest = [0u8; 32];
        image_digest.copy_from_slice(&bytes[o..o + 32]);
        o += 32;
        let image_bytes = get_u32(bytes, o);
        o += 4;
        let class = FabricClass::from_code(bytes[o]).ok_or(WireError::BadEnum("FabricClass"))?;
        o += 1;
        let footprint = get_budget(bytes, o);
        o += 12;
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&bytes[o..o + 64]);
        o += 64;
        let nonce = get_u64(bytes, o);
        o += 8;
        if o != bytes.len() {
            return Err(WireError::TrailingBytes(bytes.len() - o));
        }
        Ok(PlacementOffer {
            from,
            to,
            image_digest,
            image_bytes,
            class,
            footprint,
            signature,
            nonce,
        })
    }
}

// ---- PlacementDecision
const DECISION_ACCEPTED: u8 = 0;
const DECISION_DECLINED: u8 = 1;

impl WireEncode for PlacementDecision {
    fn encode(&self) -> Vec<u8> {
        let mut v = header(MSG_DECISION);
        match self {
            PlacementDecision::Accepted {
                lease_id,
                remaining,
            } => {
                v.push(DECISION_ACCEPTED);
                v.extend_from_slice(&lease_id.to_be_bytes());
                put_budget(&mut v, remaining);
            }
            PlacementDecision::Declined(reason) => {
                v.push(DECISION_DECLINED);
                v.push(reason.code());
            }
        }
        v
    }
}

impl WireDecode for PlacementDecision {
    fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let o = check_header(bytes, MSG_DECISION)?;
        if bytes.len() < o + 1 {
            return Err(WireError::TooShort {
                need: o + 1,
                got: bytes.len(),
            });
        }
        match bytes[o] {
            DECISION_ACCEPTED => {
                const NEED: usize = 5 + 8 + 12;
                if bytes.len() < NEED {
                    return Err(WireError::TooShort {
                        need: NEED,
                        got: bytes.len(),
                    });
                }
                let lease_id = get_u64(bytes, o + 1);
                let remaining = get_budget(bytes, o + 9);
                if o + 1 + 8 + 12 != bytes.len() {
                    return Err(WireError::TrailingBytes(bytes.len() - (o + 21)));
                }
                Ok(PlacementDecision::Accepted {
                    lease_id,
                    remaining,
                })
            }
            DECISION_DECLINED => {
                if bytes.len() < o + 2 {
                    return Err(WireError::TooShort {
                        need: o + 2,
                        got: bytes.len(),
                    });
                }
                let reason = DeclineReason::from_code(bytes[o + 1])
                    .ok_or(WireError::BadEnum("DeclineReason"))?;
                Ok(PlacementDecision::Declined(reason))
            }
            other => Err(WireError::BadEnum(if other > 1 {
                "PlacementDecision tag"
            } else {
                "PlacementDecision"
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap() -> FabricCapability {
        FabricCapability {
            node_id: [3u8; 32],
            class: FabricClass::Ice40,
            total: ResourceBudget::new(7680, 7680, 32),
            free: ResourceBudget::new(6709, 7281, 12),
            max_image_bytes: 65536,
            epoch: 17,
        }
    }

    fn offer() -> PlacementOffer {
        PlacementOffer {
            from: [7u8; 32],
            to: [3u8; 32],
            image_digest: [9u8; 32],
            image_bytes: 4096,
            class: FabricClass::Ice40,
            footprint: ResourceBudget::new(971, 399, 20),
            signature: [5u8; 64],
            nonce: 42,
        }
    }

    #[test]
    fn capability_roundtrips() {
        let c = cap();
        let bytes = c.encode();
        assert_eq!(bytes.len(), 73);
        assert_eq!(FabricCapability::decode(&bytes).unwrap(), c);
    }

    #[test]
    fn offer_roundtrips() {
        let o = offer();
        let bytes = o.encode();
        assert_eq!(bytes.len(), 189);
        assert_eq!(PlacementOffer::decode(&bytes).unwrap(), o);
    }

    #[test]
    fn decision_accepted_roundtrips() {
        let d = PlacementDecision::Accepted {
            lease_id: 999,
            remaining: ResourceBudget::new(1, 2, 3),
        };
        assert_eq!(PlacementDecision::decode(&d.encode()).unwrap(), d);
    }

    #[test]
    fn decision_declined_roundtrips() {
        let d = PlacementDecision::Declined(DeclineReason::Replay);
        assert_eq!(PlacementDecision::decode(&d.encode()).unwrap(), d);
    }

    #[test]
    fn frames_are_radio_sized() {
        // The control plane must fit a BLE write (517 B) with room to spare, and
        // a couple of LoRa frames (~255 B each). These bounds are the "any
        // signal" guarantee — assert them so a future field can't silently blow it.
        assert!(cap().encode().len() <= 255);
        assert!(offer().encode().len() <= 255);
    }

    #[test]
    fn rejects_wrong_version() {
        let mut b = cap().encode();
        b[0] = 0xFF;
        b[1] = 0xFF;
        assert!(matches!(
            FabricCapability::decode(&b),
            Err(WireError::BadVersion { .. })
        ));
    }

    #[test]
    fn rejects_truncated_frame() {
        let b = offer().encode();
        assert!(matches!(
            PlacementOffer::decode(&b[..b.len() - 3]),
            Err(WireError::TooShort { .. })
        ));
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut b = cap().encode();
        b.push(0xAA);
        assert!(matches!(
            FabricCapability::decode(&b),
            Err(WireError::TrailingBytes(1))
        ));
    }

    #[test]
    fn rejects_bad_class_enum() {
        let mut b = cap().encode();
        b[4 + 32] = 200; // class byte -> invalid
        assert!(matches!(
            FabricCapability::decode(&b),
            Err(WireError::BadEnum("FabricClass"))
        ));
    }

    #[test]
    fn cross_message_type_is_rejected() {
        // Decoding an offer's bytes as a capability must fail on msg type.
        let b = offer().encode();
        assert!(matches!(
            FabricCapability::decode(&b),
            Err(WireError::UnknownMsgType(MSG_OFFER))
        ));
    }
}

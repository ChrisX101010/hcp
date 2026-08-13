//! # hcp-admit
//!
//! Bitstream **admission control** — the gate a design must pass before a node
//! will place it on shared fabric.
//!
//! ## Why this exists
//!
//! Published research on FPGA-as-a-Service (e.g. "What is All the FaaS About? —
//! Remote Exploitation of FPGA-as-a-Service Platforms", IACR ePrint 2021/746)
//! shows the central risk of *sharing reconfigurable fabric*: a hostile or
//! malformed partial-reconfiguration bitstream can attack or deny-service the
//! host. Cloud FaaS platforms struggle with this because their trust model is
//! "customer paid, so run it."
//!
//! HCP's trust model is different — consent + identity + integrity — and this
//! crate adds the missing final check: **before** a signed, trusted, intact
//! design is placed, does it satisfy the *host's own* admission policy?
//! Resource ceilings, size limits, required metadata, fabric-class match, and a
//! digest allow/deny list. A node stays in control of its own silicon.
//!
//! This is policy, not a bitstream disassembler — it can't prove an arbitrary
//! vendor bitstream is benign (nobody can, in general). What it does is let a
//! host state, and mechanically enforce, the conditions under which it will
//! accept work: the piece the FaaS security literature identifies as missing.

#![forbid(unsafe_code)]

use std::collections::HashSet;

/// A design's declared properties, taken from its signed manifest (`hcp.json`)
/// and the placement offer. These are what the policy judges. They are trusted
/// only because identity + integrity checks already passed upstream; admission
/// is the *authorization* step after *authentication*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignClaims {
    /// Content address of the bitstream (SHA-256), 32 bytes.
    pub digest: [u8; 32],
    /// Target fabric class identifier (e.g. "ice40"), matched against the host.
    pub fabric_class: String,
    /// Declared resource footprint.
    pub luts: u32,
    pub ffs: u32,
    pub brams: u32,
    /// Bitstream size in bytes.
    pub image_bytes: u32,
    /// Whether the manifest carries an ECC report (HCP designs should).
    pub has_ecc_report: bool,
    /// Free-form capability tags the design requests (e.g. "dsp", "ext-io").
    pub requested_tags: Vec<String>,
}

/// One reason a design was refused. Explicit so the requester can fix and retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenyReason {
    WrongFabricClass { want: String, got: String },
    TooManyLuts { limit: u32, got: u32 },
    TooManyFfs { limit: u32, got: u32 },
    TooManyBrams { limit: u32, got: u32 },
    ImageTooLarge { limit: u32, got: u32 },
    EccReportRequired,
    DigestDenylisted,
    DigestNotAllowlisted,
    CapabilityNotOffered(String),
}

impl core::fmt::Display for DenyReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use DenyReason::*;
        match self {
            WrongFabricClass { want, got } => {
                write!(
                    f,
                    "fabric class mismatch: host is {want}, design targets {got}"
                )
            }
            TooManyLuts { limit, got } => write!(f, "LUTs {got} exceed limit {limit}"),
            TooManyFfs { limit, got } => write!(f, "FFs {got} exceed limit {limit}"),
            TooManyBrams { limit, got } => write!(f, "BRAMs {got} exceed limit {limit}"),
            ImageTooLarge { limit, got } => write!(f, "image {got}B exceeds limit {limit}B"),
            EccReportRequired => write!(f, "manifest is missing the required ECC report"),
            DigestDenylisted => write!(f, "bitstream digest is on the denylist"),
            DigestNotAllowlisted => write!(f, "allowlist mode: digest is not approved"),
            CapabilityNotOffered(c) => write!(
                f,
                "design requests capability '{c}' the host does not offer"
            ),
        }
    }
}

/// The outcome of an admission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Admit,
    /// Refused; carries every failed rule so the requester sees the full picture.
    Deny(Vec<DenyReason>),
}

impl Admission {
    pub fn is_admitted(&self) -> bool {
        matches!(self, Admission::Admit)
    }
}

/// How the digest list is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListMode {
    /// Digests in the set are refused; everything else is allowed. (default)
    #[default]
    Denylist,
    /// Only digests in the set are allowed; everything else is refused.
    Allowlist,
}

/// A host's admission policy. Built with the builder methods, then reused for
/// every incoming design. Cheap to evaluate — pure comparisons, no allocation
/// beyond the reason list on denial.
#[derive(Debug, Clone)]
pub struct AdmissionPolicy {
    fabric_class: String,
    max_luts: u32,
    max_ffs: u32,
    max_brams: u32,
    max_image_bytes: u32,
    require_ecc_report: bool,
    list_mode: ListMode,
    digest_list: HashSet<[u8; 32]>,
    offered_capabilities: HashSet<String>,
}

impl AdmissionPolicy {
    /// A policy for a host of the given fabric class with resource ceilings.
    /// Defaults: no ECC requirement, denylist mode (open), no capability limits.
    pub fn new(
        fabric_class: impl Into<String>,
        max_luts: u32,
        max_ffs: u32,
        max_brams: u32,
    ) -> Self {
        AdmissionPolicy {
            fabric_class: fabric_class.into(),
            max_luts,
            max_ffs,
            max_brams,
            max_image_bytes: u32::MAX,
            require_ecc_report: false,
            list_mode: ListMode::Denylist,
            digest_list: HashSet::new(),
            offered_capabilities: HashSet::new(),
        }
    }

    pub fn max_image_bytes(mut self, n: u32) -> Self {
        self.max_image_bytes = n;
        self
    }

    /// Require the design's manifest to include an ECC report. HCP designs
    /// carry one; requiring it is a cheap way to reject non-HCP payloads.
    pub fn require_ecc_report(mut self, yes: bool) -> Self {
        self.require_ecc_report = yes;
        self
    }

    /// Switch to allowlist mode: only explicitly approved digests are admitted.
    /// The safest posture for a host that only runs designs it has vetted.
    pub fn allowlist_mode(mut self) -> Self {
        self.list_mode = ListMode::Allowlist;
        self
    }

    /// Add a digest to the list (deny or allow, per mode).
    pub fn with_digest(mut self, digest: [u8; 32]) -> Self {
        self.digest_list.insert(digest);
        self
    }

    /// Declare a capability this host offers designs may request.
    pub fn offering(mut self, capability: impl Into<String>) -> Self {
        self.offered_capabilities.insert(capability.into());
        self
    }

    /// Evaluate a design against the policy. Collects *all* failures, not just
    /// the first, so a requester can fix everything in one round trip.
    pub fn evaluate(&self, d: &DesignClaims) -> Admission {
        let mut reasons = Vec::new();

        if d.fabric_class != self.fabric_class {
            reasons.push(DenyReason::WrongFabricClass {
                want: self.fabric_class.clone(),
                got: d.fabric_class.clone(),
            });
        }
        if d.luts > self.max_luts {
            reasons.push(DenyReason::TooManyLuts {
                limit: self.max_luts,
                got: d.luts,
            });
        }
        if d.ffs > self.max_ffs {
            reasons.push(DenyReason::TooManyFfs {
                limit: self.max_ffs,
                got: d.ffs,
            });
        }
        if d.brams > self.max_brams {
            reasons.push(DenyReason::TooManyBrams {
                limit: self.max_brams,
                got: d.brams,
            });
        }
        if d.image_bytes > self.max_image_bytes {
            reasons.push(DenyReason::ImageTooLarge {
                limit: self.max_image_bytes,
                got: d.image_bytes,
            });
        }
        if self.require_ecc_report && !d.has_ecc_report {
            reasons.push(DenyReason::EccReportRequired);
        }
        match self.list_mode {
            ListMode::Denylist => {
                if self.digest_list.contains(&d.digest) {
                    reasons.push(DenyReason::DigestDenylisted);
                }
            }
            ListMode::Allowlist => {
                if !self.digest_list.contains(&d.digest) {
                    reasons.push(DenyReason::DigestNotAllowlisted);
                }
            }
        }
        for tag in &d.requested_tags {
            if !self.offered_capabilities.contains(tag) {
                reasons.push(DenyReason::CapabilityNotOffered(tag.clone()));
            }
        }

        if reasons.is_empty() {
            Admission::Admit
        } else {
            Admission::Deny(reasons)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good_design() -> DesignClaims {
        DesignClaims {
            digest: [0xAB; 32],
            fabric_class: "ice40".into(),
            luts: 971,
            ffs: 399,
            brams: 20,
            image_bytes: 4096,
            has_ecc_report: true,
            requested_tags: vec![],
        }
    }

    fn hx8k_policy() -> AdmissionPolicy {
        // iCE40HX8K-ish ceilings
        AdmissionPolicy::new("ice40", 7680, 7680, 32).max_image_bytes(65536)
    }

    #[test]
    fn well_formed_design_is_admitted() {
        assert_eq!(hx8k_policy().evaluate(&good_design()), Admission::Admit);
    }

    #[test]
    fn oversized_design_is_denied_with_reason() {
        let mut d = good_design();
        d.brams = 40; // exceeds 32
        match hx8k_policy().evaluate(&d) {
            Admission::Deny(rs) => {
                assert!(rs.contains(&DenyReason::TooManyBrams { limit: 32, got: 40 }));
            }
            _ => panic!("should deny"),
        }
    }

    #[test]
    fn all_failures_are_collected() {
        let mut d = good_design();
        d.luts = 99999;
        d.brams = 99;
        d.fabric_class = "ecp5".into();
        match hx8k_policy().evaluate(&d) {
            Admission::Deny(rs) => assert_eq!(rs.len(), 3),
            _ => panic!("should deny"),
        }
    }

    #[test]
    fn wrong_fabric_class_denied() {
        let mut d = good_design();
        d.fabric_class = "xilinx7".into();
        assert!(!hx8k_policy().evaluate(&d).is_admitted());
    }

    #[test]
    fn ecc_report_can_be_required() {
        let policy = hx8k_policy().require_ecc_report(true);
        let mut d = good_design();
        d.has_ecc_report = false;
        match policy.evaluate(&d) {
            Admission::Deny(rs) => assert!(rs.contains(&DenyReason::EccReportRequired)),
            _ => panic!("should deny"),
        }
    }

    #[test]
    fn denylist_blocks_specific_digest() {
        let bad = [0x66u8; 32];
        let policy = hx8k_policy().with_digest(bad);
        let mut d = good_design();
        d.digest = bad;
        assert!(!policy.evaluate(&d).is_admitted());
    }

    #[test]
    fn allowlist_admits_only_approved() {
        let approved = [0x11u8; 32];
        let policy = hx8k_policy().allowlist_mode().with_digest(approved);

        // approved digest passes
        let mut ok = good_design();
        ok.digest = approved;
        assert!(policy.evaluate(&ok).is_admitted());

        // anything else is refused, even if otherwise valid
        let other = good_design(); // digest 0xAB..
        match policy.evaluate(&other) {
            Admission::Deny(rs) => assert!(rs.contains(&DenyReason::DigestNotAllowlisted)),
            _ => panic!("should deny"),
        }
    }

    #[test]
    fn capability_requests_must_be_offered() {
        let policy = hx8k_policy().offering("dsp");
        let mut needs_dsp = good_design();
        needs_dsp.requested_tags = vec!["dsp".into()];
        assert!(policy.evaluate(&needs_dsp).is_admitted());

        let mut needs_ext = good_design();
        needs_ext.requested_tags = vec!["ext-io".into()];
        match policy.evaluate(&needs_ext) {
            Admission::Deny(rs) => {
                assert!(rs.contains(&DenyReason::CapabilityNotOffered("ext-io".into())))
            }
            _ => panic!("should deny"),
        }
    }

    #[test]
    fn admission_is_deterministic() {
        let p = hx8k_policy();
        let d = good_design();
        assert_eq!(p.evaluate(&d), p.evaluate(&d));
    }
}

//! What a node advertises it is willing to lend.

/// Family of reconfigurable fabric. Determines which bitstream format a design
/// must carry to be placeable here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FabricClass {
    Ice40,
    Ecp5,
    Xilinx7,
    /// Cycle-accurate software simulation (Verilator/Icarus). Always available
    /// on a node with spare CPU; useful as a universal fallback target.
    Simulator,
}

impl FabricClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            FabricClass::Ice40 => "ice40",
            FabricClass::Ecp5 => "ecp5",
            FabricClass::Xilinx7 => "xilinx7",
            FabricClass::Simulator => "sim",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "ice40" => FabricClass::Ice40,
            "ecp5" => FabricClass::Ecp5,
            "xilinx7" => FabricClass::Xilinx7,
            "sim" => FabricClass::Simulator,
            _ => return None,
        })
    }

    pub const fn code(self) -> u8 {
        match self {
            FabricClass::Ice40 => 1,
            FabricClass::Ecp5 => 2,
            FabricClass::Xilinx7 => 3,
            FabricClass::Simulator => 255,
        }
    }

    pub fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            1 => FabricClass::Ice40,
            2 => FabricClass::Ecp5,
            3 => FabricClass::Xilinx7,
            255 => FabricClass::Simulator,
            _ => return None,
        })
    }
}

/// A coarse resource budget. Deliberately small and fabric-neutral: LUTs, flip-
/// flops, and block RAMs cover the "does this design fit?" question for the FPGA
/// families we target without pulling in vendor-specific detail. A simulator
/// advertises effectively unbounded budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudget {
    pub luts: u32,
    pub ffs: u32,
    pub brams: u32,
}

impl ResourceBudget {
    pub const fn new(luts: u32, ffs: u32, brams: u32) -> Self {
        Self { luts, ffs, brams }
    }

    /// A budget large enough to hold anything — used by simulator fabric.
    pub const UNBOUNDED: Self = Self {
        luts: u32::MAX,
        ffs: u32::MAX,
        brams: u32::MAX,
    };

    /// Does a design needing `req` fit within this budget?
    pub const fn fits(&self, req: &ResourceBudget) -> bool {
        req.luts <= self.luts && req.ffs <= self.ffs && req.brams <= self.brams
    }

    /// Remaining budget after subtracting `used`, saturating at zero.
    pub fn minus(&self, used: &ResourceBudget) -> ResourceBudget {
        ResourceBudget {
            luts: self.luts.saturating_sub(used.luts),
            ffs: self.ffs.saturating_sub(used.ffs),
            brams: self.brams.saturating_sub(used.brams),
        }
    }
}

/// What a node broadcasts to say "I have spare fabric and I'm willing to host."
///
/// The presence of this advertisement is the first half of consent: a node that
/// never emits a `FabricCapability` is never a placement target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricCapability {
    /// Stable node identity (public-key fingerprint, 32 bytes). Placement
    /// offers are addressed and signed against this.
    pub node_id: [u8; 32],
    pub class: FabricClass,
    /// Total fabric the node is lending (not its whole chip — whatever it
    /// chooses to offer).
    pub total: ResourceBudget,
    /// Currently free portion of `total`.
    pub free: ResourceBudget,
    /// Max bitstream bytes the node will accept — bounds memory on tiny nodes
    /// and lets a sender skip a target that can't hold the image.
    pub max_image_bytes: u32,
    /// Monotonic counter; a newer advertisement supersedes an older one from
    /// the same node. Prevents stale "I'm free" claims from lingering.
    pub epoch: u64,
}

impl FabricCapability {
    /// Can this node, right now, host a design of the given size and footprint?
    pub fn can_host(&self, image_bytes: u32, footprint: &ResourceBudget) -> bool {
        image_bytes <= self.max_image_bytes && self.free.fits(footprint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_fit() {
        let board = ResourceBudget::new(7680, 7680, 32); // iCE40HX8K-ish
        let aes = ResourceBudget::new(971, 399, 20); // measured AES-128 core
        assert!(board.fits(&aes));

        let hx1k = ResourceBudget::new(1280, 1280, 16);
        assert!(!hx1k.fits(&aes)); // 20 BRAM > 16 — matches our real finding
    }

    #[test]
    fn simulator_hosts_anything() {
        let sim = FabricCapability {
            node_id: [0u8; 32],
            class: FabricClass::Simulator,
            total: ResourceBudget::UNBOUNDED,
            free: ResourceBudget::UNBOUNDED,
            max_image_bytes: u32::MAX,
            epoch: 0,
        };
        assert!(sim.can_host(1_000_000, &ResourceBudget::new(999_999, 1, 1)));
    }

    #[test]
    fn minus_saturates() {
        let b = ResourceBudget::new(100, 100, 4);
        let r = b.minus(&ResourceBudget::new(150, 10, 8));
        assert_eq!(r, ResourceBudget::new(0, 90, 0));
    }

    #[test]
    fn class_code_roundtrip() {
        for c in [
            FabricClass::Ice40,
            FabricClass::Ecp5,
            FabricClass::Xilinx7,
            FabricClass::Simulator,
        ] {
            assert_eq!(FabricClass::from_code(c.code()), Some(c));
            assert_eq!(FabricClass::from_str(c.as_str()), Some(c));
        }
    }
}

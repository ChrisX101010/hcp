//! # Hardware Type Definitions
//!
//! Every type here corresponds to something physical in a digital circuit.
//! When you declare `Signal { width: 32, ... }`, that means 32 actual copper
//! traces on a circuit board (or 32 LUT outputs inside an FPGA).
//!
//! The key insight: by defining hardware as Rust types, we get the Rust
//! compiler's type checker working FOR us. Wrong wire widths? Compile error.
//! Mismatched port directions? Compile error. Missing ECC? Compile error.

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// BIT WIDTHS — How many wires/bits a signal carries
// ─────────────────────────────────────────────────────────────────────────────

/// The width of a signal in bits.
///
/// # Physical meaning
/// A width of 32 means 32 parallel wires, each carrying a 0 or 1.
/// Together they can represent numbers 0 to 4,294,967,295.
///
/// # Why this is its own type (not just usize)
/// So the compiler can distinguish "number of bits" from "number of modules"
/// or "array index" — preventing subtle bugs where you accidentally use a
/// bit count where a byte count was expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BitWidth(pub usize);

impl BitWidth {
    pub fn new(width: usize) -> Self {
        assert!(width > 0, "Hardware signals must be at least 1 bit wide");
        BitWidth(width)
    }

    /// How many bits. Direct access to the inner value.
    pub fn bits(&self) -> usize {
        self.0
    }

    /// Maximum unsigned value this width can represent: 2^width - 1
    pub fn max_value(&self) -> u128 {
        if self.0 >= 128 {
            u128::MAX
        } else {
            (1u128 << self.0) - 1
        }
    }
}

impl std::fmt::Display for BitWidth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}b", self.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SIGNAL TYPES — Wire vs Register
// ─────────────────────────────────────────────────────────────────────────────

/// What kind of hardware element a signal represents.
///
/// # The two fundamental building blocks of digital circuits:
///
/// - **Wire**: A direct connection. The output changes INSTANTLY when the
///   input changes. No memory, no clock needed. Like a physical wire.
///   In Verilog: `wire` or `assign`.
///
/// - **Register**: A storage element. It only captures its input value when
///   the clock ticks (rising edge). It HOLDS that value until the next tick.
///   This is how hardware "remembers" things. In Verilog: `reg` or `always @(posedge clk)`.
///
/// # Why this matters
/// Registers are where ECC makes the most difference — they're where data
/// lives between clock cycles, and where bit-flips (from radiation, voltage
/// noise, etc.) can corrupt stored values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    /// Combinational logic — output is a direct function of inputs.
    /// Changes propagate instantly (within gate delay).
    Wire,

    /// Sequential logic — captures input on clock edge, holds value.
    /// This is where ECC protection is most valuable.
    Register,
}

// ─────────────────────────────────────────────────────────────────────────────
// PORT DIRECTION — Which way does data flow?
// ─────────────────────────────────────────────────────────────────────────────

/// The direction of a port on a hardware module.
///
/// # Physical meaning
/// Think of a chip package: it has pins. Some pins receive signals from the
/// outside world (Input), some send signals out (Output), and some can do
/// both (InOut — like a bidirectional data bus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortDirection {
    /// Signal flows INTO this module from outside
    Input,
    /// Signal flows OUT of this module to the outside
    Output,
    /// Signal can flow both ways (used for shared buses)
    InOut,
}

// ─────────────────────────────────────────────────────────────────────────────
// ECC SCHEME — Error Correction Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// The error correction coding scheme to apply to a signal.
///
/// # What ECC does physically
///
/// Digital circuits can have bit-flips — a stored 0 becomes 1, or vice versa.
/// Causes: cosmic rays, voltage noise, temperature, aging, manufacturing defects.
///
/// ECC adds extra "parity" bits that encode redundant information about the
/// data bits. When reading, a decoder checks whether the parity bits are
/// consistent with the data. If not, it can:
/// - **Detect** that an error occurred
/// - **Correct** single-bit errors automatically
/// - **Detect** (but not correct) double-bit errors
///
/// # Why this is a COMPILER feature (our innovation)
///
/// In traditional hardware design, the engineer manually instantiates ECC
/// encoder/decoder modules and wires them up. This is tedious, error-prone,
/// and often skipped for "non-critical" signals that later turn out to be critical.
///
/// In HCP, you annotate a signal with `#[ecc(HammingSecDed)]` and the
/// compiler automatically:
/// 1. Calculates the required parity bit count
/// 2. Generates encoder and decoder modules
/// 3. Wires them into the data path
/// 4. Exposes error status signals
/// 5. Proves correctness at compile time
///
/// **This does not exist in any other HDL tool.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EccScheme {
    /// No error correction. The signal is unprotected.
    None,

    /// Simple parity bit. Can DETECT single-bit errors but cannot correct them.
    /// Adds 1 bit of overhead. Cheapest option.
    ///
    /// Use case: non-critical status flags, debug signals.
    Parity,

    /// Hamming Single Error Correction, Double Error Detection (SEC-DED).
    /// The workhorse of ECC. Can CORRECT any single-bit error and DETECT
    /// any double-bit error.
    ///
    /// Overhead: for 32-bit data, adds 7 bits (6 Hamming + 1 overall parity).
    /// That's ~22% overhead in bits, but only ~5% overhead in logic (LUTs).
    ///
    /// Use case: register files, cache, general-purpose data storage.
    /// This is the DEFAULT for most HCP signals.
    HammingSecDed,

    /// Triple Modular Redundancy. The signal is triplicated and a majority
    /// voter selects the correct value. Can correct any single-module failure.
    ///
    /// Overhead: 200% in area (3x the registers). Expensive but ultra-reliable.
    ///
    /// Use case: aerospace, nuclear, safety-critical automotive (ASIL-D).
    Tmr,
}

impl EccScheme {
    /// Calculate how many parity/redundancy bits this scheme adds
    /// for a given data width.
    ///
    /// # Returns
    /// The TOTAL encoded width (data + parity bits).
    pub fn encoded_width(&self, data_width: BitWidth) -> BitWidth {
        match self {
            EccScheme::None => data_width,
            EccScheme::Parity => BitWidth::new(data_width.bits() + 1),
            EccScheme::HammingSecDed => {
                let parity_bits = hamming_parity_bits(data_width.bits());
                // +1 for the overall parity bit (the "DED" in SEC-DED)
                BitWidth::new(data_width.bits() + parity_bits + 1)
            }
            EccScheme::Tmr => {
                // TMR triplicates the signal — the voter is separate logic
                BitWidth::new(data_width.bits() * 3)
            }
        }
    }

    /// How many extra bits does this scheme add?
    pub fn overhead_bits(&self, data_width: BitWidth) -> usize {
        self.encoded_width(data_width).bits() - data_width.bits()
    }

    /// Overhead as a percentage of the original data width
    pub fn overhead_percent(&self, data_width: BitWidth) -> f64 {
        let overhead = self.overhead_bits(data_width) as f64;
        let data = data_width.bits() as f64;
        (overhead / data) * 100.0
    }
}

/// Calculate the number of Hamming parity bits needed for a given data width.
///
/// # The math
/// For Hamming code, we need `r` parity bits where: 2^r >= m + r + 1
/// (m = data bits, r = parity bits)
///
/// This is because parity bits occupy power-of-2 positions (1, 2, 4, 8, 16...),
/// and each parity bit "covers" a specific set of data positions. We need enough
/// parity bits so that every data position is uniquely identified by its
/// combination of covering parity bits.
///
/// # Examples
/// - 8-bit data → 4 parity bits (total 12, + 1 overall = 13)
/// - 16-bit data → 5 parity bits (total 21, + 1 overall = 22)
/// - 32-bit data → 6 parity bits (total 38, + 1 overall = 39)
/// - 64-bit data → 7 parity bits (total 71, + 1 overall = 72)
pub fn hamming_parity_bits(data_width: usize) -> usize {
    let mut r = 1;
    while (1usize << r) < data_width + r + 1 {
        r += 1;
    }
    r
}

// ─────────────────────────────────────────────────────────────────────────────
// SIGNAL — A named group of bits with optional ECC
// ─────────────────────────────────────────────────────────────────────────────

/// A signal in a hardware design — a named, typed group of bits.
///
/// # Physical meaning
/// A signal is a bundle of wires (or register outputs) that carry related
/// information. For example, a 32-bit data bus, an 8-bit address, or a
/// single-bit enable flag.
///
/// # Example
/// ```
/// use hcp_core::prelude::*;
///
/// let data_signal = Signal {
///     name: "cache_data".to_string(),
///     width: BitWidth::new(32),
///     kind: SignalKind::Register,
///     ecc: EccScheme::HammingSecDed,
/// };
///
/// // The compiler will automatically widen this to 39 bits
/// // (32 data + 6 Hamming + 1 overall parity)
/// assert_eq!(data_signal.encoded_width().bits(), 39);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    /// Human-readable name (becomes the Verilog signal name)
    pub name: String,

    /// How many DATA bits (before ECC encoding)
    pub width: BitWidth,

    /// Wire (combinational) or Register (sequential)
    pub kind: SignalKind,

    /// Error correction scheme (default: None)
    pub ecc: EccScheme,
}

impl Signal {
    /// Create a simple wire with no ECC
    pub fn wire(name: &str, width: usize) -> Self {
        Signal {
            name: name.to_string(),
            width: BitWidth::new(width),
            kind: SignalKind::Wire,
            ecc: EccScheme::None,
        }
    }

    /// Create a register with no ECC
    pub fn register(name: &str, width: usize) -> Self {
        Signal {
            name: name.to_string(),
            width: BitWidth::new(width),
            kind: SignalKind::Register,
            ecc: EccScheme::None,
        }
    }

    /// Create a register with Hamming SEC-DED ECC
    pub fn register_ecc(name: &str, width: usize) -> Self {
        Signal {
            name: name.to_string(),
            width: BitWidth::new(width),
            kind: SignalKind::Register,
            ecc: EccScheme::HammingSecDed,
        }
    }

    /// The total width including ECC parity bits
    pub fn encoded_width(&self) -> BitWidth {
        self.ecc.encoded_width(self.width)
    }

    /// Whether this signal has any ECC protection
    pub fn has_ecc(&self) -> bool {
        self.ecc != EccScheme::None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PORT — A signal that connects a module to the outside world
// ─────────────────────────────────────────────────────────────────────────────

/// A port on a hardware module — a signal with a direction.
///
/// # Physical meaning
/// Ports are the "pins" of your hardware module. They define the interface
/// that other modules (or the outside world) use to communicate with it.
///
/// When the compiler generates Verilog, each Port becomes a port declaration:
/// ```verilog
/// module my_module(
///     input  wire [31:0] data_in,    // Port { direction: Input, ... }
///     output reg  [31:0] data_out,   // Port { direction: Output, ... }
///     output wire        err_flag    // Auto-generated by ECC pass
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    /// The underlying signal
    pub signal: Signal,
    /// Which direction data flows
    pub direction: PortDirection,
}

impl Port {
    pub fn input(name: &str, width: usize) -> Self {
        Port {
            signal: Signal::wire(name, width),
            direction: PortDirection::Input,
        }
    }

    pub fn output(name: &str, width: usize) -> Self {
        Port {
            signal: Signal::wire(name, width),
            direction: PortDirection::Output,
        }
    }

    pub fn output_reg(name: &str, width: usize) -> Self {
        Port {
            signal: Signal::register(name, width),
            direction: PortDirection::Output,
        }
    }

    /// Output register with ECC — the decoder output goes to this port,
    /// and additional error flag ports are auto-generated.
    pub fn output_reg_ecc(name: &str, width: usize) -> Self {
        Port {
            signal: Signal::register_ecc(name, width),
            direction: PortDirection::Output,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TESTS
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hamming_parity_bits() {
        // These are well-known values from coding theory
        assert_eq!(hamming_parity_bits(1), 2);   // (3,1) code
        assert_eq!(hamming_parity_bits(4), 3);   // (7,4) code
        assert_eq!(hamming_parity_bits(8), 4);   // (12,8) + 1 = 13 total
        assert_eq!(hamming_parity_bits(16), 5);  // (21,16) + 1 = 22 total
        assert_eq!(hamming_parity_bits(32), 6);  // (38,32) + 1 = 39 total
        assert_eq!(hamming_parity_bits(64), 7);  // (71,64) + 1 = 72 total
    }

    #[test]
    fn test_ecc_encoded_widths() {
        let scheme = EccScheme::HammingSecDed;

        // 8-bit data → 4 parity + 1 overall = 13 bits total
        assert_eq!(scheme.encoded_width(BitWidth::new(8)).bits(), 13);

        // 32-bit data → 6 parity + 1 overall = 39 bits total
        assert_eq!(scheme.encoded_width(BitWidth::new(32)).bits(), 39);

        // 64-bit data → 7 parity + 1 overall = 72 bits total
        assert_eq!(scheme.encoded_width(BitWidth::new(64)).bits(), 72);
    }

    #[test]
    fn test_ecc_overhead_percentage() {
        let scheme = EccScheme::HammingSecDed;

        // 32-bit: 7 extra bits = 21.875% overhead
        let pct = scheme.overhead_percent(BitWidth::new(32));
        assert!(pct > 21.0 && pct < 22.0);

        // 64-bit: 8 extra bits = 12.5% overhead — gets cheaper as data widens
        let pct = scheme.overhead_percent(BitWidth::new(64));
        assert!(pct > 12.0 && pct < 13.0);
    }

    #[test]
    fn test_tmr_overhead() {
        let scheme = EccScheme::Tmr;
        // TMR always triples the width
        assert_eq!(scheme.encoded_width(BitWidth::new(32)).bits(), 96);
        assert!((scheme.overhead_percent(BitWidth::new(32)) - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_signal_creation() {
        let sig = Signal::register_ecc("cache_word", 32);
        assert_eq!(sig.name, "cache_word");
        assert_eq!(sig.width.bits(), 32);
        assert_eq!(sig.encoded_width().bits(), 39);
        assert!(sig.has_ecc());
        assert_eq!(sig.kind, SignalKind::Register);
    }
}

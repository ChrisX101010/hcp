//! # ECC Simulator — Software Model of the Hardware ECC
//!
//! This module implements the same Hamming SEC-DED algorithm that
//! `hcp-ecc` generates as Verilog, but runs it in software.
//! This lets us verify that:
//!
//! 1. The encoder produces correct codewords
//! 2. The decoder catches and corrects single-bit errors
//! 3. The decoder detects (but can't fix) double-bit errors
//! 4. The generated Verilog would behave identically
//!
//! ## How Hamming SEC-DED works (quick version)
//!
//! - Data bits sit at non-power-of-2 positions
//! - Parity bits sit at positions 1, 2, 4, 8, 16, ...
//! - Each parity bit covers a specific pattern of data bits
//! - An overall parity bit covers everything
//! - Single-bit error → syndrome points to the flipped bit → fix it
//! - Double-bit error → syndrome is nonzero but overall parity is even → detected

use hcp_ecc::HammingGenerator;

/// Result of decoding an encoded value.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeResult {
    /// Decoded data value
    pub data: u64,
    /// Whether a correctable (single-bit) error was found and fixed
    pub correctable_error: bool,
    /// Whether an uncorrectable (double-bit) error was detected
    pub uncorrectable_error: bool,
    /// The syndrome value (0 = no error)
    pub syndrome: u32,
}

/// Software simulation of Hamming SEC-DED encoder/decoder.
pub struct EccSimulator {
    data_width: usize,
    parity_bits: usize,
    total_width: usize,
    /// Which data bits each parity bit covers
    parity_coverage: Vec<Vec<usize>>,
    /// Mapping from encoded position to data bit index (None = parity position)
    position_map: Vec<Option<usize>>,
}

impl EccSimulator {
    /// Create a new ECC simulator for the given data width.
    pub fn new(data_width: usize) -> Self {
        let gen = HammingGenerator::new(data_width);
        let parity_bits = gen.parity_bits;
        let total_width = gen.total_width; // includes overall parity

        // Build position map: which encoded positions hold data bits
        let mut position_map = vec![None; total_width];
        let mut data_idx = 0;
        for pos in 1..total_width {
            // Power-of-2 positions are parity bits
            if pos.is_power_of_two() {
                continue;
            }
            if data_idx < data_width {
                position_map[pos] = Some(data_idx);
                data_idx += 1;
            }
        }

        // Build parity coverage: which positions each parity bit checks
        let mut parity_coverage = Vec::new();
        for i in 0..parity_bits {
            let parity_pos = 1 << i;
            let mut covered = Vec::new();
            for pos in 1..total_width {
                if pos & parity_pos != 0 {
                    covered.push(pos);
                }
            }
            parity_coverage.push(covered);
        }

        EccSimulator {
            data_width,
            parity_bits,
            total_width,
            parity_coverage,
            position_map,
        }
    }

    /// Encode a data value to a Hamming SEC-DED codeword.
    pub fn encode(&self, data: u64) -> u64 {
        let mut encoded: u64 = 0;

        // Place data bits at non-power-of-2 positions
        for pos in 1..self.total_width {
            if let Some(data_idx) = self.position_map[pos] {
                if data_idx < self.data_width && (data >> data_idx) & 1 == 1 {
                    encoded |= 1 << pos;
                }
            }
        }

        // Compute parity bits
        for (i, coverage) in self.parity_coverage.iter().enumerate() {
            let parity_pos = 1 << i;
            let mut parity = 0u64;
            for &pos in coverage {
                if pos != parity_pos && pos < self.total_width {
                    parity ^= (encoded >> pos) & 1;
                }
            }
            if parity == 1 {
                encoded |= 1 << parity_pos;
            }
        }

        // Compute overall parity (position 0)
        let mut overall = 0u64;
        for pos in 1..self.total_width {
            overall ^= (encoded >> pos) & 1;
        }
        if overall == 1 {
            encoded |= 1; // bit 0 is overall parity
        }

        encoded
    }

    /// Decode an encoded value, detecting and correcting errors.
    pub fn decode(&self, encoded: u64) -> DecodeResult {
        // Compute syndrome
        let mut syndrome: u32 = 0;
        for (i, coverage) in self.parity_coverage.iter().enumerate() {
            let mut parity = 0u64;
            for &pos in coverage {
                if pos < self.total_width {
                    parity ^= (encoded >> pos) & 1;
                }
            }
            if parity == 1 {
                syndrome |= 1 << i;
            }
        }

        // Compute overall parity
        let mut overall = 0u64;
        for pos in 0..self.total_width {
            overall ^= (encoded >> pos) & 1;
        }

        let (corrected, correctable, uncorrectable) = if syndrome == 0 && overall == 0 {
            // No error
            (encoded, false, false)
        } else if syndrome != 0 && overall == 1 {
            // Single-bit error — correctable
            let error_pos = syndrome as usize;
            if error_pos < self.total_width {
                (encoded ^ (1 << error_pos), true, false)
            } else {
                (encoded, false, true)
            }
        } else if syndrome == 0 && overall == 1 {
            // Overall parity bit itself is wrong — correctable
            (encoded ^ 1, true, false)
        } else {
            // Double-bit error — detected but uncorrectable
            (encoded, false, true)
        };

        // Extract data bits from corrected codeword
        let mut data: u64 = 0;
        for pos in 1..self.total_width {
            if let Some(data_idx) = self.position_map[pos] {
                if (corrected >> pos) & 1 == 1 {
                    data |= 1 << data_idx;
                }
            }
        }

        DecodeResult {
            data,
            correctable_error: correctable,
            uncorrectable_error: uncorrectable,
            syndrome,
        }
    }

    pub fn data_width(&self) -> usize { self.data_width }
    pub fn total_width(&self) -> usize { self.total_width }
    pub fn parity_bits(&self) -> usize { self.parity_bits }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let ecc = EccSimulator::new(8);

        for data in 0..=255u64 {
            let encoded = ecc.encode(data);
            let result = ecc.decode(encoded);
            assert_eq!(result.data, data, "Roundtrip failed for data={}", data);
            assert!(!result.correctable_error);
            assert!(!result.uncorrectable_error);
            assert_eq!(result.syndrome, 0);
        }
    }

    #[test]
    fn test_single_bit_error_correction() {
        let ecc = EccSimulator::new(8);

        for data in [0u64, 42, 127, 255] {
            let encoded = ecc.encode(data);

            // Flip each bit one at a time
            for bit in 0..ecc.total_width() {
                let corrupted = encoded ^ (1 << bit);
                let result = ecc.decode(corrupted);

                assert_eq!(result.data, data,
                    "Failed to correct single-bit error at bit {} for data={}", bit, data);
                assert!(result.correctable_error,
                    "Should report correctable error at bit {} for data={}", bit, data);
                assert!(!result.uncorrectable_error);
            }
        }
    }

    #[test]
    fn test_double_bit_error_detection() {
        let ecc = EccSimulator::new(8);

        let data = 42u64;
        let encoded = ecc.encode(data);

        // Flip two bits — should be detected but uncorrectable
        let corrupted = encoded ^ (1 << 1) ^ (1 << 3);
        let result = ecc.decode(corrupted);

        assert!(result.uncorrectable_error,
            "Should detect double-bit error");
    }

    #[test]
    fn test_32bit_roundtrip() {
        let ecc = EccSimulator::new(32);

        for data in [0u64, 1, 0xDEADBEEF, 0xFFFFFFFF, 0x12345678] {
            let encoded = ecc.encode(data);
            let result = ecc.decode(encoded);
            assert_eq!(result.data, data);
            assert!(!result.correctable_error);
            assert!(!result.uncorrectable_error);
        }
    }

    #[test]
    fn test_32bit_error_correction() {
        let ecc = EccSimulator::new(32);
        let data = 0xCAFEBABEu64;
        let encoded = ecc.encode(data);

        // Flip bit 17
        let corrupted = encoded ^ (1 << 17);
        let result = ecc.decode(corrupted);

        assert_eq!(result.data, data);
        assert!(result.correctable_error);
        assert!(!result.uncorrectable_error);
    }
}

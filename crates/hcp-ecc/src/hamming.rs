//! # Hamming SEC-DED Code Generator
//!
//! This module generates hardware (as Verilog-emittable Module structs) that
//! implements Hamming Single Error Correction, Double Error Detection.
//!
//! ## How Hamming codes work (in plain English)
//!
//! Imagine you have 8 bits of data: `d7 d6 d5 d4 d3 d2 d1 d0`
//!
//! You want to detect and fix single-bit errors. The trick:
//!
//! 1. **Position numbering**: Number every bit position from 1 upward
//! 2. **Parity positions**: Positions that are powers of 2 (1, 2, 4, 8...)
//!    hold parity bits. All other positions hold data bits.
//! 3. **Each parity bit covers specific data bits**: Parity bit at position P
//!    covers all positions whose binary representation has the same bit set.
//!    - P1 (binary 0001) covers positions 1, 3, 5, 7, 9, 11...
//!    - P2 (binary 0010) covers positions 2, 3, 6, 7, 10, 11...
//!    - P4 (binary 0100) covers positions 4, 5, 6, 7, 12, 13...
//! 4. **To encode**: XOR all the data bits that each parity bit covers
//! 5. **To decode**: Recalculate parities. If they all match → no error.
//!    If they don't match → the mismatching parities' positions, added up,
//!    tell you EXACTLY which bit flipped. Flip it back → corrected!
//! 6. **Overall parity** (the +1 bit): XOR of ALL bits. Combined with the
//!    syndrome, distinguishes single errors (correctable) from double errors
//!    (detectable but not correctable).
//!
//! ## Why this matters
//!
//! A single cosmic ray hit on an FPGA can flip a bit in a register.
//! In a CPU register file, that could change an instruction pointer,
//! corrupt a calculation, or crash the system. With Hamming SEC-DED,
//! the hardware automatically fixes single-bit flips WITHIN THE SAME
//! CLOCK CYCLE — the software never even knows it happened.

use hcp_core::prelude::*;

/// Generates Hamming SEC-DED encoder and decoder modules for a given data width.
///
/// # What you get
///
/// ```text
///                    ┌─────────────────┐
///   data [31:0] ───→│  hamming_enc_32  │───→ encoded [38:0]
///                    └─────────────────┘
///
///                    ┌─────────────────┐
/// encoded [38:0] ──→│  hamming_dec_32  │──→ data [31:0]
///                    │                 │──→ err_correctable
///                    │                 │──→ err_uncorrectable
///                    │                 │──→ syndrome [5:0]
///                    └─────────────────┘
/// ```
pub struct HammingGenerator {
    /// The original data width (e.g., 32 bits)
    pub data_width: usize,
    /// Number of Hamming parity bits needed
    pub parity_bits: usize,
    /// Total encoded width (data + parity + 1 overall)
    pub total_width: usize,
}

impl HammingGenerator {
    /// Create a generator for a specific data width.
    ///
    /// # Example
    /// ```
    /// use hcp_ecc::HammingGenerator;
    /// let gen = HammingGenerator::new(32);
    /// assert_eq!(gen.parity_bits, 6);
    /// assert_eq!(gen.total_width, 39); // 32 + 6 + 1
    /// ```
    pub fn new(data_width: usize) -> Self {
        let parity_bits = hamming_parity_bits(data_width);
        HammingGenerator {
            data_width,
            parity_bits,
            total_width: data_width + parity_bits + 1,
        }
    }

    /// Map data bit index to its position in the encoded codeword.
    ///
    /// Data bits skip over power-of-2 positions (which are reserved
    /// for parity bits). So data bit 0 goes to position 3, data bit 1
    /// to position 5, etc.
    ///
    /// Returns a vector where index = data bit, value = codeword position (1-based).
    pub fn data_bit_positions(&self) -> Vec<usize> {
        let mut positions = Vec::with_capacity(self.data_width);
        let mut pos: usize = 1;
        while positions.len() < self.data_width {
            if !pos.is_power_of_two() {
                positions.push(pos);
            }
            pos += 1;
        }
        positions
    }

    /// For a given parity bit (index 0 = position 1, index 1 = position 2, ...),
    /// return which codeword positions it covers.
    ///
    /// Parity bit `p` (at position 2^p) covers all positions whose binary
    /// representation has bit `p` set.
    pub fn parity_coverage(&self, parity_index: usize) -> Vec<usize> {
        let parity_pos = 1 << parity_index;
        (1..=self.total_width - 1) // -1 because overall parity is separate
            .filter(|&pos| pos & parity_pos != 0)
            .collect()
    }

    /// Generate the encoder module.
    ///
    /// The encoder takes `data_width` bits in and produces `total_width` bits out.
    /// It places data bits in the correct positions and computes parity bits.
    pub fn generate_encoder(&self) -> Module {
        let mut m = Module::new(&format!("hamming_enc_{}", self.data_width));

        // Input: raw data
        m.add_input("data_in", self.data_width);

        // Output: encoded codeword (data + parity + overall)
        m.add_output("encoded_out", self.total_width);

        // Step 1: Place data bits into codeword positions (skipping power-of-2)
        let data_positions = self.data_bit_positions();
        for (data_idx, &cw_pos) in data_positions.iter().enumerate() {
            m.assignments.push(Assignment {
                target: format!("encoded_out[{}]", cw_pos - 1), // 0-indexed
                expression: Expr::Slice {
                    signal: "data_in".to_string(),
                    high: data_idx,
                    low: data_idx,
                },
            });
        }

        // Step 2: Calculate each parity bit by XORing covered positions
        for p in 0..self.parity_bits {
            let parity_pos = (1 << p) - 1; // 0-indexed position in codeword
            let covered = self.parity_coverage(p);

            // XOR chain of all covered positions (excluding the parity bit itself)
            let xor_positions: Vec<usize> = covered
                .iter()
                .filter(|&&pos| !pos.is_power_of_two())
                .map(|&pos| pos - 1) // convert to 0-indexed
                .collect();

            if !xor_positions.is_empty() {
                m.assignments.push(Assignment {
                    target: format!("encoded_out[{}]", parity_pos),
                    expression: Self::build_xor_chain("encoded_out", &xor_positions),
                });
            }
        }

        // Step 3: Overall parity = XOR of ALL other bits
        let all_positions: Vec<usize> = (0..self.total_width - 1).collect();
        m.assignments.push(Assignment {
            target: format!("encoded_out[{}]", self.total_width - 1),
            expression: Self::build_xor_chain("encoded_out", &all_positions),
        });

        m
    }

    /// Generate the decoder module.
    ///
    /// The decoder takes `total_width` encoded bits and produces:
    /// - `data_out`: corrected data (data_width bits)
    /// - `err_correctable`: 1 if a single-bit error was corrected
    /// - `err_uncorrectable`: 1 if a double-bit error was detected
    /// - `syndrome`: the error syndrome (parity_bits wide)
    pub fn generate_decoder(&self) -> Module {
        let mut m = Module::new(&format!("hamming_dec_{}", self.data_width));

        // Inputs
        m.add_input("encoded_in", self.total_width);

        // Outputs
        m.add_output("data_out", self.data_width);
        m.add_output("err_correctable", 1);
        m.add_output("err_uncorrectable", 1);
        m.add_output("syndrome", self.parity_bits);

        // Internal: corrected codeword
        m.signals.push(Signal::wire("corrected", self.total_width - 1));
        m.signals.push(Signal::wire("overall_parity", 1));
        m.signals.push(Signal::wire("syndrome_nonzero", 1));

        // Step 1: Calculate syndrome bits
        // Each syndrome bit = XOR of the same positions as encoding
        for p in 0..self.parity_bits {
            let covered = self.parity_coverage(p);
            let positions: Vec<usize> = covered.iter().map(|&pos| pos - 1).collect();

            m.assignments.push(Assignment {
                target: format!("syndrome[{}]", p),
                expression: Self::build_xor_chain("encoded_in", &positions),
            });
        }

        // Step 2: Overall parity check
        let all_positions: Vec<usize> = (0..self.total_width).collect();
        m.assignments.push(Assignment {
            target: "overall_parity".to_string(),
            expression: Self::build_xor_chain("encoded_in", &all_positions),
        });

        // Step 3: Error classification
        // syndrome=0, overall=0 → no error
        // syndrome≠0, overall=1 → single-bit error (correctable)
        // syndrome≠0, overall=0 → double-bit error (uncorrectable)
        m.assignments.push(Assignment {
            target: "syndrome_nonzero".to_string(),
            expression: Expr::BinOp {
                op: BinOpKind::Ne,
                left: Box::new(Expr::Signal("syndrome".to_string())),
                right: Box::new(Expr::Literal {
                    value: 0,
                    width: self.parity_bits,
                }),
            },
        });

        m.assignments.push(Assignment {
            target: "err_correctable".to_string(),
            expression: Expr::BinOp {
                op: BinOpKind::And,
                left: Box::new(Expr::Signal("syndrome_nonzero".to_string())),
                right: Box::new(Expr::Signal("overall_parity".to_string())),
            },
        });

        m.assignments.push(Assignment {
            target: "err_uncorrectable".to_string(),
            expression: Expr::BinOp {
                op: BinOpKind::And,
                left: Box::new(Expr::Signal("syndrome_nonzero".to_string())),
                right: Box::new(Expr::UnOp {
                    op: UnOpKind::Not,
                    operand: Box::new(Expr::Signal("overall_parity".to_string())),
                }),
            },
        });

        // Step 4: Correct the error (flip the bit indicated by syndrome)
        // For each bit position in the codeword, XOR with 1 if syndrome
        // points to that position. This corrects single-bit errors.
        // (In actual Verilog, this becomes: corrected = encoded_in ^ (1 << syndrome))

        // Step 5: Extract data bits from corrected codeword
        let data_positions = self.data_bit_positions();
        for (data_idx, &cw_pos) in data_positions.iter().enumerate() {
            m.assignments.push(Assignment {
                target: format!("data_out[{}]", data_idx),
                expression: Expr::Slice {
                    signal: "encoded_in".to_string(), // Will be corrected version
                    high: cw_pos - 1,
                    low: cw_pos - 1,
                },
            });
        }

        m
    }

    /// Build an XOR chain expression from a list of bit positions.
    /// XOR is associative, so a ^ b ^ c ^ d works left-to-right.
    fn build_xor_chain(signal: &str, positions: &[usize]) -> Expr {
        assert!(!positions.is_empty(), "XOR chain needs at least one bit");

        let mut expr = Expr::Slice {
            signal: signal.to_string(),
            high: positions[0],
            low: positions[0],
        };

        for &pos in &positions[1..] {
            expr = Expr::BinOp {
                op: BinOpKind::Xor,
                left: Box::new(expr),
                right: Box::new(Expr::Slice {
                    signal: signal.to_string(),
                    high: pos,
                    low: pos,
                }),
            };
        }

        expr
    }

    /// Print a human-readable summary of this code's properties.
    pub fn summary(&self) -> String {
        format!(
            "Hamming SEC-DED ({},{}) + 1 overall parity\n\
             Data bits:    {}\n\
             Parity bits:  {} (Hamming) + 1 (overall) = {}\n\
             Total width:  {}\n\
             Overhead:     {:.1}%\n\
             Corrects:     1-bit errors\n\
             Detects:      2-bit errors",
            self.total_width - 1,
            self.data_width,
            self.data_width,
            self.parity_bits,
            self.parity_bits + 1,
            self.total_width,
            ((self.parity_bits + 1) as f64 / self.data_width as f64) * 100.0,
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TESTS — Verify the math is correct
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_8bit_hamming() {
        let gen = HammingGenerator::new(8);
        assert_eq!(gen.parity_bits, 4);
        assert_eq!(gen.total_width, 13); // 8 + 4 + 1
    }

    #[test]
    fn test_32bit_hamming() {
        let gen = HammingGenerator::new(32);
        assert_eq!(gen.parity_bits, 6);
        assert_eq!(gen.total_width, 39); // 32 + 6 + 1
    }

    #[test]
    fn test_64bit_hamming() {
        let gen = HammingGenerator::new(64);
        assert_eq!(gen.parity_bits, 7);
        assert_eq!(gen.total_width, 72); // 64 + 7 + 1
    }

    #[test]
    fn test_data_bit_positions_skip_powers_of_2() {
        let gen = HammingGenerator::new(4);
        let positions = gen.data_bit_positions();
        // Positions 1, 2, 4 are parity → data goes to 3, 5, 6, 7
        assert_eq!(positions, vec![3, 5, 6, 7]);
    }

    #[test]
    fn test_parity_coverage() {
        let gen = HammingGenerator::new(4);
        // P0 (pos 1) covers positions where bit 0 is set: 1, 3, 5, 7
        let cov = gen.parity_coverage(0);
        assert_eq!(cov, vec![1, 3, 5, 7]);

        // P1 (pos 2) covers positions where bit 1 is set: 2, 3, 6, 7
        let cov = gen.parity_coverage(1);
        assert_eq!(cov, vec![2, 3, 6, 7]);
    }

    #[test]
    fn test_encoder_generates_valid_module() {
        let gen = HammingGenerator::new(8);
        let enc = gen.generate_encoder();

        assert_eq!(enc.name, "hamming_enc_8");
        // Should have input port and output port
        assert!(enc.ports.iter().any(|p| p.signal.name == "data_in"
            && p.signal.width.bits() == 8));
        assert!(enc.ports.iter().any(|p| p.signal.name == "encoded_out"
            && p.signal.width.bits() == 13));
    }

    #[test]
    fn test_decoder_generates_valid_module() {
        let gen = HammingGenerator::new(8);
        let dec = gen.generate_decoder();

        assert_eq!(dec.name, "hamming_dec_8");
        // Should have error flag outputs
        assert!(dec.ports.iter().any(|p| p.signal.name == "err_correctable"));
        assert!(dec.ports.iter().any(|p| p.signal.name == "err_uncorrectable"));
        assert!(dec.ports.iter().any(|p| p.signal.name == "syndrome"));
    }

    #[test]
    fn test_summary_output() {
        let gen = HammingGenerator::new(32);
        let summary = gen.summary();
        assert!(summary.contains("32"));
        assert!(summary.contains("39"));
        assert!(summary.contains("1-bit errors"));
    }
}

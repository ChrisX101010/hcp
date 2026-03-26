//! # VCD Writer — Standard Waveform Export
//!
//! VCD (Value Change Dump) is the standard format for digital waveforms.
//! Any EDA tool can read it: GTKWave, WaveDrom, Surfer, Sigrok, etc.
//!
//! This lets users visually verify their hardware behavior —
//! see the counter counting up, see ECC error flags fire when a bit flips,
//! see the decoder correct the corrupted value.
//!
//! ## VCD format (simplified)
//!
//! ```text
//! $timescale 1ns $end
//! $scope module counter_ecc $end
//! $var wire 8 ! count [7:0] $end
//! $var wire 1 " clk $end
//! $upscope $end
//! $enddefinitions $end
//! #0
//! b00000000 !
//! 0"
//! #10
//! b00000001 !
//! 1"
//! ```

use crate::signals::SignalTrace;

/// Writes a VCD file from a signal trace.
pub struct VcdWriter {
    module_name: String,
    timescale: String,
}

impl VcdWriter {
    pub fn new(module_name: &str) -> Self {
        VcdWriter {
            module_name: module_name.to_string(),
            timescale: "1ns".to_string(),
        }
    }

    /// Set the timescale (e.g., "1ns", "10ps", "1us").
    pub fn timescale(mut self, ts: &str) -> Self {
        self.timescale = ts.to_string();
        self
    }

    /// Generate VCD content from a signal trace.
    pub fn generate(&self, trace: &SignalTrace, total_cycles: u64) -> String {
        let mut vcd = String::new();
        let signals = trace.signal_names();

        // Header
        vcd.push_str(&format!("$date\n  HCP Simulation\n$end\n"));
        vcd.push_str(&format!("$version\n  HCP Simulator 0.4.0\n$end\n"));
        vcd.push_str(&format!("$timescale {} $end\n", self.timescale));

        // Signal declarations
        vcd.push_str(&format!("$scope module {} $end\n", self.module_name));
        for (i, name) in signals.iter().enumerate() {
            let width = trace.width(name).unwrap_or(1);
            let id = vcd_id(i);
            if width == 1 {
                vcd.push_str(&format!("$var wire {} {} {} $end\n", width, id, name));
            } else {
                vcd.push_str(&format!("$var wire {} {} {} [{}:0] $end\n",
                    width, id, name, width - 1));
            }
        }
        vcd.push_str("$upscope $end\n");
        vcd.push_str("$enddefinitions $end\n");

        // Initial values
        vcd.push_str("#0\n");
        for (i, name) in signals.iter().enumerate() {
            let val = trace.value_at(name, 0).unwrap_or(0);
            let width = trace.width(name).unwrap_or(1);
            let id = vcd_id(i);
            vcd.push_str(&format_vcd_value(val, width, &id));
        }

        // Value changes
        for cycle in 1..total_cycles {
            let time_ns = cycle * 10; // 10ns per cycle (100MHz clock)
            let mut changes = String::new();

            for (i, name) in signals.iter().enumerate() {
                let prev = trace.value_at(name, cycle - 1).unwrap_or(0);
                let curr = trace.value_at(name, cycle).unwrap_or(0);
                if curr != prev {
                    let width = trace.width(name).unwrap_or(1);
                    let id = vcd_id(i);
                    changes.push_str(&format_vcd_value(curr, width, &id));
                }
            }

            if !changes.is_empty() {
                vcd.push_str(&format!("#{}\n{}", time_ns, changes));
            }
        }

        vcd
    }

    /// Generate VCD and return it as a string (for saving to file).
    pub fn to_string(&self, trace: &SignalTrace, total_cycles: u64) -> String {
        self.generate(trace, total_cycles)
    }
}

/// Generate a VCD identifier from an index.
/// Uses printable ASCII: !, ", #, $, ... up to ~
fn vcd_id(index: usize) -> String {
    let c = (b'!' + (index as u8) % 94) as char;
    c.to_string()
}

/// Format a value for VCD output.
fn format_vcd_value(value: u64, width: usize, id: &str) -> String {
    if width == 1 {
        format!("{}{}\n", if value != 0 { '1' } else { '0' }, id)
    } else {
        let binary: String = (0..width)
            .rev()
            .map(|bit| if (value >> bit) & 1 == 1 { '1' } else { '0' })
            .collect();
        format!("b{} {}\n", binary, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vcd_basic_output() {
        let mut trace = SignalTrace::new();
        trace.register("clk", 1);
        trace.register("count", 8);

        for cycle in 0..4 {
            trace.set("clk", cycle, cycle % 2);
            trace.set("count", cycle, cycle);
        }

        let writer = VcdWriter::new("counter_ecc");
        let vcd = writer.generate(&trace, 4);

        assert!(vcd.contains("$timescale 1ns $end"));
        assert!(vcd.contains("counter_ecc"));
        assert!(vcd.contains("$var wire 1"));
        assert!(vcd.contains("$var wire 8"));
        assert!(vcd.contains("#0"));
        assert!(vcd.contains("$enddefinitions"));
    }

    #[test]
    fn test_vcd_format_values() {
        assert_eq!(format_vcd_value(0, 1, "!"), "0!\n");
        assert_eq!(format_vcd_value(1, 1, "!"), "1!\n");
        assert_eq!(format_vcd_value(5, 8, "!"), "b00000101 !\n");
        assert_eq!(format_vcd_value(255, 8, "!"), "b11111111 !\n");
    }

    #[test]
    fn test_vcd_ids() {
        assert_eq!(vcd_id(0), "!");
        assert_eq!(vcd_id(1), "\"");
        assert_eq!(vcd_id(2), "#");
    }
}

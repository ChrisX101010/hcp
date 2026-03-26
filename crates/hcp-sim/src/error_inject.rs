//! # Error Injector — Scheduled Fault Injection
//!
//! This module lets you schedule bit flips at specific cycles,
//! simulating cosmic rays, memory corruption, or transmission errors.
//! The ECC system should catch and correct (or detect) these.
//!
//! This is the key differentiator: you can *prove* your ECC works
//! by deliberately breaking things and watching the decoder fix them.

/// A scheduled error injection event.
#[derive(Debug, Clone)]
pub struct InjectionEvent {
    /// Which cycle to inject the error
    pub cycle: u64,
    /// Which signal to corrupt
    pub signal: String,
    /// Which bit(s) to flip (bitmask)
    pub bit_mask: u64,
    /// Description of what this injection tests
    pub description: String,
}

/// Result of an injection — what happened when the error was injected.
#[derive(Debug, Clone)]
pub struct InjectionResult {
    pub event: InjectionEvent,
    /// The value before corruption
    pub original_value: u64,
    /// The value after corruption (before ECC)
    pub corrupted_value: u64,
    /// The value after ECC correction
    pub corrected_value: u64,
    /// Whether ECC successfully corrected the error
    pub corrected: bool,
    /// Whether ECC detected an uncorrectable error
    pub detected_uncorrectable: bool,
}

/// Manages error injection events during simulation.
pub struct ErrorInjector {
    events: Vec<InjectionEvent>,
    results: Vec<InjectionResult>,
}

impl ErrorInjector {
    pub fn new() -> Self {
        ErrorInjector {
            events: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Schedule a single-bit flip on a signal at a given cycle.
    pub fn inject_single_bit(&mut self, cycle: u64, signal: &str, bit: usize) {
        self.events.push(InjectionEvent {
            cycle,
            signal: signal.to_string(),
            bit_mask: 1 << bit,
            description: format!("Single-bit flip: {}[{}] at cycle {}", signal, bit, cycle),
        });
    }

    /// Schedule a double-bit flip (uncorrectable) at a given cycle.
    pub fn inject_double_bit(&mut self, cycle: u64, signal: &str, bit_a: usize, bit_b: usize) {
        self.events.push(InjectionEvent {
            cycle,
            signal: signal.to_string(),
            bit_mask: (1 << bit_a) | (1 << bit_b),
            description: format!(
                "Double-bit flip: {}[{},{}] at cycle {} (should be uncorrectable)",
                signal, bit_a, bit_b, cycle
            ),
        });
    }

    /// Get all injection events scheduled for a specific cycle and signal.
    pub fn events_at(&self, cycle: u64, signal: &str) -> Vec<&InjectionEvent> {
        self.events
            .iter()
            .filter(|e| e.cycle == cycle && e.signal == signal)
            .collect()
    }

    /// Apply corruption to a value based on an event's bitmask.
    pub fn corrupt(&self, value: u64, event: &InjectionEvent) -> u64 {
        value ^ event.bit_mask
    }

    /// Record the result of an injection.
    pub fn record_result(&mut self, result: InjectionResult) {
        self.results.push(result);
    }

    /// Get all recorded results.
    pub fn results(&self) -> &[InjectionResult] {
        &self.results
    }

    /// Get all scheduled events.
    pub fn events(&self) -> &[InjectionEvent] {
        &self.events
    }

    /// Generate a summary report.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("╔══════════════════════════════════════════════╗\n"));
        out.push_str(&format!("║       Error Injection Report                 ║\n"));
        out.push_str(&format!("╠══════════════════════════════════════════════╣\n"));
        out.push_str(&format!("║  Events scheduled: {:>4}                     ║\n", self.events.len()));
        out.push_str(&format!("║  Events executed:  {:>4}                     ║\n", self.results.len()));

        let corrected = self.results.iter().filter(|r| r.corrected).count();
        let detected = self.results.iter().filter(|r| r.detected_uncorrectable).count();
        let silent = self.results.len() - corrected - detected;

        out.push_str(&format!("║  Corrected (SEC):  {:>4}                     ║\n", corrected));
        out.push_str(&format!("║  Detected (DED):   {:>4}                     ║\n", detected));
        out.push_str(&format!("║  Silent failures:  {:>4}                     ║\n", silent));
        out.push_str(&format!("╚══════════════════════════════════════════════╝\n"));

        for result in &self.results {
            out.push_str(&format!("  Cycle {}: {}\n", result.event.cycle, result.event.description));
            out.push_str(&format!("    Before: 0x{:X} → Corrupted: 0x{:X} → After ECC: 0x{:X}\n",
                result.original_value, result.corrupted_value, result.corrected_value));
            if result.corrected {
                out.push_str("    Result: ✓ CORRECTED by Hamming SEC\n");
            } else if result.detected_uncorrectable {
                out.push_str("    Result: ⚠ DETECTED (uncorrectable double-bit error)\n");
            } else {
                out.push_str("    Result: ✗ UNDETECTED — this should not happen with proper ECC\n");
            }
        }

        out
    }
}

impl Default for ErrorInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_single_bit() {
        let mut injector = ErrorInjector::new();
        injector.inject_single_bit(5, "count", 3);

        let events = injector.events_at(5, "count");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].bit_mask, 0b1000);

        let corrupted = injector.corrupt(0b0000, &events[0]);
        assert_eq!(corrupted, 0b1000);
    }

    #[test]
    fn test_inject_double_bit() {
        let mut injector = ErrorInjector::new();
        injector.inject_double_bit(10, "data", 1, 5);

        let events = injector.events_at(10, "data");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].bit_mask, (1 << 1) | (1 << 5));
    }

    #[test]
    fn test_corruption_is_xor() {
        let injector = ErrorInjector::new();
        let event = InjectionEvent {
            cycle: 0,
            signal: "test".to_string(),
            bit_mask: 0xFF,
            description: "test".to_string(),
        };

        // XOR with 0xFF flips the low 8 bits
        assert_eq!(injector.corrupt(0x00, &event), 0xFF);
        assert_eq!(injector.corrupt(0xFF, &event), 0x00);
        assert_eq!(injector.corrupt(0xAA, &event), 0x55);
    }
}

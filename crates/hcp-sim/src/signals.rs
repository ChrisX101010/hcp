//! # Signal Trace — Recording Hardware State Over Time
//!
//! Every signal in the simulation is tracked cycle-by-cycle.
//! This is the equivalent of hooking up a logic analyzer to your circuit.

use std::collections::HashMap;

/// A single recorded value change.
#[derive(Debug, Clone)]
pub struct ValueChange {
    pub cycle: u64,
    pub value: u64,
}

/// Tracks the complete history of all signals in a simulation.
#[derive(Debug, Clone)]
pub struct SignalTrace {
    /// signal_name → list of value changes (sorted by cycle)
    traces: HashMap<String, Vec<ValueChange>>,
    /// Current values of all signals
    current: HashMap<String, u64>,
    /// Signal widths (in bits)
    widths: HashMap<String, usize>,
}

impl SignalTrace {
    pub fn new() -> Self {
        SignalTrace {
            traces: HashMap::new(),
            current: HashMap::new(),
            widths: HashMap::new(),
        }
    }

    /// Register a signal with its bit width.
    pub fn register(&mut self, name: &str, width: usize) {
        self.traces.entry(name.to_string()).or_default();
        self.current.insert(name.to_string(), 0);
        self.widths.insert(name.to_string(), width);
    }

    /// Set a signal value at a given cycle. Records a change only if the value differs.
    pub fn set(&mut self, name: &str, cycle: u64, value: u64) {
        let prev = self.current.get(name).copied().unwrap_or(0);
        self.current.insert(name.to_string(), value);

        // Only record if value actually changed (or it's cycle 0)
        if value != prev || cycle == 0 {
            self.traces
                .entry(name.to_string())
                .or_default()
                .push(ValueChange { cycle, value });
        }
    }

    /// Get the current value of a signal.
    pub fn get(&self, name: &str) -> Option<u64> {
        self.current.get(name).copied()
    }

    /// Get the full history of a signal.
    pub fn history(&self, name: &str) -> Option<&[ValueChange]> {
        self.traces.get(name).map(|v| v.as_slice())
    }

    /// Get the value of a signal at a specific cycle.
    pub fn value_at(&self, name: &str, cycle: u64) -> Option<u64> {
        let history = self.traces.get(name)?;
        // Find the last change at or before this cycle
        let mut result = None;
        for change in history {
            if change.cycle <= cycle {
                result = Some(change.value);
            } else {
                break;
            }
        }
        result
    }

    /// Get all registered signal names.
    pub fn signal_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.widths.keys().cloned().collect();
        names.sort();
        names
    }

    /// Get the width of a signal.
    pub fn width(&self, name: &str) -> Option<usize> {
        self.widths.get(name).copied()
    }

    /// Total number of value changes recorded.
    pub fn total_changes(&self) -> usize {
        self.traces.values().map(|v| v.len()).sum()
    }

    /// Format the trace as a simple ASCII timing diagram.
    pub fn ascii_dump(&self, max_cycles: u64) -> String {
        let mut out = String::new();
        let names = self.signal_names();
        let max_name_len = names.iter().map(|n| n.len()).max().unwrap_or(0);

        for name in &names {
            out.push_str(&format!("{:>width$} │ ", name, width = max_name_len));
            for cycle in 0..max_cycles {
                let val = self.value_at(name, cycle).unwrap_or(0);
                let width = self.width(name).unwrap_or(1);
                if width == 1 {
                    out.push(if val != 0 { '█' } else { '░' });
                } else {
                    out.push_str(&format!("{:>3} ", val));
                }
            }
            out.push('\n');
        }
        out
    }
}

impl Default for SignalTrace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_trace_basic() {
        let mut trace = SignalTrace::new();
        trace.register("clk", 1);
        trace.register("count", 8);

        // Simulate a few cycles
        for cycle in 0..5 {
            trace.set("clk", cycle, cycle % 2);
            trace.set("count", cycle, cycle);
        }

        assert_eq!(trace.get("count"), Some(4));
        assert_eq!(trace.value_at("count", 2), Some(2));
        assert_eq!(trace.signal_names(), vec!["clk", "count"]);
    }

    #[test]
    fn test_only_records_changes() {
        let mut trace = SignalTrace::new();
        trace.register("sig", 8);

        trace.set("sig", 0, 0);
        trace.set("sig", 1, 0); // same value — should NOT record
        trace.set("sig", 2, 5); // different — should record
        trace.set("sig", 3, 5); // same — should NOT record
        trace.set("sig", 4, 3); // different — should record

        let history = trace.history("sig").unwrap();
        assert_eq!(history.len(), 3); // cycle 0, 2, 4
        assert_eq!(history[0].value, 0);
        assert_eq!(history[1].value, 5);
        assert_eq!(history[2].value, 3);
    }

    #[test]
    fn test_value_at_interpolation() {
        let mut trace = SignalTrace::new();
        trace.register("data", 8);

        trace.set("data", 0, 10);
        trace.set("data", 5, 20);
        trace.set("data", 10, 30);

        // Between changes, value should hold
        assert_eq!(trace.value_at("data", 0), Some(10));
        assert_eq!(trace.value_at("data", 3), Some(10));
        assert_eq!(trace.value_at("data", 5), Some(20));
        assert_eq!(trace.value_at("data", 7), Some(20));
        assert_eq!(trace.value_at("data", 10), Some(30));
    }
}

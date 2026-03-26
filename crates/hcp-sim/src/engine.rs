//! # Simulation Engine — The Beating Heart
//!
//! This is the cycle-accurate execution engine. It takes a module definition,
//! steps through clock cycles, evaluates expressions, runs ECC encode/decode,
//! applies error injections, and records everything to a signal trace.
//!
//! ## How it works
//!
//! Each cycle:
//! 1. Evaluate all combinational logic (expressions, assignments)
//! 2. Encode ECC-protected signals through Hamming encoder
//! 3. Apply any scheduled error injections (corrupt encoded bits)
//! 4. Decode through Hamming decoder (catch/correct errors)
//! 5. Update registers on clock edge
//! 6. Record all signal values to the trace
//!
//! This mirrors exactly what happens in real hardware on each clock edge.

use std::collections::HashMap;
use hcp_core::prelude::*;
use crate::signals::SignalTrace;
use crate::ecc_sim::EccSimulator;
use crate::error_inject::{ErrorInjector, InjectionResult};

/// Simulation configuration.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Number of cycles to simulate
    pub cycles: u64,
    /// Whether to print cycle-by-cycle output
    pub verbose: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            cycles: 20,
            verbose: false,
        }
    }
}

/// Result of a complete simulation run.
#[derive(Debug)]
pub struct SimResult {
    /// Total cycles simulated
    pub cycles: u64,
    /// The signal trace (all recorded values)
    pub trace: SignalTrace,
    /// ECC error injection results
    pub injection_results: Vec<InjectionResult>,
    /// Total ECC corrections during simulation
    pub ecc_corrections: u64,
    /// Total uncorrectable errors detected
    pub ecc_uncorrectable: u64,
}

/// The simulation engine.
pub struct SimEngine {
    /// The module being simulated
    module: Module,
    /// Current register values
    registers: HashMap<String, u64>,
    /// ECC simulator (if module has ECC-protected signals)
    ecc: Option<EccSimulator>,
    /// Which signals are ECC-protected
    ecc_signals: Vec<String>,
    /// Encoded (ECC) values for protected signals
    ecc_encoded: HashMap<String, u64>,
    /// Error injector
    injector: ErrorInjector,
    /// Configuration
    config: SimConfig,
}

impl SimEngine {
    /// Create a new simulation engine for a module.
    pub fn new(module: Module) -> Self {
        let ecc_signals: Vec<String> = module.ports.iter()
            .filter(|p| matches!(p.signal.ecc, EccScheme::HammingSecDed))
            .map(|p| p.signal.name.clone())
            .collect();

        let ecc = if !ecc_signals.is_empty() {
            // Use the width of the first ECC signal to create the simulator
            let width = module.ports.iter()
                .find(|p| matches!(p.signal.ecc, EccScheme::HammingSecDed))
                .map(|p| p.signal.width.bits())
                .unwrap_or(8);
            Some(EccSimulator::new(width))
        } else {
            None
        };

        SimEngine {
            module,
            registers: HashMap::new(),
            ecc,
            ecc_signals,
            ecc_encoded: HashMap::new(),
            injector: ErrorInjector::new(),
            config: SimConfig::default(),
        }
    }

    /// Set the simulation configuration.
    pub fn configure(mut self, config: SimConfig) -> Self {
        self.config = config;
        self
    }

    /// Get a mutable reference to the error injector.
    pub fn injector_mut(&mut self) -> &mut ErrorInjector {
        &mut self.injector
    }

    /// Initialize all registers to 0.
    fn init_registers(&mut self) {
        for port in &self.module.ports {
            if matches!(port.signal.kind, SignalKind::Register) {
                self.registers.insert(port.signal.name.clone(), 0);
            }
        }
    }

    /// Evaluate an expression given current register values.
    fn eval_expr(&self, expr: &Expr) -> u64 {
        match expr {
            Expr::Signal(name) => {
                self.registers.get(name).copied().unwrap_or(0)
            }
            Expr::Literal { value, width: _ } => *value,
            Expr::BinOp { op, left, right } => {
                let l = self.eval_expr(left);
                let r = self.eval_expr(right);
                match op {
                    BinOpKind::Add => l.wrapping_add(r),
                    BinOpKind::Sub => l.wrapping_sub(r),
                    BinOpKind::Mul => l.wrapping_mul(r),
                    BinOpKind::And => l & r,
                    BinOpKind::Or => l | r,
                    BinOpKind::Xor => l ^ r,
                    BinOpKind::Shl => l.wrapping_shl(r as u32),
                    BinOpKind::Shr => l.wrapping_shr(r as u32),
                    BinOpKind::Eq => if l == r { 1 } else { 0 },
                    BinOpKind::Ne => if l != r { 1 } else { 0 },
                    BinOpKind::Lt => if l < r { 1 } else { 0 },
                    BinOpKind::Gt => if l > r { 1 } else { 0 },
                    BinOpKind::Le => if l <= r { 1 } else { 0 },
                    BinOpKind::Ge => if l >= r { 1 } else { 0 },
                }
            }
            Expr::UnOp { op, operand } => {
                let val = self.eval_expr(operand);
                match op {
                    UnOpKind::Not => !val,
                    UnOpKind::ReduceXor => {
                        let mut result = 0u64;
                        let mut v = val;
                        while v != 0 { result ^= v & 1; v >>= 1; }
                        result
                    }
                }
            }
            Expr::Slice { signal, high, low } => {
                let val = self.registers.get(signal).copied().unwrap_or(0);
                let width = high - low + 1;
                (val >> low) & ((1u64 << width) - 1)
            }
            Expr::Concat(exprs) => {
                let mut result = 0u64;
                let mut shift = 0;
                for e in exprs.iter().rev() {
                    let val = self.eval_expr(e);
                    result |= val << shift;
                    shift += 8; // approximate — concat items assumed 8-bit
                }
                result
            }
        }
    }

    /// Mask a value to fit within a given bit width.
    fn mask_to_width(&self, value: u64, signal_name: &str) -> u64 {
        let width = self.module.ports.iter()
            .find(|p| p.signal.name == signal_name)
            .map(|p| p.signal.width.bits())
            .unwrap_or(64);
        if width >= 64 { value } else { value & ((1u64 << width) - 1) }
    }

    /// Run the complete simulation.
    pub fn run(mut self) -> SimResult {
        let mut trace = SignalTrace::new();
        let mut ecc_corrections: u64 = 0;
        let mut ecc_uncorrectable: u64 = 0;

        // Register all signals in the trace
        trace.register("clk", 1);
        trace.register("rst", 1);
        for port in &self.module.ports {
            let name = &port.signal.name;
            if name != "clk" && name != "rst" {
                trace.register(name, port.signal.width.bits());
            }
        }

        // Register ECC-specific signals
        for ecc_name in &self.ecc_signals {
            if let Some(ref ecc) = self.ecc {
                trace.register(&format!("{}_encoded", ecc_name), ecc.total_width());
                trace.register(&format!("{}_err_correctable", ecc_name), 1);
                trace.register(&format!("{}_err_uncorrectable", ecc_name), 1);
            }
        }

        self.init_registers();

        let cycles = self.config.cycles;
        let verbose = self.config.verbose;

        // Clone what we need for the always blocks since we borrow self
        let always_blocks = self.module.always_blocks.clone();

        for cycle in 0..cycles {
            let clk = cycle % 2;
            let rst = if cycle < 2 { 1 } else { 0 };

            trace.set("clk", cycle, clk);
            trace.set("rst", cycle, rst);

            // On rising clock edge and not in reset
            if clk == 1 && rst == 0 {
                // Evaluate always blocks
                for block in &always_blocks {
                    for stmt in &block.statements {
                        match stmt {
                            Statement::Assign { target, value } => {
                                let val = self.eval_expr(value);
                                let masked = self.mask_to_width(val, target);
                                self.registers.insert(target.clone(), masked);
                            }
                            Statement::If { condition, then_body, else_body } => {
                                let cond = self.eval_expr(condition);
                                let body = if cond != 0 { then_body } else { else_body };
                                for inner in body {
                                    if let Statement::Assign { target, value } = inner {
                                        let val = self.eval_expr(value);
                                        let masked = self.mask_to_width(val, target);
                                        self.registers.insert(target.clone(), masked);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Record register values and run ECC
            for port in &self.module.ports {
                let name = &port.signal.name;
                if name == "clk" || name == "rst" { continue; }

                let value = self.registers.get(name).copied().unwrap_or(0);
                trace.set(name, cycle, value);

                // ECC encode → inject → decode
                if self.ecc_signals.contains(name) {
                    if let Some(ref ecc) = self.ecc {
                        let encoded = ecc.encode(value);
                        let mut live_encoded = encoded;

                        // Check for error injections at this cycle
                        let injection_events: Vec<_> = self.injector
                            .events_at(cycle, name)
                            .iter()
                            .map(|e| (*e).clone())
                            .collect();

                        for event in &injection_events {
                            let corrupted = self.injector.corrupt(live_encoded, event);
                            let decode_result = ecc.decode(corrupted);

                            self.injector.record_result(InjectionResult {
                                event: event.clone(),
                                original_value: value,
                                corrupted_value: corrupted,
                                corrected_value: decode_result.data,
                                corrected: decode_result.correctable_error,
                                detected_uncorrectable: decode_result.uncorrectable_error,
                            });

                            if decode_result.correctable_error {
                                ecc_corrections += 1;
                            }
                            if decode_result.uncorrectable_error {
                                ecc_uncorrectable += 1;
                            }

                            live_encoded = corrupted;
                        }

                        // Decode (even without injection, to show normal operation)
                        let decoded = ecc.decode(live_encoded);

                        trace.set(&format!("{}_encoded", name), cycle, live_encoded);
                        trace.set(&format!("{}_err_correctable", name), cycle,
                            if decoded.correctable_error { 1 } else { 0 });
                        trace.set(&format!("{}_err_uncorrectable", name), cycle,
                            if decoded.uncorrectable_error { 1 } else { 0 });

                        self.ecc_encoded.insert(name.clone(), live_encoded);
                    }
                }
            }

            if verbose {
                let mut line = format!("  cycle {:>3} │ clk={} rst={}", cycle, clk, rst);
                for port in &self.module.ports {
                    let name = &port.signal.name;
                    if name == "clk" || name == "rst" { continue; }
                    let val = self.registers.get(name).copied().unwrap_or(0);
                    line.push_str(&format!(" │ {}={}", name, val));
                }
                println!("{}", line);
            }
        }

        SimResult {
            cycles,
            trace,
            injection_results: self.injector.results().to_vec(),
            ecc_corrections,
            ecc_uncorrectable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_counter_module() -> Module {
        let mut counter = Module::with_ecc("counter_ecc", EccScheme::HammingSecDed);
        counter.add_input("clk", 1);
        counter.add_input("rst", 1);
        counter.add_output_reg("count", 8);
        counter.always_blocks.push(AlwaysBlock {
            clock: "clk".to_string(),
            edge: ClockEdge::Rising,
            reset: Some(ResetConfig {
                signal: "rst".to_string(),
                active_high: true,
                synchronous: true,
            }),
            statements: vec![Statement::Assign {
                target: "count".to_string(),
                value: Expr::BinOp {
                    op: BinOpKind::Add,
                    left: Box::new(Expr::Signal("count".to_string())),
                    right: Box::new(Expr::Literal { value: 1, width: 8 }),
                },
            }],
        });
        counter
    }

    #[test]
    fn test_basic_counter_simulation() {
        let counter = make_counter_module();
        let result = SimEngine::new(counter)
            .configure(SimConfig { cycles: 20, verbose: false })
            .run();

        assert_eq!(result.cycles, 20);

        // Counter increments on rising edge (odd cycles) after reset (cycle 2+)
        // Cycle 0: clk=0, rst=1 → count=0
        // Cycle 1: clk=1, rst=1 → count=0 (in reset)
        // Cycle 2: clk=0, rst=0 → count=0
        // Cycle 3: clk=1, rst=0 → count=1 (first increment)
        // Cycle 5: clk=1, rst=0 → count=2
        // Cycle 7: clk=1, rst=0 → count=3
        assert_eq!(result.trace.value_at("count", 3), Some(1));
        assert_eq!(result.trace.value_at("count", 5), Some(2));
        assert_eq!(result.trace.value_at("count", 7), Some(3));
    }

    #[test]
    fn test_counter_with_ecc() {
        let counter = make_counter_module();
        let result = SimEngine::new(counter)
            .configure(SimConfig { cycles: 10, verbose: false })
            .run();

        // ECC encoded values should be present
        let encoded = result.trace.value_at("count_encoded", 5);
        assert!(encoded.is_some(), "Should have encoded values");
        assert!(encoded.unwrap() > 0, "Encoded value should be nonzero for count=2");

        // No errors should be reported without injection
        assert_eq!(result.ecc_corrections, 0);
        assert_eq!(result.ecc_uncorrectable, 0);
    }

    #[test]
    fn test_error_injection_single_bit() {
        let counter = make_counter_module();
        let mut engine = SimEngine::new(counter)
            .configure(SimConfig { cycles: 10, verbose: false });

        // Inject a single-bit error at cycle 5
        engine.injector_mut().inject_single_bit(5, "count", 3);

        let result = engine.run();

        // Should have been corrected
        assert_eq!(result.ecc_corrections, 1);
        assert_eq!(result.ecc_uncorrectable, 0);
        assert_eq!(result.injection_results.len(), 1);
        assert!(result.injection_results[0].corrected);
    }

    #[test]
    fn test_error_injection_double_bit() {
        let counter = make_counter_module();
        let mut engine = SimEngine::new(counter)
            .configure(SimConfig { cycles: 10, verbose: false });

        // Inject a double-bit error at cycle 7
        engine.injector_mut().inject_double_bit(7, "count", 1, 3);

        let result = engine.run();

        // Should be detected but not corrected
        assert_eq!(result.ecc_corrections, 0);
        assert_eq!(result.ecc_uncorrectable, 1);
        assert_eq!(result.injection_results.len(), 1);
        assert!(result.injection_results[0].detected_uncorrectable);
    }

    #[test]
    fn test_multiple_injections() {
        let counter = make_counter_module();
        let mut engine = SimEngine::new(counter)
            .configure(SimConfig { cycles: 20, verbose: false });

        // Mix of single and double-bit errors
        engine.injector_mut().inject_single_bit(5, "count", 0);
        engine.injector_mut().inject_single_bit(9, "count", 7);
        engine.injector_mut().inject_double_bit(13, "count", 2, 4);

        let result = engine.run();

        assert_eq!(result.injection_results.len(), 3);
        assert_eq!(result.ecc_corrections, 2);     // Two single-bit fixes
        assert_eq!(result.ecc_uncorrectable, 1);    // One double-bit detection
    }

    #[test]
    fn test_counter_wraps_at_8bit() {
        let counter = make_counter_module();
        let result = SimEngine::new(counter)
            .configure(SimConfig { cycles: 600, verbose: false })
            .run();

        // After 256 rising edges past reset, counter should wrap
        // Rising edges happen at odd cycles: 3, 5, 7, ... 
        // Cycle 3 + 255*2 = cycle 513 should give count=0 (wrapped)
        let val_at_wrap = result.trace.value_at("count", 513);
        assert_eq!(val_at_wrap, Some(0), "Counter should wrap at 256");
    }
}

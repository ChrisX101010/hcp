//! # Hardware Module Definition
//!
//! A Module is the fundamental unit of hardware design. It's analogous to
//! a function in software, but with a crucial difference: all the "code"
//! inside runs SIMULTANEOUSLY, not sequentially.
//!
//! ## Physical meaning
//! A module becomes a Verilog `module` — a self-contained block of logic
//! with defined inputs and outputs. Modules can contain other modules
//! (hierarchy), just like functions can call other functions.
//!
//! ## Example
//! A simple counter module has:
//! - Input: clock, reset
//! - Output: count value
//! - Internal: a register that increments each clock cycle

use serde::{Deserialize, Serialize};
use crate::types::*;

/// A complete hardware module definition.
///
/// This is what gets compiled into Verilog. The compiler will:
/// 1. Check that all port widths match their connections
/// 2. Inject ECC encoder/decoder for signals marked with ECC
/// 3. Generate error status output ports automatically
/// 4. Emit synthesizable Verilog/SystemVerilog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// Module name (becomes the Verilog module name)
    pub name: String,

    /// External interface — how the outside world talks to this module
    pub ports: Vec<Port>,

    /// Internal signals — wires and registers inside the module
    pub signals: Vec<Signal>,

    /// Assignments — combinational logic (wire = expression)
    pub assignments: Vec<Assignment>,

    /// Always blocks — sequential logic (on clock edge, do something)
    pub always_blocks: Vec<AlwaysBlock>,

    /// Sub-module instantiations — other modules used inside this one
    pub instances: Vec<Instance>,

    /// Default ECC scheme for all registers in this module.
    /// Individual signals can override this.
    pub default_ecc: EccScheme,
}

impl Module {
    /// Create a new empty module
    pub fn new(name: &str) -> Self {
        Module {
            name: name.to_string(),
            ports: Vec::new(),
            signals: Vec::new(),
            assignments: Vec::new(),
            always_blocks: Vec::new(),
            instances: Vec::new(),
            default_ecc: EccScheme::None,
        }
    }

    /// Create a module with default ECC on all registers
    pub fn with_ecc(name: &str, ecc: EccScheme) -> Self {
        let mut m = Module::new(name);
        m.default_ecc = ecc;
        m
    }

    /// Add an input port
    pub fn add_input(&mut self, name: &str, width: usize) {
        self.ports.push(Port::input(name, width));
    }

    /// Add an output port
    pub fn add_output(&mut self, name: &str, width: usize) {
        self.ports.push(Port::output(name, width));
    }

    /// Add an output register (with module's default ECC)
    pub fn add_output_reg(&mut self, name: &str, width: usize) {
        let mut port = Port::output_reg(name, width);
        if self.default_ecc != EccScheme::None {
            port.signal.ecc = self.default_ecc.clone();
        }
        self.ports.push(port);
    }

    /// Add an internal register (with module's default ECC)
    pub fn add_register(&mut self, name: &str, width: usize) {
        let mut sig = Signal::register(name, width);
        if self.default_ecc != EccScheme::None {
            sig.ecc = self.default_ecc.clone();
        }
        self.signals.push(sig);
    }

    /// Get all signals (ports + internal) that have ECC enabled
    pub fn ecc_signals(&self) -> Vec<&Signal> {
        let port_signals = self.ports.iter().map(|p| &p.signal);
        let internal_signals = self.signals.iter();

        port_signals
            .chain(internal_signals)
            .filter(|s| s.has_ecc())
            .collect()
    }

    /// Total ECC overhead in bits across all protected signals
    pub fn total_ecc_overhead_bits(&self) -> usize {
        self.ecc_signals()
            .iter()
            .map(|s| s.ecc.overhead_bits(s.width))
            .sum()
    }
}

/// A combinational assignment: `assign wire_name = expression;`
///
/// In hardware, this creates a direct connection — the output
/// continuously reflects the expression's value with no delay
/// (other than gate propagation delay, typically < 1ns).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    /// The signal being driven
    pub target: String,
    /// The expression producing the value (stored as string for now,
    /// will become a proper AST expression in Phase 2)
    pub expression: Expr,
}

/// An always block — sequential logic triggered by clock edges.
///
/// This is where state changes happen. On each rising edge of the clock,
/// all the statements inside execute "simultaneously" (in hardware terms,
/// all register values update at the same instant).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlwaysBlock {
    /// Which clock signal triggers this block
    pub clock: String,
    /// Rising edge (posedge) or falling edge (negedge)
    pub edge: ClockEdge,
    /// Optional reset signal
    pub reset: Option<ResetConfig>,
    /// Statements to execute on each clock edge
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockEdge {
    Rising,  // posedge — the standard; 99% of designs use this
    Falling, // negedge — rare, used for specific timing requirements
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetConfig {
    pub signal: String,
    /// Is reset active when the signal is HIGH (true) or LOW (false)?
    pub active_high: bool,
    /// Synchronous (checked on clock edge) or asynchronous (immediate)?
    pub synchronous: bool,
}

/// A statement inside an always block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    /// Assign a value to a register: `reg_name <= expression;`
    Assign { target: String, value: Expr },
    /// Conditional: `if (condition) { ... } else { ... }`
    If {
        condition: Expr,
        then_body: Vec<Statement>,
        else_body: Vec<Statement>,
    },
}

/// An expression in the hardware language.
///
/// Every expression ultimately computes a fixed-width bit value.
/// There are no floating-point numbers in basic digital hardware —
/// everything is integers (or fixed-point, which is integers with
/// an implied decimal point).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    /// A literal constant value
    Literal { value: u64, width: usize },
    /// Reference to a signal by name
    Signal(String),
    /// Binary operation: left OP right
    BinOp {
        op: BinOpKind,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Unary operation: OP operand
    UnOp { op: UnOpKind, operand: Box<Expr> },
    /// Bit slice: signal[high:low]
    Slice {
        signal: String,
        high: usize,
        low: usize,
    },
    /// Concatenation: {a, b, c}
    Concat(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOpKind {
    Add, Sub, Mul,
    And, Or, Xor,
    Eq, Ne, Lt, Gt, Le, Ge,
    Shl, Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOpKind {
    Not,       // Bitwise NOT (~)
    ReduceXor, // XOR all bits together (used in parity calculation)
}

/// An instance of another module used inside this module.
///
/// This is hardware composition — like calling a function, but
/// the "function" runs continuously in parallel with everything else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// The module being instantiated
    pub module_name: String,
    /// Instance name (must be unique within parent module)
    pub instance_name: String,
    /// Port connections: maps port name → expression
    pub connections: Vec<(String, Expr)>,
}

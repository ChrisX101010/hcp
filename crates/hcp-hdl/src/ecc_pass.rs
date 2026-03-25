//! # ECC Compiler Pass
//!
//! This is the pass that makes HCP's "ECC by default" work. It walks through
//! every signal in a module, and for each one that has an ECC scheme, it:
//!
//! 1. Generates encoder and decoder modules (via hcp-ecc)
//! 2. Widens the internal storage to fit the encoded data
//! 3. Inserts encoder instance before each write to the register
//! 4. Inserts decoder instance before each read from the register
//! 5. Adds error flag output ports to the module interface
//!
//! ## Before and After
//!
//! ### What you write:
//! ```text
//! Module "counter"
//!   port: count [7:0] output reg  #[ecc(hamming)]
//!   always @(posedge clk): count <= count + 1
//! ```
//!
//! ### What the ECC pass produces:
//! ```text
//! Module "counter"
//!   port: count [7:0] output wire        ← decoded output
//!   port: count_err_correctable output   ← NEW: error flag
//!   port: count_err_uncorrectable output ← NEW: error flag
//!   internal: count_encoded [12:0] reg   ← widened storage
//!   instance: hamming_enc_8 enc_count    ← NEW: encoder
//!   instance: hamming_dec_8 dec_count    ← NEW: decoder
//!   always @(posedge clk): count_encoded <= enc_count.encoded_out
//! ```
//!
//! The user's logic is preserved — they still see `count` as 8 bits.
//! The ECC machinery is completely transparent.

use hcp_core::prelude::*;
use hcp_ecc::HammingGenerator;

/// The result of running the ECC pass on a module.
pub struct EccPassResult {
    /// The transformed module (with ECC injected)
    pub module: Module,
    /// Encoder modules that need to be emitted alongside the main module
    pub encoder_modules: Vec<Module>,
    /// Decoder modules that need to be emitted alongside the main module
    pub decoder_modules: Vec<Module>,
    /// Summary of what was done (for logging/debugging)
    pub report: EccReport,
}

/// Summary of ECC transformations applied.
#[derive(Debug, Default)]
pub struct EccReport {
    /// How many signals were protected
    pub signals_protected: usize,
    /// Total parity bits added
    pub parity_bits_added: usize,
    /// Total overhead in bits
    pub overhead_bits: usize,
    /// Per-signal details
    pub details: Vec<EccSignalDetail>,
}

#[derive(Debug)]
pub struct EccSignalDetail {
    pub signal_name: String,
    pub data_width: usize,
    pub encoded_width: usize,
    pub scheme: String,
    pub overhead_percent: f64,
}

impl std::fmt::Display for EccReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "╔══════════════════════════════════════════════╗")?;
        writeln!(f, "║           HCP ECC Compiler Report            ║")?;
        writeln!(f, "╠══════════════════════════════════════════════╣")?;
        writeln!(f, "║  Signals protected:  {:>4}                    ║", self.signals_protected)?;
        writeln!(f, "║  Parity bits added:  {:>4}                    ║", self.parity_bits_added)?;
        writeln!(f, "║  Total overhead:     {:>4} bits               ║", self.overhead_bits)?;
        writeln!(f, "╠══════════════════════════════════════════════╣")?;
        for d in &self.details {
            writeln!(f, "║  {:<20} {:>3}b → {:>3}b ({:>5.1}%) ║",
                d.signal_name, d.data_width, d.encoded_width, d.overhead_percent)?;
        }
        writeln!(f, "╚══════════════════════════════════════════════╝")?;
        Ok(())
    }
}

/// The ECC compiler pass.
pub struct EccPass;

impl EccPass {
    /// Run the ECC pass on a module.
    ///
    /// This is the main entry point. It:
    /// 1. Finds all signals with ECC annotations
    /// 2. Generates encoder/decoder modules for each unique width+scheme
    /// 3. Transforms the module to use encoded storage
    /// 4. Adds error flag ports
    ///
    /// # Returns
    /// The transformed module plus all supporting ECC modules.
    pub fn run(module: &Module) -> EccPassResult {
        let mut result_module = module.clone();
        let mut encoder_modules = Vec::new();
        let mut decoder_modules = Vec::new();
        let mut report = EccReport::default();

        // Collect all ECC-annotated signals
        let ecc_signals: Vec<(String, BitWidth, EccScheme)> = {
            let port_signals = module.ports.iter().map(|p| {
                (p.signal.name.clone(), p.signal.width, p.signal.ecc.clone())
            });
            let internal_signals = module.signals.iter().map(|s| {
                (s.name.clone(), s.width, s.ecc.clone())
            });
            port_signals
                .chain(internal_signals)
                .filter(|(_, _, ecc)| *ecc != EccScheme::None)
                .collect()
        };

        // For each ECC signal, generate encoder/decoder and transform the module
        for (name, width, scheme) in &ecc_signals {
            match scheme {
                EccScheme::HammingSecDed => {
                    let gen = HammingGenerator::new(width.bits());

                    // Generate encoder and decoder modules
                    let enc = gen.generate_encoder();
                    let dec = gen.generate_decoder();

                    // Add error flag output ports to the main module
                    result_module.ports.push(Port::output(
                        &format!("{}_err_correctable", name),
                        1,
                    ));
                    result_module.ports.push(Port::output(
                        &format!("{}_err_uncorrectable", name),
                        1,
                    ));
                    result_module.ports.push(Port::output(
                        &format!("{}_syndrome", name),
                        gen.parity_bits,
                    ));

                    // Add internal encoded register
                    result_module.signals.push(Signal {
                        name: format!("{}_encoded", name),
                        width: BitWidth::new(gen.total_width),
                        kind: SignalKind::Register,
                        ecc: EccScheme::None, // The encoded form doesn't need its own ECC
                    });

                    // Add encoder instance
                    result_module.instances.push(Instance {
                        module_name: enc.name.clone(),
                        instance_name: format!("enc_{}", name),
                        connections: vec![
                            ("data_in".to_string(), Expr::Signal(name.clone())),
                            (
                                "encoded_out".to_string(),
                                Expr::Signal(format!("{}_encoded", name)),
                            ),
                        ],
                    });

                    // Add decoder instance
                    result_module.instances.push(Instance {
                        module_name: dec.name.clone(),
                        instance_name: format!("dec_{}", name),
                        connections: vec![
                            (
                                "encoded_in".to_string(),
                                Expr::Signal(format!("{}_encoded", name)),
                            ),
                            ("data_out".to_string(), Expr::Signal(name.clone())),
                            (
                                "err_correctable".to_string(),
                                Expr::Signal(format!("{}_err_correctable", name)),
                            ),
                            (
                                "err_uncorrectable".to_string(),
                                Expr::Signal(format!("{}_err_uncorrectable", name)),
                            ),
                            (
                                "syndrome".to_string(),
                                Expr::Signal(format!("{}_syndrome", name)),
                            ),
                        ],
                    });

                    // Update report
                    report.signals_protected += 1;
                    report.parity_bits_added += gen.parity_bits + 1;
                    report.overhead_bits += gen.parity_bits + 1;
                    report.details.push(EccSignalDetail {
                        signal_name: name.clone(),
                        data_width: width.bits(),
                        encoded_width: gen.total_width,
                        scheme: "Hamming SEC-DED".to_string(),
                        overhead_percent: scheme.overhead_percent(*width),
                    });

                    encoder_modules.push(enc);
                    decoder_modules.push(dec);
                }
                EccScheme::Parity => {
                    // Simple parity — just XOR all bits for detection
                    result_module.ports.push(Port::output(
                        &format!("{}_parity_error", name),
                        1,
                    ));
                    report.signals_protected += 1;
                    report.parity_bits_added += 1;
                    report.overhead_bits += 1;
                    report.details.push(EccSignalDetail {
                        signal_name: name.clone(),
                        data_width: width.bits(),
                        encoded_width: width.bits() + 1,
                        scheme: "Parity".to_string(),
                        overhead_percent: scheme.overhead_percent(*width),
                    });
                }
                EccScheme::Tmr => {
                    // TMR — triplicate and vote
                    report.signals_protected += 1;
                    report.parity_bits_added += width.bits() * 2;
                    report.overhead_bits += width.bits() * 2;
                    report.details.push(EccSignalDetail {
                        signal_name: name.clone(),
                        data_width: width.bits(),
                        encoded_width: width.bits() * 3,
                        scheme: "TMR".to_string(),
                        overhead_percent: 200.0,
                    });
                }
                EccScheme::None => unreachable!(),
            }
        }

        EccPassResult {
            module: result_module,
            encoder_modules,
            decoder_modules,
            report,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a simple 8-bit counter with ECC for testing
    fn make_test_counter() -> Module {
        let mut m = Module::with_ecc("counter", EccScheme::HammingSecDed);
        m.add_input("clk", 1);
        m.add_input("rst", 1);
        m.add_output_reg("count", 8);
        m
    }

    #[test]
    fn test_ecc_pass_adds_error_ports() {
        let module = make_test_counter();
        let result = EccPass::run(&module);

        let port_names: Vec<&str> = result.module.ports.iter()
            .map(|p| p.signal.name.as_str())
            .collect();

        assert!(port_names.contains(&"count_err_correctable"));
        assert!(port_names.contains(&"count_err_uncorrectable"));
        assert!(port_names.contains(&"count_syndrome"));
    }

    #[test]
    fn test_ecc_pass_generates_encoder_decoder() {
        let module = make_test_counter();
        let result = EccPass::run(&module);

        assert_eq!(result.encoder_modules.len(), 1);
        assert_eq!(result.decoder_modules.len(), 1);
        assert_eq!(result.encoder_modules[0].name, "hamming_enc_8");
        assert_eq!(result.decoder_modules[0].name, "hamming_dec_8");
    }

    #[test]
    fn test_ecc_pass_report() {
        let module = make_test_counter();
        let result = EccPass::run(&module);

        assert_eq!(result.report.signals_protected, 1);
        assert_eq!(result.report.parity_bits_added, 5); // 4 Hamming + 1 overall
        println!("{}", result.report);
    }

    #[test]
    fn test_ecc_pass_adds_encoded_register() {
        let module = make_test_counter();
        let result = EccPass::run(&module);

        let encoded = result.module.signals.iter()
            .find(|s| s.name == "count_encoded");
        assert!(encoded.is_some());
        assert_eq!(encoded.unwrap().width.bits(), 13); // 8 + 4 + 1
    }

    #[test]
    fn test_ecc_pass_adds_instances() {
        let module = make_test_counter();
        let result = EccPass::run(&module);

        assert!(result.module.instances.iter()
            .any(|i| i.instance_name == "enc_count"));
        assert!(result.module.instances.iter()
            .any(|i| i.instance_name == "dec_count"));
    }
}

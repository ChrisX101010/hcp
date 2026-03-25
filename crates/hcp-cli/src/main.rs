use hcp_core::prelude::*;
use hcp_ecc::HammingGenerator;
use hcp_package::ImageBuilder;

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     HCP — Hardware Context Protocol v0.2.0                  ║");
    println!("║     Phase 1: HDL Compiler + ECC   │  Phase 2: Packaging     ║");
    println!("║                                                              ║");
    println!("║     Dedicated to the memory of Zoran Modli (1948-2020)       ║");
    println!("║     and the Galaksija movement — hardware for everyone.      ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // ── PHASE 1: ECC Analysis + Module Definition ──

    println!("━━━ PHASE 1: Compile Hardware with ECC ━━━\n");

    println!("  ECC overhead by data width:");
    println!("  Width │ Parity │ Total │ Overhead");
    println!("  ──────┼────────┼───────┼─────────");
    for width in [8, 16, 32, 64] {
        let gen = HammingGenerator::new(width);
        println!("  {:>5} │ {:>6} │ {:>5} │ {:>5.1}%",
            width, gen.parity_bits + 1, gen.total_width,
            ((gen.parity_bits + 1) as f64 / width as f64) * 100.0);
    }
    println!();

    let mut counter = Module::with_ecc("counter_ecc", EccScheme::HammingSecDed);
    counter.add_input("clk", 1);
    counter.add_input("rst", 1);
    counter.add_output_reg("count", 8);
    counter.always_blocks.push(AlwaysBlock {
        clock: "clk".to_string(),
        edge: ClockEdge::Rising,
        reset: Some(ResetConfig {
            signal: "rst".to_string(), active_high: true, synchronous: true,
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

    println!("  Module: {} (8-bit counter with ECC)", counter.name);
    println!("  ECC:    Hamming SEC-DED on all registers\n");

    // ── PHASE 2: Package as Hardware Image ──

    println!("━━━ PHASE 2: Package as Hardware Image ━━━\n");

    let output_dir = "output/counter-ecc-image";
    let _ = std::fs::remove_dir_all(output_dir);

    let result = ImageBuilder::new("counter-ecc", "0.2.0")
        .author("Hristo")
        .description("8-bit counter with Hamming SEC-DED ECC — built on Modli's shoulders")
        .module(counter)
        .target_fpga("ice40-hx8k", "lattice")
        .target_fpga("ecp5-85f", "lattice")
        .target_sim("verilator")
        .target_wasm()
        .build(output_dir)
        .expect("Build failed");

    println!("{}", result);
    println!("  ECC Compiler Report:");
    for line in result.ecc_report.lines() {
        println!("  {}", line);
    }

    // ── Show what was created on disk ──

    println!("\n━━━ Image contents on disk ━━━\n");
    println!("  {}/", output_dir);
    println!("  ├── oci-layout              ← OCI marker");
    println!("  ├── index.json              ← OCI index");
    println!("  ├── hcp.json                ← HCP manifest");
    println!("  ├── blobs/sha256/           ← Content-addressed blobs");
    if let Ok(entries) = std::fs::read_dir(std::path::Path::new(output_dir).join("blobs/sha256")) {
        println!("  │   └── {} blob files", entries.count());
    }
    println!("  └── verilog/");
    for file in &result.verilog_files {
        println!("      ├── {}", file);
    }

    // ── Reopen & verify (simulating `hcp pull`) ──

    println!("\n━━━ Verify & Inspect (simulating `hcp pull`) ━━━\n");

    let image = hcp_package::HardwareImage::open(std::path::Path::new(output_dir))
        .expect("Failed to open image");

    println!("  Package:      {} v{}", image.manifest.package.name, image.manifest.package.version);
    println!("  Author:       {}", image.manifest.package.author);
    println!("  Description:  {}", image.manifest.package.description);
    println!("  ECC signals:  {}", image.manifest.ecc.signals_protected);
    for d in &image.manifest.ecc.signal_details {
        println!("    └─ {} ({}b → {}b, {:.1}% overhead)", d.signal_name, d.data_width, d.encoded_width, d.overhead_percent);
    }
    println!("  Targets:");
    for t in &image.manifest.targets {
        println!("    └─ {} {}", t.kind, t.name);
    }

    let verify = image.verify().expect("Verification failed");
    println!("  Integrity:    {}", verify);

    // ── Show manifest JSON ──

    println!("\n━━━ hcp.json (first 25 lines) ━━━\n");
    let json = image.manifest.to_json().unwrap();
    for (i, line) in json.lines().enumerate() {
        if i >= 25 { println!("  ... ({} more lines)", json.lines().count() - 25); break; }
        println!("  {}", line);
    }

    // ── Summary ──

    println!("\n━━━ Summary ━━━\n");
    println!("  ✓ Phase 1: Compiled counter_ecc with Hamming SEC-DED");
    println!("  ✓ Phase 2: Packaged as OCI-compatible hardware image");
    println!("  ✓ {} Verilog files with SHA-256 integrity verification", result.verilog_files.len());
    println!("  ✓ {} content layers, {} bytes total\n", image.manifest.layers.len(), image.total_size());
    println!("  Like Modli broadcasting software over Radio Beograd 202,");
    println!("  this image can now travel to anyone who needs it.\n");
    println!("  Next: Phase 3 — HCP Protocol Server (the 'radio tower')");
}

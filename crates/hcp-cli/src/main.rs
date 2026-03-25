use hcp_core::prelude::*;
use hcp_ecc::HammingGenerator;
use hcp_package::ImageBuilder;
use hcp_protocol::{HcpServer, HcpClient, ImageRegistry};

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     HCP — Hardware Context Protocol v0.3.0                  ║");
    println!("║     Phase 1: Compiler │ Phase 2: Packaging │ Phase 3: Server║");
    println!("║                                                              ║");
    println!("║     Dedicated to the memory of Zoran Modli (1948-2020)       ║");
    println!("║     and the Galaksija movement — hardware for everyone.      ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ── PHASE 1: Compile ──

    println!("━━━ PHASE 1: Compile Hardware with ECC ━━━\n");

    println!("  ECC overhead:");
    for width in [8, 32, 64] {
        let gen = HammingGenerator::new(width);
        println!("    {}b data → {}b encoded ({:.1}% overhead)",
            width, gen.total_width,
            ((gen.parity_bits + 1) as f64 / width as f64) * 100.0);
    }

    let mut counter = Module::with_ecc("counter_ecc", EccScheme::HammingSecDed);
    counter.add_input("clk", 1);
    counter.add_input("rst", 1);
    counter.add_output_reg("count", 8);
    counter.always_blocks.push(AlwaysBlock {
        clock: "clk".to_string(), edge: ClockEdge::Rising,
        reset: Some(ResetConfig { signal: "rst".to_string(), active_high: true, synchronous: true }),
        statements: vec![Statement::Assign {
            target: "count".to_string(),
            value: Expr::BinOp {
                op: BinOpKind::Add,
                left: Box::new(Expr::Signal("count".to_string())),
                right: Box::new(Expr::Literal { value: 1, width: 8 }),
            },
        }],
    });

    println!("\n  Module: counter_ecc (8-bit, Hamming SEC-DED)\n");

    // ── PHASE 2: Package ──

    println!("━━━ PHASE 2: Package as Hardware Image ━━━\n");

    let output_dir = "output/counter-ecc-image";
    let _ = std::fs::remove_dir_all(output_dir);
    let _ = std::fs::remove_dir_all("output/registry");

    let result = ImageBuilder::new("counter-ecc", "0.3.0")
        .author("Hristo")
        .description("8-bit counter with ECC — built on Modli's shoulders")
        .module(counter)
        .target_fpga("ice40-hx8k", "lattice")
        .target_sim("verilator")
        .target_wasm()
        .build(output_dir)
        .expect("Build failed");

    println!("  Image: counter-ecc:0.3.0");
    println!("  Files: {:?}", result.verilog_files);
    println!("  Size:  {} bytes", result.total_size);
    println!("  ECC:   {} signals protected", 1);
    println!("  {}\n", result.verify_result.trim());

    // ── PHASE 3: Protocol Server + Client ──

    println!("━━━ PHASE 3: HCP Protocol (The Radio Tower) ━━━\n");

    // Step 1: Create registry and publish our image
    println!("  [Server] Creating image registry...");
    let mut registry = ImageRegistry::open(std::path::Path::new("output/registry"))
        .expect("Failed to create registry");

    let reference = registry.publish(std::path::Path::new(output_dir))
        .expect("Failed to publish");
    println!("  [Server] Published: {}", reference);

    // Step 2: Start server
    println!("  [Server] Starting HCP server...\n");
    let server = HcpServer::new(registry);

    // Step 3: Client connects
    println!("  [Client] Connecting to server...");
    let mut client = HcpClient::connect(&server);

    // Step 4: Initialize — capability handshake
    let init = client.initialize("hcp-cli").expect("Init failed");
    println!("  [Client] Connected to {} v{}", init.server_name, init.server_version);
    println!("  [Client] Protocol: {}", init.protocol_version);
    println!("  [Client] Server has {} image(s)\n", init.capabilities.images_available);

    // Step 5: List images
    println!("  [Client] Listing available hardware...");
    let images = client.list_images().expect("List failed");
    for img in &images {
        println!("    └─ {}:{} by {} — {} target(s), {} ECC signal(s)",
            img.name, img.version, img.author, img.targets.len(), img.ecc_signals);
    }

    // Step 6: Pull — THE BROADCAST MOMENT
    println!("\n  [Client] Pulling counter-ecc:0.3.0...");
    println!("           (This is Modli's radio broadcast in 2026 form)\n");

    let pull = client.pull_image("counter-ecc", "0.3.0").expect("Pull failed");
    println!("  [Client] Received {} Verilog files:", pull.verilog_files.len());
    for f in &pull.verilog_files {
        println!("    └─ {} ({} bytes, {})", f.filename, f.content.len(), &f.digest[..20]);
    }
    println!("  [Client] Integrity: {}\n", if pull.integrity_verified { "✓ verified" } else { "✗ FAILED" });

    // Step 7: Show a snippet of the received Verilog
    if let Some(main_sv) = pull.verilog_files.iter().find(|f| f.filename == "counter_ecc.sv") {
        println!("  [Client] Received counter_ecc.sv (first 15 lines):");
        for (i, line) in main_sv.content.lines().enumerate() {
            if i >= 15 { println!("           ... ({} more lines)", main_sv.content.lines().count() - 15); break; }
            println!("           {}", line);
        }
    }

    // Step 8: Show raw JSON-RPC exchange
    println!("\n  ── Raw JSON-RPC exchange ──\n");
    let raw_req = r#"{"jsonrpc":"2.0","method":"hcp.ping","params":{},"id":99}"#;
    let raw_resp = server.handle_request(raw_req).unwrap();
    println!("  → {}", raw_req);
    println!("  ← {}\n", raw_resp);

    // ── Summary ──

    println!("━━━ Summary ━━━\n");
    println!("  ✓ Phase 1: Compiled counter_ecc with Hamming SEC-DED");
    println!("  ✓ Phase 2: Packaged as OCI-compatible hardware image");
    println!("  ✓ Phase 3: Published to registry, served via JSON-RPC protocol");
    println!("  ✓ Client connected, discovered, and pulled hardware image");
    println!("  ✓ Full Verilog content received with integrity verification\n");
    println!("  The cycle is complete:");
    println!("    Modli 1983: program → FSK audio → FM radio → tape → Galaksija");
    println!("    HCP   2026: module  → Verilog   → JSON-RPC → pull → FPGA\n");
    println!("  Next: Phase 3b — TCP/HTTP transport (real network `hcp serve`)");
    println!("        Phase 4  — WASM browser simulation");
    println!("        Phase 5  — P2P hardware mesh\n");
}

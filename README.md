# HCP — Hardware Context Protocol

## What is this?

HCP is a **hardware-software co-design framework** — think "Docker for hardware definitions." You write hardware in Rust, the compiler generates real silicon designs with error correction (ECC) baked in automatically, and eventually you'll share and deploy them via an API like container images.

### What category does this fall into?

It's a hybrid:
- **Not just software** — it generates real synthesizable hardware (Verilog/SystemVerilog)
- **Not just hardware** — the toolchain, compiler, and upcoming API server are software
- **Closest analogies**: Docker (but for hardware), npm/cargo (but packages are circuits), MCP (but "tools" are FPGA resources)

## Who benefits and how?

| User | Benefit |
|------|---------|
| **You (developer)** | Write hardware once in Rust, deploy to any FPGA/sim. ECC is automatic — no manual wiring |
| **Low-RAM users** | The compiler uses ~2MB RAM. Compare: Xilinx Vivado uses 4-64GB |
| **Students** | Access FPGA labs remotely (Phase 3). Learn hardware without $500 boards |
| **Companies** | Share custom accelerators between teams like Docker images (Phase 2) |
| **Researchers** | Reproducible hardware experiments with version control |
| **IoT/Edge** | Custom FPGA accelerators that use 1mW-1W vs a GPU's 50-700W |

## How does this beat NVIDIA/existing tools?

We don't try to beat NVIDIA at training GPT-5. We beat them where their architecture is overkill:

- **Zero launch overhead**: CUDA kernel launch costs ~10-20μs. Our FPGA path: zero — the hardware IS the computation
- **No memory wall**: GPU needs PCIe bus transfers. FPGA distributed RAM is right next to the logic
- **1000x less power**: iCE40 draws ~1mW vs H100's 700W
- **Right-sized**: Custom accelerators use exactly the resources needed
- **RAM**: Our compiler: ~2MB. Vivado: 4-64GB. CUDA runtime: 1-8GB minimum

## What's in Phase 1 (this release)?

```
hcp/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── hcp-core/                 # Core types: Bits, Signals, Ports, Modules, ECC schemes
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs          # BitWidth, SignalKind, EccScheme, Signal, Port
│   │       ├── module.rs         # Module, Assignment, AlwaysBlock, Expr
│   │       └── error.rs
│   ├── hcp-ecc/                  # ECC code generators
│   │   └── src/
│   │       ├── lib.rs
│   │       └── hamming.rs        # ⭐ Hamming SEC-DED encoder/decoder generator
│   ├── hcp-hdl/                  # HDL compiler
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ecc_pass.rs       # ⭐ ECC compiler pass — the core innovation
│   │       └── verilog.rs        # SystemVerilog code emitter
│   ├── hcp-package/              # Phase 2: OCI hardware image packaging
│   │   └── src/
│   │       ├── manifest.rs       # hcp.json — package metadata + ECC report
│   │       ├── image.rs          # OCI Image Layout with SHA-256 content addressing
│   │       └── builder.rs        # ⭐ One-command pipeline: Module → ECC → Verilog → OCI image
│   ├── hcp-protocol/             # Phase 3: JSON-RPC 2.0 protocol server
│   │   └── src/
│   │       ├── jsonrpc.rs        # ⭐ JSON-RPC 2.0 wire protocol (same as MCP)
│   │       ├── messages.rs       # 10 protocol methods (initialize, pull, deploy, etc.)
│   │       ├── registry.rs       # Image registry — stores and serves hardware images
│   │       ├── server.rs         # HCP server — routes JSON-RPC to handlers
│   │       └── client.rs         # HCP client — typed Rust API wrapping JSON-RPC
│   └── hcp-cli/                  # Command-line tool
│       └── src/
│           └── main.rs           # Demo: full Phase 1+2+3 lifecycle
```

### What the ⭐ starred files do:

**hamming.rs** — Given a data width (e.g., 32 bits), generates complete hardware modules for:
- An **encoder** that takes 32-bit data and produces 39-bit encoded output (32 data + 6 Hamming parity + 1 overall parity)
- A **decoder** that takes 39-bit encoded input and produces 32-bit corrected data + error flags
- Error flags: `err_correctable` (single-bit error, automatically fixed), `err_uncorrectable` (double-bit error, detected but not fixable)

**ecc_pass.rs** — A compiler pass that transforms your module automatically:
- Finds every signal annotated with ECC
- Generates encoder/decoder for each
- Widens internal storage to fit encoded data
- Adds error flag output ports to the module interface
- Produces a report showing overhead (bits added, % overhead)

## Quick Start

```bash
# Build everything
cargo build --workspace

# Run all 22 tests
cargo test --workspace

# Run the full demo (see ECC analysis, module definition, Verilog output)
cargo run -p hcp-cli

# Build optimized release binary (13MB, runs in ~2MB RAM)
cargo build --release -p hcp-cli
./target/release/hcp
```

## What the demo shows you

1. **ECC overhead table** — how many parity bits each data width needs
2. **Module definition** — an 8-bit counter with `#[ecc(HammingSecDed)]`
3. **ECC compiler pass** — automatically transforms the module
4. **SystemVerilog output** — encoder, decoder, and main module ready for synthesis

## The generated Verilog

The compiler produces three modules:
- `hamming_enc_8` — encodes 8-bit data to 13-bit codeword
- `hamming_dec_8` — decodes 13-bit codeword back to 8-bit data with error detection
- `counter_ecc` — your counter, with ECC wired in automatically

This Verilog can be fed directly into:
- **Yosys** → open-source synthesis for iCE40, ECP5 FPGAs ($5-$30 boards)
- **Vivado** → Xilinx/AMD FPGAs
- **Quartus** → Intel/Altera FPGAs
- **Verilator** → cycle-accurate simulation (no hardware needed)

## Roadmap

| Phase | What | When |
|-------|------|------|
| **1 ✅** | HDL compiler + ECC pass + Verilog backend | Done |
| **2 ✅** | OCI hardware images — packaging, SHA-256 integrity, manifest | Done |
| **3 ✅** | HCP protocol server (JSON-RPC 2.0, image registry, client/server) | Done |
| **4** | Lightweight virtualization — WASM sim, Verilator JIT, Docker containers | After 3 |
| **5** | P2P hardware mesh — share FPGA resources over network | After 4 |

## Belgrade Heritage — Standing on the Shoulders of Zoran Modli

In the autumn of 1983, something remarkable happened in Belgrade, Yugoslavia. Zoran Modli,
the host of *Ventilator 202* on Radio Beograd 202, began broadcasting computer programs
over FM radio waves. Listeners would hold their cassette recorders up to the radio,
tape the strange screeching sounds, then load those tapes into their Galaksija, ZX Spectrum,
or Commodore 64 computers. The programs would come alive — games, flight simulators,
even a digital magazine called *Hack News*.

The technical method was **Frequency Shift Keying (FSK)** — encoding digital 0s and 1s
as two different audible tones, transmitted at roughly 280 bits per second over standard
FM frequencies. Over three years, Ventilator 202 broadcast 150 pieces of software.
Listeners would modify the programs and send them back to Modli for re-broadcast,
creating one of the world's first collaborative software distribution networks —
a decade before the World Wide Web.

The Galaksija computer itself was designed by Voja Antonić as a build-it-yourself
machine, published as schematics in a magazine. No import restrictions, no expensive
Western hardware needed. Just standard off-the-shelf components and a soldering iron.
It embodied the idea that technology should be for everyone.

**HCP carries this same spirit forward:**

| 1983 Belgrade | 2026 HCP |
|---------------|----------|
| Software encoded as FSK audio tones | Hardware encoded as OCI container images |
| Broadcast over FM radio waves | Shared over internet via gRPC + mTLS |
| 280 bits per second | 1+ Gbps |
| Galaksija Z80 processor | Any FPGA, RISC-V, ARM, or simulator |
| One-way broadcast | Two-way API with telemetry |
| No error correction | ECC built into every signal automatically |
| Tape recorder as "download client" | `hcp pull` as the download command |
| Listeners modify and re-broadcast | Git-style version control + OCI registry |

The fundamental principle is identical: **encode instructions, transmit them through
a medium, rebuild functional computation on the receiver's hardware.** Modli sent
software that tells hardware what to do. We send the hardware definitions themselves.

Zoran Modli passed away on February 23, 2020. He was a journalist, radio DJ,
professional Boeing 727 pilot, and one of the most beloved personalities in Yugoslav
broadcasting history. HCP is dedicated to his memory and to the communitarian spirit
of the Galaksija movement — the belief that powerful technology belongs to everyone.

*"Galaksija stands tall as a monument to a different kind of technological life,
one teeming with exploration, experimentation, and communitarian spirit."*

## Setup Guide

### Prerequisites

- **Rust toolchain** (rustc + cargo): https://rustup.rs
- **Git**: to clone and version-control your work
- **Optional**: Yosys + nextpnr + icestorm (for actual FPGA synthesis)

### Quick Start

```bash
# Clone the repo
git clone https://github.com/ChrisX101010/hcp.git
cd hcp

# Install Rust if you don't have it
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Build everything
cargo build --workspace

# Run all 47 tests
cargo test --workspace -- --test-threads=1

# Run the full demo (ECC analysis → Verilog generation → OCI packaging)
cargo run -p hcp-cli

# Build optimized release binary (~13MB, runs in ~3MB RAM)
cargo build --release -p hcp-cli
./target/release/hcp
```

Works on Linux and macOS.

### Optional: Install FPGA Toolchain

```bash
# Linux (apt)
sudo apt install yosys nextpnr-ice40 fpga-icestorm

# macOS (homebrew)
brew install yosys nextpnr icestorm

# Then synthesize the generated Verilog for a real FPGA:
# yosys -p 'read_verilog counter_ecc.sv; synth_ice40 -top counter_ecc'
```

## License

Apache-2.0

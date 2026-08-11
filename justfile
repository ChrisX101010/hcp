set dotenv-load

default:
    just build

build:
    cargo build --workspace

test:
    cargo test --workspace -- --test-threads=1

lint:
    cargo clippy --workspace -- -D warnings
    cargo fmt --check

fix:
    cargo clippy --workspace --fix --allow-dirty
    cargo fmt

run:
    cargo run -p hcp-cli -- {{rest}}

demo:
    cargo run -p hcp-cli -- demo

sim:
    cargo run -p hcp-cli -- simulate --cycles {{cycles|20}}

fpga-ice40:
    cargo run -p hcp-cli -- fpga deploy --board ice40 --flash

release:
    cargo build --release -p hcp-cli
    @echo "✓ Release binary: target/release/hcp"

ci: lint test build

completions:
    mkdir -p completions
    cargo run -p hcp-cli -- completion bash > completions/hcp.bash
    cargo run -p hcp-cli -- completion zsh > completions/hcp.zsh
    cargo run -p hcp-cli -- completion fish > completions/hcp.fish
# ---------------------------------------------------------------------------
# hcp-crypto recipes — append these to the repo's existing justfile.
# ---------------------------------------------------------------------------

# Run the crate's Rust tests (RTL simulation runs too if iverilog is present)
aes-test:
    cargo test -p hcp-crypto

# Regenerate the committed Verilog from the Rust generator
aes-gen:
    cargo run -p hcp-crypto --example emit > crates/hcp-crypto/rtl/aes128_enc.sv
    @echo "wrote crates/hcp-crypto/rtl/aes128_enc.sv"

# Simulate the committed RTL against the FIPS-197 vectors (needs iverilog)
aes-sim:
    iverilog -g2012 -o /tmp/hcp_aes_sim \
        crates/hcp-crypto/rtl/aes128_enc.sv \
        crates/hcp-crypto/tb/aes128_enc_tb.sv
    vvp /tmp/hcp_aes_sim

# Synthesize for iCE40 and print resource usage (needs yosys)
aes-synth:
    #!/usr/bin/env bash
    yosys -p "read_verilog -sv crates/hcp-crypto/rtl/aes128_enc.sv; \
              synth_ice40 -top aes128_enc; stat" 2>&1 \
      | awk '/=== aes128_enc ===/{n++} n==2' | head -20

# Fail if the committed RTL drifted from the generator
aes-check-gen:
    cargo run -p hcp-crypto --example emit > /tmp/aes_check.sv
    diff -u crates/hcp-crypto/rtl/aes128_enc.sv /tmp/aes_check.sv

# Everything
aes-all: aes-test aes-sim aes-synth aes-check-gen

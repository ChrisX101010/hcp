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

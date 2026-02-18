# HyperMachine development task runner
# Install: cargo install just
# Usage: just <recipe>

# Default recipe - show available commands
default:
    @just --list

# Build all crates in debug mode
build:
    cargo build --workspace --exclude hv1-core --exclude hv1-boot

# Build in release mode
build-release:
    cargo build --workspace --release --exclude hv1-core --exclude hv1-boot

# Run all tests
test:
    cargo test --workspace --exclude hv1-core --exclude hv1-boot

# Run tests with output
test-verbose:
    cargo test --workspace --exclude hv1-core --exclude hv1-boot -- --nocapture

# Run clippy lints
lint:
    cargo clippy --workspace --all-targets --exclude hv1-core --exclude hv1-boot -- -D warnings

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Format all code
fmt:
    cargo fmt --all

# Run clippy + fmt check (CI equivalent)
ci: lint fmt-check test

# Run benchmarks
bench:
    cargo bench -p hv2-core --bench crypto_bench -- --noplot
    cargo bench -p hv2-api --bench api_bench -- --noplot

# Run security audit
audit:
    cargo deny check
    cargo audit

# Generate documentation
doc:
    cargo doc --workspace --no-deps --exclude hv1-core --exclude hv1-boot

# Open documentation in browser  
doc-open:
    cargo doc --workspace --no-deps --exclude hv1-core --exclude hv1-boot --open

# Clean build artifacts
clean:
    cargo clean

# Check workspace compiles without building
check:
    cargo check --workspace --all-targets --exclude hv1-core --exclude hv1-boot

# Run a specific crate's tests
test-crate crate:
    cargo test -p {{crate}}

# Run the HyperMachine CLI
run-cli *args:
    cargo run -p hm-cli -- {{args}}

# Run the hypervisor CLI
run-hv2 *args:
    cargo run -p hv2-cli -- {{args}}

# Build container image
container-build:
    docker build -f Containerfile -t hypermachine:dev .

# Run coverage (requires cargo-tarpaulin)
coverage:
    cargo tarpaulin --workspace --exclude hv1-core --exclude hv1-boot --out html --output-dir target/coverage

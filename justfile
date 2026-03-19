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

# ── HV1 (Type 1 bare-metal hypervisor) — requires nightly ──

# Check hv1-core compiles for bare-metal target
hv1-check:
    cargo +nightly check -p hv1-core --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem

# Check hv1-boot compiles for bare-metal target
hv1-check-boot:
    cargo +nightly check -p hv1-boot --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem

# Run hv1-core unit tests (on host)
hv1-test:
    cargo +nightly test -p hv1-core

# Run hv1-core unit tests with output
hv1-test-verbose:
    cargo +nightly test -p hv1-core -- --nocapture

# Build hv1-core for bare-metal target
hv1-build:
    cargo +nightly build -p hv1-core --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem

# Build hv1-boot UEFI disk image
hv1-build-boot:
    cargo +nightly build -p hv1-boot --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem

# Run clippy on hv1-core (nightly)
hv1-lint:
    cargo +nightly clippy -p hv1-core --target x86_64-unknown-none -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem -- -D warnings

# Full HV1 CI equivalent
hv1-ci: hv1-check hv1-check-boot hv1-test hv1-lint

# Build HV1 UEFI disk image (debug)
hv1-image:
    python tools/mk-hv1-image.py

# Build HV1 UEFI disk image (release)
hv1-image-release:
    python tools/mk-hv1-image.py --release

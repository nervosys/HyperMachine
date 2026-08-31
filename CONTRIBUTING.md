# Contributing to HyperMachine

Thank you for your interest in contributing to HyperMachine! This document provides guidelines and information for contributors.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/HyperMachine.git`
3. Create a branch: `git checkout -b feature/my-feature`
4. Make your changes
5. Run tests: `cargo test --workspace --exclude hv1-core --exclude hv1-boot`
6. Submit a pull request

## Development Setup

### Prerequisites

- Rust 1.87 or later (see `rust-toolchain.toml`)
- Linux, macOS, or Windows with WSL2
- Optional: KVM/QEMU for testing

### Building

```bash
# Build all crates
cargo build --workspace --exclude hv1-core --exclude hv1-boot

# Build with release optimizations
cargo build --workspace --exclude hv1-core --exclude hv1-boot --release

# Run tests
cargo test --workspace --exclude hv1-core --exclude hv1-boot

# Quick sanity check (same excludes -- see note below)
cargo check --workspace --exclude hv1-core --exclude hv1-boot
# ...or the shorthand alias defined in .cargo/config.toml:
cargo check-ws
```

> **Note:** `hv1-core` and `hv1-boot` are Type-1, bare-metal, nightly-only
> crates. A bare `cargo check --workspace` (without the excludes above) will
> fail on stable while building the `bootloader` crate with an error like
> `` the `-Z` flag is only accepted on the nightly channel of Cargo ``. This
> is expected -- those two crates are checked separately in CI on a pinned
> nightly toolchain with `-Zbuild-std` (see the `hv1-check`/`hv1-clippy` jobs
> in `.github/workflows/ci.yml`). It is not something to fix locally; always
> pass the `--exclude` flags (or use `cargo check-ws`) when working outside
> those two crates.

## Code Style

- Follow Rust standard formatting: `cargo fmt --all -- --check`
- Ensure no clippy warnings: `cargo clippy --workspace --exclude hv1-core --exclude hv1-boot -- -D warnings`
- Write documentation for public APIs
- Add tests for new functionality

## Testing

- Unit tests for individual components
- Integration tests for end-to-end scenarios
- Property tests with proptest where applicable
- Benchmarks for performance-critical code

## Pull Request Process

1. **Sign the CLA** — First-time contributors must sign our [Contributor License Agreement](CLA.md).
   The CLA bot will prompt you automatically on your first pull request.
2. Update documentation for any API changes
3. Add tests for new features
4. Ensure all tests pass
5. Update CHANGELOG.md
6. Request review from maintainers

## Code of Conduct

- Be respectful and inclusive
- Welcome newcomers
- Focus on constructive feedback
- Collaborate openly

## Areas for Contribution

- Core VM engine improvements
- New device emulation
- GPU acceleration
- Network stack enhancements
- AI agent features
- Documentation
- Examples and tutorials
- Performance optimization
- Bug fixes

## Questions?

Open an issue or start a discussion on GitHub!

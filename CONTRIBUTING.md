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

- Rust 1.75 or later
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
```

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

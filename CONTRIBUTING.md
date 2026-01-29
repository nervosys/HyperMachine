# Contributing to HV2

Thank you for your interest in contributing to HV2! This document provides guidelines and information for contributors.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/hv2.git`
3. Create a branch: `git checkout -b feature/my-feature`
4. Make your changes
5. Run tests: `cargo test --all`
6. Submit a pull request

## Development Setup

### Prerequisites

- Rust 1.75 or later
- Linux, macOS, or Windows with WSL2
- Optional: KVM/QEMU for testing

### Building

```bash
# Build all crates
cargo build --all

# Build with release optimizations
cargo build --all --release

# Run tests
cargo test --all

# Run examples
cargo run --example basic
```

## Code Style

- Follow Rust standard formatting: `cargo fmt`
- Ensure no clippy warnings: `cargo clippy --all`
- Write documentation for public APIs
- Add tests for new functionality

## Testing

- Unit tests for individual components
- Integration tests for end-to-end scenarios
- Property tests with proptest where applicable
- Benchmarks for performance-critical code

## Pull Request Process

1. Update documentation for any API changes
2. Add tests for new features
3. Ensure all tests pass
4. Update CHANGELOG.md
5. Request review from maintainers

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

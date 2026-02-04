# Development Setup

Set up a development environment for contributing to HyperMachine.

## Prerequisites

- Rust 1.75+
- Linux with KVM, Windows with WHPX, or macOS with HVF
- Git

## Clone and Build

```bash
git clone https://github.com/nervosys/HyperMachine
cd HyperMachine

# Build all crates
cargo build

# Build release
cargo build --release

# Run tests
cargo test --workspace
```

## Development Workflow

```bash
# Run the CLI
cargo run -p hm-cli -- t2 list

# Run with logging
RUST_LOG=debug cargo run -p hm-cli -- t2 list

# Run the GUI
cargo run -p hm-gui

# Run MCP server
cargo run -p hm-cli -- mcp serve --api-key test
```

## IDE Setup

### VS Code

Recommended extensions:
- rust-analyzer
- CodeLLDB
- Even Better TOML

### IntelliJ/CLion

- Install Rust plugin
- Open as Cargo project

## Running Tests

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p hv2-core

# With output
cargo test -- --nocapture

# Integration tests
cargo test --test integration
```

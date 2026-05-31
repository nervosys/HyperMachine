# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Real post-quantum cryptography** (`hv2-core`): ML-KEM (FIPS 203), ML-DSA
  (FIPS 204), and SLH-DSA (FIPS 205) now delegate to the audited pure-Rust
  RustCrypto crates (`ml-kem`, `ml-dsa`, `slh-dsa`) with real
  keygen/encapsulation/sign/verify and canonical byte serialization, replacing
  the previous SHA-256/HMAC API placeholders. Gated behind the default `pqc`
  feature.
- **Real RSA** (`hv2-core`): RSA key generation and PKCS#1 v1.5 / PSS signing
  via the pure-Rust `rsa` crate (the `ring` backend cannot generate RSA keys).
- **Agent-driven VM workflow example** (`hm-cli`): `agent_vm_workflow` shows an
  AI agent discovering tools (OpenAI/Anthropic/Gemini schemas) and driving a
  full VM lifecycle (`vm.create` → `start` → `execute_script` → `delete`)
  through the typed `ToolExecutor`.
- **MCP workload example** (`hv2-agent`): `agent_mcp_workflow` drives a complete
  VM lifecycle over the `McpServer` tool surface — capability-scoped session,
  tool discovery, provision → boot → `guest.exec` → snapshot → resize → restore
  → teardown — and prints the resulting audit log.
- **LLM tool-schema example** (`hv2-agent`): `llm_tool_schemas` projects the MCP
  tool registry into the OpenAI, Anthropic, and Gemini tool-use formats.
- **MCP workflow regression tests** (`hv2-agent`): `tests/mcp_workflow.rs` locks
  in the schema↔handler parameter contract for the common-workload tools.
- **Recursive planner** (`hv2-agent`): the GOAP planner now performs real
  recursive backward chaining with backtracking to satisfy action
  preconditions, replacing the previous single-level stub.

### Changed
- **Crypto enabled by default** (`hv2-core`): `default = ["ring", "pqc"]`, so
  AES-256-GCM/SHA use the validated `ring` backend out of the box. The insecure
  software AES-GCM fallback is removed — without `ring` the operations return
  `NotImplemented` instead of weak crypto.
- **README/docs**: reconciled crypto claims — "FIPS-approved algorithm
  implementations, not a FIPS 140-3 validated module"; AES-256-GCM only; added
  an ECDSA P-521 caveat; PQC described as real and RustCrypto-backed.

### Fixed
- **KVM/HVF FFI doc-comment corruption** (`hv2-core`): repaired control bytes
  (VT/FF/ESC/BEL) and a split `///` comment in the Linux (`kvm_ffi.rs`) and
  macOS (`hvf.rs`) backends that produced module-scope syntax errors — invisible
  on Windows (cfg-gated) but breaking Linux/macOS compilation and `cargo fmt`.
- **Clippy**: resolved lints surfaced by enabling `ring`/`pqc` and by the
  toolchain bump (`needless_return`, `collapsible_match`, `unnecessary_sort_by`,
  `manual_checked_ops`); workspace is `clippy -D warnings` and `cargo fmt`
  clean. Aligned `clippy.toml` MSRV to 1.95.
- **MCP tool schema/handler mismatch** (`hv2-agent`): the published tool schemas
  disagreed with the dispatcher, so agents following the advertised contract got
  wrong results — `vm.create` ignored `cpu_cores`/`memory_gb` (silently creating
  a 1-CPU/512 MB VM), and VM/snapshot/network tools advertised `vm_name`/
  `snapshot_name`/`network_name`/`interface_name` while the handler read
  `vm_id`/`snapshot_id`/`network`/`interface_id`. Schemas and handlers are now
  consistent and covered by regression tests.

### Security
- Triaged RUSTSEC-2023-0071 (the `rsa` crate's Marvin-attack timing
  side-channel) in `deny.toml` with justification; no fixed upstream release
  exists and HyperMachine does not expose RSA as an online decryption oracle.

## [1.0.0] - 2026-03-25

### Added
- **TLS/HTTPS support** (`hv2-api`): `TlsConfig`, `build_rustls_config()`, and
  `serve_tls()` for encrypted API transport using rustls.
- **Permission middleware**: Graph-based permission middleware wired into the API
  server with resource scope hierarchy and role-based access control.
- **Fuzz testing targets**: 7 `cargo-fuzz` targets covering API parsing, VM config,
  agent messages, CPU instruction decoding, memory operations, PCI config, and
  interrupt controller state.
- **Integration tests**: 15 tests for `hv2-agent`, 31 for `hv2-cpu`, 15 headless
  CI tests for `hm-gui`, and 15 end-to-end stack tests for `hv2-api`.
- **End-to-end stack tests** (`hv2-api`): Full-router tests exercising health checks,
  VM CRUD lifecycle, runtime sessions, workload scheduling, Prometheus metrics,
  agentic ontology (JSON-LD), AI tool formats, feature toggling, and snapshots.
- **Feature-gated GPU** (`hv2-gpu`): `wgpu-backend` and `vulkan-backend` features
  with optional dependencies so the crate compiles with `--no-default-features`.

### Changed
- **Axum 0.7 → 0.8 upgrade**: Bumped `axum` workspace dependency from 0.7 to 0.8,
  migrated WebSocket `Message::Text` to `Utf8Bytes`, updated all route path parameters
  from `:param` syntax to `{param}` syntax across 7 source files.
- **Version bump**: Workspace version `0.1.0` → `1.0.0` across all 13 crates.
- **Documentation link**: Updated `documentation` field in `Cargo.toml` from invalid
  `docs.rs/hypermachine` to GitHub docs directory.

### Fixed
- **Axum route parameter syntax**: All VM CRUD routes in `rest.rs` and ontology routes
  in `ontology.rs` were using `{id}` (Axum 0.8 syntax) while depending on Axum 0.7,
  causing all parameterized routes to return 404. Fixed by upgrading to Axum 0.8.
- **Unsafe audit**: Added `SAFETY` comments to 33 unsafe blocks across 7 files
  (`kvm.rs`, `interrupt.rs`, `memory.rs`, `serial.rs`, `svm.rs`, `vmx.rs`, `vm.rs`).

## [0.3.0] - 2026-03-24

### Added
- **Graph-based hierarchical permissions**: DAG-structured permission system for
  agentic AI systems with resource scope hierarchy (Root → Org → Tenant → Project → VM),
  role inheritance with cycle detection, controlled delegation with attenuation and
  depth limits, permission resolution engine, and append-only audit trail.
  23 unit tests covering the full permission lifecycle.
- **Unikernel lifecycle integration tests**: 19 tests covering boot protocols, guest
  memory management, and pool operations.
- **Expanded test coverage**: +32 tests across hm-cli, hv2-cpu, hv2-net, and hv2-api.

### Changed
- Removed internal business strategy and session planning documents from public
  repository (moved to .gitignore).
- Removed duplicate docs/GETTING_STARTED.md (root copy is canonical).
- Bumped Containerfile Rust version to 1.87.
- Replaced invalid crates.io category `"os"` with `"emulators"`.

### Fixed
- Resolved all remaining clippy warnings across the workspace.

## [0.2.0] - 2025-07-13

### Fixed
- **PIC 8259 cascade deadlock**: `acknowledge_interrupt()` and `send_eoi()` held master
  lock while re-acquiring it in slave path, deadlocking on `parking_lot::Mutex`. Added
  `drop(master)` before slave branch in both methods.
- **E1000 DMA undefined behavior**: `process_rx_ring_dma()` and `process_tx_ring_dma()`
  took `&[u8]` but wrote through immutable references via raw pointer casts (UB at
  opt-level=1). Changed to `&mut [u8]` with safe `copy_from_slice`.
- **E1000 RX queue drain**: `process_rx_queue()` consumed packets without DMA writeback
  when ring was configured, starving `process_rx_ring_dma()`.
- **IOAPIC EOI re-delivery**: `end_of_interrupt()` now checks `irq_state` bitmap after
  clearing `remote_irr` to re-assert level-triggered interrupts still active.

### Added
- **MCP guest agent operations**: `execute_command`, `read_file`, `write_file`,
  `list_processes` on the MCP guest agent interface.
- **E1000 DMA**: Full RX/TX DMA with guest memory read/write for the Intel 82540EM NIC.
- **VirtIO GPU**: Capset dispatch (Virgl, Virgl2, Venus, Cross-Domain) and
  `transfer_to_host_2d` with guest memory DMA.
- **xHCI USB**: Transfer ring processing (Normal, Setup, Data, Status TRBs).
- **Intel HDA**: CORB verb dispatch (get/set parameters, power, pin config, stream format,
  AMP gain, connections).
- **RSA crypto**: Software modular exponentiation for encrypt/decrypt.
- **Post-quantum crypto**: ML-DSA (Dilithium) and SLH-DSA (SPHINCS+) signature
  verification with hash-based schemes.
- **FIPS AES-GCM fallback**: Hand-rolled GHASH + CTR mode when hardware AES-NI unavailable.
- **TAP loopback mode**: Memory buffer mode for testing without OS TAP devices.
- **DurableStore External backend**: HTTP-based storage with retry and circuit breaker.
- **Linux boot helper**: `boot_with_mapper()` loads kernel, initrd, and cmdline via
  address-space mapper.
- **ACPI DSDT enhancements**: `_CRS` resource blocks, `CPU0` device scope, `\_S5_` sleep
  state object in AML bytecode.
- **PIC cascade tests**: `test_slave_pic_cascade` (fixed, formerly ignored) and
  `test_slave_pic_multiple_irqs` for IRQs 8/10/12/15 through cascade.

## [0.1.0] - 2025-01-01

### Added

- **Type-2 hosted hypervisor** (`hv2-core`) with vCPU, memory, and device management
  - KVM (Linux), WHPX (Windows), and HVF (macOS) backend support
  - Zero-copy memory mapping with `memmap2`
  - Full device model: serial console, block storage, RTC, PCI bus, interrupt controller
  - FIPS 140-3 compliant cryptography (AES-256-GCM, SHA-256/512, ECDSA, RSA)
  - JIT compilation engine for dynamic code
  - Snapshot and restore for VM state
- **Type-1 bare-metal hypervisor** (`hv1-core`, `hv1-boot`) with UEFI bootloader
- **CPU virtualization** (`hv2-cpu`) with x86_64, ARM64, and RISC-V instruction decoding
- **GPU virtualization** (`hv2-gpu`) with Vulkan/WebGPU, passthrough, and virtual GPU
- **Networking** (`hv2-net`) with full TCP/IP stack, TAP/TUN, virtio-net, and DHCP
- **AI agent interface** (`hv2-agent`) with MCP server, WASM plugin runtime, and scripting API
  - OpenAI/Claude/Gemini tool format support
  - Agent lifecycle management and learning framework
- **REST/gRPC API server** (`hv2-api`) with WebSocket streaming
- **CLI tool** (`hm-cli`) with integrated MCP server and VM management commands
- **Desktop GUI** (`hm-gui`) with virt-manager style interface and AI automation API
- **Deployment infrastructure**: Helm chart, Kubernetes manifests, Terraform configs
- **CI/CD**: Build, test, security audit, coverage, benchmarks, release, and deploy workflows
- **Security**: `cargo-deny` configuration, Dependabot, seccomp filtering, capability-based access

[Unreleased]: https://github.com/nervosys/HyperMachine/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/nervosys/HyperMachine/compare/v0.3.0...v1.0.0
[0.3.0]: https://github.com/nervosys/HyperMachine/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nervosys/HyperMachine/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nervosys/HyperMachine/releases/tag/v0.1.0

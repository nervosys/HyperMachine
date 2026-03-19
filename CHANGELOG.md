# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/nervosys/HyperMachine/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/nervosys/HyperMachine/releases/tag/v0.1.0

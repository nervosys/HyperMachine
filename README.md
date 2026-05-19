# HyperMachine

**Agentic hypervisors for autonomous AI systems.**

[![CI](https://github.com/nervosys/HyperMachine/actions/workflows/ci.yml/badge.svg)](https://github.com/nervosys/HyperMachine/actions)
[![codecov](https://codecov.io/gh/nervosys/HyperMachine/branch/master/graph/badge.svg)](https://codecov.io/gh/nervosys/HyperMachine)
[![License](https://img.shields.io/badge/license-AGPL--3.0--only%20OR%20Commercial-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.87%2B-orange.svg)](https://www.rust-lang.org)

A high-performance hypervisor framework in Rust with first-class AI agent support. Type-1 bare-metal and Type-2 hosted modes.

## Features

| Category        | Capabilities                                                                 |
| --------------- | ---------------------------------------------------------------------------- |
| **Performance** | Zero-copy memory, JIT compilation, hardware GPU virtualization               |
| **Networking**  | Full TCP/IP stack, TAP/TUN, gRPC/REST APIs, distributed orchestration        |
| **GPU**         | Vulkan/WebGPU, passthrough, virtual GPU, CUDA/OpenCL                         |
| **AI-First**    | MCP server, scriptable API, WASM plugins, LLM tool formats                   |
| **GUI**         | Desktop app, AI-driven automation, semantic control API                      |
| **Security**    | FIPS 140-3 crypto, seccomp filtering, capability-based access, audit logging |

### 1. Agentic-First Virtualization

HyperMachine is the first hypervisor designed from the ground up for AI agent workloads. Every VM is an MCP-addressable resource: agents discover capabilities via ontology endpoints, invoke typed tools (`vm.create`, `vm.exec`, `gpu.reserve`), and receive structured results — no shell scraping or brittle CLI wrappers. Multi-LLM tool schemas ship built-in for OpenAI, Anthropic, and Google formats.

### 2. Dual-Mode Architecture (Type-1 + Type-2)

A single codebase runs as both a **Type-2 hosted hypervisor** (KVM, WHPX, HVF) and a **Type-1 bare-metal hypervisor** (Intel VMX, AMD SVM) with no code duplication. The same VM definitions, device models, and API surface work in both modes — develop on your laptop, deploy bare-metal in production.

### 3. GPU Fabric with Topology-Aware Placement

HyperMachine models GPU interconnect topology (NVLink, NVSwitch, PCIe) and makes placement decisions based on real bandwidth and latency. Capacity reservations with SLA tiers (platinum/gold/silver/bronze) prevent noisy-neighbor GPU contention. Fleet-wide GPU health monitoring tracks utilization, temperature, and ECC errors across hosts.

### 4. Post-Quantum Cryptography

Alongside classical FIPS 140-3 algorithms (AES-GCM, RSA, ECDSA), HyperMachine ships ML-KEM (Kyber) for key encapsulation, ML-DSA (Dilithium) for digital signatures, and SLH-DSA (SPHINCS+) for hash-based signatures — all NIST-standardized, quantum-resistant, and available today.

### 5. Pure Rust, Zero Unsafe in Business Logic

~206,000 lines of Rust across 13 crates with zero `todo!()`, `unimplemented!()`, or placeholder stubs. The full stack — from bare-metal boot sequence to REST middleware to GPU scheduling — is implemented in safe Rust. 4,480+ tests, zero clippy warnings, zero known advisories.

### 6. Semantic GUI Automation

AI agents control the desktop GUI through a typed command API (`gui.navigate`, `gui.dialog.open`, `gui.form.set_field`) rather than pixel-based screen scraping. This is deterministic, resolution-independent, and orders of magnitude faster than vision-based approaches like screen capture automation.

### 7. Enterprise API Middleware Stack

The REST API ships with 28 composable middleware layers out of the box: rate limiting, circuit breakers, request replay protection, tenant isolation, geo-IP enrichment, W3C trace propagation, HMAC payload signing, schema validation, slow-request detection, maintenance mode, and more — all configurable, all tested.

## Architecture

```
+----------------------------------------------------------+
|                   AI Agent Interface                     |
|            (MCP Server - OpenAI/Claude/Gemini)           |
+----------------------------------------------------------+
|                    Remote API Layer                      |
|                  (gRPC - REST - WebSocket)               |
+-------------+-------------+-------------+----------------+
|     CPU     |     GPU     |   Network   |     Memory     |
|  Emulation  |   Vulkan    |   TAP/TUN   |   Management   |
+-------------+-------------+-------------+----------------+
|                   HyperMachine Core                      |
|       HV2 (KVM/WHPX/HVF) - HV1 (VMX/SVM bare-metal)      |
+----------------------------------------------------------+
```

## Requirements

- **Rust** 1.87+ (stable) for Type-2 crates; nightly for Type-1 (`hv1-core`, `hv1-boot`)
- **Hypervisor backend** (Type-2 mode): KVM (Linux), WHPX (Windows), or HVF (macOS)
- **protoc** (Protocol Buffers compiler) for building gRPC components

## Quick Start

```bash
# Build (excludes nightly-only Type-1 crates)
git clone https://github.com/nervosys/HyperMachine && cd HyperMachine
cargo build --release --workspace --exclude hv1-core --exclude hv1-boot

# Create and run a VM (Type-2 hosted mode)
hm t2 create --name myvm --cpu 4 --memory 8G --gpu
hm t2 start myvm

# Start MCP server for AI agents
hm mcp serve --api-key "your-key"
```

## AI Agent Integration

HyperMachine exposes a Model Context Protocol (MCP) server for AI agents:

```bash
# Discover available tools
curl http://localhost:8080/mcp/tools

# LLM-specific tool formats
curl http://localhost:8080/agentic/tools/openai     # GPT-4o, o1, o3
curl http://localhost:8080/agentic/tools/anthropic  # Claude 4, Sonnet
curl http://localhost:8080/agentic/tools/gemini     # Gemini 2.5

# Execute operations
curl -X POST http://localhost:8080/mcp/call \
  -H "Authorization: Bearer your-key" \
  -d '{"tool": "vm.create", "arguments": {"name": "ai-sandbox", "cpu_cores": 4}}'
```

**Python SDK:**

```python
from hypermachine import HyperMachine

hm = HyperMachine("http://localhost:8080", api_key="your-key")
vm = hm.create_vm("sandbox", cpu=4, memory="8G", gpu=True)
vm.start()
vm.exec("echo 'Hello from AI agent'")
```

## GUI Automation

HyperMachine includes a desktop GUI with **semantic automation API** for AI agents:

```rust
use hm_gui::{AutomationHandle, GuiCommand, DialogType, FormType};

// Create automation handle
let (handle, receiver) = AutomationHandle::new();

// AI agent controls the GUI
handle.open_dialog(DialogType::CreateVm)?;
handle.set_field(FormType::CreateVm, "name", "ai-sandbox")?;
handle.set_field(FormType::CreateVm, "cpus", 4)?;
handle.set_field(FormType::CreateVm, "memory_mb", 8192)?;
handle.execute(GuiCommand::SubmitDialog(DialogType::CreateVm))?;
```

**Available GUI Tools (13 total):**

| Tool                           | Description                                             |
| ------------------------------ | ------------------------------------------------------- |
| `gui.navigate`                 | Navigate views (welcome, vm_details, console, settings) |
| `gui.dialog.open/close/submit` | Manage dialogs (create_vm, settings, about)             |
| `gui.vm.select`                | Select VM by id, name, or partial match                 |
| `gui.vm.action`                | VM operations (start, stop, pause, delete, console)     |
| `gui.form.set_field`           | Set form values programmatically                        |
| `gui.get_state`                | Query current GUI state                                 |

**LLM JSON Commands:**

```json
{"type":"OpenDialog","params":"create_vm"}
{"type":"SetFormField","params":{"form":"create_vm","field":"name","value":"my-vm"}}
{"type":"SubmitDialog","params":"create_vm"}
```

This semantic approach is **superior to screen-based automation** (like Anthropic Computer Use) because it is deterministic, fast, and layout-independent.

## Cryptography

FIPS 140-3 compliant cryptographic modules:

| Type             | Algorithms                                             |
| ---------------- | ------------------------------------------------------ |
| **Symmetric**    | AES-128/256-GCM, SHA-256/384/512, HMAC, HKDF           |
| **Asymmetric**   | RSA-2048/3072/4096, ECDSA P-256/P-384/P-521            |
| **Post-Quantum** | ML-KEM (Kyber), ML-DSA (Dilithium), SLH-DSA (SPHINCS+) |

## Deployment

### GPU Fabric API

HyperMachine provides a GPU Fabric REST API for topology-aware GPU placement and fleet management:

```bash
# Query GPU topology
curl http://localhost:8080/api/v1/gpu-fabric/topology

# List fleet hosts
curl http://localhost:8080/api/v1/gpu-fabric/fleet

# Check capacity for GPU workloads
curl -X POST http://localhost:8080/api/v1/gpu-fabric/capacity/check \
  -H "Content-Type: application/json" \
  -d '{"gpu_count": 4, "min_vram_mb": 40960, "interconnect": "NvLink"}'

# Reserve GPU capacity
curl -X POST http://localhost:8080/api/v1/gpu-fabric/capacity/reserve \
  -H "Authorization: Bearer your-key" \
  -d '{"gpu_count": 8, "sla_tier": "Premium", "duration_secs": 3600}'
```

### Kubernetes / Terraform

```bash
# Kubernetes
helm install hypermachine ./deploy/helm/hypermachine \
  --set environment=production \
  --set replicaCount=3

# Terraform (AWS EKS)
cd deploy/terraform && terraform apply -var="environment=production"
```

## Performance

Benchmarks on AMD Ryzen 9 7950X:

| Operation           | Throughput |
| ------------------- | ---------- |
| AES-256-GCM encrypt | ~600 MiB/s |
| AES-256-GCM decrypt | ~700 MiB/s |
| SHA-256             | ~3.7 GiB/s |
| SHA-512             | ~3.5 GiB/s |

```bash
cargo bench -p hv2-core --bench crypto_bench
```

## Project Structure

```bash
crates/
  hm-cli      # CLI + MCP server
  hm-gui      # Desktop GUI with AI automation API
  hv1-arm     # ARM64 EL2 hypervisor backend (127 tests)
  hv1-boot    # Type-1 bare-metal bootloader (nightly)
  hv1-core    # Type-1 bare-metal hypervisor core (nightly)
  hv2-core    # Core engine (CPU, memory, devices, crypto)
  hv2-cpu     # CPU virtualization and instruction decoding
  hv2-gpu     # GPU virtualization (Vulkan/WebGPU, passthrough)
  hv2-net     # Networking (TCP/IP stack, TAP/TUN, virtio-net)
  hv2-agent   # AI agent interface (MCP, WASM plugins)
  hv2-api     # REST/gRPC API server + GPU Fabric endpoints
  hv2-cli     # Standalone hypervisor CLI
  hv2-runtime # Fleet runtime, scheduler, GPU observability
deploy/
  k8s/        # Kubernetes manifests
  helm/       # Helm charts
  terraform/  # Infrastructure as code
```

## Documentation

- **[Getting Started](GETTING_STARTED.md)** — Installation, prerequisites, first VM
- **[API Quickstart](docs/API_QUICKSTART.md)** — REST/gRPC API reference and examples
- **[Architecture](docs/architecture.md)** — System design and internals
- **[Deployment Guide](docs/DEPLOYMENT_GUIDE.md)** — Production deployment on Kubernetes/Terraform
- **[GPU Virtualization](docs/gpu.md)** — Vulkan/WebGPU passthrough and virtual GPU
- **[Guest Programming Guide](docs/GUEST_PROGRAMMING_GUIDE.md)** — Writing code for guest VMs

## Troubleshooting

| Problem                         | Solution                                                                              |
| ------------------------------- | ------------------------------------------------------------------------------------- |
| `KVM not available`             | Enable VT-x/AMD-V in BIOS; `modprobe kvm_intel` or `kvm_amd`                          |
| `WHPX not found` (Windows)      | Enable "Windows Hypervisor Platform" in Windows Features                              |
| `protoc not found`              | Install protobuf compiler: `apt install protobuf-compiler` or `brew install protobuf` |
| Build fails on nightly crates   | Type-1 crates require nightly; use `--exclude hv1-core --exclude hv1-boot`            |
| Permission denied on `/dev/kvm` | Add user to kvm group: `sudo usermod -aG kvm $USER`                                   |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, coding standards, and PR process.

## License

This project is dual-licensed:

- **AGPL-3.0** (GNU Affero General Public License v3) — free and open source with strong copyleft. See [LICENSE](LICENSE).
- **Commercial License** — available for use without AGPL obligations. See [LICENSE-COMMERCIAL](LICENSE-COMMERCIAL).

For commercial licensing inquiries, contact
[licensing@nervosys.ai](mailto:licensing@nervosys.ai).

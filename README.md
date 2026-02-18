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

## Architecture

```
+---------------------------------------------------------+
|                   AI Agent Interface                     |
|            (MCP Server - OpenAI/Claude/Gemini)          |
+---------------------------------------------------------+
|                    Remote API Layer                      |
|                  (gRPC - REST - WebSocket)              |
+-------------+-------------+-------------+----------------+
|     CPU     |     GPU     |   Network   |     Memory     |
|  Emulation  |   Vulkan    |   TAP/TUN   |   Management   |
+-------------+-------------+-------------+----------------+
|                     HyperMachine Core                    |
|         HV2 (KVM/WHPX/HVF) - HV1 (VMX/SVM bare-metal)  |
+---------------------------------------------------------+
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
  hv1-boot    # Type-1 bare-metal bootloader (nightly)
  hv1-core    # Type-1 bare-metal hypervisor core (nightly)
  hv2-core    # Core engine (CPU, memory, devices, crypto)
  hv2-cpu     # CPU virtualization and instruction decoding
  hv2-gpu     # GPU virtualization (Vulkan/WebGPU, passthrough)
  hv2-net     # Networking (TCP/IP stack, TAP/TUN, virtio-net)
  hv2-agent   # AI agent interface (MCP, WASM plugins)
  hv2-api     # REST/gRPC API server
  hv2-cli     # Standalone hypervisor CLI
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
[licensing@nervosys.com](mailto:licensing@nervosys.com).

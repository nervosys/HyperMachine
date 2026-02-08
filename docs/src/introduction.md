# HyperMachine

<div class="hero">
<h2>Agentic Hypervisors for Autonomous AI Systems</h2>
</div>

[![CI](https://github.com/nervosys/HyperMachine/actions/workflows/ci.yml/badge.svg)](https://github.com/nervosys/HyperMachine/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/nervosys/HyperMachine/blob/master/LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

**HyperMachine** is a high-performance hypervisor framework written in Rust with first-class AI agent support. It provides both Type-1 (bare-metal) and Type-2 (hosted) virtualization modes.

## Key Features

| Category        | Capabilities                                                                 |
| --------------- | ---------------------------------------------------------------------------- |
| **Performance** | Zero-copy memory, JIT compilation, hardware GPU virtualization               |
| **Networking**  | Full TCP/IP stack, TAP/TUN, gRPC/REST APIs, distributed orchestration        |
| **GPU**         | Vulkan/WebGPU, passthrough, virtual GPU, CUDA/OpenCL                         |
| **AI-First**    | MCP server, scriptable API, WASM plugins, LLM tool formats                   |
| **GUI**         | Desktop app, AI-driven automation, semantic control API                      |
| **Security**    | FIPS 140-3 crypto, seccomp filtering, capability-based access, audit logging |

## Why HyperMachine?

### Built for AI Agents

HyperMachine is designed from the ground up for AI agent orchestration:

- **Model Context Protocol (MCP)** server exposes all VM operations as tools
- **Native LLM tool formats** for OpenAI, Anthropic Claude, and Google Gemini
- **Semantic GUI automation** allows AI to control the desktop app programmatically
- **Python SDK** for easy integration with AI frameworks

### High Performance

- Written in Rust for memory safety and zero-cost abstractions
- Hardware-accelerated virtualization (KVM, WHPX, HVF)
- Zero-copy memory operations
- GPU passthrough and virtual GPU support

### Production Ready

- FIPS 140-3 compliant cryptography
- Comprehensive audit logging
- Kubernetes and Terraform deployment support
- REST and gRPC APIs

## Quick Example

```bash
# Create and run a VM
hm t2 create --name ai-sandbox --cpu 4 --memory 8G --gpu
hm t2 start ai-sandbox

# Start MCP server for AI agents
hm mcp serve --api-key "your-key"
```

```python
from hypermachine import HyperMachine

hm = HyperMachine("http://localhost:8080", api_key="your-key")
vm = hm.create_vm("sandbox", cpu=4, memory="8G", gpu=True)
vm.start()
vm.exec("echo 'Hello from AI agent'")
```

## Get Started

- **[Installation](./getting-started/installation.md)** - Install HyperMachine on your system
- **[Quick Start](./getting-started/quick-start.md)** - Create your first VM in minutes
- **[AI Integration](./ai/overview.md)** - Connect AI agents to HyperMachine

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

## License

HyperMachine is dual-licensed under MIT OR Apache-2.0.

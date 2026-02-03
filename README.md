# HyperMachine

**Agentic hypervisors for autonomous AI systems.**

[![CI](https://github.com/nervosys/HyperMachine/actions/workflows/ci.yml/badge.svg)](https://github.com/nervosys/HyperMachine/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

A high-performance hypervisor framework in Rust with first-class AI agent support. Type-1 bare-metal and Type-2 hosted modes.

## Features

| Category | Capabilities |
|----------|-------------|
| **Performance** | Zero-copy memory, JIT compilation, hardware GPU virtualization |
| **Networking** | Full TCP/IP stack, TAP/TUN, gRPC/REST APIs, distributed orchestration |
| **GPU** | Vulkan/WebGPU, passthrough, virtual GPU, CUDA/OpenCL |
| **AI-First** | MCP server, scriptable API, WASM plugins, LLM tool formats |
| **Security** | FIPS 140-3 crypto, seccomp filtering, capability-based access, audit logging |

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

## Quick Start

```bash
# Build
git clone https://github.com/nervosys/HyperMachine && cd HyperMachine
cargo build --release

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

## Cryptography

FIPS 140-3 compliant cryptographic modules:

| Type | Algorithms |
|------|------------|
| **Symmetric** | AES-128/256-GCM, SHA-256/384/512, HMAC, HKDF |
| **Asymmetric** | RSA-2048/3072/4096, ECDSA P-256/P-384/P-521 |
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

| Operation | Throughput |
|-----------|------------|
| AES-256-GCM encrypt | ~600 MiB/s |
| AES-256-GCM decrypt | ~700 MiB/s |
| SHA-256 | ~3.7 GiB/s |
| SHA-512 | ~3.5 GiB/s |

```bash
cargo bench -p hv2-core --bench crypto_bench
```

## Project Structure

```
crates/
  hm-cli      # CLI + MCP server
  hv1-boot    # Type-1 bare-metal bootloader
  hv2-core    # Core engine (CPU, memory, devices, crypto)
  hv2-api     # REST/gRPC APIs
  hv2-agent   # AI agent interface
deploy/
  k8s/        # Kubernetes manifests
  helm/       # Helm charts
  terraform/  # Infrastructure as code
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT OR Apache-2.0

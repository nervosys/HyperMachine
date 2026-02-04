# Architecture Overview

HyperMachine is a modular hypervisor framework supporting both Type-1 (bare-metal) and Type-2 (hosted) virtualization.

## System Architecture

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
|                      Hardware                            |
|          CPU (VT-x/AMD-V) - GPU - NIC - Storage         |
+---------------------------------------------------------+
```

## Components

### HyperMachine Core (`hv2-core`)

The core virtualization engine providing:

- **CPU Virtualization** - Hardware-assisted (KVM, WHPX, HVF) or emulated
- **Memory Management** - EPT/NPT, memory ballooning, zero-copy transfers
- **Device Emulation** - virtio devices, ACPI, PCI passthrough
- **Interrupt Handling** - APIC emulation, MSI/MSI-X support

### Type-1 Hypervisor (`hv1-boot`)

Bare-metal hypervisor booting directly on hardware:

- Custom bootloader (UEFI/BIOS)
- Direct VMX/SVM control
- Minimal attack surface
- Suitable for high-security deployments

### Type-2 Hypervisor (`hv2-core`)

Hosted hypervisor running on existing OS:

- Uses host OS hypervisor APIs (KVM, WHPX, HVF)
- Easy installation and integration
- Suitable for development and general use

### API Layer (`hv2-api`)

Multiple API interfaces for different use cases:

- **REST API** - HTTP/JSON for web integration
- **gRPC API** - High-performance binary protocol
- **WebSocket** - Real-time streaming and console access

### AI Agent Interface (`hv2-agent`)

Specialized layer for AI agent integration:

- **MCP Server** - Model Context Protocol implementation
- **Tool Definitions** - Native formats for OpenAI, Anthropic, Google
- **Semantic Operations** - High-level VM operations as tools

### CLI & GUI (`hm-cli`, `hm-gui`)

User interfaces:

- **CLI** - Command-line interface for scripting and automation
- **GUI** - Desktop application with AI-driven automation API

## Data Flow

### VM Creation Flow

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  AI Agent    │────▶│  MCP Server  │────▶│   VM Core    │
│  (Claude)    │     │  (Tool Call) │     │  (Create VM) │
└──────────────┘     └──────────────┘     └──────────────┘
                                                  │
                     ┌──────────────┐     ┌──────────────┐
                     │   Response   │◀────│  Hypervisor  │
                     │   (VM ID)    │     │  (KVM/WHPX)  │
                     └──────────────┘     └──────────────┘
```

### Memory Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Guest Physical Memory                 │
├─────────────────────────────────────────────────────────┤
│                    EPT/NPT Translation                   │
├─────────────────────────────────────────────────────────┤
│                    Host Physical Memory                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐│
│  │  VM 1    │  │  VM 2    │  │  Shared  │  │  Device  ││
│  │  Memory  │  │  Memory  │  │  Pages   │  │  Memory  ││
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘│
└─────────────────────────────────────────────────────────┘
```

## Crate Structure

```
crates/
├── hm-cli          # CLI application + MCP server
├── hm-gui          # Desktop GUI with automation API
├── hv1-boot        # Type-1 bare-metal bootloader
├── hv2-core        # Core virtualization engine
│   ├── cpu/        # CPU emulation and VMX/SVM
│   ├── memory/     # Memory management
│   ├── device/     # Device emulation
│   ├── crypto/     # FIPS 140-3 cryptography
│   └── network/    # Network stack
├── hv2-api         # REST/gRPC API layer
└── hv2-agent       # AI agent interface
```

## Platform Support

| Platform | Hypervisor Backend | Status |
|----------|-------------------|--------|
| Linux | KVM | ✅ Full support |
| Windows | WHPX | ✅ Full support |
| macOS | HVF | ✅ Full support |
| Bare-metal | VMX/SVM | 🚧 In development |

## Next Steps

- [Type-1 Hypervisor](./type-1.md) - Bare-metal architecture details
- [Type-2 Hypervisor](./type-2.md) - Hosted mode architecture
- [Memory Management](./memory.md) - Memory subsystem details
- [GPU Virtualization](./gpu.md) - GPU passthrough and virtual GPU

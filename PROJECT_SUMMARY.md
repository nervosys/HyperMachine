# HV2 Project Summary

## What is HV2?

HV2 is a **fast, remotely scriptable, network- and GPU-enabled Type 2 hypervisor and emulator framework written in Rust with first-class support for agentic AI**. It's designed from the ground up to be controlled by AI agents while maintaining security, performance, and observability.

> **Type 2 Hypervisor**: HV2 is a hosted hypervisor that runs on top of an existing operating system (Windows, Linux, macOS). A future Type 1 (bare-metal) variant called HV1 is planned for deployment directly on hardware.

## Key Features

### 🚀 High Performance
- **Zero-copy memory management** using mmap for guest memory
- **Async-first architecture** built on Tokio
- **Hardware acceleration** via GPU passthrough and virtualization
- **Optimized for modern multi-core CPUs**

### 🤖 AI-First Design
- **Safe scripting interface** using Rhai scripting language
- **Sandboxed execution** with resource limits and capability-based security
- **Built-in observability** with OpenTelemetry integration
- **WASM plugin system** for extensible agent behaviors
- **Timeout protection** and resource quotas prevent runaway scripts

### 🌐 Network-Enabled
- **Full network virtualization** with TAP/TUN support
- **VirtIO network devices** for high-performance networking
- **Remote control APIs** via gRPC and REST
- **WebSocket streaming** for real-time events

### 🎮 GPU Acceleration
- **Virtual GPU** using WebGPU/Vulkan
- **GPU passthrough** via VFIO for bare-metal performance
- **CUDA/OpenCL support** for compute workloads
- **Graphics rendering** for cloud gaming and visualization

### 🔒 Security by Default
- **Capability-based access control** - agents only get explicit permissions
- **Seccomp syscall filtering** to limit attack surface
- **Memory isolation** between host and guest
- **Audit logging** for all AI operations

## Project Structure

```
HV2/
├── crates/
│   ├── hv2-core/           # Core VM engine and abstractions
│   ├── hv2-cpu/            # CPU emulation (x86-64, AArch64)
│   ├── hv2-gpu/            # GPU virtualization
│   ├── hv2-net/            # Network stack
│   ├── hv2-agent/          # AI agent scripting interface
│   ├── hv2-api/            # Remote APIs (gRPC, REST)
│   └── hv2-cli/            # Command-line interface
├── docs/
│   ├── architecture.md     # System architecture
│   ├── agent-api.md        # AI agent API reference
│   └── gpu.md              # GPU virtualization guide
├── examples/
│   ├── basic.rs            # Basic VM management
│   └── agent_script.rs     # AI agent scripting
├── Cargo.toml              # Workspace configuration
├── README.md               # Project overview
├── GETTING_STARTED.md      # Quick start guide
└── CONTRIBUTING.md         # Contribution guidelines
```

## Technology Stack

- **Language**: Rust 2021 Edition
- **Async Runtime**: Tokio
- **Scripting**: Rhai (embedded scripting language)
- **GPU**: WebGPU (wgpu), Vulkan (ash)
- **Networking**: TAP/TUN, VirtIO
- **APIs**: gRPC (tonic), REST (axum)
- **Observability**: OpenTelemetry, tracing, Prometheus
- **Security**: seccompiler, capability-based access

## Use Cases

### 1. AI Research & Development
- Safe sandboxes for autonomous agent experimentation
- Reproducible environments for AI training
- Isolated testing of agent behaviors

### 2. Cloud Gaming & Streaming
- GPU-enabled game streaming infrastructure
- Low-latency remote gaming
- Multi-user cloud gaming platforms

### 3. DevOps & CI/CD
- Ephemeral build environments
- Automated infrastructure testing
- Self-healing systems managed by AI

### 4. Security Research
- Malware analysis in isolated environments
- Vulnerability testing and fuzzing
- Security tool development

### 5. Edge Computing
- Lightweight VMs for distributed workloads
- IoT device emulation
- Edge AI processing

## Architecture Highlights

### Memory Management
- **Zero-copy design** - Guest memory mapped directly into host address space
- **Huge page support** for reduced TLB misses
- **Memory isolation** between VMs

### CPU Emulation
- **x86-64 support** with full instruction set
- **AArch64 (ARM64)** support
- **Future**: JIT compilation for hot paths

### Device Management
- **Pluggable device architecture** - Easy to add new devices
- **Async device I/O** - Non-blocking operations
- **VirtIO support** - High-performance para-virtualized devices

### AI Agent Interface
```
Agent Script → Syntax Validation → Capability Check → 
Sandbox Entry → Execute with Timeout → Collect Metrics → 
Return Results
```

## Quick Example

```rust
use hv2_agent::AgentVM;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create VM with AI capabilities
    let vm = AgentVM::builder()
        .name("my-vm")
        .cpu_cores(4)
        .memory_gb(8)
        .enable_gpu(true)
        .with_tracing()
        .build()
        .await?;
    
    // Start the VM
    vm.start().await?;
    
    // AI agent controls the VM
    vm.execute_agent_script(r#"
        if vm_state == "Running" {
            let cpu = get_cpu_usage();
            if cpu > 0.8 {
                scale_vcpu(vcpu_count + 2);
            }
        }
    "#).await?;
    
    Ok(())
}
```

## Development Status

HV2 is currently in **early development**. The foundational architecture is in place:

✅ **Completed**:
- Core VM engine and abstractions
- Memory management infrastructure
- vCPU state management
- Device management framework
- AI agent scripting interface
- Capability-based security model
- Sandboxed script execution
- gRPC and REST APIs
- CLI tool
- Comprehensive documentation

🚧 **In Progress**:
- Full CPU instruction emulation
- GPU virtualization implementation
- Network stack completion
- Hardware acceleration integration

📋 **Planned**:
- JIT compilation for performance
- Snapshot/restore functionality
- Live migration support
- Natural language VM control
- Kubernetes operator
- Cloud provider integrations

## Performance Targets

- **VM Boot Time**: < 100ms
- **Memory Overhead**: < 50MB per VM
- **Network Throughput**: 10Gbps+ with SR-IOV
- **GPU Performance**: 95%+ of native with passthrough
- **Script Execution**: < 1ms overhead

## Contributing

We welcome contributions! Areas where help is needed:

- CPU instruction emulation
- GPU device implementation
- Network stack optimization
- Documentation and examples
- Testing and benchmarks
- Security audits

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

AetherVM is dual-licensed under MIT and Apache-2.0. You can choose either license.

## Future Vision

HV2 aims to become the **de facto standard for AI-controlled virtualization**:

### Type 2 (Current - HV2)
- Runs on existing OS (Windows, Linux, macOS)
- Easy development and debugging
- Good for development, testing, and edge deployments

### Type 1 (Planned - HV1)
- Runs directly on hardware (bare-metal)
- Maximum performance and isolation
- Ideal for production cloud infrastructure
- Direct hardware access and control

### Unified Vision

- Natural language VM management
- Self-optimizing resource allocation
- Predictive failure prevention
- Automated security hardening
- Multi-cloud orchestration
- Edge-to-cloud continuum

## Getting Started

```bash
# Clone the repository
git clone https://github.com/nervosys/hv2
cd hv2

# Build the project
cargo build --release

# Run an example
cargo run --example basic

# Install the CLI
cargo install --path crates/hv2-cli

# Create your first VM
hv2 create --name my-vm --cpu 2 --memory 4
```

For detailed instructions, see [GETTING_STARTED.md](GETTING_STARTED.md).

## Contact & Community

- **GitHub**: https://github.com/nervosys/hv2
- **Issues**: Report bugs and request features
- **Discussions**: Ask questions and share ideas

---

**HV2** - Type 2 Hypervisor empowering AI agents to manage virtualization infrastructure safely, efficiently, and intelligently. 🚀🤖

*Coming Soon: **HV1** - Type 1 bare-metal hypervisor for maximum performance*

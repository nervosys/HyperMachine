# HyperMachine

A high-performance, remotely scriptable hypervisor and emulator framework written in Rust with first-class support for agentic AI. Supports Type-1 (hv1) and Type-2 (hv2) operational modes.

> **Note**: HV2 is a Type 2 (hosted) hypervisor. A Type 1 (bare-metal) hypervisor variant (HV1) is planned for future development.

## Features

🚀 **High Performance**
- Zero-copy memory management
- JIT compilation support
- Hardware-accelerated GPU virtualization
- Multi-threaded execution

🌐 **Network-Enabled**
- Full TCP/IP stack virtualization
- TAP/TUN device support
- Remote control via gRPC/REST APIs
- Distributed VM orchestration

🎮 **GPU Acceleration**
- Vulkan/WebGPU support
- GPU passthrough capabilities
- Virtual GPU device emulation
- CUDA/OpenCL workload support

🤖 **AI-First Design**
- Scriptable API for autonomous agents
- Safe sandboxed execution
- Built-in telemetry and observability
- Natural language VM control interface
- WASM plugin system for agent extensions

🔒 **Security**
- Seccomp-based syscall filtering
- Capability-based access control
- Memory isolation and protection
- Audit logging for AI operations

## Architecture

HyperMachine supports two operational modes:

### Type 2 Mode (HV2) - Hosted Hypervisor
Runs on top of an existing operating system (Windows, Linux, macOS).
```
┌─────────────────────────────────────────────────────────┐
│                    AI Agent Interface                    │
│              (Scriptable, Safe, Observable)              │
├─────────────────────────────────────────────────────────┤
│                     Remote API Layer                     │
│                   (gRPC, REST, WebSocket)                │
├──────────────┬──────────────┬──────────────┬────────────┤
│   CPU Core   │  GPU Module  │   Network    │   Memory   │
│  Emulation   │ (Vulkan/GPU) │  (TAP/TUN)   │ Management │
├──────────────┴──────────────┴──────────────┴────────────┤
│                      HV2 Core Engine                     │
│            (Type 2 Hypervisor - KVM/WHPX/HVF)            │
├─────────────────────────────────────────────────────────┤
│                    Host OS (Linux/Windows/macOS)         │
└─────────────────────────────────────────────────────────┘
```

### Type 1 Mode (HV1) - Bare-Metal Hypervisor *(Planned)*
Runs directly on hardware without a host OS for maximum performance.
```
┌─────────────────────────────────────────────────────────┐
│                    AI Agent Interface                    │
├─────────────────────────────────────────────────────────┤
│                     Remote API Layer                     │
├──────────────┬──────────────┬──────────────┬────────────┤
│   CPU Core   │  GPU Module  │   Network    │   Memory   │
├──────────────┴──────────────┴──────────────┴────────────┤
│                      HV1 Core Engine                     │
│              (Type 1 Hypervisor - VMX/SVM)               │
├─────────────────────────────────────────────────────────┤
│                    Hardware (x86-64/ARM64)               │
└─────────────────────────────────────────────────────────┘
```

## Quick Start

### Installation

```bash
git clone https://github.com/nervosys/HyperMachine
cd HyperMachine
cargo build --release --package hm-cli
cargo install --path crates/hm-cli
# Or: cp target/release/hm ~/.cargo/bin/
```

### Create and Run a VM

```bash
# Type 2 (hosted) hypervisor - runs on your OS
hm t2 create --name myvm --cpu 4 --memory 8 --gpu --network
hm t2 start myvm
hm t2 status myvm
hm t2 script myvm --script "vm_state"

# Type 1 (bare-metal) hypervisor - planned
hm t1 create --name prod-vm --cpu 16 --memory 64
```

### AI Agent Integration

```rust
use hv2_agent::AgentVM;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create VM with AI agent capabilities
    let vm = AgentVM::builder()
        .cpu_cores(4)
        .memory_gb(8)
        .enable_gpu(true)
        .enable_networking(true)
        .with_tracing()
        .build()
        .await?;
    
    // AI agents can script VM operations
    vm.execute_agent_script(r#"
        // Boot the VM
        vm.boot();
        
        // Monitor CPU usage
        let cpu = vm.cpu_usage();
        if cpu > 0.8 {
            vm.scale_cpu(8);
        }
        
        // Execute commands
        vm.exec("apt-get update");
    "#).await?;
    
    Ok(())
}
```

## Project Structure

- `crates/hm-cli` - **Unified CLI** (`hm t1/t2` commands)
- `crates/hv2-core` - Core VM engine and architecture
- `crates/hv2-cpu` - CPU emulation and execution
- `crates/hv2-gpu` - GPU virtualization layer
- `crates/hv2-net` - Network stack and devices
- `crates/hv2-agent` - AI agent interface and scripting
- `crates/hv2-api` - Remote control APIs

## Use Cases

- **AI Research**: Safe sandboxes for autonomous agent experimentation
- **Cloud Gaming**: GPU-enabled game streaming infrastructure
- **CI/CD**: Ephemeral build environments with full observability
- **Security Research**: Malware analysis in isolated environments
- **Edge Computing**: Lightweight VMs for distributed workloads

## Building from Source

```bash
git clone https://github.com/nervosys/HyperMachine
cd HyperMachine
cargo build --release
```

## Documentation

- [Architecture Guide](docs/architecture.md)
- [AI Agent API](docs/agent-api.md)
- [GPU Virtualization](docs/gpu.md)
- [Network Configuration](docs/networking.md)
- [Security Model](docs/security.md)

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

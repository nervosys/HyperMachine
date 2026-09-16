# AetherVM Architecture

## Overview

AetherVM is designed as a high-performance, modular virtual machine framework with first-class support for AI agents. The architecture prioritizes:

- **Performance**: Zero-copy memory, JIT compilation, hardware acceleration
- **Scriptability**: Safe, sandboxed scripting APIs for autonomous agents
- **Observability**: Built-in tracing, metrics, and debugging
- **Modularity**: Clean separation of concerns across crates

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                       AI Agent Layer                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ Script Engine│  │   Sandbox    │  │ Capabilities │      │
│  │    (Rhai)    │  │  (seccomp)   │  │   System     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Remote API Layer                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │     gRPC     │  │  REST API    │  │  WebSocket   │      │
│  │   (Tonic)    │  │   (Axum)     │  │              │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Core VM Engine                           │
│  ┌──────────────┬──────────────┬──────────────┬──────────┐  │
│  │     CPU      │     GPU      │   Network    │  Memory  │  │
│  │  Emulation   │ Acceleration │  (TAP/TUN)   │ Manager  │  │
│  └──────────────┴──────────────┴──────────────┴──────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Device Manager                           │  │
│  │  • Serial • Keyboard • Mouse • Disk • Timer • RTC    │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  Hardware Abstraction                        │
│  • KVM • WHPX (Windows) • HVF (macOS)                       │
└─────────────────────────────────────────────────────────────┘
```

## Crate Structure

### `aethervm-core`
Core VM abstractions and engine:
- Guest memory management (zero-copy with mmap)
- Virtual CPU state and lifecycle
- Device management infrastructure
- VM lifecycle management

### `aethervm-cpu`
CPU emulation and execution:
- x86-64 instruction emulation
- AArch64 (ARM64) support
- RISC-V support (planned)
- JIT compilation (planned)

### `aethervm-gpu`
GPU virtualization:
- Virtual GPU via WebGPU/Vulkan
- GPU passthrough (VFIO)
- CUDA/OpenCL workload support
- Graphics rendering pipeline

### `aethervm-net`
Network virtualization:
- TAP/TUN device support
- VirtIO network device
- Full TCP/IP stack
- Network isolation and bridging

### `aethervm-agent`
AI agent interface:
- Safe script execution (Rhai)
- WASM plugin system
- Capability-based security
- Resource limits and sandboxing
- Timeout protection

### `aethervm-api`
Remote control APIs:
- gRPC service (high-performance)
- REST API (HTTP/JSON)
- WebSocket streaming
- Event notifications

### `aethervm-cli`
Command-line interface:
- VM management commands
- Interactive script execution
- API server hosting
- Configuration management

## Key Design Principles

### 1. Zero-Copy Memory
- Memory-mapped guest memory
- Direct access without copying
- Huge page support for performance

### 2. Async-First
- Built on Tokio async runtime
- Non-blocking I/O throughout
- Concurrent VM operation

### 3. Type Safety
- Strong Rust type system
- Compile-time correctness
- Minimal runtime overhead

### 4. Security by Default
- Sandboxed script execution
- Capability-based permissions
- Seccomp syscall filtering
- Memory isolation

### 5. Observability
- OpenTelemetry integration
- Structured logging with tracing
- Prometheus metrics
- Distributed tracing support

## AI Agent Integration

### Script Execution Flow

```
1. Agent submits script
   ↓
2. Validate syntax & capabilities
   ↓
3. Enter sandbox (resource limits)
   ↓
4. Execute with timeout
   ↓
5. Collect metrics & logs
   ↓
6. Return results
```

### Security Model

- **Capability-based**: Agents only get explicit permissions
- **Sandboxed**: Scripts run in isolated environment
- **Resource limited**: CPU, memory, and time limits enforced
- **Audited**: All operations logged for review

### Example Use Cases

1. **Autonomous DevOps**: Agents manage VM lifecycles based on metrics
2. **Security Testing**: AI explores vulnerabilities in isolated VMs
3. **Cloud Orchestration**: Self-healing infrastructure management
4. **Research Platforms**: Safe environments for agent experimentation

## Performance Optimizations

- **Memory**: mmap-backed zero-copy guest memory
- **CPU**: Hot path optimization, SIMD instructions (AVX2/AVX-512 dispatch in
  `hv2-infer`)
- **I/O**: synchronous `std::fs` (`devices/disk_image.rs`). This line claimed
  io_uring; `io_uring` appears nowhere in the workspace, and neither does
  `tokio::fs` — there is no asynchronous file I/O of any kind.
- **Network**: Kernel bypass with AF_XDP (planned; no implementation)
- **GPU**: Direct GPU memory access, shader caching (`hv2-gpu`'s vGPU caches
  render pipelines by shader id)

## Future Roadmap

- [x] Snapshot/restore functionality — `hv2-core`'s `SnapshotManager`, exposed
      over REST by `hv2-api`'s `snapshot_routes`
- [x] Multi-VM orchestration — `hv2-runtime::fleet` and
      `hv2-agent::orchestration`. In-process: it tracks per-host state rather
      than dispatching across a network, as the README's note says.
- [ ] JIT compilation for hot code paths — nothing implements a CPU JIT, and
      the README records that the claim was removed rather than deferred
- [ ] Live migration support — `hv2-core::migration` exists and is tested in
      isolation, but nothing outside that module calls it, so no VM migrates
- [x] Suspend and resume a running guest — `VM::pause` kicks every vCPU out
      of `KVM_RUN` and parks it; verified against a guest that cannot answer
      while suspended and does once resumed
- [ ] Natural language VM control
- [ ] Integration with LangChain/AutoGPT
- [ ] Cloud-native deployment (K8s operator)

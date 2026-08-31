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
| **Performance** | Zero-copy guest memory access, GPU passthrough (VFIO, Linux)                 |
| **Networking**  | TAP/TUN, gRPC/REST APIs, fleet & multi-agent orchestration                   |
| **GPU**         | Vulkan/WebGPU (via `wgpu`), passthrough, virtual GPU, GPU-class fleet metadata (labels like `cuda-12.4`, not a CUDA/OpenCL runtime) |
| **AI-First**    | MCP server, Rhai scriptable API, LLM tool formats (OpenAI/Anthropic/Gemini)  |
| **GUI**         | Desktop app, AI-driven automation, semantic control API                      |
| **Security**    | FIPS-approved + post-quantum crypto, capability-based access, audit logging |
| **Governance**  | Optional policy evaluation per agent tool call, digest-keyed image admission |

A few of these need more context than a table cell gives:
- **GPU passthrough** is real VFIO/IOMMU code (`crates/hv2-gpu/src/passthrough.rs`) — it opens `/dev/vfio`, binds the device, and maps BARs via real ioctls — but it is Linux-only and has never run against physical GPU/IOMMU hardware in CI (no such hardware exists there), so the ioctl path is exercised by unit tests on parsed structs, not by an end-to-end attach.
- **Virtual GPU** (`crates/hv2-gpu/src/vgpu.rs`) is a software GPU built on `wgpu`, not a hardware-virtualized GPU (SR-IOV/mediated device). There is no CUDA or OpenCL runtime anywhere in the codebase; the `cuda-12.4`-style strings that appear are fleet scheduling labels, not an implemented API.
- **Fleet & multi-agent orchestration** (`hv2-agent::orchestration`, `hv2-runtime::fleet`) coordinates VM claims, locks, and per-host rollout state in-process; it does not itself make network calls to remote hosts, so "distributed" only holds in the sense that the data model tracks per-host state, not that a control plane dispatches work across a network.
- **WASM plugins were removed from this table**: `wasmtime` is an optional dependency behind hv2-agent's `wasm-scripts` feature, which is not in any default feature set, and no source file in the workspace references `wasmtime` — there is no WASM plugin support to advertise today.
- There is no full guest-facing TCP/IP stack (e.g. a user-mode network stack like slirp/smoltcp); it was removed from this table for the same reason. Guest networking is TAP/TUN bridging to the host's own stack.
- **JIT compilation was removed**: nothing in the workspace implements a CPU JIT/dynamic-recompilation path.

## Benchmarks

Measured with [Criterion](https://github.com/bheisler/criterion.rs) on an
**AMD Ryzen 9 9900X** (reproduce with `cargo bench`):

| Benchmark | Median | What it measures |
| --- | ---: | --- |
| **Agent spawn — CoW clone** | **~9 ns (O(1))** | fork a sandbox from a warm baseline; **constant** at 1 / 16 / 64 baseline units (8.9 / 9.3 / 9.1 ns). A *full copy* is 206 µs → 9.3 ms and grows with size — so 64 agents cost ~one baseline, not 64. |
| CoW first-write fault | 73 ns | the per-page copy paid once, when a clone first dirties a page |
| Guest memory read / write | 27–96 ns / 9–19 ns | 64 B – 4 KiB accesses (zero-copy mapping) |
| MCP tool dispatch | 547 ns | one agent tool call (`vm.list`) end-to-end |
| MCP dispatch ×64 (concurrent) | 47 µs (~0.7 µs/call) | concurrent agent tool calls over the dispatch path |
| Snapshot — vCPU regs / 10 devices | 18 ns / 21 ns | serialize control registers + device state |
| Tool-schema projection (OpenAI) | 3.3 µs | render the MCP registry to an LLM tool format |
| AES-256-GCM (`ring`, AES-NI) | ~9–10 GiB/s | crypto throughput — see [Performance](#performance) |

The defining number is **O(1) agent spawn**: a copy-on-write clone is ~9 ns
regardless of fleet size, so 100 idle agents cost roughly one baseline's memory
rather than 100 — the foundation of the agent runtime's fleet density.

### 1. Agentic-First Virtualization

HyperMachine is the first hypervisor designed from the ground up for AI agent workloads. Every VM is an MCP-addressable resource: agents discover capabilities via ontology endpoints, invoke typed tools (`vm.create`, `vm.exec`, `gpu.reserve`), and receive structured results — no shell scraping or brittle CLI wrappers. Multi-LLM tool schemas ship built-in for OpenAI, Anthropic, and Google formats.

A built-in **agent runtime** turns this into a fleet service: agents spawn in O(1) as copy-on-write clones of a warm baseline (100 idle agents cost ~one baseline, not 100), run tool-calling loops over a fast MCP dispatch path, and have their sessions and memory reclaimed automatically. It is exposed over a tenant-scoped, optionally-authenticated REST API (`/api/v1/agents`).

### 2. Dual-Mode Architecture (Type-1 + Type-2)

A single codebase runs as both a **Type-2 hosted hypervisor** (KVM, WHPX, HVF) and a **Type-1 bare-metal hypervisor** (Intel VMX, AMD SVM) with no code duplication. The same VM definitions, device models, and API surface work in both modes — develop on your laptop, deploy bare-metal in production.

### 3. GPU Fabric with Topology-Aware Placement

HyperMachine models GPU interconnect topology (NVLink, NVSwitch, PCIe) and makes placement decisions based on real bandwidth and latency. Capacity reservations with SLA tiers (platinum/gold/silver/bronze) prevent noisy-neighbor GPU contention. Fleet-wide GPU health monitoring tracks utilization, temperature, and ECC errors across hosts.

### 4. Post-Quantum Cryptography

Alongside classical FIPS-approved algorithms (AES-GCM, RSA, ECDSA), HyperMachine ships ML-KEM (Kyber) for key encapsulation, ML-DSA (Dilithium) for digital signatures, and SLH-DSA (SPHINCS+) for hash-based signatures — all NIST-standardized, quantum-resistant, and backed by the audited pure-Rust [RustCrypto](https://github.com/RustCrypto) implementations (not placeholders).

### 5. Pure Rust, Zero Unsafe in Business Logic

~240,000 lines of Rust across 13 crates, in safe Rust, and `cargo clippy -D warnings` clean, on Linux, macOS and Windows. The workspace test suite passes across all three in CI (see the `CI` workflow); it is split across dozens of per-crate and per-platform jobs with overlapping binaries, so a single aggregate test count is not reliably computable from CI logs and is not quoted here — treat any specific number you see elsewhere in this repo's local status notes as a stale, moment-in-time snapshot. That does not mean every module does what its name suggests: `hv2-core`'s `container` module models the OCI runtime spec in types but runs nothing — `ContainerRuntime::start` refuses rather than fabricate a process — and OS-level confinement that is actually enforced lives in `hv2-sandbox` instead (see [Agent Governance](#agent-governance) and `docs/SANDBOXES.md`).

Every advisory in the dependency graph is triaged in [`deny.toml`](deny.toml), and `cargo deny check` passes on advisories, bans, licenses and sources. Five are accepted with written justification because no upgrade path exists: the `rsa` crate's Marvin-attack timing advisory (no fixed release upstream), two `quick-xml` parser advisories reachable only through the Linux GUI accessibility stack, and two unmaintained-crate notices (`smartstring` via the scripting engine, `ttf-parser` via the desktop GUI's font layer). `wasmtime` is deliberately optional and off by default, which keeps its advisories out of normal builds.

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

# Create and boot a VM (Type-2 hosted mode)
hm t2 create --name myvm --cpu 4 --memory 8 \
    --kernel /boot/vmlinuz --initrd /boot/initrd.img \
    --cmdline "console=ttyS0 root=/dev/vda"
hm t2 start myvm

# Or boot a raw image (boot sector, unikernel) at 0x7C00
hm t2 create --name unikernel --cpu 1 --memory 1 --image ./kernel.bin

# Without --kernel or --image a VM is created with vCPUs and empty guest
# memory: it can be started, but there is no guest code to execute.
hm t2 create --name empty --cpu 2 --memory 4

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

### Agent runtime API

Spawn and operate a fleet of copy-on-write agents over HTTP. Each agent gets an
MCP session plus an O(1) sandbox cloned from a warm baseline. Requests are
tenant-scoped via `X-Tenant-Id` (an agent is owned by the tenant that spawned
it), with an optional `Authorization: Bearer <token>`:

```bash
# Spawn a tenant-scoped agent (O(1) CoW sandbox from the warm baseline)
curl -X POST http://localhost:8080/api/v1/agents \
  -H "X-Tenant-Id: acme" -H "Content-Type: application/json" \
  -d '{"agent_id": "researcher", "capabilities": "operator"}'

curl http://localhost:8080/api/v1/agents        -H "X-Tenant-Id: acme"  # list
curl http://localhost:8080/api/v1/agents/fleet  -H "X-Tenant-Id: acme"  # fleet memory
curl -X POST http://localhost:8080/api/v1/agents/reap -d '{"max_idle_secs": 600}'
curl -X DELETE http://localhost:8080/api/v1/agents/<session_id> -H "X-Tenant-Id: acme"
```

**Python SDK** (planned — not yet shipped in this repository):

```python
from hypermachine import HyperMachine

hm = HyperMachine("http://localhost:8080", api_key="your-key")
vm = hm.create_vm("sandbox", cpu=4, memory="8G", gpu=True)
vm.start()
vm.exec("echo 'Hello from AI agent'")
```

### Runnable examples

These compile and run today (`cargo run -p <crate> --example <name>`):

| Example | Crate | Shows |
| ------- | ----- | ----- |
| `agent_mcp_workflow` | `hv2-agent` | An agent driving a full VM lifecycle over the MCP tool surface (provision → boot → `guest.exec` → snapshot → resize → restore → teardown) with an audit log |
| `llm_tool_schemas`   | `hv2-agent` | The MCP tool registry projected into OpenAI / Anthropic / Gemini tool-use formats |
| `agent_vm_workflow`  | `hm-cli`    | Tool discovery + VM lifecycle through the typed `ToolExecutor` and agentic ontology |
| `multi_agent_orchestration` | `hv2-agent` | Multiple role-scoped agents coordinating: exclusive VM claims, role enforcement, inter-agent messaging |
| `agent_runtime` | `hv2-agent` | End-to-end agent runtime: 100 agents spawned from one warm baseline (O(1), ~100× memory density), kept isolated, calling tools, then reclaimed |
| `gpu_fabric_reservation` | `hv2-runtime` | Publishing a GPU VM class and reserving capacity with SLA tiers via `CapacityManager` |
| `agent_script` / `integrated` | `hv2-agent` | Rhai-scripted agent decision-making and agent↔device (serial/MMIO) interaction |

```bash
cargo run -p hv2-agent --example agent_mcp_workflow
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

Implementations of FIPS-approved classical algorithms plus the NIST
post-quantum schemes. Classical AES-GCM/SHA come from the [`ring`](https://github.com/briansmith/ring)
backend, RSA from the pure-Rust [`rsa`](https://github.com/RustCrypto/RSA) crate,
and the post-quantum schemes from [RustCrypto](https://github.com/RustCrypto)
(`ml-kem`, `ml-dsa`, `slh-dsa`). These are validated _algorithm_ implementations,
not a FIPS 140-3 _validated module_.

| Type             | Algorithms                                             |
| ---------------- | ------------------------------------------------------ |
| **Symmetric**    | AES-256-GCM, SHA-256/384/512, HMAC, HKDF               |
| **Asymmetric**   | RSA-2048/3072/4096, ECDSA P-256/P-384[^p521]           |
| **Post-Quantum** | ML-KEM (Kyber), ML-DSA (Dilithium), SLH-DSA (SPHINCS+) |

[^p521]: ECDSA P-521 keys can be represented but key generation and signing
    require a backend other than `ring`, which does not support that curve.

## Agent Governance

An agent driving VMs is gated by two things out of the box: the capabilities on
its session, and VM ownership — a session cannot touch a VM it did not create,
and a probe for someone else’s VM is indistinguishable from one that does not
exist. Two further controls are available and off by default.

**Policy evaluation.** `McpServer::set_policy_set` installs a `PolicySet` that is
evaluated before every tool call. Capabilities say what kind of tool an agent may
use; a policy says whether *this* action, on *this* resource, is allowed right
now — denying deletion of one named VM, or destructive actions outside a
maintenance window. A denial is refused and written to the audit log, because an
unrecorded denial is the one an incident review needs. Note that `PolicySet`
denies by default: an installed set must name everything agents may do,
including tools added after it was written.

**Image admission.** `VM::set_image_registry` makes `VM::provision` refuse a boot
image the allowlist does not admit, keyed on the SHA-256 of the bytes about to be
loaded — so renaming or moving a kernel cannot change the verdict. The check runs
before the backend is asked for a partition, so a refusal costs no hypervisor
resources, and a digest that cannot be computed is a denial rather than a pass.
The API server shares one registry between `/api/v1/images` and the VMs it
creates when `enforce_image_admission` is set; that flag is off by default
because the registry enforces by default, so enabling it against an empty
catalogue refuses every boot image until images are registered and approved.

What is *not* enforced is worth stating plainly. `hv2-agent`’s `limits` module is
a toolkit that takes effect only where a caller consults it, and the MCP tool
path does not — per-session rate limiting comes from `McpConfig::rate_limit`
instead. `Sandbox` is a policy object, not OS-level containment: what keeps an
agent script off the network and filesystem is that the Rhai engine registers no
I/O at all. And `execute_script` evaluates on the host against a read-only view
of a VM; it does not run anything inside the guest.

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

Crypto throughput from `crypto_bench` on an AMD Ryzen 9 9900X (64 KiB blocks).
AES-GCM and SHA run on the validated `ring` backend (AES-NI hardware
acceleration), which is enabled by default:

| Operation           | Throughput  |
| ------------------- | ----------- |
| AES-256-GCM encrypt | ~9.0 GiB/s  |
| AES-256-GCM decrypt | ~10.1 GiB/s |
| SHA-256             | ~2.5 GiB/s  |
| SHA-512             | ~0.76 GiB/s |
| HMAC-SHA256         | ~2.3 GiB/s  |

```bash
cargo bench -p hv2-core --bench crypto_bench
```

> Numbers are hardware- and backend-dependent. AES-GCM is AES-NI accelerated;
> SHA throughput reflects `ring`'s software implementation on this CPU. Run the
> command above to reproduce on your own hardware.

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
| `cargo check --workspace` fails on `bootloader`/`x86_64` with `-Z flag is only accepted on the nightly channel` | Expected on stable: those are `hv1-boot`/`hv1-core` dependencies that need a nightly toolchain and `-Zbuild-std`. Run `cargo check-ws` (an alias defined in `.cargo/config.toml`) or add `--exclude hv1-core --exclude hv1-boot` yourself; see CI's `hv1-check`/`hv1-clippy` jobs for the pinned-nightly invocation |
| Permission denied on `/dev/kvm` | Add user to kvm group: `sudo usermod -aG kvm $USER`                                   |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, coding standards, and PR process.

## License

This project is dual-licensed:

- **AGPL-3.0** (GNU Affero General Public License v3) — free and open source with strong copyleft. See [LICENSE](LICENSE).
- **Commercial License** — available for use without AGPL obligations. See [LICENSE-COMMERCIAL](LICENSE-COMMERCIAL).

For commercial licensing inquiries, contact
[licensing@nervosys.ai](mailto:licensing@nervosys.ai).

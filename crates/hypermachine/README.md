# hypermachine

The frontdoor crate for [**HyperMachine**](https://github.com/nervosys/HyperMachine) —
a high-performance hypervisor framework with first-class AI-agent support.

This crate re-exports the Type-2 (hosted) `hv2` stack under stable module paths, so
you can depend on one crate instead of wiring up each component:

```toml
[dependencies]
hypermachine = "1.1"
```

```rust
use hypermachine::{core, agent, runtime, api};
```

| Module | Crate | Role |
|---|---|---|
| `core`    | [`hv2-core`](https://crates.io/crates/hv2-core)       | VM lifecycle, memory, hypervisor backends (KVM / WHPX / HVF) |
| `cpu`     | [`hv2-cpu`](https://crates.io/crates/hv2-cpu)         | vCPU execution and exit handling |
| `gpu`     | [`hv2-gpu`](https://crates.io/crates/hv2-gpu)         | GPU acceleration (WGPU compute, virtio-gpu, passthrough) |
| `net`     | [`hv2-net`](https://crates.io/crates/hv2-net)         | Virtual networking |
| `agent`   | [`hv2-agent`](https://crates.io/crates/hv2-agent)     | MCP tool registry and agent control plane |
| `runtime` | [`hv2-runtime`](https://crates.io/crates/hv2-runtime) | Orchestration |
| `api`     | [`hv2-api`](https://crates.io/crates/hv2-api)         | Agentic HTTP/gRPC API and tool ontology |

The bare-metal Type-1 (`hv1-*`) crates and the `hm-cli` / `hv2-cli` binaries are
published separately and are not part of this facade.

## License

Licensed `AGPL-3.0-only OR LicenseRef-Commercial` — AGPLv3 for open-source use, with
a commercial license available. See the [repository](https://github.com/nervosys/HyperMachine).

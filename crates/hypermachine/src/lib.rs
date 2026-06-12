//! # HyperMachine
//!
//! The frontdoor crate for the [HyperMachine](https://github.com/nervosys/HyperMachine)
//! hypervisor framework — a high-performance hypervisor with first-class AI-agent
//! support. It re-exports the Type-2 (hosted) `hv2` stack under stable module
//! paths so you can depend on a single crate instead of wiring up each component:
//!
//! ```toml
//! [dependencies]
//! hypermachine = "1.1"
//! ```
//!
//! ## Layout
//!
//! | Module | Crate | Role |
//! |---|---|---|
//! | [`core`]    | `hv2-core`    | VM lifecycle, memory, and the hypervisor backend abstraction (KVM / WHPX / HVF) |
//! | [`cpu`]     | `hv2-cpu`     | vCPU execution and instruction/exit handling |
//! | [`gpu`]     | `hv2-gpu`     | GPU acceleration (WGPU compute, virtio-gpu, passthrough) |
//! | [`net`]     | `hv2-net`     | Virtual networking |
//! | [`agent`]   | `hv2-agent`   | MCP tool registry and the agent control plane |
//! | [`runtime`] | `hv2-runtime` | Orchestration tying the stack together |
//! | [`api`]     | `hv2-api`     | The agentic HTTP/gRPC API and tool ontology |
//!
//! Each module is the corresponding crate re-exported verbatim; see that crate's
//! own documentation for details. The bare-metal Type-1 (`hv1-*`) crates and the
//! `hm-cli` / `hv2-cli` binaries are published separately and are not part of this
//! facade.

#![forbid(unsafe_code)]

pub use hv2_agent as agent;
pub use hv2_api as api;
pub use hv2_core as core;
pub use hv2_cpu as cpu;
pub use hv2_gpu as gpu;
pub use hv2_net as net;
pub use hv2_runtime as runtime;

/// The HyperMachine framework version (matches the workspace release).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

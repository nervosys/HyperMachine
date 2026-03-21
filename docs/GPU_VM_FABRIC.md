# Trusted GPU VM Fabric

**Positioning**
**The control plane for secure, high-utilization AI VMs.**

**What it is**
A VM operations layer for cloud providers and large AI companies that turns raw GPU infrastructure into a trusted, schedulable, observable, and monetizable AI platform.

**Problem**
GPU VMs are expensive, operationally messy, and hard to differentiate. Providers and AI labs already have hypervisors, clusters, and provisioning tools, but still struggle with:

* poor GPU utilization
* weak topology-aware placement
* image and runtime drift
* multi-tenant isolation risk
* limited attestation and policy enforcement
* fragmented observability
* difficulty packaging infrastructure into premium AI products

**Solution**
Trusted GPU VM Fabric sits above the virtualization stack and below customer workloads to standardize how AI VMs are provisioned, placed, governed, and monitored.

**Core capabilities**

* **Trusted provisioning**
  Signed images, measured boot, attestation hooks, and policy checks before workloads run.
* **AI-aware scheduling**
  GPU topology-aware placement, reservations, preemption, priority classes, and workload-aware allocation.
* **Runtime lifecycle management**
  Standardized rollout of drivers, CUDA stacks, images, and dependencies across heterogeneous fleets.
* **Observability for AI infrastructure**
  Unified visibility into GPU utilization, VM health, storage bottlenecks, and workload efficiency.
* **Policy and tenant governance**
  Image allowlists, runtime restrictions, audit trails, approval workflows, and network/egress controls.
* **Commercial packaging**
  Quotas, metering, reserved capacity, premium secure tiers, and SLA-aligned VM classes.

**Who it’s for**

* GPU cloud providers launching differentiated AI compute offerings
* large AI companies operating internal or hybrid GPU fleets
* enterprise infrastructure teams building secure private AI clouds
* regulated or defense-adjacent operators needing stronger control and auditability

**Primary use cases**

* premium secure GPU VM offerings
* confidential or attested AI compute tiers
* training/inference fleet scheduling
* internal AI infrastructure standardization
* runtime/image consistency across clusters
* multi-tenant GPU cloud governance

**Why it wins**
Unlike generic cloud management tools, this platform is built specifically for the operational realities of AI infrastructure.

**Key differentiators**

* designed for GPU economics, not generic VM orchestration
* combines trust, scheduling, and governance in one layer
* improves utilization while preserving tenant isolation
* supports commercial cloud and self-hosted AI environments
* helps providers turn infrastructure into premium SKUs

**Value proposition**

* increase GPU fleet yield
* reduce operational drift and support burden
* launch differentiated secure AI infrastructure products
* improve trust for high-value tenants and workloads
* standardize VM operations across mixed hardware environments

**Messaging pillars**

* **Trusted** — verifiable images, controlled runtime state, auditability
* **Efficient** — smarter placement, higher utilization, less wasted GPU capacity
* **Governed** — policy controls, tenant boundaries, approval workflows
* **Monetizable** — premium tiers, quotas, metering, and enterprise-ready packaging

**Suggested tagline options**

* **Trusted operations for AI VMs**
* **Secure, schedulable GPU infrastructure at scale**
* **From raw GPU hosts to premium AI cloud**
* **The control plane for high-value AI compute**

**Ideal call to action**

* Launch a pilot on one GPU cluster
* Standardize your secure AI VM offering
* Book an architecture session

---

## Architecture

### System Layers

```
┌──────────────────────────────────────────────────────────────┐
│                      Tenant Workloads                         │
│            (training jobs, inference, notebooks)              │
├──────────────────────────────────────────────────────────────┤
│                     GPU VM Fabric                             │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐   │
│  │ Topology │ │  Fleet   │ │ Capacity │ │ Image         │   │
│  │ Scheduler│ │ Manager  │ │ Manager  │ │ Registry      │   │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────┘   │
├──────────────────────────────────────────────────────────────┤
│              HyperMachine Hypervisor (HV2/HV1)               │
│      VM lifecycle · Memory · Devices · Networking · GPU      │
├──────────────────────────────────────────────────────────────┤
│              Hardware (GPU · CPU · NIC · NVMe)               │
└──────────────────────────────────────────────────────────────┘
```

### Module Map

| Module              | Crate       | Lines | Tests | Purpose                                                                            |
| ------------------- | ----------- | ----- | ----- | ---------------------------------------------------------------------------------- |
| `topology.rs`       | hv2-runtime | 734   | 20+   | GPU interconnect discovery, NVLink/NVSwitch/PCIe scoring, topology-aware placement |
| `fleet.rs`          | hv2-runtime | 955   | 20+   | Rolling/canary driver and image rollouts, per-host lifecycle, maintenance windows  |
| `capacity.rs`       | hv2-runtime | 675   | 20+   | SLA tiers, VM classes, reserved capacity blocks, committed-use contracts           |
| `image_registry.rs` | hv2-core    | 658   | 20+   | Image allowlists/denylists, signature verification, admission control              |
| `pool.rs`           | hv2-runtime | —     | 13    | Warm-standby VM pool, provision/acquire/recycle lifecycle                          |
| `scheduler.rs`      | hv2-runtime | —     | —     | Bin-pack/spread/best-fit/random placement with affinity constraints                |

### Topology-Aware Scheduling

The `GpuTopologyMap` discovers the physical GPU interconnect graph (NVLink, NVSwitch, PCIe peer,
PCIe root, cross-NUMA) and scores placement candidates based on aggregate peer-to-peer bandwidth.

**Interconnect hierarchy (scored 0.0–1.0):**

| Interconnect | Score | Bandwidth | Use case                                |
| ------------ | ----- | --------- | --------------------------------------- |
| NVSwitch     | 1.0   | ~900 GB/s | All-to-all multi-GPU training           |
| NVLink       | 0.9   | ~300 GB/s | Paired GPU training / pipeline parallel |
| PCIe peer    | 0.6   | ~32 GB/s  | Inference, small models                 |
| PCIe root    | 0.3   | ~16 GB/s  | Mixed workloads                         |
| Cross-NUMA   | 0.1   | ~8 GB/s   | Last resort                             |

**Key types:**

* `GpuDevice` — physical GPU descriptor (BDF address, VRAM, compute capability, NUMA node)
* `GpuRequirements` — workload GPU request (count, min VRAM, min compute, interconnect preference)
* `GpuPlacement` — scored allocation result (device set + aggregate affinity score)
* `TopologyLink` — directional link between two GPUs with bandwidth and interconnect type

### Fleet Lifecycle Management

The `FleetManager` orchestrates driver, CUDA toolkit, VM image, firmware, and custom artifact
rollouts across a heterogeneous fleet. Each rollout is a state machine:

```
Created → InProgress → [ Paused ] → Completed
                    ↘ Failed ↗
```

**Rollout strategies:**

* `Rolling` — upgrade hosts in batches (configurable batch size, pause between batches)
* `Canary` — upgrade a subset first, validate, then continue
* `BlueGreen` — provision new hosts, drain old, switch traffic

**Key types:**

* `FleetHost` — host record with current driver/CUDA/image versions and health status
* `ArtifactVersion` — versioned artifact with SHA-256 hash and release notes
* `RolloutConfig` — strategy, target version, batch size, timeout, max failures
* `Rollout` — active rollout tracking per-host phase (Pending → Downloading → Installing → Verifying → Done/Failed)

### Capacity Reservations

The `CapacityManager` maps SLA tiers to scheduling guarantees and manages reserved capacity blocks.

**SLA tiers:**

| Tier       | Availability | Scheduling | Preemption                       |
| ---------- | ------------ | ---------- | -------------------------------- |
| BestEffort | —            | Fill gaps  | Always preemptible               |
| Standard   | 99.9%        | Normal     | Preemptible by Premium/Dedicated |
| Premium    | 99.95%       | Priority   | Not preemptible                  |
| Dedicated  | 99.99%       | Guaranteed | Dedicated hosts, never preempted |

**Key types:**

* `VmClass` — named class (e.g., "secure-gpu-premium") with vCPU/memory/GPU/SLA/priority/billing config
* `Reservation` — committed capacity block with start/end time, state machine (Pending → Active → Expired/Cancelled)
* `CapacityManager` — tracks reservations, enforces limits, reports utilization

### Image Admission Control

The `ImageRegistry` enforces policy over which images can launch workloads.

**Enforcement modes:**

* `AllowlistOnly` — only explicitly approved images may run
* `DenylistOnly` — any image except denied ones may run
* `AllowAndDeny` — allowlist takes priority, denylist blocks the rest

**Key types:**

* `ImageEntry` — registry record with kind, version, SHA-256 digest, approval status, signature
* `ImageSignature` — signer identity + signature bytes + algorithm
* `AdmissionDecision` — Allowed / Denied (with reason) / RequiresReview
* `RegistryConfig` — enforcement mode, require signatures flag, auto-approve after N reviewers

### Integration Points

```
Workload submission
        │
        ▼
  ┌─────────────┐    ┌─────────────┐
  │  Image       │───▶│  Admission  │── Denied? → reject
  │  Registry    │    │  Decision   │
  └─────────────┘    └──────┬──────┘
                            │ Allowed
                            ▼
                   ┌─────────────────┐
                   │ Capacity Manager │── Check reservation & SLA tier
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │ Topology Scorer  │── Score GPU placement options
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │  VM Pool /       │── Acquire warm VM or provision new
                   │  Scheduler       │
                   └────────┬────────┘
                            │
                            ▼
                      VM Running
```

---

*Last Updated: March 21, 2026*

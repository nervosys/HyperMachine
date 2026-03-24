# LLM Serving with HyperMachine Unikernels

Estimated performance, security, and energy improvements enabled by serving large language models with HyperMachine unikernels versus conventional Linux VMs and optimized containers.

---

## Performance Increase (Inference Throughput & Latency)

### Boot & Cold-Start: 100–1,000× Faster

A unikernel strips the guest kernel down to the inference runtime alone — no init system, no scheduler, no filesystem daemon. HyperMachine's warm-pool architecture (slot states: Provisioning → Warm → Assigned → Draining → Recycling in `hv2-runtime/src/pool.rs`) keeps pre-booted unikernels in standby, reducing cold-start from **30–60 s** (full Linux VM + model load) to **10–50 ms**. This is the single largest win for serverless or autoscaling LLM endpoints.

### Token-Generation Latency: 15–30% Reduction

| Mechanism | Estimated Gain | Source |
|-----------|---------------|--------|
| Eliminated kernel context switches (unikernel = single address space, no syscall overhead) | 5–10% | No ring transitions for CUDA/GPU driver calls |
| Huge-page model weight allocation (2 MB / 1 GB pages via `hv1-core/src/memory.rs`) | 3–8% | Eliminates TLB misses traversing 70B-parameter KV-cache |
| Interrupt coalescing (100 µs window, max 16, via `hv2-core/src/perf.rs`) | 2–5% | Batches GPU completion interrupts during decode |
| Topology-aware GPU placement (NVLink 0.9 → PCIe peer 0.6 scoring in the GPU fabric) | 5–12% | Keeps tensor-parallel shards on NVLink-connected GPUs instead of crossing NUMA |
| Zero-copy guest memory (mmap-backed GuestMemory via `hv2-core/src/memory.rs`) | 2–4% | Avoids bounce-buffer copies for KV-cache pages |

**Net estimate**: For a Llama-3-70B model across 4×A100 GPUs, expect **~20% higher tokens/sec** compared to the same model in a full Linux VM behind QEMU/KVM, and **~10–15%** over an optimized container (which still carries kernel overhead and scheduler jitter).

### Throughput (Requests/Sec at Saturation): 20–40% Higher Density

Unikernel memory footprint is 50–200 MB vs 1–4 GB for a minimal Linux guest. On a 2 TB host serving 70B-parameter models (each needing ~140 GB in FP16 across GPUs), the savings in *host-side* overhead translate to **2–4 additional concurrent inference VMs** per node — a direct throughput multiplier.

---

## Security Improvement

### Attack Surface Reduction: ~99% Fewer CVE-Exposed Code Paths

| Metric | Linux VM Guest | HyperMachine Unikernel |
|--------|---------------|----------------------|
| Kernel syscalls exposed | ~450 | ~10–20 (only those the inference runtime uses) |
| Running daemons | 20–50 (sshd, systemd, cron, …) | 0 |
| Writable filesystem | Full | None (immutable image, verified via `image_registry.rs` SHA-256 digest) |
| Shell / package manager | Present | Absent — no lateral movement possible |

### Confidential Computing: Hardware-Rooted Isolation

HyperMachine's memory encryption module (`hv2-core/src/security/memory_encryption.rs`) supports AMD SEV-SNP and Intel TDX with per-page KeyId tracking. For LLM providers, this means:

- **Model weights encrypted in DRAM** — even a compromised hypervisor host admin cannot extract proprietary model parameters.
- **vTPM attestation** (`hv2-core/src/security/vtpm.rs`) — clients can cryptographically verify the unikernel image hash before sending prompts, enabling **verifiable confidential inference**.
- **Secure boot chain** (`hv2-core/src/security/secure_boot.rs`) — RSA-2048/ECDSA signature verification from PlatformKey through to the inference binary.

### Post-Quantum Readiness

The `hv2-core/src/crypto/pqc.rs` module implements ML-KEM (FIPS 203) and ML-DSA (FIPS 204). API traffic between client and inference endpoint can use ML-KEM-768 key encapsulation (192-bit post-quantum security), protecting against harvest-now-decrypt-later attacks on model queries containing sensitive data. No other hypervisor ships this natively today.

### Quantified Improvement

Using MITRE ATT&CK mapping: a Linux-VM LLM serving stack exposes **T1059 (command scripting), T1053 (scheduled tasks), T1021 (remote services), T1078 (valid accounts)** — all eliminated in a unikernel. Conservative estimate: **80–95% reduction in exploitable attack techniques** relevant to inference workloads.

---

## Energy Use Reduction

### Per-Inference Energy: 15–35% Lower

| Factor | Saving | Rationale |
|--------|--------|-----------|
| No idle guest kernel overhead | 10–15% of CPU energy | Eliminates scheduler ticks, timer interrupts, background daemons consuming ~5–10 W per VM continuously |
| Faster cold-start → shorter spin-up power draw | 5–10% amortized | A 40 ms boot vs 40 s boot eliminates 40 s × ~300 W (GPU idle power) = ~3.3 Wh per scale-up event |
| Higher VM density → fewer physical servers | 10–15% fleet energy | 2–4 extra VMs per node means ~20–30% fewer nodes at the same throughput |
| Interrupt coalescing reduces CPU wake-ups | 2–5% | Fewer C-state exits during batch decoding |

### Fleet-Level Estimate

For a 1,000-GPU inference cluster (typical mid-tier AI serving provider):

| Scenario | Servers (8×H100 each) | Power (kW) | Annual Energy (MWh) |
|----------|----------------------|------------|---------------------|
| Linux VMs (baseline) | 125 | 500 | 4,380 |
| HyperMachine unikernels | 95–105 | 380–420 | 3,330–3,680 |
| **Savings** | **20–30 servers** | **80–120 kW** | **700–1,050 MWh/yr** |

At $0.10/kWh, that is **$70K–$105K/year in electricity alone**, plus the CapEx savings of 20–30 fewer servers at ~$300K each (**$6–9M**).

### Carbon Impact

At the US grid average of 0.39 kg CO₂/kWh, the 700–1,050 MWh reduction translates to **273–410 metric tons of CO₂ avoided per year** per 1,000-GPU cluster.

---

## Summary

| Dimension | vs. Linux VM | vs. Optimized Container | Key HyperMachine Enabler |
|-----------|-------------|------------------------|--------------------------|
| Cold-start latency | 100–1,000× | 10–100× | Warm pool + unikernel boot |
| Token generation latency | −15–30% | −10–15% | Huge pages, zero-copy, topology-aware GPU placement |
| Throughput density | +20–40% | +10–20% | 50–200 MB footprint vs 1–4 GB |
| Attack surface (syscalls) | −97% | −90% | Single-address-space unikernel, no shell |
| Confidential inference | Not available in most | Not available | SEV-SNP/TDX + vTPM attestation |
| Post-quantum protection | Not available | Not available | Native ML-KEM/ML-DSA |
| Energy per inference | −15–35% | −10–20% | No idle kernel, higher density |
| Fleet servers needed | −20–25% | −10–15% | Memory footprint reduction |

The largest dollar-value impact is **GPU density** — fitting more inference VMs per physical server directly reduces the dominant cost line (GPU hardware at $25K–$40K per H100). The security story — immutable images, confidential computing, post-quantum crypto — is the *differentiator* that justifies premium pricing over commodity container platforms.

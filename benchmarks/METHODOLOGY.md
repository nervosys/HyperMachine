# Cross-hypervisor benchmark methodology

This document specifies **how** HyperMachine is to be compared against other
hypervisors, **what** each workload measures, and **why this repository ships
no published comparison numbers**. It is the rigorous companion to the
user-facing [`README.md`](README.md) and the harness internals in
[`harness/README.md`](harness/README.md).

The guiding principle is *honest, reproducible comparison*: every number must
come from the same hardware, the same guest image, and the same measurement
code across all hypervisors, with the exact environment disclosed. We would
rather publish **no numbers** than numbers we cannot reproduce on
representative hardware.

---

## 1. Validity controls

A comparison is only meaningful if everything except the hypervisor under test
is held constant. The harness enforces the following (see
[`config.toml`](config.toml)):

| Control | How it is held constant |
| --- | --- |
| **Guest image** | One image (`images/build/debian-12-bench.qcow2`) built once by [`images/build-guest.sh`](images/build-guest.sh) and used by every adapter. Same kernel, same userland, same benchmark binaries. |
| **Guest shape** | Identical `cpus`, `mem_mib`, `disk_gib`, and network mode (`[guest]`) for every hypervisor; per-workload overrides apply equally to all. |
| **Host CPU placement** | Guest vCPUs pinned to a fixed set of host cores (`[host].cpu_pin`) so results are not perturbed by scheduler migration. |
| **Frequency scaling** | CPU governor set to `performance` (`[host].governor`) to remove DVFS jitter. |
| **Background noise** | Listed host services stopped for the duration (`[host].stop_services`); the harness warns rather than fails if they are absent. |
| **Warm-up** | One warm-up run is executed and **discarded** before any timed sample, for every per-sample workload. |
| **Repetition** | `[run].samples` timed samples per (hypervisor, workload) pair (default 5), with a `cooldown_sec` sleep between samples. |
| **Measurement code** | The *same* workload script measures every hypervisor. Hypervisor-specific logic lives only in the 8-verb adapter contract (`harness/lib/adapter.sh`), never in the measurement. |

If an adapter cannot perform an operation a workload needs (e.g. snapshot for
`boot_warm`), it returns exit code `78` and the runner **skips** that workload
for that hypervisor rather than substituting a different measurement.

---

## 2. Hypervisor matrix

Defined in `[hypervisor.*]` blocks; each maps to an adapter under
`harness/adapters/`.

| Hypervisor | Adapter | Class | Notes |
| --- | --- | --- | --- |
| **hypermachine** | `hypermachine.sh` | Type-2 (this repo) | Reference implementation; `cargo build --release -p hm-cli`. |
| **qemu_kvm** | `qemu_kvm.sh` | Type-2 / KVM | `-cpu host -machine q35,accel=kvm`. |
| **firecracker** | `firecracker.sh` | microVM / KVM | Needs uncompressed kernel + ext4 rootfs. |
| **cloud_hypervisor** | `cloud_hypervisor.sh` | microVM / KVM | |
| **virtualbox** | `virtualbox.sh` | Type-2 | `VBoxManage`. |
| **hyper_v** | `hyper_v.ps1` | Type-1 (Windows) | Admin shell + Hyper-V role; driven by `run.ps1`. |

A hypervisor participates only when `enabled = true` **and** it is installed and
runnable by the current user. Adding one is "drop an adapter + flip a flag"; no
workload changes are required.

---

## 3. Workload catalog

Each workload script prints TSV `metric<TAB>value<TAB>unit` lines; the runner
collects them into `raw/<metric>.raw` and then a per-metric
`<metric>.summary.csv` with `n,min,p50,p95,p99,max,mean,stdev`. The aggregator
([`report/aggregate.py`](report/aggregate.py)) renders `summary.md` with
hypervisors as columns, reporting **p50 (± stdev)** and a per-metric
*direction* (whether higher or lower is better).

| Workload key | Metric(s) | Unit | Better | Measures |
| --- | --- | --- | --- | --- |
| `boot_cold` | `boot_cold_seconds` | s | lower | Runner times VM start → SSH-ready (full cold boot to usable). |
| `boot_warm` | `boot_warm_seconds` | s | lower | Restore-from-snapshot → ready (skipped if the adapter lacks snapshot/restore). |
| `cpu_single` | `cpu_events_per_sec`, `cpu_latency_avg_ms` | evt/s, ms | higher / lower | Single-thread sysbench CPU throughput + average latency in-guest. |
| `cpu_multi` | `cpu_events_per_sec`, `cpu_latency_avg_ms` | evt/s, ms | higher / lower | All-vCPU sysbench CPU throughput + latency. |
| `mem_bandwidth` | sysbench memory write rate | MiB/s | higher | Sustained memory bandwidth. |
| `disk_rand_read_4k` | fio IOPS / latency | IOPS, µs | higher / lower | 4 KiB random read at `iodepth`. |
| `disk_rand_write_4k` | fio IOPS / latency | IOPS, µs | higher / lower | 4 KiB random write, `fsync`. |
| `disk_seq_read_1m` | fio MiB/s | MiB/s | higher | 1 MiB sequential read. |
| `net_throughput` | iperf3 Gbit/s | Gbit/s | higher | Guest↔host TCP throughput, `parallel` streams. |
| `net_latency` | ping RTT | µs | lower | Round-trip latency. |
| `vmexit` | cycles per exit type | cycles | lower | In-guest microbench compiling & timing VM-exit-inducing instructions (`ITERATIONS` each). |
| `kernel_build` | wallclock | s | lower | `make -j` of a pinned Linux tarball (heavy; opt-in). |
| `density` | `density_vms_started`, `density_avg_rss_MiB`, `density_total_rss_MiB` | count, MiB | higher / lower | Host memory cost of `count` idle VMs after `settle_s` (per-VM RSS via the adapter's `metrics` verb). |
| `mgmt_latency` | create+start+ready+destroy wallclock | s | lower | Control-plane latency over `cycles` lifecycle iterations. |

Directions are encoded in `aggregate.py::direction()`; latency/time/RSS are
lower-is-better, throughput/IOPS/bandwidth/count are higher-is-better.

---

## 4. Statistical treatment

- **Central tendency**: median (p50) is the headline figure — robust to the
  occasional outlier sample that VM workloads are prone to.
- **Dispersion**: standard deviation is reported alongside p50; tail behaviour
  is captured by p95/p99/max in the raw summary CSV.
- **No cross-host normalization**: numbers are only ever compared *within a
  single run on a single host*. Figures from different hosts/kernels are never
  placed in the same table.
- **Sample size**: the default of 5 timed samples is adequate for the stable
  metrics (CPU, memory, throughput) and should be raised for high-variance ones
  (boot, mgmt latency) when publishing.

---

## 5. Reproducing a run

```bash
cd benchmarks
./images/build-guest.sh                 # one-time: build the shared guest image
$EDITOR config.toml                      # enable the hypervisors/workloads you have
./harness/run.sh --config config.toml --out results/$(date +%Y%m%d-%H%M%S)
python3 report/aggregate.py results/<run-id>   # -> results/<run-id>/summary.md
```

A valid published result **must** accompany the `summary.md` with: exact CPU
model, core/NUMA layout, RAM, kernel version, host OS, and the version/commit of
every hypervisor in the matrix. A run missing any of these is not citable.

---

## 6. Why this repository ships no comparison numbers

**Short version: the only hardware available to this repository's authors and
to CI cannot run a fair comparison, and publishing numbers from unrepresentative
hardware would be dishonest.**

A meaningful cross-hypervisor comparison requires *all* of the following on one
machine:

1. A **bare-metal Linux host** with hardware virtualization (Intel VT-x / AMD-V)
   exposed to the OS — i.e. KVM. The KVM-class competitors (QEMU/KVM,
   Firecracker, Cloud Hypervisor) do not run without it.
2. **Root / privileged access** to set the CPU governor, pin cores, manage TAP
   bridges, and load each hypervisor.
3. **Every competitor installed** at a known version, plus a built guest image
   and SSH key material.
4. **A quiet, dedicated machine** — no other tenants, so cooldown and pinning
   actually isolate the measurement.

Neither of the environments this project is developed and validated in satisfies
that:

- **The development host is Windows** (where this code is built and tested).
  KVM is unavailable; only Hyper-V/WHPX and VirtualBox could run, so the
  KVM-class field — the most interesting comparison — is absent. Numbers from a
  2-of-6 matrix would misrepresent the field.
- **CI runners are virtualized** (GitHub-hosted runners are themselves VMs).
  Nested virtualization is either disabled or non-representative, the machines
  are shared and noisy, and their hardware varies run-to-run. Any number
  produced there is unreproducible by definition and would violate §4's
  "single host, disclosed hardware" rule.

Consequently:

- This repo ships the **harness, adapters, workloads, and aggregator** so that
  anyone with representative bare-metal hardware can produce numbers — but it
  deliberately ships **no `results/`** (the directory is `.gitignore`d) and
  quotes no comparison figures in the README or docs.
- The performance numbers that *do* appear (e.g. the crypto throughput table in
  the top-level README) are **single-tool microbenchmarks** of HyperMachine's
  own code via `cargo bench`/criterion on a stated CPU — not cross-hypervisor
  comparisons — and are labelled as such.

This mirrors the project's broader posture: claims are backed by code that can
be run and verified, and figures that cannot be reproduced on representative
hardware are not published.

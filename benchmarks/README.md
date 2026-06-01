# HyperMachine cross-hypervisor benchmark harness

A reproducible suite for comparing **HyperMachine** against other hypervisors
(QEMU/KVM, Firecracker, Cloud Hypervisor, VirtualBox, Microsoft Hyper-V) on
the same hardware with the same guest image.

> **Status**: harness scaffold. Workloads & adapters are wired but require a
> test host with the chosen hypervisors and a built guest image before they
> will produce numbers. See [`images/README.md`](images/README.md) and
> [`harness/README.md`](harness/README.md).
>
> **No comparison numbers are published in this repository.** A fair run needs
> a dedicated bare-metal Linux host with KVM and every competitor installed —
> which neither the (Windows) dev host nor the (virtualized) CI runners provide.
> See [`METHODOLOGY.md`](METHODOLOGY.md) for the full measurement methodology
> and the rationale for shipping the harness but no figures.

## Quick start

### Linux host (KVM-class hypervisors)

```bash
cd benchmarks
# 1. Build the standard guest image (one-time)
./images/build-guest.sh
# 2. Edit the matrix to pick hypervisors & workloads
$EDITOR config.toml
# 3. Run
./harness/run.sh --config config.toml --out results/$(date +%Y%m%d-%H%M%S)
# 4. Aggregate
python3 report/aggregate.py results/<run-id>
```

### Windows host (Hyper-V / WHPX-HyperMachine / VirtualBox)

```powershell
cd benchmarks
.\harness\run.ps1 -Config config.toml -Out "results/$(Get-Date -Format yyyyMMdd-HHmmss)"
python report\aggregate.py results\<run-id>
```

## What gets measured

| Workload             | Metric                                     | Tool                                                |
| -------------------- | ------------------------------------------ | --------------------------------------------------- |
| Cold boot            | seconds from `vm start` → first SSH accept | wallclock + ssh polling                             |
| Warm boot (snapshot) | seconds for snapshot restore → SSH         | wallclock                                           |
| CPU single-thread    | events/sec                                 | `sysbench cpu --threads=1`                          |
| CPU multi-thread     | events/sec                                 | `sysbench cpu --threads=N`                          |
| Memory bandwidth     | MiB/s                                      | `sysbench memory` + STREAM (optional)               |
| Disk 4K random read  | IOPS, p99 latency                          | `fio --rw=randread --bs=4k --iodepth=32`            |
| Disk 4K random write | IOPS, p99 latency                          | `fio --rw=randwrite --bs=4k --iodepth=32 --fsync=1` |
| Disk 1M sequential   | MiB/s                                      | `fio --rw=read/write --bs=1M --iodepth=4`           |
| Network throughput   | Gbit/s                                     | `iperf3 -t 30` (host↔guest, both directions)        |
| Network latency      | µs (p50/p99)                               | `netperf TCP_RR`                                    |
| VM exit cost         | ns/exit (cpuid, hlt, mmio)                 | in-guest microbench (see `workloads/vmexit.c`)      |
| Kernel build         | seconds                                    | `make -j$(nproc) defconfig && time make -j$(nproc)` |
| Density              | resident MiB / idle VM                     | parent-side RSS sampling                            |
| Mgmt latency         | ms to create+destroy                       | adapter `create`/`destroy` timing                   |

## Layout

```
benchmarks/
├── README.md                  ← this file
├── config.toml                ← hypervisor + workload matrix
├── images/
│   ├── README.md
│   └── build-guest.sh         ← Debian cloud image + cloud-init seed
├── harness/
│   ├── README.md
│   ├── run.sh                 ← Linux orchestrator
│   ├── run.ps1                ← Windows orchestrator
│   ├── lib/
│   │   ├── common.sh
│   │   ├── ssh.sh
│   │   ├── metrics.sh
│   │   └── adapter.sh         ← adapter ABI contract
│   ├── adapters/
│   │   ├── qemu_kvm.sh
│   │   ├── firecracker.sh
│   │   ├── cloud_hypervisor.sh
│   │   ├── virtualbox.sh
│   │   ├── hypermachine.sh    ← uses hm-cli
│   │   └── hyper_v.ps1
│   └── workloads/
│       ├── boot_time.sh
│       ├── cpu_sysbench.sh
│       ├── mem_sysbench.sh
│       ├── disk_fio.sh
│       ├── net_iperf3.sh
│       ├── net_latency.sh
│       ├── vmexit.c           ← compiled in-guest
│       ├── kernel_build.sh
│       ├── density.sh
│       └── mgmt_latency.sh
├── report/
│   ├── aggregate.py
│   ├── requirements.txt
│   └── templates/
│       └── summary.md.jinja
└── results/                   ← per-run subdirs; gitignored except .gitkeep
    └── .gitkeep
```

## Adapter ABI

Every hypervisor adapter exposes the same shell-level contract so workloads
remain hypervisor-agnostic. See [`harness/lib/adapter.sh`](harness/lib/adapter.sh).

```
HV=qemu_kvm   hv_setup    <image> <cpus> <mem_mib> <disk_path> <netif>
HV=qemu_kvm   hv_start    <vm_id>                       # async; prints pid
HV=qemu_kvm   hv_wait_ssh <vm_id> <ssh_port> <timeout>  # block until ready
HV=qemu_kvm   hv_snapshot <vm_id> <name>                # optional
HV=qemu_kvm   hv_restore  <vm_id> <name>                # optional
HV=qemu_kvm   hv_stop     <vm_id>
HV=qemu_kvm   hv_destroy  <vm_id>
HV=qemu_kvm   hv_metrics  <vm_id>                       # prints "rss_kib=...,cpu_pct=..."
```

Workloads invoke these via the dispatcher in `lib/common.sh::hv` so the same
script runs against every hypervisor.

## Repeatability rules

The orchestrator enforces:

- **Pinning**: each VM is pinned to a fixed set of host CPUs (taskset/cpuset).
- **Frequency**: `cpupower frequency-set -g performance` on Linux; equivalent
  power plan on Windows. The orchestrator records `/proc/cpuinfo` cur_freq for
  each sample.
- **Warm-up**: every workload runs once and is discarded, then N=5 timed runs.
- **Cool-down**: 10 s sleep between runs; VM destroyed between samples for
  cold-boot workloads.
- **Isolation**: host services known to add jitter are stopped (configurable
  list in `config.toml`). The harness warns instead of hard-failing.

Each run directory contains:

```
results/<run-id>/
├── env.json                   # cpu, kernel, hypervisor versions, BIOS revision
├── config.snapshot.toml       # exact matrix used
├── <hypervisor>/
│   └── <workload>/
│       ├── raw/               # per-sample JSON + stdout/stderr
│       └── summary.csv        # median / p99 / stddev / count
└── summary.md                 # produced by aggregate.py
```

## What this harness intentionally does NOT do

- It does **not** install hypervisors. You must have them installed and
  accessible (`qemu-system-x86_64 --version`, `VBoxManage --version`,
  `hm-cli --version`, etc.) before running.
- It does **not** download large guest images automatically — see
  `images/README.md`.
- It does **not** publish results anywhere.

## License

Same as the parent project — see workspace root [`LICENSE`](../LICENSE).

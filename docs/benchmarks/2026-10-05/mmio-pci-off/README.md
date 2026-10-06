# MMIO guests boot with `pci=off`

**Change:** an MMIO guest (the default transport) now boots with `pci=off`. Such a guest has no
PCI device: its vsock and network devices are virtio-mmio. Yet the daemon still attached a PCI
root complex, and the guest probed all of bus 0 plus buses 254 and 255 on every boot.

Firecracker boots its PCI-less guests with `pci=off` too. Its harness does not pass the flag;
Firecracker appends it itself when PCI support is disabled, which is its default. PCI guests
(`--guest-transport pci`) keep enumeration, as before.

## Exit counts (load-independent)

Method: the host kernel's `kvm_exit`, `kvm_pio` and `kvm_mmio` tracepoints recorded every exit
while `tools/bench-local-engines.py` booted three guests per engine. Exits are grouped by vCPU
thread: HyperMachine's are named `vcpu-0`, Firecracker's `fc_vcpu 0`. No other VM was running.
Scripts: `run.sh`, `analyze.py`, `pci.py`. Results: `exit-counts/`.

| Per guest boot | Baseline | Candidate (`pci=off`) | Firecracker 1.17.0 |
|---|---:|---:|---:|
| VM exits | 26,319 | 23,958 | ~23,450 |
| Port I/O exits | 2,995 | ~620 | ~400 |
| PCI config-space accesses (data) | ~655, over 321 bus/dev/fn | 1 | 0 |
| vCPU active span, first to last exit | 419–433 ms | 402–404 ms | 385–411 ms |

**The difference was the PCI probe.** All other exit reasons (page faults, `cpuid`, MSRs) match
Firecracker within a few percent. About 500 exits remain: serial, RTC (`0x70`/`0x71`), PIC and
IOAPIC accesses.

## Latency (shared host, interleaved)

Each block runs both engines; the order of variants is baseline, candidate, candidate,
baseline. Firecracker in every block is the control for host drift. The host was a shared WSL
machine at roughly 50% CPU from other work. Raw reports are in `c1/`, `c8/` and `c50/`.

| Concurrency | Attempts passed | Baseline gap to FC (p50) | Candidate gap to FC (p50) | Reading |
|---|---:|---:|---:|---|
| 1 (10 pairs/block) | 80/80 | 39.5, 45.3 ms | 19.1, 18.6 ms | Consistent: HM p50 457 → 432 ms |
| 8 (4 bursts/block) | 256/256 | mean 105.5 ms | mean 78.4 ms | Smaller and noisy; one candidate block ran high while FC stayed flat |
| 50 (2 bursts/block) | 400/400 | mean 675.5 ms | mean 615.0 ms | **Inconclusive.** FC's own p50 swung 1,583–2,239 ms; the adjacent blocks 3/4 show no gain |

**What this establishes:**
- Each boot no longer performs ~2,360 VM exits that did nothing.
- A single guest's cold start falls by about 25 ms, which halves the gap to Firecracker.

**What it does not establish:**
- A gain under heavy concurrency on this host.
- Any win over Firecracker. HyperMachine is still about 19 ms slower for a single guest.
- Anything about boxd or exe.dev, whose endpoints were not measured.

Binaries and guest inputs are identified in `artifact-sha256.txt`. The two daemons were built
from one tree, with and without the change.

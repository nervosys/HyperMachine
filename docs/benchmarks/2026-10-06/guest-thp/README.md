# Guest RAM on transparent huge pages

**Change:** the KVM backend maps guest RAM on a 2 MiB boundary and marks it `MADV_HUGEPAGE`
(`map_guest_ram` in `crates/hv2-core/src/backends/kvm.rs`). Previously it was a plain anonymous
mapping, which on a host with THP set to `madvise` (the common default, and this host's) is
backed by 4 KiB pages only.

Nearly every exit during a cold boot was a nested page fault: the guest touching a 4 KiB page
for the first time, which the host then faults in. With huge pages one fault maps 2 MiB. KVM
uses a 2 MiB EPT entry only where the host address and the guest physical address agree modulo
2 MiB, so the mapping is over-allocated and trimmed to an aligned base rather than relying on
the kernel to align it. The mapping is still lazy, and the advice is best-effort: a host with
THP set to `never` keeps 4 KiB pages and behaves exactly as before.

## Exit counts (load-independent)

Method: as in [`2026-10-05/mmio-pci-off`](../../2026-10-05/mmio-pci-off/README.md). The host
kernel's `kvm_exit`, `kvm_pio` and `kvm_mmio` tracepoints recorded every exit while
`tools/bench-local-engines.py` booted three guests per engine, Firecracker 1.17.0 alongside as
the control. `AnonHugePages` in `/proc/meminfo` was sampled every 200 ms during the run.
Scripts: `build-thp.sh`, `run-thp.sh`, `analyze.py`. Results: `exit-counts/`.

| Per guest boot | Baseline (master 1fdc4170) | Candidate (huge pages) | Firecracker 1.17.0 |
|---|---:|---:|---:|
| VM exits | 24,159–24,624 | **2,809–2,991** | 23,536–25,061 (both runs) |
| Nested page faults (`npf`) | 22,100 | **659** | 21,631–21,891 (run means) |
| Other exits (`cpuid`, `io`, `msr`, ...) | ~2,350 | ~2,220 | ~2,000 |
| Host `AnonHugePages`, peak during run | 0 kB | 90,112 kB | — |

**About 88% fewer exits per boot.** The remaining ~660 nested page faults are MMIO accesses
(about 615 per boot to the IOAPIC, virtio-mmio and LAPIC pages) and the few 2 MiB first
touches. `cpuid` (671) and port I/O (653) counts are identical in both variants; MSR and
interrupt exits vary from run to run in both engines.

## Memory

Daemon `Rss`/`Pss` at guest readiness, one guest live, same three boots:

| Boot | Baseline Rss | Candidate Rss |
|---|---:|---:|
| 1 | 97,304 kB | 103,456 kB |
| 2 | 112,456 kB | 118,088 kB |
| 3 | 127,184 kB | 132,984 kB |

About **+6 MB (5%) per running guest**: touched memory is rounded up to 2 MiB pages. The
growth from boot to boot appears in both variants and predates this change.

## Latency: not established

The host was a shared WSL machine at 100% CPU from other work; Firecracker's own readiness
ranged 861–2,152 ms across the two runs. The `ready_ms` values in `exit-counts/*-bench.json`
are recorded but support no latency claim. Removing ~21,400 exits per boot should shorten a
cold start; that has to be measured on a quiet host with the interleaved ABBA method used for
`pci=off`.

**What this establishes:**
- A HyperMachine cold boot performs about 2,900 VM exits, against 23,500–25,000 for Firecracker
  1.17.0 booting the same kernel and initrd.
- The guest really is backed by 2 MiB pages on a `madvise` THP host.
- The memory cost is about 6 MB per guest for this workload.

**What it does not establish:**
- Any latency gain, or any win over Firecracker on latency.
- The memory cost for guests that touch memory sparsely, where 2 MiB rounding costs more.
- Behaviour on hosts with THP `never`, or under memory fragmentation, where `defrag=madvise`
  may stall a fault on compaction.
- Anything about boxd or exe.dev, whose endpoints were not measured.

Binaries and guest inputs are identified in `artifact-sha256.txt`; the bench reports carry the
same daemon hashes. The two daemons were built from one tree, with and without the change.

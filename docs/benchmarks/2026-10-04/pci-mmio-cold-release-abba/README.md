# Matched cold release MMIO / PCI ABBA

The same frozen release executable used in ../pci-mmio-release-abba now runs with --no-template. Order is MMIO–PCI–PCI–MMIO, on the same owned WSL/KVM host, buffered guest/kernel, one vCPU, 1 GiB and eight-CPU affinity. Each cohort has two excluded host warmups and sixteen scored fresh guest creates. All 72 exact-command/create/delete/inventory gates pass; 64 samples are scored. Each raw daemon log records 18 Linux image loads and zero snapshot restores.

| Pooled scored result | MMIO | PCI |
| --- | ---: | ---: |
| Cold API create P50 | 425.68 ms | 753.93 ms |
| Cold API create P95 | 2114.16 ms | 1346.20 ms |
| Create + exact command P50 | 428.02 ms | 757.19 ms |
| Create + exact command P95 | 2130.41 ms | 1363.78 ms |
| Mean sampled process CPU / create + command | 978.75 ms | 570.63 ms |
| Immediately held whole-daemon PSS P50 | 193.63 MiB | 163.72 MiB |

PCI has higher median cold latency despite its lower prepared-create median in the separate gate. Its pooled cold P95, sampled CPU and immediately held PSS are lower here. These measurements establish tradeoffs, not an overall winning transport. Keep the default MMIO choice unchanged.

The modes use different configured guest boot argument bundles; this is not an isolated transport-cost experiment. Guest creation is cold, but host file/page caches are warm. P95 is nearest-rank over 32 samples per mode; no confidence intervals or production-tail claims follow. Latency includes localhost HTTP and readiness; resource sampling/deletion checks are outside latency. CPU is whole-process stat ticks with 10-ms granularity and includes the preceding PSS read. Held PSS is measured immediately after the command, not a five-second idle hold or per-VM footprint.

The guest image includes the owned buffered fixtures; the earlier Firecracker cold comparison uses a different shared guest image and idle-memory measurement, so its numbers cannot be merged with this table. No managed endpoint, throughput, higher concurrency or competitor advantage is measured here. All daemons stop without forced kills; every guest is deleted to an empty inventory.

Source hashes and frozen binary identity match the preceding prepared comparison. source-context is a nine-file permitted catalog, not a complete build closure. Protected root backend/boot sources remain excluded. driver.py uses exclusive fresh stores and the recorded frozen executable. verify-metrics.py recomputes metrics from raw samples; verify.py also validates cold-mode metadata and boot/restore logs.

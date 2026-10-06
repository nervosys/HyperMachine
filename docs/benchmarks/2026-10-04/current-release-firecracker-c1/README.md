# Current release versus Firecracker: matched C1 cold creation

The current frozen release daemon and Firecracker 1.17.0 each pass 8/8 cold-create-to-exact-command attempts, in eight alternating AB/BA pairs on one owned WSL/KVM host. Both use the same immutable kernel and output-drain guest image, eight-CPU affinity, one vCPU and 1,024 MiB. HyperMachine uses default MMIO and --no-template; admission budgeting is disabled. Inputs/harness hashes remain unchanged. All guests are cleaned up, the daemon exits normally and inventory is empty.

| C1 result | HyperMachine | Firecracker |
| --- | ---: | ---: |
| Passed / attempted | 8 / 8 | 8 / 8 |
| Cold readiness P50 | 477.35 ms | 445.64 ms |
| Cold readiness P95 / P99 | 1315.94 ms | 1495.04 ms |
| Median held PSS after five seconds | 151.98 MiB | 85.88 MiB |
| Median incremental PSS | 84.92 MiB | 85.88 MiB |

HyperMachine remains slower at the observed median and retains more whole-process PSS. Its observed P95/P99 is lower in this small run; with eight attempts these tail quantiles are maxima, not reliable production-tail estimates. Incremental PSS subtracts the same-batch empty-daemon baseline for HyperMachine; Firecracker starts with zero processes. The persistent HTTP daemon's retained baseline is a continuing memory gap even when incremental guest measurements are similar. These values do not identify its cause or prove memory-density parity.

This refresh covers C1 only. The October 3 C8/C50/C100 data remains tied to its older executable and is not revalidated by this run. The separate October 4 PCI/MMIO comparison uses a buffered guest and immediate PSS sampling, so its numbers cannot be merged with these matched-engine results.

Latency includes engine creation, agent readiness and exact uniquely marked shell output. Earlier guests remain held until measurement; at C1 this is one guest per batch. The requested idle hold is five seconds and actual ages/durations are retained. HyperMachine has persistent HTTP process overhead; Firecracker uses fresh VMM processes and Unix APIs. PSS excludes kernel/host allocations. Affinity does not reserve CPUs from other WSL work. Managed boxd/exe.dev/E2B, higher concurrency, snapshots, throughput and independent hosts remain outside this result. No across-the-board winner is established.

Reproduce using bench-local-engines-concurrent.py --concurrency 1 --pairs 8 --memory-idle-seconds 5, preserving input hashes and fixed affinity from context.json, with a fresh output directory. The three exact harness sources and raw report are retained. analysis.py independently recomputes coverage, readiness quantiles and memory medians; verify.py also checks frozen identities and real idle holds. source-context is the permitted nine-file release catalog with accepted isolated protected hashes, not a complete build closure. Protected root backend/boot files were excluded.

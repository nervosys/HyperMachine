# Matched release MMIO / PCI prepared-create comparison

One frozen release daemon completes MMIO–PCI–PCI–MMIO cohorts on the same owned WSL/KVM host, accepted kernel/initrd, one-vCPU/1-GiB guest and eight-CPU affinity. Every cohort uses a fresh required template/store, two excluded warmups and sixteen scored creates. All 72 creates run an exact uniquely marked guest command, delete the sandbox and verify empty inventory; 64 samples are scored, 32 per mode. All daemons stop without forced kills.

| Pooled scored result | MMIO | PCI |
| --- | ---: | ---: |
| API create P50 | 22.50 ms | 19.87 ms |
| API create P95 | 31.40 ms | 24.31 ms |
| Command P50 | 10.67 ms | 10.24 ms |
| Command P95 | 18.88 ms | 16.01 ms |
| Create + exact command P50 | 33.59 ms | 29.73 ms |
| Create + exact command P95 | 42.67 ms | 41.71 ms |
| Mean sampled process CPU / create + command | 36.88 ms | 31.56 ms |
| Held whole-daemon PSS P50 | 75,337 KiB | 76,685 KiB |

PCI has 11.69% lower pooled create P50, 22.57% lower pooled create P95 and 14.41% lower sampled CPU, but 1.79% higher held process PSS in this run. The default remains MMIO; these observations do not establish a universal winner.

| Cohort | Create P50 | Create P95 | Create + command P50 | Mean sampled CPU |
| --- | ---: | ---: | ---: | ---: |
| 1 MMIO | 21.22 ms | 31.15 ms | 31.63 ms | 33.75 ms |
| 2 PCI | 19.32 ms | 24.31 ms | 29.45 ms | 30.00 ms |
| 3 PCI | 20.88 ms | 31.60 ms | 31.45 ms | 33.12 ms |
| 4 MMIO | 24.67 ms | 31.81 ms | 38.03 ms | 40.00 ms |

P95 is the nearest-rank quantile, not an interpolated percentile; the small cohorts do not support confidence intervals or tail reliability claims. PCI cohort 3 P95 exceeds MMIO cohort 1, despite the lower pooled P95. API timing includes localhost HTTP client/server work and guest readiness. Template boot, deletes, inventory checks and process sampling are outside the latency intervals. CPU uses whole-process /proc stat ticks (10-ms resolution here); its interval includes the preceding PSS read. PSS is the held daemon, including guest and server state, not per-VM memory. Raw stable PID/start-time identities and before/held snapshots are retained.

This compares configured product modes, whose guest boot argument bundles differ. It does not isolate IRQ implementation cost. Network is disabled; throughput, cold boot, higher concurrency, independent hosts and managed boxd/exe.dev/E2B endpoints are not measured. Existing Firecracker comparisons have different workload contexts and cannot be combined with these timings. No competitor superiority is claimed.

Build: `cargo build --locked --release -p hv2-sandboxd` in the accepted isolated tree. The frozen executable is /var/tmp/hm-transport-release-node-v1, copied from its release target. Source-context records nine permitted source hashes and accepted isolated protected hashes, not a complete build closure. Its prior debug-input catalog is retained for provenance; context.json and release_binary_sha256 identify the actual measured release executable. Protected root backend/boot files were excluded.

`verify-metrics.py <archive-directory>` independently recomputes metrics from all raw samples and checks exact/create/delete/inventory gates and process identities. driver.py uses exclusive fresh stores and the frozen binary; input paths must match recorded hashes before rerunning.

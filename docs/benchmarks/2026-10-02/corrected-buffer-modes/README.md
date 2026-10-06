# Buffer ownership comparison with corrected timing

All 1,600 restores pass, with cleanup and runtime activation verified. Two fresh-daemon owned/borrowed pairs each contain two counterbalanced HyperMachine/Firecracker pairs at concurrency 100, one CPU and 1,024 MiB per guest. All resource checks occur after the entire timed batch. Both modes use the same immutable executable; guest commands and prepared state are identical.

| Mode | Passed | Mean ms | P99 ms | Held PSS MiB | Incremental PSS MiB | FC control P99 ms | FC held PSS MiB |
|---|---:|---:|---:|---:|---:|---:|---:|
| owned | 400/400 | 866.384 | 1081.401 | 340.810 | 283.176 | 1711.327 | 280.587 |
| borrowed | 400/400 | 955.309 | 1437.015 | 321.000 | 276.205 | 1298.681 | 281.625 |

Pair 0: borrowed reduces mean by -100.314 ms, P99 by -358.509 ms and held PSS by 14.601 MiB. Negative reductions denote regressions. Firecracker control mean shifts by 33.445 ms.

Pair 1: borrowed reduces mean by -77.535 ms, P99 by -113.829 ms and held PSS by 26.406 MiB. Negative reductions denote regressions. Firecracker control mean shifts by -245.027 ms.

This is an isolated candidate comparison, not a managed competitor benchmark or an adopted runtime change. Shared WSL/KVM scheduling and Python-client overhead remain uncontrolled. Owned mode is a counterfactual within the refactor, not the original accepted executable. The previously frozen comparisons remain unchanged. Build, source-generation and core/KVM test evidence is retained in [the original mode archive](../boot-buffer-modes/README.md); the binary SHA and three source hashes bind this repeat to that tested build.

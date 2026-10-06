# Diagnostic setup stage profile

A separate instrumented release daemon passes **51 KVM checks** and **128/128 scored operations**. An owned temporary checker tags only matched host-to-target UDP benchmark requests. All 136 server records, including warmups, match exact payload/pair/path identities. The profile times pre-authorization/lock work, the first fresh authorization barrier, guest-port opening, temporary loopback socket-pair creation, the second fresh authorization barrier and total handler setup. It excludes TLS handshake and HTTP dispatch before handler entry, later upgrade/relay registration, echo work and source guest Ethernet. Authorization is zero for the standard path because its empty conditional is timed.

These are diagnostic cohort medians; they are not production benchmark scores. Timers/logging can perturb execution, and medians of components do not add exactly to the median total.

| Payload / path | Fresh authorization total P50 (ms) | Guest-port open P50 (ms) | Loopback pair P50 (ms) | Server setup P50 (ms) |
|---|---:|---:|---:|---:|
| 64 B / private | 0.287 | 0.876 | 0.092 | 1.260 |
| 64 B / standard | 0.000 | 0.861 | 0.091 | 0.961 |
| 65,507 B / private | 0.280 | 0.760 | 0.091 | 1.123 |
| 65,507 B / standard | 0.000 | 0.743 | 0.092 | 0.839 |

The fresh authorization pair accounts for about 0.28–0.29 ms at the private-path median. Guest-port opening is the largest absolute stage at about 0.74–0.88 ms, while loopback-pair creation is about 0.09 ms. Paired authorization differences are retained independently of total differences; do not derive exact attribution percentages by adding/subtracting medians. The results support a next experiment that overlaps independent guest-port opening and loopback-pair creation while preserving the first barrier and keeping the final barrier after both resources are ready, including cancellation-safe cleanup. This experiment is not implemented or measured here.

The production root source/checker are unchanged. Instrumentation is applied only to the accepted isolated receiver during the diagnostic build; a `finally` block restores its exact bytes. All 139 permitted root/isolate source pairs and accepted isolated core hashes are revalidated after the run. Protected root core files were not read or built. The distinct diagnostic binary is `a9416733652cd5db4d443eb708b786d0d5afa5b3ea03bf5657b11bd63bcfcf01`; use the prior ABBA evidence for production performance. The temporary checker logs no payloads or credentials; its labels contain only payload size, pair and private/standard path. HTTP CRLF escaping is verified in the reproducible build driver.

All lifecycle/authentication/data checks pass and every guest/tracked process is cleaned up. No performance improvement, resource saving, end-to-end guest latency or competitor win is claimed. Guest image/kernel and other fixture inputs are unchanged. Run `python3 verify-results.py` for exact tag matching, raw logs/journal consistency, independently recomputed phase and paired medians, source hashes and cleanup. `analyze.py` retains the original owned analysis paths; build/run drivers require fresh outputs. Manifest and source context pin every diagnostic input and payload. IPv6, other races/stress and independent-host/external performance remain incomplete or unmeasured.

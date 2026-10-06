# Matched IPv4/IPv6 guest-family characterization

This ABBA comparison uses the same CLI binary for both configurations. IPv4 local peers stay fixed; only guest destination family changes from 127.0.0.1/hv2-udp/1 to ::1/hv2-udp6/1. The same immutable node/control/kernel/image inputs and guest resources (one vCPU/1 GiB) are used in four fresh HTTPS/mTLS/Redis/KVM stacks. The fixture launches an IPv4-only or IPv6-only echo service according to the explicit guest family. This is a configuration comparison, not a code-optimization experiment.

Eight concurrent peers each perform ten warmups then 1,000 measured tagged 4 KiB round trips, with synchronized starts. Rates divide 8,000 verified replies by the slowest peer's measured interval and exclude warmups.

| Metric | IPv4 guest | IPv6 guest |
|---|---:|---:|
| Average cohort rate (round trips/sec) | 3,916.1 | 3,912.3 |
| Mean per-peer sample medians (ms) | 1.9881 | 1.9897 |

IPv6's average rate is -0.10% relative to IPv4. Cohort latency directions are mixed. This small sampled difference does not establish statistical equivalence, a universal performance win or production overhead. Four short unpinned WSL development-build cohorts without concurrent compilation do not establish sustained capacity, resource efficiency, overload loss or fleet tails. Local IPv6 peers are not the variable in this comparison.

All ten IPv4 and twelve IPv6 correctness/cleanup checks pass per cohort. Both preserve empty/binary/maximum payloads, peer isolation and lifecycle closure, with credential/trust/malformed-frame refusal. IPv6 adds family-protocol and direct node credential refusal. These extra checks occur before timing. All owned services are reaped, no guests remain, and every input hash stays unchanged. Six-crate source and parsed dependencies match the current root as recorded in the preceding alignment audit; accepted isolated core remains the execution boundary.

The runner now records explicit baseline/candidate guest/local IPv6 configurations, verifies measured family fields and labels comparisons as binary, configuration or mixed. Same binary contents are permitted only when explicit configurations differ. Identical binary/configuration pairs and out-of-bound peer counts are verified to fail before output creation. Existing binary-comparison defaults remain unchanged.

Reproduce the archived compare-udp-cli.py using summary.json inputs, --candidate-guest-ipv6 --peer-count 8 --payload-bytes 4096 --samples-per-peer 1000 and a fresh output. Both CLI paths deliberately identify the same immutable executable. Raw reports, individual latency samples, input hashes and checker/runner source are preserved. No Boxd or exe.dev endpoint was available; their performance remains unmeasured.

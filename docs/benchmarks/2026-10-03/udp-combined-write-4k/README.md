# UDP combined writes with 4 KiB payloads

This follow-up to [the 64-byte experiment](../udp-combined-write/README.md) uses the same preserved baseline and combined-write CLI binaries and matched daemon/control/kernel/image inputs. Four fresh HTTPS/mTLS/Redis/KVM stacks run in baseline/candidate/candidate/baseline order. Each has two concurrent peers, ten warmups per peer and a synchronized measured interval of 1,000 exact tagged 4,096-byte replies per peer. Rates exclude warmups and divide 2,000 replies by the slower peer's measured interval.

| CLI | Cohort round trips/second | Average |
|---|---:|---:|
| Baseline | 1,356.7; 1,499.1 | 1,427.9 |
| Combined write | 1,662.7; 1,631.9 | 1,647.3 |

The average rate increases 15.4%. Per-peer median latency is mixed (about 1.18–1.20 ms); no consistent median improvement is claimed. Sampled P99 spans 1.65–1.98 ms for baseline and 1.57–1.67 ms for candidate. All ten correctness checks pass in every cohort, including authentication/trust refusal, empty/binary/maximum datagrams, malformed-frame recovery, peer isolation and pause/resume/delete. All owned services are reaped and guest inventory is empty. Raw samples and recorded input hashes are preserved in each report.

Reproduce with the archived checker, --tls --mtls --concurrent-samples 1000 --concurrent-payload-bytes 4096, recorded input binaries, and four fresh output directories in ABBA order. Baseline/candidate source and binary hashes are preserved in the preceding experiment. The checker adds a bounded configurable payload size; its default remains 64 bytes.

This is a short local development-build closed-loop experiment on unpinned WSL CPUs, one guest vCPU/1 GiB, without a concurrent build. It supports retaining the combined-write optimization for these tested workloads. It does not establish sustained capacity, packet loss under overload, fleet tails, resource efficiency, IPv6/native UDP performance or superiority over a competing service. No competitor endpoint was available.

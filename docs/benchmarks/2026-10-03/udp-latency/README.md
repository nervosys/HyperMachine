# Local HTTPS/KVM UDP round-trip latency

Two fresh owned full-stack runs each measure 100 byte-verified 64-byte request/reply samples after ten warmups per path, with one outstanding request. Both retain all eight existing correctness/trust/lifecycle checks and reap all owned processes. Raw per-sample times and binary/image identities are archived.

| Path | Run | P50 ms | P95 ms | P99 ms |
|---|---|---:|---:|---:|
| Direct upgraded control API | First | 0.972 | 1.143 | 1.225 |
| Direct upgraded control API | Repeat | 0.935 | 1.149 | 1.432 |
| CLI loopback listener through control API | First | 0.956 | 1.127 | 1.535 |
| CLI loopback listener through control API | Repeat | 0.968 | 1.162 | 2.442 |

Each request traverses API HTTPS, the separately launched control plane, its HTTP node hop, real vsock relay and KVM guest UDP echo socket. Timing includes send, receive and exact byte verification; session setup is excluded. CLI timing additionally includes the local UDP socket/listener. Paths run sequentially in each cohort, not randomized or interleaved, so these rows are characterization rather than a controlled overhead comparison. P99 uses the nearest-rank 99th sample of only 100 and is a weak estimate of sustained tail behavior.

Reproduce with check-udp-cluster-kvm.py --tls --latency-samples 100 and the archived input paths, then repeat with a fresh output directory. Inputs use a development build, one guest vCPU/1 GiB, unpinned host CPUs and local WSL resources. Host page cache, other work and scheduler activity are not controlled. All replies in measured samples match; this is not a loss/throughput/load test. No competitor endpoint, native public UDP, IPv6, mTLS or fleet workload is measured. No competitor superiority or code speedup is claimed.
